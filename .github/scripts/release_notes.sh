#!/bin/sh
# A release's notes (release.yml, job publish): the newest day's block of CHANGELOG.md, from its "## " heading up to
# the next. Releases of the same day repeat that block as it grows.
#   .github/scripts/release_notes.sh <CHANGELOG.md of this release>
# Prints nothing when there is no such block; never fails, so the notes can never stop a release.
[ -s "${1:-}" ] || exit 0
awk '/^## / { if (seen) exit; seen = 1 } seen' "$1" 2>/dev/null || true
exit 0
