#!/bin/bash
# Whether the iPhone and Mac apps of a commit may go to TestFlight (ship-apple.yml decides with it, ios_testflight.sh and
# mac_testflight.sh check it again on the Mac right before the upload):
#   ios_release_gate.sh decide         prints deliver=true|false (for GITHUB_OUTPUT); exit 1 when the gate does not hold
#   ios_release_gate.sh wait|verify    exit 0: upload; 3: already delivered; any other failure: refused.
# The gate:
#   - the run RUN_ID is ship-apple.yml of this repository on main (a push), of COMMIT
#   - its job "iOS client (check)" passed, and "Core (check)" passed or was skipped (the core did not
#     change): the apps build on the core
#   - the iOS inputs of COMMIT (.github/scripts/inputs.sh) differ from those last delivered to TestFlight
#     (.github/scripts/ios_delivered.sh; not known: they differ). SAME_OK=1 (a delivery by hand) skips this one.
# TF_DESTINATION=mac checks the independently delivered Mac TestFlight inputs (default: ios).
# Environment: GH_TOKEN (actions: read), GITHUB_REPOSITORY, RUN_ID, COMMIT, RUNNER_TEMP; a checkout of COMMIT.
set -euo pipefail

mode=${1:-}
case "$mode" in decide|wait|verify) ;; *) echo "usage: ios_release_gate.sh decide|wait|verify" >&2; exit 2 ;; esac
for name in GH_TOKEN GITHUB_REPOSITORY RUN_ID COMMIT RUNNER_TEMP; do
  [ -n "${!name:-}" ] || { echo "::error::$name is not set"; exit 1; }
done
printf '%s' "$RUN_ID" | grep -Eq '^[1-9][0-9]{0,19}$' || { echo "::error::not a run id: $RUN_ID"; exit 1; }
printf '%s' "$COMMIT" | grep -Eq '^[0-9a-f]{40}$' || { echo "::error::not a commit: $COMMIT"; exit 1; }
here=$(cd "$(dirname "$0")" && pwd)

gh api "repos/$GITHUB_REPOSITORY/actions/runs/$RUN_ID" > "$RUNNER_TEMP/gate-run.json"
if [ "$(jq -r .path "$RUNNER_TEMP/gate-run.json")" = .github/workflows/main.yml ]; then
  bash "$here/main_release_gate.sh" apple
else
jq -e --arg r "$GITHUB_REPOSITORY" --arg c "$COMMIT" \
  '.path == ".github/workflows/ship-apple.yml" and .head_branch == "main" and .event == "push"
   and .repository.full_name == $r and .head_repository.full_name == $r and .head_sha == $c' \
  "$RUNNER_TEMP/gate-run.json" > /dev/null || { echo "::error::run $RUN_ID is not a run of ship-apple.yml for $COMMIT on main of this repository: nothing is uploaded"; exit 1; }
gh api --paginate "repos/$GITHUB_REPOSITORY/actions/runs/$RUN_ID/jobs?filter=latest&per_page=100" \
  --jq '.jobs[] | "\(.name)\t\(.conclusion)"' > "$RUNNER_TEMP/gate-jobs.txt"
conclusion() { awk -F'\t' -v n="$1" '$1 == n { print $2; exit }' "$RUNNER_TEMP/gate-jobs.txt"; }
client=$(conclusion 'iOS client (check)') core=$(conclusion 'Core (check)')
[ "$client" = success ] || { echo "::error::iOS client (check) of run $RUN_ID: ${client:-not there}: nothing is uploaded"; exit 1; }
case "$core" in
  success|skipped) echo "the checks of run $RUN_ID: iOS client $client, core $core" >&2 ;;
  *) echo "::error::Core (check) of run $RUN_ID: ${core:-not there}: nothing is uploaded"; exit 1 ;;
esac

fi

sum=$(sh "$here/inputs.sh" "$COMMIT" | sed -n 's/^ios //p')
[ -n "$sum" ] || { echo "::error::no iOS inputs for $COMMIT"; exit 1; }
delivered=$(sh "$here/ios_delivered.sh" "${TF_DESTINATION:-ios}") || delivered=
if [ -n "$delivered" ] && [ "$delivered" = "$sum" ]; then
  if [ "$mode" = decide ]; then
    echo "deliver=false"
    exit 0
  fi
  if [ "${SAME_OK:-}" != 1 ]; then
    echo "::notice::TestFlight already has these ${TF_DESTINATION:-ios} inputs ($sum): nothing is uploaded"
    exit 3
  fi
fi
[ "$mode" != decide ] || echo "deliver=true"
echo "iOS inputs of $COMMIT: $sum, last delivered: ${delivered:-not known}: the upload may go" >&2
