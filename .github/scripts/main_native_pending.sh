#!/bin/bash
# Recheck under the destination lock: an earlier main run may have delivered while this one waited.
set -euo pipefail
case "${1:-}" in
  ios|mac-testflight)
    destination=ios
    [ "$1" != mac-testflight ] || destination=mac
    TF_DESTINATION=$destination bash .github/scripts/ios_release_gate.sh decide
    ;;
  mac-direct)
    GITHUB_OUTPUT="$RUNNER_TEMP/mac-pending" bash .github/scripts/mac_release.sh check >&2
    if grep -q '^mode=build$' "$RUNNER_TEMP/mac-pending"; then echo deliver=true; else echo deliver=false; fi
    ;;
  *) echo 'unknown native destination' >&2; exit 2 ;;
esac
