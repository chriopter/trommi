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
script_dir=$(cd "$(dirname "$0")" && pwd)
for iteration in 1 2 3; do
  folder=$(xcrun simctl get_app_container "$sim" com.trommi.ios data)
  python3 "$script_dir/startup_report.py" "$sim" "$fixture_path" "$folder/Documents/native-desk-startup.json" \
    "$evidence/startup-$iteration.json" "$evidence/launch-$iteration.log" "$expected_rows" "${DESK_BUDGET_MS:-2000}"
done
