#!/usr/bin/env python3
"""Checks that a signed Trommi.app and its three extensions carry their entitlements files, value for value.

    ios_entitlements.py <AppStore folder> <Trommi.app> [--distribution] [--mac]

Every entry of TrommiApp.entitlements must be in the app's signature, and each extension's file in its own
signature (TrommiShare, TrommiNotify, TrommiLive). A signature may carry more (application-identifier, the team);
it may not carry less or another value. `$(AppIdentifierPrefix)` in a file (the keychain access groups) is the
signing team followed by a dot: the team the signature names, or, in an ad hoc signature without a team, any team or
none. With --distribution the app's signature must also name a team and say get-task-allow false (a build App Store
Connect takes). --mac: the Mac app, TrommiMac.entitlements and no extensions (there, a build App Store Connect takes
carries no com.apple.security.get-task-allow). Exit 1 names every difference."""
import plistlib
import re
import subprocess
import sys
from pathlib import Path

PREFIX = "$(AppIdentifierPrefix)"


def same(want, have, team) -> bool:
    """Whether a signed value is the file's value, with Xcode's team prefix filled in."""
    if isinstance(want, str) and PREFIX in want:
        if not isinstance(have, str):
            return False
        head, _, rest = want.partition(PREFIX)
        if team:
            return have == f"{head}{team}.{rest}"
        return re.fullmatch(re.escape(head) + r"(?:[A-Z0-9]{10}\.)?" + re.escape(rest), have) is not None
    if isinstance(want, list):
        return isinstance(have, list) and len(want) == len(have) and all(same(w, h, team) for w, h in zip(want, have))
    return want == have

PARTS = [
    ("", "TrommiApp.entitlements"),
    ("PlugIns/TrommiShare.appex", "TrommiShare.entitlements"),
    ("PlugIns/TrommiNotify.appex", "TrommiNotify.entitlements"),
    ("PlugIns/TrommiLive.appex", "TrommiLive.entitlements"),
]

MAC_PARTS = [("", "TrommiMac.entitlements")]


def signed(bundle: Path) -> dict:
    out = subprocess.run(["codesign", "-d", "--entitlements", "-", "--xml", str(bundle)],
                         capture_output=True, check=False)
    if out.returncode != 0:
        raise SystemExit(f"::error::{bundle.name} is not signed: {out.stderr.decode(errors='replace').strip()}")
    return plistlib.loads(out.stdout) if out.stdout.strip() else {}


def main() -> int:
    appstore, app = Path(sys.argv[1]), Path(sys.argv[2])
    distribution = "--distribution" in sys.argv[3:]
    mac = "--mac" in sys.argv[3:]
    wrong = []
    for sub, name in (MAC_PARTS if mac else PARTS):
        bundle = app / sub if sub else app
        want = plistlib.loads((appstore / name).read_bytes())
        have = signed(bundle)
        for key, value in want.items():
            if key not in have:
                wrong.append(f"{bundle.name}: {key} is missing")
            elif not same(value, have[key], have.get("com.apple.developer.team-identifier")):
                wrong.append(f"{bundle.name}: {key} is {have[key]!r}, the file says {value!r}")
        if distribution:
            if mac:
                if have.get("com.apple.security.get-task-allow"):
                    wrong.append(f"{bundle.name}: com.apple.security.get-task-allow is set")
            elif have.get("get-task-allow") is not False:
                wrong.append(f"{bundle.name}: get-task-allow is {have.get('get-task-allow')!r}, not false")
            if not have.get("com.apple.developer.team-identifier"):
                wrong.append(f"{bundle.name}: no team identifier")
        print(f"{bundle.name}: {len(want)} entitlements as in {name}" + (" (distribution)" if distribution else ""))
    for line in wrong:
        print(f"::error::{line}")
    return 1 if wrong else 0


if __name__ == "__main__":
    sys.exit(main())
