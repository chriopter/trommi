#!/bin/bash
# On main the iOS app is archived while the release of the same build run is made (build.yml starts deploy_ios.yml
# without waiting for it); only the upload waits here. Only a released app ships:
#   ios_release_gate.sh wait     waits until the run's job "Release / Publish the release" is done, then `verify`
#   ios_release_gate.sh verify   the release v$VERSION is published, its manifest's signature holds
#                                (release/public-key.pem), it is v$VERSION of $COMMIT in this repository, and its iOS
#                                inputs are this commit's (.github/scripts/inputs.sh) and marked changed
# Exit 0: upload. Anything else: nothing is uploaded (a release that failed, was skipped or refused its tag).
# Environment: GH_TOKEN (actions: read, contents: read), GITHUB_REPOSITORY, RUN_ID (the build run; wait only),
# VERSION, COMMIT, RUNNER_TEMP. Needs gh, jq, openssl and a checkout of COMMIT (release/sign.sh, inputs.sh).
set -euo pipefail

mode=${1:-}
case "$mode" in wait|verify) ;; *) echo "usage: ios_release_gate.sh wait|verify" >&2; exit 2 ;; esac
for name in GH_TOKEN GITHUB_REPOSITORY VERSION COMMIT RUNNER_TEMP; do
  [ -n "${!name:-}" ] || { echo "::error::$name is not set"; exit 1; }
done
printf '%s' "$VERSION" | grep -Eq '^[1-9][0-9]{0,11}$' || { echo "::error::not a release number: $VERSION"; exit 1; }
printf '%s' "$COMMIT" | grep -Eq '^[0-9a-f]{40}$' || { echo "::error::not a commit: $COMMIT"; exit 1; }
# release/sign.sh checks Ed25519 with `openssl pkeyutl -rawin`: OpenSSL 3. macOS's own openssl is LibreSSL, so on a
# Mac the Homebrew OpenSSL of the runner image goes first.
for dir in /opt/homebrew/opt/openssl@3/bin /usr/local/opt/openssl@3/bin; do
  if [ -x "$dir/openssl" ]; then PATH=$dir:$PATH; break; fi
done
openssl version | grep -q '^OpenSSL 3' || { echo "::error::release/sign.sh needs OpenSSL 3, this is $(openssl version)"; exit 1; }
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)

if [ "$mode" = wait ]; then
  [ -n "${RUN_ID:-}" ] || { echo "::error::RUN_ID is not set"; exit 1; }
  # The release's jobs show up when it starts: "Release" alone when it is skipped, "Release / <job>" when it runs.
  # 90 rounds of 20 s: the release waits for every part's build (the connector's is the longest, about 15 minutes).
  for i in $(seq 1 90); do
    gh api --paginate "repos/$GITHUB_REPOSITORY/actions/runs/$RUN_ID/jobs?per_page=100" \
      --jq '.jobs[] | select(.name == "Release" or (.name | startswith("Release / "))) | "\(.name)\t\(.status)\t\(.conclusion)"' \
      > "$RUNNER_TEMP/release-jobs.txt"
    if awk -F'\t' '$2 == "completed" && $3 != "success" && $3 != "skipped"' "$RUNNER_TEMP/release-jobs.txt" | grep -q .; then
      cat "$RUNNER_TEMP/release-jobs.txt"
      echo "::error::the release of this run did not succeed: nothing is uploaded"; exit 1
    fi
    if grep -Eq $'^Release\tcompleted\tskipped$' "$RUNNER_TEMP/release-jobs.txt"; then
      echo "::error::the release of this run was skipped: nothing is uploaded"; exit 1
    fi
    if grep -Eq $'^Release / Publish the release\tcompleted\tsuccess$' "$RUNNER_TEMP/release-jobs.txt"; then
      echo "the release job is done (round $i)"
      break
    fi
    [ "$i" -lt 90 ] || { echo "::error::no finished release after 30 minutes: nothing is uploaded"; exit 1; }
    echo "round $i: waiting for the release ($(cut -f1,2 "$RUNNER_TEMP/release-jobs.txt" | tr '\t\n' ' '))"
    sleep 20
  done
fi

tag=v$VERSION
dir=$RUNNER_TEMP/release-gate
rm -rf "$dir"
mkdir -p "$dir"
gh release download "$tag" --repo "$GITHUB_REPOSITORY" --pattern manifest.json --pattern manifest.json.sig --dir "$dir" \
  || { echo "::error::release $tag is not published (or has no manifest): nothing is uploaded"; exit 1; }
sh "$repo/release/sign.sh" verify "$dir/manifest.json" || { echo "::error::the manifest of $tag is not signed by the release key"; exit 1; }
jq -e --arg r "$GITHUB_REPOSITORY" --arg t "$tag" --arg v "$VERSION" --arg c "$COMMIT" \
  '.product == "trommi" and .repository == $r and .tag == $t and (.version | tostring) == $v and .commit == $c' \
  "$dir/manifest.json" > /dev/null || { echo "::error::the manifest of $tag is not $tag of $COMMIT in this repository"; exit 1; }
sum=$(sh "$here/inputs.sh" "$COMMIT" | sed -n 's/^ios //p')
[ -n "$sum" ] || { echo "::error::no iOS inputs for $COMMIT"; exit 1; }
jq -e --arg s "$sum" '.inputs.ios == $s and .changed.ios == true' "$dir/manifest.json" > /dev/null \
  || { echo "::error::$tag does not release this iOS app (inputs or changed differ): nothing is uploaded"; exit 1; }
echo "release $tag: signed, of $COMMIT, iOS inputs $sum: the upload may go"
