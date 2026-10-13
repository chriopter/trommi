#!/bin/sh
# Expand, then contract (hub/deploy/README.md, "Migrations"): every hub the updater may put back after this build's
# hub has served must still start and answer /healthz on a database this build's hub has migrated. Otherwise putting
# the program back after a failed deploy would leave a hub that cannot run.
#
#   .github/scripts/hub_compat.sh <this build's hub binary> <target>      (GH_TOKEN for gh; or OLD_HUB=<binary>)
#
# 1. The hubs that may be put back: the one the server runs now (hub.trommi.com/healthz names its commit; the newest
#    release of the series hub-v<N> or v<N> made from that commit) and the newest hub released (hub-v<N> or v<N>),
#    when that is another one. Each: its manifest's signature checked against release/public-key.pem, the binary
#    against the manifest. OLD_HUB names one binary instead, for a run by hand.
# 2. hub/data-epoch lower than a predecessor's: an error (an epoch never goes down). Higher: this build declares a
#    migration that hub cannot run on; not checked for it, and the updater never puts it back once the new hub served.
# 3. Otherwise per predecessor: it makes a database, this build's hub migrates it, it starts on it again and must
#    answer /healthz with 200. Its last lines are shown when it does not.
# Fails closed: a predecessor that cannot be found or checked is an error, not a pass.
set -eu
new=${1:?usage: $0 <hub binary> <target>}
target=${2:?usage: $0 <hub binary> <target>}
repository=${GITHUB_REPOSITORY:-trommi/trommi}
here=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d)
trap 'rc=$?; kill "$pid" 2>/dev/null || true; rm -rf "$work"; exit "$rc"' EXIT
pid=''
epoch=$(tr -d ' \n' < "$here/hub/data-epoch")
asset="trommi-hub-$target"

# starts a hub on the data, waits for its port and health; prints the answer of /healthz
run() {
  : > "$work/out"
  HUB_HOST=127.0.0.1 HUB_PORT=0 HUB_DATA="$data" HUB_QUIET=1 "$1" > "$work/out" 2> "$work/err" &
  pid=$!
  port=''
  for _ in $(seq 1 60); do
    port=$(sed -n '1s/.*"port":\([0-9]*\).*/\1/p' "$work/out")
    [ -n "$port" ] && break
    kill -0 "$pid" 2>/dev/null || break
    sleep 0.5
  done
  got=''
  if [ -n "$port" ]; then
    for _ in $(seq 1 40); do
      if got=$(curl -fsS --max-time 2 "http://127.0.0.1:$port/healthz"); then break; fi
      got=''
      kill -0 "$pid" 2>/dev/null || break
      sleep 0.5
    done
  fi
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  pid=''
  if [ -z "$got" ]; then
    tail -n 5 "$work/err" >&2
    return 1
  fi
  printf '%s\n' "$got"
}
# check <old binary> <its name>: the hub before on data this build migrated
check() {
  data="$work/data-$2"
  mkdir -p "$data"
  run "$1" > /dev/null || { echo "::error::the hub of $2 does not start on an empty folder"; exit 1; }
  run "$new" > /dev/null || { echo "::error::this build's hub does not start on the database of the hub of $2"; exit 1; }
  if ! run "$1" > /dev/null; then
    echo "::error::the hub of $2 does not run on a database this build migrated. Make the migration expand-only (hub/deploy/README.md, \"Migrations\"), or raise hub/data-epoch to say that the program cannot go back past this release."
    exit 1
  fi
  echo "the hub of $2 runs on data this build migrated"
}

if [ -n "${OLD_HUB:-}" ]; then
  check "$OLD_HUB" "OLD_HUB"
  exit 0
fi

# every release of the hub's two series: `<number> <tag> <commit>`, highest first
gh api --paginate "repos/$repository/releases?per_page=100" \
  --jq '.[] | select(.draft == false and (.tag_name | test("^(hub-)?v[1-9][0-9]{0,11}$"))) | "\(.tag_name) \(.target_commitish)"' > "$work/list" \
  || { echo "::error::the releases could not be listed: the hubs that may be put back are not known"; exit 1; }
sed 's/^\(hub-\)\{0,1\}v\([0-9]*\) /\2 &/' "$work/list" | sort -k1,1nr > "$work/releases"
if [ ! -s "$work/releases" ]; then
  echo "no release of the hub yet: nothing to check against"
  exit 0
fi
newest=$(head -n 1 "$work/releases" | cut -d' ' -f2)
running=$(curl -fsS --max-time 10 https://hub.trommi.com/healthz | jq -r '.commit // ""') \
  || { echo "::error::hub.trommi.com/healthz did not answer: the hub that runs is not known"; exit 1; }
printf '%s' "$running" | grep -Eq '^[0-9a-f]{40}$' || { echo "::error::hub.trommi.com/healthz names no commit"; exit 1; }
runs=$(awk -v c="$running" '$3 == c { print $2; exit }' "$work/releases")
[ -n "$runs" ] || { echo "::error::no release of the hub was made from $running, the commit hub.trommi.com runs"; exit 1; }
echo "the hub that runs: $runs ($running); the newest released: $newest"
for tag in $runs $newest; do
  [ -d "$work/$tag" ] && continue
  mkdir -p "$work/$tag"
  gh release download "$tag" --repo "$repository" --pattern manifest.json --pattern manifest.json.sig --pattern "$asset" --dir "$work/$tag"
  sh "$here/release/sign.sh" verify "$work/$tag/manifest.json"
  jq -e --arg r "$repository" --arg t "$tag" '.repository == $r and .tag == $t' "$work/$tag/manifest.json" > /dev/null \
    || { echo "::error::the manifest of $tag is not that release of this repository"; exit 1; }
  sum=$(sha256sum "$work/$tag/$asset" | cut -d' ' -f1)
  jq -e --arg n "$asset" --arg s "$sum" '[.assets[] | select(.name == $n and .sha256 == $s)] | length == 1' "$work/$tag/manifest.json" > /dev/null \
    || { echo "::error::the hub of $tag is not the file its manifest names"; exit 1; }
  old_epoch=$(jq -r '.data_epoch // 0' "$work/$tag/manifest.json")
  if [ "$epoch" -lt "$old_epoch" ]; then
    echo "::error::hub/data-epoch is $epoch, lower than $old_epoch of $tag: an epoch never goes down"
    exit 1
  fi
  if [ "$epoch" -gt "$old_epoch" ]; then
    echo "::notice::hub/data-epoch is $epoch, $tag has $old_epoch: this build declares a migration the hub of $tag cannot run on. Not checked; the updater will not put it back past this release once the new hub has served."
    continue
  fi
  chmod +x "$work/$tag/$asset"
  check "$work/$tag/$asset" "$tag"
done
