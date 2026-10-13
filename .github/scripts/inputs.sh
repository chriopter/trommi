#!/bin/sh
# What each part is built from, as one SHA-256 per part: `.github/scripts/inputs.sh [<commit>]` prints lines
# `<part> <sha256>` for web, connector, ios, hub, updater. Each part is delivered on its own, when this hash differs
# from that of what it has out there (.github/scripts/deployed.sh; iOS: ios_delivered.sh; the Mac app: mac_release.sh).
#
# The hash is over the lines `git ls-tree -r <commit>` prints for the part's paths (mode, type, object id, path),
# never over the working tree, so it is the same on every machine for the same commit. Two commits with the same
# value for a part build the same part. A path too many costs one needless delivery; a path too few ships a stale
# part: when in doubt a path is in. Each part names the workflows and scripts that build and deliver it (build
# settings live there); tests are in none (they ship nothing).
# `.github/scripts/inputs.sh paths <part>` prints the paths themselves (the shared ones included), for the release
# notes (release_notes.mjs: the pull requests that touch a part).
set -eu
commit=${1:-HEAD}
# every part: the shared core and client core (in every app and in the hub's checks of what it stores), the Rust
# workspace and its toolchain, this script
shared='core/ client/ Cargo.toml Cargo.lock rust-toolchain.toml .github/scripts/inputs.sh'
# what builds and signs the hub, the updater and the connector (build.yml's jobs, the signed release of one part)
rust_release='.github/workflows/build.yml .github/workflows/release_part.yml release/'
paths() {
  case $1 in
    web) echo "app/web/ connector/tools.json connector/prompt.md demo/ .node-version .github/workflows/build.yml .github/scripts/web_release.mjs .github/scripts/web_smoke.sh .github/tools/wrangler/ .github/workflows/deploy_web.yml" ;;
    connector) echo "connector/ install.sh .claude-plugin/ .node-version $rust_release" ;;
    # the iPhone app and the Mac app (built from the same paths): ios.yml after build.yml, not build.yml itself
    ios) echo "app/ios/ demo/ .github/scripts/ios_testflight.sh .github/scripts/ios_archive.sh .github/scripts/ios_entitlements.py .github/scripts/ios_release_gate.sh .github/scripts/ios_delivered.sh .github/scripts/mac_testflight.sh .github/scripts/mac_direct.sh .github/scripts/mac_release.sh .github/workflows/deploy_ios.yml .github/workflows/ios.yml .github/workflows/mac.yml" ;;
    # the hub program: the hub's crate (hub/updater and hub/deploy belong to the updater)
    hub) echo "hub/Cargo.toml hub/src/ hub/build.rs hub/static/ hub/data-epoch .github/workflows/deploy_hub.yml .github/scripts/hub_compat.sh $rust_release" ;;
    updater) echo "hub/updater/ hub/deploy/ .github/workflows/deploy_hub.yml $rust_release" ;;
  esac
}
if [ "$commit" = paths ]; then
  [ -n "$(paths "${2:-}")" ] || { echo "inputs.sh paths: no part ${2:-}" >&2; exit 2; }
  echo "$shared $(paths "$2")"
  exit 0
fi
for part in web connector ios hub updater; do
  # shellcheck disable=SC2046,SC2086
  sum=$(git ls-tree -r "$commit" -- $shared $(paths "$part") | LC_ALL=C sort -k4 | sha256sum | cut -d' ' -f1)
  echo "$part $sum"
done
