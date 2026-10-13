#!/bin/sh
# The iOS inputs (.github/scripts/inputs.sh, line `ios`) of the app last delivered to TestFlight from main, or nothing
# when that is not known. ios.yml and ios_release_gate.sh deliver a release whose iOS inputs differ from these even when
# the release itself does not mark iOS changed: its iOS change came with an earlier release whose delivery did not
# happen (replaced while it waited, or failed), and a later release must not leave it undelivered.
#
# Last delivered: the commit of the newest finished run of ios.yml after a build of main (workflow_run) in this
# repository whose job "iOS / TestFlight" (deploy_ios.yml's upload of the iPhone app) ended in success. Only the newest
# 50 such runs are looked at; none found: nothing is printed, and only a release that marks iOS changed delivers.
#
#   GH_TOKEN (actions: read), GITHUB_REPOSITORY; a checkout of this repository (the commit is fetched when missing)
set -u
here=$(cd "$(dirname "$0")" && pwd)
repo=$GITHUB_REPOSITORY
runs=$(gh api "repos/$repo/actions/workflows/ios.yml/runs?event=workflow_run&status=completed&per_page=50" \
  --jq '.workflow_runs[] | select(.head_branch == "main" and .head_repository.full_name == "'"$repo"'" and .repository.full_name == "'"$repo"'") | "\(.id) \(.head_sha)"') \
  || { echo "ios_delivered.sh: the runs of ios.yml could not be listed" >&2; exit 0; }
commit=
while read -r run sha; do
  [ -n "$run" ] || continue
  if gh api "repos/$repo/actions/runs/$run/jobs?filter=latest&per_page=100" \
    --jq '.jobs[] | select(.name == "iOS / TestFlight" and .conclusion == "success") | .name' 2>/dev/null | grep -q .; then
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
