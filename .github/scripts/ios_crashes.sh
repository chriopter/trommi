#!/bin/bash
# The TestFlight crash reports of the iOS app, read through the App Store Connect API; nightly.yml (job ios_crashes) runs it under
# `op run`, which is where the App Store Connect key comes from. Read only: nothing on App Store Connect is changed.
#
#   ios_crashes.sh      asc.py crashes into CRASH_OUT for the last CRASH_DAYS days: each crash log as a file, the
#                       diagnostics of the newest builds, summary.md and issue.md (the same, no tester data);
#                       writes crashes=N to GITHUB_OUTPUT
#
# Environment, from the 1Password Environment: APPLE_ASC_KEY (the .p8; it may arrive with its line breaks turned into
# spaces), APPLE_ASC_KEY_ID, APPLE_ASC_ISSUER_ID. From the workflow: RUNNER_TEMP, CRASH_OUT (the folder written),
# CRASH_DAYS (1 to 90). Nothing here prints a value of the first three. The key lies in one file under RUNNER_TEMP,
# mode 600, removed when the script ends (and once more by the workflow, whatever happened).
set -euo pipefail

for name in APPLE_ASC_KEY APPLE_ASC_KEY_ID APPLE_ASC_ISSUER_ID RUNNER_TEMP CRASH_OUT CRASH_DAYS; do
  [ -n "${!name:-}" ] || { echo "::error::$name is not set for this run"; exit 1; }
done
printf '%s' "$CRASH_DAYS" | grep -Eq '^([1-9]|[1-8][0-9]|90)$' || { echo "::error::CRASH_DAYS is a number of days from 1 to 90, not $CRASH_DAYS"; exit 1; }

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
BUNDLE_ID=com.trommi.ios
export BUNDLE_ID

# (a copy of ios_testflight.sh's pem(): a key with its line breaks turned into spaces is put back into PEM form)
pem() {
  body=$(printf '%s' "$1" | sed -e 's/-----[A-Z ]*-----//g' | tr -d ' \n\r\t')
  printf -- '-----BEGIN PRIVATE KEY-----\n%s\n-----END PRIVATE KEY-----\n' "$(printf '%s' "$body" | fold -w 64)"
}

keys=$RUNNER_TEMP/asc
cleanup() { rm -rf "$keys"; }
trap cleanup EXIT
umask 077
mkdir -p "$keys"
key=$keys/AuthKey.p8
pem "$APPLE_ASC_KEY" > "$key"
chmod 600 "$key"
openssl pkey -in "$key" -noout 2>/dev/null || { echo "::error::APPLE_ASC_KEY is not readable as a private key"; exit 1; }
umask 022
export ASC_KEY_ID=$APPLE_ASC_KEY_ID ASC_ISSUER_ID=$APPLE_ASC_ISSUER_ID ASC_KEY_PATH=$key

since=$(date -u -d "-$CRASH_DAYS days" +%Y-%m-%dT%H:%M:%SZ)
mkdir -p "$CRASH_OUT"
python3 "$repo/app/ios/TrommiApp/AppStore/asc.py" crashes "$CRASH_OUT" "$since"
