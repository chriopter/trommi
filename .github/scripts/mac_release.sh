#!/bin/bash
# The release side of the Mac app for direct download (README.md "Mac app (direct download)"), on Linux; mac.yml runs
# it, mac_direct.sh builds the app on a Mac.
#   mac_release.sh check           the release v$VERSION: published, its manifest signed by the release key
#                                  (release/public-key.pem), v$VERSION of this repository (and of $COMMIT when set),
#                                  its iOS inputs those of its commit (.github/scripts/inputs.sh: the Mac app is built
#                                  from the same paths). Writes to GITHUB_OUTPUT: mode (build: the release marks the
#                                  app changed; carry: it does not; none: no such release), commit, inputs_ios
#   mac_release.sh prepare <dir>   <dir> holds Trommi-macOS.dmg and its .sha256 from the Mac: checked, `check` again
#                                  (mode build, of $COMMIT), and Trommi-macOS.json written beside them (the release,
#                                  the commit, the iOS inputs, the file's SHA-256 and size)
#   mac_release.sh upload <dir>    the three files onto the release v$VERSION. A file of that name there already must
#                                  be the same bytes: a published file is never replaced
#   mac_release.sh carry <dir>     the release leaves the app unchanged: the newest earlier release's three files,
#                                  checked (their SHA-256, the same iOS inputs as this release, the GitHub build
#                                  attestation of mac.yml in this repository), go onto v$VERSION too, so that
#                                  releases/latest/download/Trommi-macOS.dmg always is the newest app
# The .dmg is not in the release's signed manifest (it is made after the release is published): what vouches for it
# is Apple's Developer ID signature and notarization, which macOS checks itself, and the build attestation.
# Environment: GH_TOKEN, GITHUB_REPOSITORY, RUNNER_TEMP, VERSION; COMMIT (prepare; check when set); MAC_VERSION
# (prepare: the app's version). Needs gh, jq, openssl 3 and a checkout with the release's commit (fetch-depth 0).
set -euo pipefail

mode=${1:-}
dir=${2:-}
case "$mode" in check) ;; prepare|upload|carry) [ -n "$dir" ] || { echo "usage: mac_release.sh $mode <dir>" >&2; exit 2; } ;;
  *) echo "usage: mac_release.sh check|prepare|upload|carry [<dir>]" >&2; exit 2 ;; esac
for var in GH_TOKEN GITHUB_REPOSITORY RUNNER_TEMP VERSION; do
  [ -n "${!var:-}" ] || { echo "::error::$var is not set"; exit 1; }
done
printf '%s' "$VERSION" | grep -Eq '^[1-9][0-9]{0,11}$' || { echo "::error::not a release number: $VERSION"; exit 1; }
[ -z "${COMMIT:-}" ] || printf '%s' "$COMMIT" | grep -Eq '^[0-9a-f]{40}$' || { echo "::error::not a commit: $COMMIT"; exit 1; }
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
tag=v$VERSION
name=Trommi-macOS
files=("$name.dmg" "$name.dmg.sha256" "$name.json")
out() { [ -z "${GITHUB_OUTPUT:-}" ] || echo "$1=$2" >> "$GITHUB_OUTPUT"; echo "$1: $2"; }
summary() { [ -z "${GITHUB_STEP_SUMMARY:-}" ] || echo "$1" >> "$GITHUB_STEP_SUMMARY"; }

# check: sets release_mode, release_commit, release_inputs
check() {
  local gate=$RUNNER_TEMP/mac-release-gate err=$RUNNER_TEMP/mac-release-gate.err
  rm -rf "$gate"
  mkdir -p "$gate"
  release_mode=none release_commit='' release_inputs=''
  if ! gh release download "$tag" --repo "$GITHUB_REPOSITORY" --pattern manifest.json --pattern manifest.json.sig --dir "$gate" 2> "$err"; then
    cat "$err" >&2
    grep -qi 'release not found' "$err" || { echo "::error::the release $tag could not be read"; exit 1; }
    return 0
  fi
  sh "$repo/release/sign.sh" verify "$gate/manifest.json" || { echo "::error::the manifest of $tag is not signed by the release key"; exit 1; }
  jq -e --arg r "$GITHUB_REPOSITORY" --arg t "$tag" --arg v "$VERSION" \
    '.product == "trommi" and .repository == $r and .tag == $t and (.version | tostring) == $v and (.commit | test("^[0-9a-f]{40}$"))' \
    "$gate/manifest.json" > /dev/null || { echo "::error::the manifest of $tag is not $tag of this repository"; exit 1; }
  release_commit=$(jq -r .commit "$gate/manifest.json")
  [ -z "${COMMIT:-}" ] || [ "$release_commit" = "$COMMIT" ] || { echo "::error::$tag is of $release_commit, not $COMMIT"; exit 1; }
  git -C "$repo" cat-file -e "$release_commit^{commit}" 2>/dev/null || { echo "::error::the checkout does not hold $release_commit"; exit 1; }
  release_inputs=$(sh "$here/inputs.sh" "$release_commit" | sed -n 's/^ios //p')
  [ -n "$release_inputs" ] || { echo "::error::no iOS inputs for $release_commit"; exit 1; }
  jq -e --arg s "$release_inputs" '.inputs.ios == $s' "$gate/manifest.json" > /dev/null \
    || { echo "::error::$tag does not name the iOS inputs of its commit"; exit 1; }
  if jq -e '.changed.ios == true' "$gate/manifest.json" > /dev/null; then release_mode=build; else release_mode=carry; fi
}

upload() {
  local there=$RUNNER_TEMP/mac-release-there new=()
  rm -rf "$there"
  mkdir -p "$there"
  gh release view "$tag" --repo "$GITHUB_REPOSITORY" --json assets --jq '.assets[].name' > "$there/names"
  for file in "${files[@]}"; do
    [ -f "$dir/$file" ] || { echo "::error::$file is missing"; exit 1; }
    if grep -Fxq "$file" "$there/names"; then
      gh release download "$tag" --repo "$GITHUB_REPOSITORY" --pattern "$file" --dir "$there"
      cmp -s "$there/$file" "$dir/$file" || { echo "::error::$tag holds another $file already: it stays, nothing is replaced"; exit 1; }
      echo "$file: on $tag already, the same bytes"
    else
      new+=("$dir/$file")
    fi
  done
  [ "${#new[@]}" = 0 ] || gh release upload "$tag" --repo "$GITHUB_REPOSITORY" "${new[@]}"
  echo "$tag holds ${files[*]}"
}

# the three files in $dir belong together and to these iOS inputs
consistent() {
  local inputs=$1
  (cd "$dir" && sha256sum -c "$name.dmg.sha256")
  [ "$(awk '{print $2}' "$dir/$name.dmg.sha256")" = "$name.dmg" ] || { echo "::error::$name.dmg.sha256 names another file"; exit 1; }
  jq -e --arg s "$(sha256sum "$dir/$name.dmg" | cut -d' ' -f1)" --argjson z "$(wc -c < "$dir/$name.dmg" | tr -d ' ')" \
    --arg i "$inputs" --arg r "$GITHUB_REPOSITORY" \
    '.product == "trommi-macos" and .repository == $r and .file == "Trommi-macOS.dmg" and .sha256 == $s and .size == $z and .inputs_ios == $i' \
    "$dir/$name.json" > /dev/null || { echo "::error::$name.json does not describe this $name.dmg of these iOS inputs"; exit 1; }
}

case "$mode" in
  check)
    check
    out mode "$release_mode"
    out commit "$release_commit"
    out inputs_ios "$release_inputs"
    case "$release_mode" in
      none) summary "- no release $tag: no Mac app" ;;
      build) summary "- $tag marks the app changed: the Mac app is built, notarized and attached" ;;
      carry) summary "- $tag leaves the app unchanged: the newest Mac app is attached to it again" ;;
    esac
    ;;
  prepare)
    [ -n "${COMMIT:-}" ] && [ -n "${MAC_VERSION:-}" ] || { echo "::error::COMMIT and MAC_VERSION are needed"; exit 1; }
    printf '%s' "$MAC_VERSION" | grep -Eq '^[0-9]+(\.[0-9]+)*$' || { echo "::error::not a version: $MAC_VERSION"; exit 1; }
    check
    [ "$release_mode" = build ] || { echo "::error::$tag does not mark the app changed ($release_mode): nothing is attached"; exit 1; }
    for file in "$name.dmg" "$name.dmg.sha256"; do [ -f "$dir/$file" ] || { echo "::error::$file is missing"; exit 1; }; done
    jq -n --arg r "$GITHUB_REPOSITORY" --arg t "$tag" --arg c "$COMMIT" --arg i "$release_inputs" --arg v "$MAC_VERSION" \
      --arg b "$VERSION" --arg s "$(sha256sum "$dir/$name.dmg" | cut -d' ' -f1)" --argjson z "$(wc -c < "$dir/$name.dmg" | tr -d ' ')" \
      '{product: "trommi-macos", repository: $r, release: $t, commit: $c, inputs_ios: $i, version: $v, build: $b,
        file: "Trommi-macOS.dmg", sha256: $s, size: $z, signed: "Developer ID Application, notarized and stapled"}' > "$dir/$name.json"
    consistent "$release_inputs"
    cat "$dir/$name.json"
    ;;
  upload)
    check
    [ "$release_mode" = build ] || [ "$release_mode" = carry ] || { echo "::error::no release $tag"; exit 1; }
    consistent "$release_inputs"
    upload
    summary "- $tag: ${files[*]} attached"
    ;;
  carry)
    check
    [ "$release_mode" = carry ] || { echo "::error::$tag is not a release that leaves the app unchanged ($release_mode)"; exit 1; }
    # the newest earlier release with a Mac app
    from=
    while read -r earlier; do
      [ "$earlier" -lt "$VERSION" ] || continue
      if gh release view "v$earlier" --repo "$GITHUB_REPOSITORY" --json assets --jq '.assets[].name' | grep -Fxq "$name.json"; then
        from=v$earlier
        break
      fi
    done < <(gh release list --repo "$GITHUB_REPOSITORY" --exclude-drafts --limit 100 --json tagName --jq '.[].tagName' \
      | sed -n 's/^v\([1-9][0-9]*\)$/\1/p' | sort -rn)
    if [ -z "$from" ]; then
      echo "::notice::no earlier release holds a Mac app: $tag gets none"
      summary "- no earlier release holds a Mac app: $tag gets none"
      exit 0
    fi
    rm -rf "$dir"
    mkdir -p "$dir"
    for file in "${files[@]}"; do gh release download "$from" --repo "$GITHUB_REPOSITORY" --pattern "$file" --dir "$dir"; done
    if ! jq -e --arg i "$release_inputs" '.inputs_ios == $i' "$dir/$name.json" > /dev/null; then
      echo "::notice::the Mac app of $from was built from other iOS inputs than $tag's (its build for a later change failed or was skipped): $tag gets none"
      summary "- the newest Mac app ($from) is not of $tag's iOS inputs: $tag gets none"
      exit 0
    fi
    consistent "$release_inputs"
    gh attestation verify "$dir/$name.dmg" --repo "$GITHUB_REPOSITORY" --signer-workflow "$GITHUB_REPOSITORY/.github/workflows/mac.yml" > /dev/null \
      || { echo "::error::$name.dmg of $from carries no build attestation of mac.yml in this repository"; exit 1; }
    upload
    summary "- $tag: the Mac app of $from attached again (same iOS inputs)"
    ;;
esac
