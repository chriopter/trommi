#!/bin/sh
# Before a part is signed or delivered (its own pipeline, on main): the checks of OTHER pipelines it rests on passed
# for this very commit, e.g. "Core (check)" of core.yml. Waits while they run (up to GATE_MINUTES, default 20).
#
#   .github/scripts/gate.sh <workflow file>=<check name>...      e.g. 'core.yml=Core (check)'
#
# A check counts when its newest check run on $GITHUB_SHA comes from GitHub Actions, from a run of that workflow file
# of this repository on main (a push or a run by hand), and ended in success or was skipped (a skipped check job: its
# part did not change since the last green run, or passed in the merge queue for this very tree). Anything else
# (failed, cancelled, timed out, not there in time, from another workflow): exit 1, nothing is signed or delivered.
#
#   GH_TOKEN (checks: read, actions: read), GITHUB_REPOSITORY, GITHUB_SHA
set -u
repo=$GITHUB_REPOSITORY
minutes=${GATE_MINUTES:-20}
[ $# -gt 0 ] || { echo "usage: gate.sh <workflow file>=<check name>..." >&2; exit 2; }
deadline=$(( $(date +%s) + minutes * 60 ))
for pair in "$@"; do
  file=${pair%%=*} name=${pair#*=}
  while :; do
    # the newest check run of that name on this commit, by GitHub Actions
    line=$(gh api -X GET "repos/$repo/commits/$GITHUB_SHA/check-runs" -f check_name="$name" -f filter=latest -f per_page=100 \
      --jq '[.check_runs[] | select(.app.slug == "github-actions")] | sort_by(.started_at) | last | "\(.status) \(.conclusion) \(.details_url)"' 2>/dev/null) || line=
    status=${line%% *} rest=${line#* } conclusion=${rest%% *} url=${rest#* }
    if [ "$status" = completed ]; then
      run=$(printf '%s' "$url" | sed -n 's|.*/actions/runs/\([0-9][0-9]*\)/job/.*|\1|p')
      origin=$(gh api "repos/$repo/actions/runs/$run" \
        --jq 'select(.repository.full_name == "'"$repo"'" and .head_branch == "main" and (.event == "push" or .event == "workflow_dispatch")) | .path' 2>/dev/null) || origin=
      [ "$origin" = ".github/workflows/$file" ] || { echo "::error::\"$name\" on $GITHUB_SHA is not of $file on main (${origin:-unknown})"; exit 1; }
      case "$conclusion" in
        success|skipped) echo "\"$name\" ($file) on $GITHUB_SHA: $conclusion"; break ;;
        *) echo "::error::\"$name\" ($file) on $GITHUB_SHA ended $conclusion: nothing is signed or delivered"; exit 1 ;;
      esac
    fi
    [ "$(date +%s)" -lt "$deadline" ] || { echo "::error::\"$name\" ($file) on $GITHUB_SHA did not finish within $minutes minutes (${status:-not there}): nothing is signed or delivered"; exit 1; }
    echo "waiting for \"$name\" ($file) on $GITHUB_SHA: ${status:-not there yet}"
    sleep 20
  done
done
