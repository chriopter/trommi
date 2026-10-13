#!/bin/sh
# Main only (ship-web.yml, ship-hub.yml, ship-connector.yml, job `decide`): which parts must be delivered, decided against what each part has out there,
# not against the release of every part. Prints `web=`, `hub=`, `connector=` lines (true or false).
#
#   .github/scripts/deployed.sh <inputs> [web|hub|connector]...       (default: all three)
#   <inputs>: lines `<part> <sha256>` of this commit (.github/scripts/inputs.sh)
#
# A part is delivered when what it has out there was built from other inputs than this commit's:
#   web        the commit app.trommi.com names (/gen/build.txt), its web inputs (inputs.sh of that commit)
#   hub        the commit of the newest delivery to the server that went through (GitHub's deployments of the
#              environment `hub`: ship-hub.yml fails when the hub or its updater was not taken), its hub and updater
#              inputs; and the commit hub.trommi.com runs (/healthz), its hub inputs (an updater-only delivery leaves
#              the hub's commit as it was, so /healthz alone cannot say whether the updater is out there)
#   connector  the newest connector-v<N> (its signature checked; before the first one: the newest v<N>), its connector
#              inputs (a published connector release is out there: installers and updates take it at once)
# So a part that went back (a revert) is delivered again, and a delivery that failed is tried again by the next run. Anything not known (no answer, a commit this
# repository does not know, a manifest whose signature does not hold): the part is delivered.
#
#   GH_TOKEN, GITHUB_REPOSITORY; a checkout with the whole history (fetch-depth 0)
set -u
inputs=${1:?usage: deployed.sh <inputs> [web|hub|connector]...}
shift
[ $# -gt 0 ] || set -- web hub connector
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
work=$(mktemp -d)
trap 'rc=$?; rm -rf "$work"; exit "$rc"' EXIT
want() { sed -n "s/^$1 \([0-9a-f]\{64\}\)\$/\1/p" "$inputs"; }
say() { echo "deployed.sh: $*" >&2; }
# the inputs of one part at a commit named out there (short or full), or nothing
inputs_at() {
  printf '%s' "$2" | grep -Eq '^[0-9a-f]{7,40}$' || return 0
  full=$(git -C "$repo" rev-parse --verify -q "$2^{commit}" 2>/dev/null) || return 0
  sh "$here/inputs.sh" "$full" | sed -n "s/^$1 \([0-9a-f]\{64\}\)\$/\1/p"
}
# the manifest of the newest release of the series v<N> and <prefix>-v<N> (the higher number), signature checked;
# its path, or nothing
newest_manifest() {
  tag=$(gh release list --repo "$GITHUB_REPOSITORY" --exclude-drafts --limit 1000 --json tagName --jq '.[].tagName' 2>/dev/null \
    | sed -n "s/^\($1-\)\{0,1\}v\([1-9][0-9]*\)\$/\2 &/p" | sort -k1,1n -k2,2 | tail -n 1 | cut -d' ' -f2)
  [ -n "$tag" ] || return 0
  dir=$work/$tag
  mkdir -p "$dir"
  gh release download "$tag" --repo "$GITHUB_REPOSITORY" --pattern manifest.json --pattern manifest.json.sig --dir "$dir" --clobber 2>/dev/null || return 0
  sh "$repo/release/sign.sh" verify "$dir/manifest.json" > /dev/null 2>&1 || { say "the manifest of $tag is not signed by the release key"; return 0; }
  jq -e --arg r "$GITHUB_REPOSITORY" --arg t "$tag" '.repository == $r and .tag == $t' "$dir/manifest.json" > /dev/null || return 0
  say "newest release that holds the $1: $tag"
  echo "$dir/manifest.json"
}
decide() {
  if [ -n "$2" ] && [ "$2" = "$3" ]; then echo "$1=false"; else echo "$1=true"; say "$1: out there ${2:-unknown}, this commit $3: delivered"; fi
}

for part in "$@"; do
case $part in
web)
served=$(curl -fsS --max-time 10 "https://app.trommi.com/gen/build.txt?ci=${GITHUB_RUN_ID:-0}-deployed" 2>/dev/null | sed -n 's/^commit: //p' | head -n 1) || served=
say "app.trommi.com serves ${served:-nothing known}"
decide web "$(inputs_at web "$served")" "$(want web)"
;;
hub)
# the newest delivery to the server that went through, and the hub that runs
delivered=
for id in $(gh api "repos/$GITHUB_REPOSITORY/deployments?environment=hub&per_page=30" --jq '.[].id' 2>/dev/null); do
  state=$(gh api "repos/$GITHUB_REPOSITORY/deployments/$id/statuses?per_page=1" --jq '.[0].state // ""' 2>/dev/null) || state=
  case "$state" in
    success) delivered=$(gh api "repos/$GITHUB_REPOSITORY/deployments/$id" --jq .sha 2>/dev/null) || delivered=; break ;;
  esac
done
running=$(curl -fsS --max-time 10 https://hub.trommi.com/healthz 2>/dev/null | jq -r '.commit // ""' 2>/dev/null) || running=
say "the newest delivery to the hub's server that went through: ${delivered:-none known}; hub.trommi.com runs ${running:-nothing known}"
hub_out=$(inputs_at hub "$delivered") updater_out=$(inputs_at updater "$delivered") hub_runs=$(inputs_at hub "$running")
if [ -n "$hub_out" ] && [ "$hub_out" = "$(want hub)" ] && [ "$hub_runs" = "$(want hub)" ] && [ -n "$updater_out" ] && [ "$updater_out" = "$(want updater)" ]; then
  echo "hub=false"
else
  echo "hub=true"
  say "hub: delivered ${hub_out:-unknown}, runs ${hub_runs:-unknown} (this commit $(want hub)); updater delivered ${updater_out:-unknown} (this commit $(want updater)): delivered"
fi
;;
connector)
manifest=$(newest_manifest connector)
connector_out=
[ -z "$manifest" ] || connector_out=$(jq -r '.inputs.connector // ""' "$manifest")
decide connector "$connector_out" "$(want connector)"
;;
*) echo "deployed.sh: not a part: $part" >&2; exit 2 ;;
esac
done
