#!/bin/sh
# One GitHub issue per failing delivery of a part (ship-hub.yml, ship-web.yml, job `report`), and per failing
# build of a part on main (ship-connector.yml, ship-apple.yml, job `report`: WHAT=build LABEL=build-failed), and per failing night of the full-size
# nightly tests (nightly.yml: Nightly, WHAT=tests LABEL=nightly-failed):
#   .github/scripts/deploy_issue.sh <Hub|Web|Main>
# With RESULT=failure: opens "<Part> <WHAT> <TAG> failed" with the label LABEL (the reason, the run, what runs now),
# or comments on the open issue of that part with that label. With RESULT=success: closes that issue with a
# comment. WHAT defaults to update, LABEL to deploy-failed. Reads TAG, RESULT, REASON, RUNNING (may be empty),
# RUN_URL, HEALTH_URL (may be empty: an address that names the commit served, as JSON "commit" or a line
# "commit: ..."); gh needs GH_TOKEN and GH_REPO.
set -eu
part=${1:?usage: $0 <Hub|Web|Main>}
what=${WHAT:-update}
label=${LABEL:-deploy-failed}
prefix="$part $what "
# what runs now, as the part itself says it (the hub's /healthz names its commit)
now=${RUNNING:-}
if [ -n "${HEALTH_URL:-}" ]; then
  page=$(curl -fsS --max-time 10 "$HEALTH_URL" 2>/dev/null) || page=''
  commit=$(printf '%s' "$page" | jq -r '.commit // empty' 2>/dev/null) || commit=''
  [ -n "$commit" ] || commit=$(printf '%s\n' "$page" | sed -n 's/^commit: //p' | head -n 1)
  commit=$(printf '%s' "$commit" | tr -cd '0-9a-f')
  now="${now:+$now, }${commit:+commit $commit}"
fi
[ -n "$now" ] || now="unknown (no answer)"
open=$(gh issue list --state open --label "$label" --limit 50 --json number,title \
  --jq "[.[] | select(.title | startswith(\"$prefix\"))][0].number // empty" 2>/dev/null) || open=''
if [ "$RESULT" = success ]; then
  [ -n "$open" ] || { echo "no open issue: $prefix$TAG has nothing to close"; exit 0; }
  gh issue close "$open" --comment "$prefix$TAG went through ($RUN_URL). Runs now: $now."
  echo "closed #$open"
  exit 0
fi
body=$(printf '%s\n\n| | |\n|---|---|\n| release | `%s` |\n| reason | %s |\n| runs now | %s |\n| run | %s |\n' \
  "$prefix$TAG did not go through." "$TAG" "${REASON:-see the run}" "$now" "$RUN_URL")
if [ -n "$open" ]; then
  gh issue comment "$open" --body "$body"
  echo "commented on #$open"
else
  gh label create "$label" --color B60205 --description "A delivery to production or a build of main failed" 2>/dev/null || true
  gh issue create --title "$prefix$TAG failed" --label "$label" --body "$body"
fi
