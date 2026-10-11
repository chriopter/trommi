#!/bin/sh
# A release's notes (release.yml, job publish): the newest day's block of CHANGELOG.md, from its "## " heading up to
# the next. Releases of the same day repeat that block as it grows.
#   .github/scripts/release_notes.sh <CHANGELOG.md of this release> [<this tag> <published releases>]
# <published releases>: a file of lines "<tag> <created, ISO 8601 UTC>" (gh release list). The heading gets the day's
# releases in brackets, counted from them: every release v<N> made on that day (Europe/Berlin), this tag included
# when the day is today. CHANGELOG.md itself names no release numbers.
# The block as GitHub shows a release: a line that goes on (hard-wrapped) is joined to the one before, since a
# release's body keeps every line break; "<", ">" and "&" outside `code` are escaped, or GitHub drops "<…>" as HTML.
# NOTES_BASE (e.g. https://raw.githubusercontent.com/<repo>/<commit>/): put in front of the picture's path, which
# CHANGELOG.md gives from the repository's root.
# Prints nothing when there is no such block; never fails, so the notes can never stop a release.
[ -s "${1:-}" ] || exit 0
tag=${2:-}
list=${3:-}
tz=Europe/Berlin

block=$(awk '/^## / { if (seen) exit; seen = 1 } seen' "$1" 2>/dev/null) || exit 0
[ -n "$block" ] || exit 0
# the day: the heading without anything in brackets after it
day=$(printf '%s\n' "$block" | sed -n '1{s/^## *//;s/ *([^)]*) *$//;s/ *$//;p;}')
range=
if iso=$(date -d "$day" +%F 2>/dev/null); then
  numbers=$(
    {
      if [ -n "$list" ] && [ -r "$list" ]; then cat "$list"; fi
      if [ -n "$tag" ]; then printf '%s %s\n' "$tag" "$(date -u +%FT%TZ)"; fi
    } | while read -r t at; do
      n=$(printf '%s' "$t" | sed -n 's/^v\([1-9][0-9]*\)$/\1/p')
      if [ -n "$n" ] && [ "$(TZ=$tz date -d "$at" +%F 2>/dev/null)" = "$iso" ]; then echo "$n"; fi
    done | sort -n | uniq
  )
  first=$(printf '%s\n' "$numbers" | sed -n '/./p' | head -n 1)
  last=$(printf '%s\n' "$numbers" | sed -n '/./p' | tail -n 1)
  if [ -n "$first" ] && [ "$first" = "$last" ]; then range=" (v$first)"
  elif [ -n "$first" ]; then range=" (v$first – v$last)"; fi
fi

printf '%s\n' "$block" | awk -v head="## $day$range" -v base="${NOTES_BASE:-}" '
  # outside `code`: &, < and > as entities
  function esc(s,   n, parts, i, out) {
    n = split(s, parts, "`")
    out = ""
    for (i = 1; i <= n; i++) {
      if (i % 2 == 1 || i == n) { gsub(/&/, "\\&amp;", parts[i]); gsub(/</, "\\&lt;", parts[i]); gsub(/>/, "\\&gt;", parts[i]) }
      out = out (i > 1 ? "`" : "") parts[i]
    }
    return out
  }
  function flush() { if (have) print esc(cur); have = 0; cur = "" }
  NR == 1 { print head; next }
  # the picture: its path from the repository root, made a whole address
  base != "" && /^!\[[^]]*\]\([^:)]+\) *$/ { flush(); i = index($0, "]("); print substr($0, 1, i + 1) base substr($0, i + 2); next }
  /^[ \t]*$/ { flush(); print ""; next }
  # a line that starts something of its own: an item, a part (**New**), a heading, a picture, a table, a quote
  /^([-*+] |[0-9]+\. |#|\*\*[^*]+\*\* *$|!\[|\||>)/ { flush(); cur = $0; have = 1; next }
  # any other line goes on with the line before it
  { line = $0; sub(/^[ \t]+/, "", line); if (have) cur = cur " " line; else { cur = line; have = 1 } }
  END { flush() }
' 2>/dev/null || true
exit 0
