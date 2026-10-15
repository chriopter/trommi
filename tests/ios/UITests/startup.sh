#!/bin/bash
# Measure an already-built Release app with a made-up full-size fixture, without XCTest's launch injection.
# startup.sh /absolute/Trommi.app /absolute/fixture.json expected-rows /absolute/evidence-folder
set -euo pipefail
app_path=${1:?Release Simulator app}
fixture_path=${2:?made-up fixture JSON}
expected_rows=${3:?expected Desk rows}
evidence=${4:?evidence folder}
[[ "$app_path" = /* && "$fixture_path" = /* && "$evidence" = /* ]] || { echo 'Use absolute paths' >&2; exit 2; }
[[ "$expected_rows" =~ ^[1-9][0-9]*$ ]] || exit 2
test -d "$app_path"; test -f "$fixture_path"
mkdir -p "$evidence"
sim=$(xcrun simctl create "trommi-startup-$$" "${SIM_DEVICE_TYPE:-com.apple.CoreSimulator.SimDeviceType.iPhone-17-Pro}" "${SIM_RUNTIME:-com.apple.CoreSimulator.SimRuntime.iOS-27-0}")
cleanup() { xcrun simctl shutdown "$sim" 2>/dev/null || true; xcrun simctl delete "$sim"; }
trap cleanup EXIT
xcrun simctl boot "$sim"
xcrun simctl bootstatus "$sim" -b > "$evidence/boot.log"
xcrun simctl install "$sim" "$app_path"
for iteration in 1 2 3; do
  folder=$(xcrun simctl get_app_container "$sim" com.trommi.ios data)
  report="$folder/Documents/native-desk-startup.json"
  python3 - "$report" <<'PY'
from pathlib import Path
import sys
Path(sys.argv[1]).unlink(missing_ok=True)
PY
  started=$(python3 -c 'import time; print(time.monotonic())')
  SIMCTL_CHILD_TROMMI_SCREEN=desk SIMCTL_CHILD_TROMMI_FIXTURE="$fixture_path" SIMCTL_CHILD_TROMMI_SPEED_TRACE=1 \
    xcrun simctl launch --terminate-running-process "$sim" com.trommi.ios > "$evidence/launch-$iteration.log"
  python3 - "$report" "$evidence/startup-$iteration.json" "$expected_rows" "${DESK_BUDGET_MS:-2000}" "$started" <<'PY'
import json, sys, time
from pathlib import Path
report, output = map(Path, sys.argv[1:3])
deadline = time.monotonic() + 15
while not report.exists() and time.monotonic() < deadline:
    time.sleep(.05)
text = report.read_text()
value = json.loads(text)
value['launch_to_desk_ms'] = (time.monotonic() - float(sys.argv[5])) * 1000
text = json.dumps(value, sort_keys=True)
output.write_text(text)
assert value['rows'] == int(sys.argv[3]), value
assert 0 < value['desk_ms'] <= value['launch_to_desk_ms'] < float(sys.argv[4]), value
print(text)
PY
  xcrun simctl terminate "$sim" com.trommi.ios
done
