#!/bin/sh
# After a delivery of the web app (ship-web.yml): what app.trommi.com serves is this release, byte for byte, and the
# page can load. Asked from outside, as a browser asks:
#   - the start page (/) answers 200 as text/html with its security headers (Strict-Transport-Security,
#     Content-Security-Policy, Cross-Origin-Opener-Policy) and is the release's public/index.html (SHA-256)
#   - every script and module the start page names under /gen/ answers 200 and is the release's file (SHA-256)
#   - /gen/build.txt names this release's commit
# Tried up to 12 times, 10 s apart (a rollout takes a moment: for up to a minute Cloudflare's edges answer from the old
# and the new version by turns, so the start page and the files it names may come from different ones; three tries
# in 20 s failed the runs of 2e76c89 and eac593d, #224). Exit 0: well; else the reason on standard output, exit 1.
#
#   .github/scripts/web_smoke.sh <release dir (manifest.json)> <commit> [<origin, default https://app.trommi.com>]
set -u
dir=${1:?usage: web_smoke.sh <release dir> <commit> [origin]}
commit=${2:?usage: web_smoke.sh <release dir> <commit> [origin]}
origin=${3:-https://app.trommi.com}
work=$(mktemp -d)
trap 'rc=$?; rm -rf "$work"; exit "$rc"' EXIT
bust="ci=${GITHUB_RUN_ID:-0}-smoke"
# the release's SHA-256 of a file the site serves at <path> (public/<path> in the manifest)
want() { jq -r --arg n "public/$1" '.files[] | select(.name == $n) | .sha256' "$dir/manifest.json"; }
check() {
  # the start page
  code=$(curl -sS --max-time 20 -D "$work/headers" -o "$work/index.html" -w '%{http_code}' "$origin/?$bust") || { echo "$origin/ did not answer"; return 1; }
  [ "$code" = 200 ] || { echo "$origin/ answers $code"; return 1; }
  tr -d '\r' < "$work/headers" | tr '[:upper:]' '[:lower:]' > "$work/h"
  grep -q '^content-type: text/html' "$work/h" || { echo "$origin/ is not text/html"; return 1; }
  for header in strict-transport-security content-security-policy cross-origin-opener-policy; do
    grep -q "^$header: " "$work/h" || { echo "$origin/ sends no $header"; return 1; }
  done
  sum=$(sha256sum "$work/index.html" | cut -d' ' -f1)
  [ "$sum" = "$(want index.html)" ] || { echo "$origin/ is not this release's index.html (SHA-256 $sum)"; return 1; }
  # what the start page loads from /gen/
  grep -oE '(src|href)="/gen/[^"?]+' "$work/index.html" | sed 's/^[a-z]*="\///' | sort -u > "$work/names"
  [ -s "$work/names" ] || { echo "the start page names nothing under /gen/"; return 1; }
  while read -r name; do
    expected=$(want "$name")
    [ -n "$expected" ] || { echo "the start page names /$name, which the release does not hold"; return 1; }
    code=$(curl -sS --max-time 30 -o "$work/file" -w '%{http_code}' "$origin/$name?$bust") || { echo "/$name did not answer"; return 1; }
    [ "$code" = 200 ] || { echo "/$name answers $code"; return 1; }
    [ "$(sha256sum "$work/file" | cut -d' ' -f1)" = "$expected" ] || { echo "/$name is not this release's file"; return 1; }
  done < "$work/names"
  served=$(curl -fsS --max-time 10 "$origin/gen/build.txt?$bust" | sed -n 's/^commit: //p' | head -n 1) || served=
  [ "$served" = "$(printf '%s' "$commit" | cut -c1-7)" ] || { echo "/gen/build.txt names ${served:-nothing}, not $commit"; return 1; }
  echo "$origin serves this release: the start page and $(wc -l < "$work/names" | tr -d ' ') files it loads, byte for byte" >&2
}
for try in 1 2 3 4 5 6 7 8 9 10 11 12; do
  if reason=$(check); then exit 0; fi
  echo "try $try: $reason" >&2
  [ "$try" = 12 ] || sleep 10
done
echo "$reason"
exit 1
