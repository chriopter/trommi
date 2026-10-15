#!/bin/bash
# The release side of the Mac app for direct download (README.md "Mac app (direct download)"), on Linux; ship-apple.yml runs
# it (ship-apple.yml), mac_direct.sh builds the app on a Mac. The app has a release series of its own: mac-v<N>, N the
# number of the ship-apple.yml run, marked "latest" (releases/latest/download/Trommi-macOS.dmg is always the newest).
#   mac_release.sh check           whether COMMIT builds the app. Writes to GITHUB_OUTPUT: mode (build: COMMIT's iOS
#                                  inputs (.github/scripts/inputs.sh: the Mac app is built from the same paths) differ
#                                  from those of the newest Mac app released; none: not), commit, inputs_ios. That its
#                                  checks passed, ship-apple.yml sees to (the build needs them)
#   mac_release.sh prepare <dir>   <dir> holds Trommi-macOS.dmg and its .sha256 from the Mac: checked, `check` again,
#                                  and Trommi-macOS.json written beside them (the release, the commit, the iOS inputs,
#                                  the file's SHA-256 and size)
#   mac_release.sh upload <dir>    the release mac-v$VERSION with the three files, marked latest unless a newer mac-v is
#                                  out already. A release of that name there already must hold the same bytes: a
#                                  published file is never replaced
# The .dmg is in no manifest signed with the release key: what vouches for it is Apple's Developer ID signature and
# notarization, which macOS checks itself, and the build attestation.
# Environment: GH_TOKEN, GITHUB_REPOSITORY, RUNNER_TEMP, VERSION, COMMIT; MAC_VERSION (prepare: the app's version).
# Needs gh, jq and a checkout with the whole history.
set -euo pipefail

mode=${1:-}
dir=${2:-}
case "$mode" in check) ;; prepare|upload) [ -n "$dir" ] || { echo "usage: mac_release.sh $mode <dir>" >&2; exit 2; } ;;
  *) echo "usage: mac_release.sh check|prepare|upload [<dir>]" >&2; exit 2 ;; esac
for var in GH_TOKEN GITHUB_REPOSITORY RUNNER_TEMP VERSION COMMIT; do
  [ -n "${!var:-}" ] || { echo "::error::$var is not set"; exit 1; }
done
printf '%s' "$VERSION" | grep -Eq '^[1-9][0-9]{0,11}$' || { echo "::error::not a build number: $VERSION"; exit 1; }
printf '%s' "$COMMIT" | grep -Eq '^[0-9a-f]{40}$' || { echo "::error::not a commit: $COMMIT"; exit 1; }
here=$(cd "$(dirname "$0")" && pwd)
tag=mac-v$VERSION
name=Trommi-macOS
files=("$name.dmg" "$name.dmg.sha256" "$name.json")
out() { [ -z "${GITHUB_OUTPUT:-}" ] || echo "$1=$2" >> "$GITHUB_OUTPUT"; echo "$1: $2"; }
summary() { [ -z "${GITHUB_STEP_SUMMARY:-}" ] || echo "$1" >> "$GITHUB_STEP_SUMMARY"; }

# the newest mac-v<N> release: sets newest_tag (empty: none) and newest_inputs (the iOS inputs its app was built from)
newest() {
  newest_tag=$(gh release list --repo "$GITHUB_REPOSITORY" --exclude-drafts --limit 1000 --json tagName --jq '.[].tagName' \
    | sed -n 's/^mac-v\([1-9][0-9]*\)$/\1/p' | sort -n | tail -n 1)
  newest_tag=${newest_tag:+mac-v$newest_tag}
  newest_inputs=
  [ -n "$newest_tag" ] || return 0
  local meta=$RUNNER_TEMP/mac-newest
  rm -rf "$meta"
  mkdir -p "$meta"
  gh release download "$newest_tag" --repo "$GITHUB_REPOSITORY" --pattern "$name.json" --dir "$meta"
  newest_inputs=$(jq -r '.inputs_ios // ""' "$meta/$name.json")
}

# check: sets release_mode and release_inputs
check() {
  release_mode=none
  release_inputs=$(sh "$here/inputs.sh" "$COMMIT" | sed -n 's/^ios //p')
  [ -n "$release_inputs" ] || { echo "::error::no iOS inputs for $COMMIT"; exit 1; }
  newest
  # no Mac app released yet (while the Developer ID certificate is missing, every build ends without one): the inputs
  # last delivered to TestFlight stand in, so that the Mac is not asked to build on every run of main
  if [ -z "$newest_tag" ]; then newest_inputs=$(sh "$here/ios_delivered.sh") || newest_inputs=; fi
  if [ "$newest_inputs" != "$release_inputs" ]; then release_mode=build; fi
}

upload() {
  local there=$RUNNER_TEMP/mac-release-there latest=--latest
  rm -rf "$there"
  mkdir -p "$there"
  for file in "${files[@]}"; do [ -f "$dir/$file" ] || { echo "::error::$file is missing"; exit 1; }; done
  if gh release view "$tag" --repo "$GITHUB_REPOSITORY" --json assets --jq '.assets[].name' > "$there/names" 2>/dev/null; then
    for file in "${files[@]}"; do
      gh release download "$tag" --repo "$GITHUB_REPOSITORY" --pattern "$file" --dir "$there"
      cmp -s "$there/$file" "$dir/$file" || { echo "::error::$tag holds another $file already: it stays, nothing is replaced"; exit 1; }
    done
    echo "$tag holds ${files[*]} already"
    return 0
  fi
  newest
  if [ -n "$newest_tag" ] && [ "${newest_tag#mac-v}" -gt "$VERSION" ]; then latest=--latest=false; fi
  # the notes: the "What's new" lines of the pull requests since the newest mac-v that touch the app (release_notes.mjs)
  local before=''
  if [ -n "$newest_tag" ]; then before=$(gh api "repos/$GITHUB_REPOSITORY/commits/$newest_tag" --jq .sha 2>/dev/null) || before=''; fi
  {
    GH_REPO=$GITHUB_REPOSITORY node "$here/release_notes.mjs" mac "$before" "$COMMIT" || true
    printf '\n---\n\nThe Mac app of commit %s: signed with Developer ID, notarized by Apple and stapled. Its build attestation: gh attestation verify %s.dmg --repo %s\n' "$COMMIT" "$name" "$GITHUB_REPOSITORY"
  } > "$RUNNER_TEMP/mac-notes.md"
  gh release create "$tag" --repo "$GITHUB_REPOSITORY" --target "$COMMIT" "$latest" --title "Trommi for Mac $tag" \
    --notes-file "$RUNNER_TEMP/mac-notes.md" "${files[@]/#/$dir/}"
  echo "$tag holds ${files[*]}"
}

# the three files in $dir belong together and to these iOS inputs
consistent() {
  local inputs=$1
  (cd "$dir" && sha256sum -c "$name.dmg.sha256")
  [ "$(awk '{print $2}' "$dir/$name.dmg.sha256")" = "$name.dmg" ] || { echo "::error::$name.dmg.sha256 names another file"; exit 1; }
  jq -e --arg s "$(sha256sum "$dir/$name.dmg" | cut -d' ' -f1)" --argjson z "$(wc -c < "$dir/$name.dmg" | tr -d ' ')" \
    --arg i "$inputs" --arg r "$GITHUB_REPOSITORY" \
    '.product == "trommi-macos" and (.repository == $r or ($r == "chriopter/trommi" and .repository == "trommi/trommi")) and .file == "Trommi-macOS.dmg" and .sha256 == $s and .size == $z and .inputs_ios == $i' \
    "$dir/$name.json" > /dev/null || { echo "::error::$name.json does not describe this $name.dmg of these iOS inputs"; exit 1; }
}

case "$mode" in
  check)
    check
    out mode "$release_mode"
    out commit "$COMMIT"
    out inputs_ios "$release_inputs"
    case "$release_mode" in
      none) summary "- no Mac app for $COMMIT: its app is out there already" ;;
      build) summary "- the Mac app of $COMMIT is built, notarized and released as $tag" ;;
    esac
    ;;
  prepare)
    [ -n "${MAC_VERSION:-}" ] || { echo "::error::MAC_VERSION is needed"; exit 1; }
    printf '%s' "$MAC_VERSION" | grep -Eq '^[0-9]+(\.[0-9]+)*$' || { echo "::error::not a version: $MAC_VERSION"; exit 1; }
    check
    [ "$release_mode" = build ] || { echo "::error::$COMMIT does not build the Mac app ($release_mode): nothing is released"; exit 1; }
    for file in "$name.dmg" "$name.dmg.sha256"; do [ -f "$dir/$file" ] || { echo "::error::$file is missing"; exit 1; }; done
    jq -n --arg r "$GITHUB_REPOSITORY" --arg t "$tag" --arg c "$COMMIT" --arg i "$release_inputs" --arg v "$MAC_VERSION" \
      --arg b "$VERSION" --arg s "$(sha256sum "$dir/$name.dmg" | cut -d' ' -f1)" --argjson z "$(wc -c < "$dir/$name.dmg" | tr -d ' ')" \
      '{product: "trommi-macos", repository: $r, release: $t, commit: $c, inputs_ios: $i, version: $v, build: $b,
        file: "Trommi-macOS.dmg", sha256: $s, size: $z, signed: "Developer ID Application, notarized and stapled"}' > "$dir/$name.json"
    consistent "$release_inputs"
    cat "$dir/$name.json"
    ;;
  upload)
    release_inputs=$(sh "$here/inputs.sh" "$COMMIT" | sed -n 's/^ios //p')
    consistent "$release_inputs"
    upload
    summary "- $tag: ${files[*]}"
    ;;
esac
