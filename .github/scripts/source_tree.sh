#!/bin/sh
# Stable build inputs from a committed tree. READMEs are documentation, never build inputs.
# Keep the git command outside a pipeline: an invalid revision must fail, not hash an empty list.
set -eu
commit=$1
shift
tree=$(mktemp "${TMPDIR:-/tmp}/trommi-inputs.XXXXXX")
trap 'rm -f "$tree"' EXIT HUP INT TERM
git ls-tree -r "$commit" -- "$@" .github/scripts/source_tree.sh > "$tree"
awk '$4 !~ /(^|\/)README\.md$/ { print }' "$tree"
