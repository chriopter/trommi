"""Measure launch and report in one process: macOS Python 3.9 clocks have process-local origins."""
import argparse
import ctypes
import json
import os
from pathlib import Path
import re
import subprocess
import time


def process_started_at(pid):
    # Darwin SDK sys/proc_info.h: PROC_PIDTBSDINFO. Read our isolated app's process only.
    class BsdInfo(ctypes.Structure):
        _fields_ = [('ids', ctypes.c_uint32 * 12), ('comm', ctypes.c_char * 16),
                    ('name', ctypes.c_char * 32), ('details', ctypes.c_uint32 * 6),
                    ('seconds', ctypes.c_uint64), ('micros', ctypes.c_uint64)]
    info = BsdInfo()
    library = ctypes.CDLL('/usr/lib/libSystem.B.dylib', use_errno=True)
    call = library.proc_pidinfo
    call.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_uint64, ctypes.c_void_p, ctypes.c_int]
    call.restype = ctypes.c_int
    assert call(pid, 3, 0, ctypes.byref(info), ctypes.sizeof(info)) == ctypes.sizeof(info), 'cannot read app process birth'
    assert info.ids[3] == pid and info.seconds > 0 and info.micros < 1_000_000, 'invalid app process birth'
    return info.seconds + info.micros / 1_000_000


def launched_pid(text):
    matches = re.findall(r'^com\.trommi\.ios:\s*(\d+)\s*$', text, re.MULTILINE)
    assert len(matches) == 1 and int(matches[0]) > 0, 'simctl did not report one launched process'
    return int(matches[0])


def wait_for_report(report, launch_log, process, clock=time.monotonic, sleep=time.sleep, timeout=15, wall=time.time):
    deadline = clock() + timeout
    seen = {}
    while clock() < deadline:
        try:
            value = json.loads(report.read_text())
            if isinstance(value, dict) and isinstance(value.get('process_id'), int):
                seen.setdefault(value['process_id'], (value, clock(), wall()))
        except (FileNotFoundError, json.JSONDecodeError):
            pass
        code = process.poll()
        if code is not None:
            assert code == 0, 'simctl launch failed'
            pid = launched_pid(launch_log.read_text())
            if pid in seen:
                return seen[pid]
        sleep(.01)
    raise AssertionError('the launched process did not publish its own startup report')


def measure_launch(sim, fixture, report, launch_log, clock=time.monotonic, sleep=time.sleep, spawn=subprocess.Popen, wall=time.time, born=process_started_at):
    report.unlink(missing_ok=True)
    env = dict(os.environ, SIMCTL_CHILD_TROMMI_SCREEN='desk', SIMCTL_CHILD_TROMMI_FIXTURE=str(fixture), SIMCTL_CHILD_TROMMI_SPEED_TRACE='1')
    started = clock()
    launch_wall = wall()
    with launch_log.open('w') as output:
        process = spawn(['xcrun', 'simctl', 'launch', '--terminate-running-process', sim, 'com.trommi.ios'], env=env, text=True, stdout=output)
        try:
            value, ready, ready_wall = wait_for_report(report, launch_log, process, clock, sleep, wall=wall)
            birth = born(value['process_id'])
            assert launch_wall <= birth <= ready_wall, 'app birth is outside this launch'
            value['process_to_desk_ms'] = (ready_wall - birth) * 1000
            value['launch_to_desk_ms'] = (ready - started) * 1000
            value['simctl_total_ms'] = (clock() - started) * 1000
            return value
        finally:
            if process.poll() is None:
                process.terminate()
            process.wait(timeout=5)


def validate_report(value, rows, budget):
    assert value['rows'] == rows, value
    assert 0 < value['desk_ms'] <= value['process_to_desk_ms'] < budget, value
    assert value['process_to_desk_ms'] <= value['launch_to_desk_ms'] + 5, value


def wait_for_exit(pid):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            return
        time.sleep(.02)
    raise AssertionError('the previous simulator app process did not terminate')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('sim')
    parser.add_argument('fixture', type=Path)
    parser.add_argument('report', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('launch_log', type=Path)
    parser.add_argument('rows', type=int)
    parser.add_argument('budget', type=float)
    args = parser.parse_args()
    value = None
    try:
        value = measure_launch(args.sim, args.fixture, args.report, args.launch_log)
        text = json.dumps(value, sort_keys=True)
        args.output.write_text(text + '\n')
        validate_report(value, args.rows, args.budget)
        print(text)
    finally:
        subprocess.run(['xcrun', 'simctl', 'terminate', args.sim, 'com.trommi.ios'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
        if value is not None:
            wait_for_exit(value['process_id'])


if __name__ == '__main__':
    main()
