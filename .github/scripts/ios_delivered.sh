#!/bin/sh
# The iOS inputs (.github/scripts/inputs.sh, line `ios`) of the app last delivered to TestFlight from main, or nothing
# when that is not known. ios.yml and ios_release_gate.sh deliver a commit whose iOS inputs differ from these: a
# delivery that did not happen (replaced while it waited, or failed) is made by the next run.
#
# Last delivered: the build commit of the newest finished run of ios.yml after a build of main (workflow_run) in this
# repository whose job "iOS / TestFlight (processed, groups, What to Test, review)" ended in success: the build is
# processed by Apple and in its groups. The build commit is the one the run's name says ("ios after build <run> of
# <commit>", ios.yml's run-name); runs from before that name: the run's head commit. Only the newest 50 such runs are
# looked at; none found: nothing is printed, and the next run delivers.
#
#   GH_TOKEN (actions: read), GITHUB_REPOSITORY; a checkout of this repository (the commit is fetched when missing)
set -u
here=$(cd "$(dirname "$0")" && pwd)
repo=$GITHUB_REPOSITORY
runs=$(gh api "repos/$repo/actions/workflows/ios.yml/runs?event=workflow_run&status=completed&per_page=50" \
  --jq '.workflow_runs[] | select(.head_branch == "main" and .head_repository.full_name == "'"$repo"'" and .repository.full_name == "'"$repo"'")
    | "\(.id) \((.display_title | capture(" of (?<c>[0-9a-f]{40})$").c) // .head_sha)"') \
  || { echo "ios_delivered.sh: the runs of ios.yml could not be listed" >&2; exit 0; }
commit=
while read -r run sha; do
  [ -n "$run" ] || continue
  if gh api "repos/$repo/actions/runs/$run/jobs?filter=latest&per_page=100" \
    --jq '.jobs[] | select(.name == "iOS / TestFlight (processed, groups, What to Test, review)" and .conclusion == "success") | .name' 2>/dev/null | grep -q .; then
    commit=$sha
    break
  fi
done <<EOF
$runs
EOF
printf '%s' "$commit" | grep -Eq '^[0-9a-f]{40}$' || { echo "ios_delivered.sh: no delivery to TestFlight found" >&2; exit 0; }
git -C "$here" cat-file -e "$commit^{commit}" 2>/dev/null \
  || git -C "$here" fetch -q --no-tags origin "$commit" 2>/dev/null \
  || { echo "ios_delivered.sh: $commit could not be fetched" >&2; exit 0; }
sum=$(sh "$here/inputs.sh" "$commit" | sed -n 's/^ios //p')
printf '%s' "$sum" | grep -Eq '^[0-9a-f]{64}$' || { echo "ios_delivered.sh: no iOS inputs for $commit" >&2; exit 0; }
echo "ios_delivered.sh: last delivered to TestFlight: $commit (run $run), iOS inputs $sum" >&2
echo "$sum"
