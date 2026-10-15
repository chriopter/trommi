#!/usr/bin/env python3
"""The iPhone fleet of the torture run (MegaFleetTests.swift) on another machine, e.g. a Mac: Apple's own
Foundation and URLSession instead of the Linux test runner's.

The torture run starts this in place of the fleet's test runner (MEGA_FLEET_RUNNER) and talks to it through command
files in TROMMI_MEGA_FLEET, as to the runner itself. It relays each file over one ssh connection to the same script
on the other machine, which hands it to the fleet there and sends the answers back. The devices there reach the hub
at the same address as here, http://127.0.0.1:<port> of the run's proxy (a hub takes plain http only on the local
host): the ssh connection forwards that port back here (`-R`), once the first command named it.

  MEGA_FLEET_SSH="ssh -i <key> <user>@<host>"   how to reach the other machine
  MEGA_FLEET_ROOT=<checkout there>              a checkout with core/bindings/swift built (`core/bindings/swift/build.sh macos` on any
                                                host, its lib/macos copied over) and `swift build --build-tests
                                                --package-path tests/ios` done there
  MEGA_FLEET_RUNNER=tests/client/mega/fleet-remote.py … torture
"""
import json
import os
import shlex
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import urllib.parse


def local() -> None:
    folder = os.environ["TROMMI_MEGA_FLEET"]
    ssh = shlex.split(os.environ["MEGA_FLEET_SSH"])
    root = os.environ["MEGA_FLEET_ROOT"]
    remote = f"cd {shlex.quote(root)} && exec python3 -u tests/client/mega/fleet-remote.py remote"
    # the first command (the first iPhone's sign-in) names the hub: its port is forwarded from there to here
    first = os.path.join(folder, "cmd-0.json")
    while not os.path.exists(first):
        time.sleep(0.02)
    with open(first) as f:
        port = urllib.parse.urlparse(json.load(f)["hub"]).port
    forward = ["-o", "ExitOnForwardFailure=yes", "-R", f"{port}:127.0.0.1:{port}"]
    link = subprocess.Popen(ssh[:1] + forward + ssh[1:] + [remote], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)

    def answers() -> None:
        for line in link.stdout:
            n, _, body = line.rstrip("\n").partition("\t")
            tmp = os.path.join(folder, f"res-{n}.json.tmp")
            with open(tmp, "w") as f:
                f.write(body)
            os.rename(tmp, os.path.join(folder, f"res-{n}.json"))
        os._exit(0)

    threading.Thread(target=answers, daemon=True).start()
    n = 0
    while True:
        path = os.path.join(folder, f"cmd-{n}.json")
        while not os.path.exists(path):
            time.sleep(0.02)
        with open(path) as f:
            cmd = json.load(f)
        link.stdin.write(f"{n}\t{json.dumps(cmd)}\n")
        link.stdin.flush()
        if cmd.get("op") == "quit":
            link.stdin.close()
            link.wait()
            return
        n += 1


def remote() -> None:
    folder = tempfile.mkdtemp(prefix="trommi-fleet-")
    log = open(os.path.join(folder, "log.txt"), "w")
    env = dict(os.environ, TROMMI_MEGA_FLEET=folder)
    runner = subprocess.Popen(
        ["swift", "test", "--skip-build", "--package-path", "tests/ios", "--filter", "TrommiShellTests.MegaFleetTests/testFleet"],
        env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=log,
    )
    open_ones: set[int] = set()
    lock = threading.Lock()

    def relay_answers() -> None:
        while True:
            with lock:
                waiting = sorted(open_ones)
            for n in waiting:
                path = os.path.join(folder, f"res-{n}.json")
                if os.path.exists(path):
                    with open(path) as f:
                        body = f.read()
                    with lock:
                        open_ones.discard(n)
                    sys.stdout.write(f"{n}\t{body}\n")
                    sys.stdout.flush()
            if runner.poll() is not None and not waiting:
                return
            time.sleep(0.02)

    relay = threading.Thread(target=relay_answers, daemon=True)
    relay.start()
    for line in sys.stdin:
        n, _, body = line.rstrip("\n").partition("\t")
        with lock:
            open_ones.add(int(n))
        tmp = os.path.join(folder, f"cmd-{n}.json.tmp")
        with open(tmp, "w") as f:
            f.write(body)
        os.rename(tmp, os.path.join(folder, f"cmd-{n}.json"))
    # the run is over (its quit was the last command, or the connection ended): the fleet and its folder go
    try:
        runner.wait(timeout=30)
    except subprocess.TimeoutExpired:
        runner.kill()
    relay.join(timeout=5)
    sys.stderr.write(open(os.path.join(folder, "log.txt")).read()[-4000:])
    shutil.rmtree(folder, ignore_errors=True)


if __name__ == "__main__":
    remote() if sys.argv[1:2] == ["remote"] else local()
