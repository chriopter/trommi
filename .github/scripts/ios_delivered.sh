#!/bin/sh
# The iOS inputs (.github/scripts/inputs.sh, line `ios`) of the app last delivered to TestFlight from main, or nothing
# when that is not known. ship-apple.yml delivers a commit whose iOS inputs differ from these: a
# delivery that did not happen (replaced while it waited, or failed) is made by the next run.
#
# Last delivered: the commit of the newest finished run of ship-apple.yml on main (a push) in this repository whose job
# "TestFlight (processed, groups, What to Test, review)" ended in success: the build is processed by Apple and in its
# groups. Before the first such run: the same of ios.yml (the workflow before; its run name "ios after build <run> of
# <commit>" names the commit). Only the newest 50 runs of each are looked at; none found: nothing is printed, and the
# next run delivers.
#
#   GH_TOKEN (actions: read), GITHUB_REPOSITORY; a checkout of this repository (the commit is fetched when missing)
set -u
here=$(cd "$(dirname "$0")" && pwd)
repo=$GITHUB_REPOSITORY
# runs: `<id> <commit> <the job that says delivered>` of a workflow
runs() {
  event="event=$2&"
  [ "$2" != all ] || event=
  status="status=completed&"
  [ "$1" != main.yml ] || status=
  gh api "repos/$repo/actions/workflows/$1/runs?${event}${status}per_page=50" \
    --jq '.workflow_runs[] | select(.head_branch == "main" and .head_repository.full_name == "'"$repo"'" and .repository.full_name == "'"$repo"'")
      | "\(.id) \((try (.display_title | capture(" of (?<c>[0-9a-f]{40})$").c) catch null) // .head_sha) '"$3"'"' 2>/dev/null
}
list=$( { runs main.yml all 'apple / ios / Publish and deliver'
          runs ship-apple.yml push 'TestFlight (processed, groups, What to Test, review)'
          runs ios.yml workflow_run 'iOS / TestFlight (processed, groups, What to Test, review)'; } ) \
  || { echo "ios_delivered.sh: the runs could not be listed" >&2; exit 0; }
commit=
while read -r run sha job; do
  [ -n "$run" ] || continue
  if gh api "repos/$repo/actions/runs/$run/jobs?filter=latest&per_page=100" \
    --jq '.jobs[] | select(.name == "'"$job"'" and .conclusion == "success") | .name' 2>/dev/null | grep -q .; then
    commit=$sha
    break
  fi
done <<EOF
$list
EOF
printf '%s' "$commit" | grep -Eq '^[0-9a-f]{40}$' || { echo "ios_delivered.sh: no delivery to TestFlight found" >&2; exit 0; }
git -C "$here" cat-file -e "$commit^{commit}" 2>/dev/null \
  || git -C "$here" fetch -q --no-tags origin "$commit" 2>/dev/null \
  || { echo "ios_delivered.sh: $commit could not be fetched" >&2; exit 0; }
sum=$(sh "$here/inputs.sh" "$commit" | sed -n 's/^ios //p')
printf '%s' "$sum" | grep -Eq '^[0-9a-f]{64}$' || { echo "ios_delivered.sh: no iOS inputs for $commit" >&2; exit 0; }
echo "ios_delivered.sh: last delivered to TestFlight: $commit (run $run), iOS inputs $sum" >&2
echo "$sum"
