#!/usr/bin/env bash
# Re-sign every commit of a branch (default: main) with an SSH signing key.
#
# Every commit is rebuilt with `git commit-tree -S`: same tree, same author
# and committer (name, e-mail, date, time zone), same message, same parent
# order (merges keep all their parents, several roots stay roots). Only the
# parent hashes (now the re-signed ones) and the signature change. An old
# signature on a commit is replaced.
#
# Works in a fresh clone in a temp dir; never touches your working copy.
# The private key is never read: signing goes through ssh-agent (for
# example the 1Password agent), the key is given as its public key.
#
# Usage:
#   dev/resign-history.sh [--source URL] [--branch main] [--keep] KEY
#   KEY   the public key text ("ssh-ed25519 AAAA… comment"), the same with
#         a "key::" prefix, or the path of a .pub file
#   --source  repository to clone (default https://github.com/chriopter/trommi.git)
#   --branch  branch to re-sign (default main)
#
# It never pushes. At the end it prints the exact push command.
set -euo pipefail

source_url=https://github.com/chriopter/trommi.git
branch=main
key=
while [ $# -gt 0 ]; do
  case "$1" in
    --source) source_url=$2; shift 2 ;;
    --branch) branch=$2; shift 2 ;;
    -h|--help) sed -n '2,22p' "$0"; exit 0 ;;
    *) key=$1; shift ;;
  esac
done
die() { echo "resign-history: $*" >&2; exit 1; }
[ -n "$key" ] || die "give the signing key (public key text, key::…, or a .pub file)"

if [ -f "$key" ]; then pubkey=$(head -n1 "$key"); else pubkey=${key#key::}; fi
case "$pubkey" in
  ssh-*|ecdsa-*|sk-*) ;;
  *) die "the key must be an SSH public key" ;;
esac
grep -q 'PRIVATE KEY' <<<"$pubkey" && die "give the public key, never the private one"
[ -n "${SSH_AUTH_SOCK:-}" ] || die "no ssh-agent (SSH_AUTH_SOCK is empty)"
# The agent must hold the key, or signing fails half-way.
keyblob=$(awk '{print $2}' <<<"$pubkey")
ssh-add -L 2>/dev/null | awk '{print $2}' | grep -qxF "$keyblob" \
  || die "the ssh-agent does not hold this key"

started=$(date +%s)
work=$(mktemp -d "${TMPDIR:-/tmp}/resign-history.XXXXXX")
repo=$work/repo
echo "== clone $source_url into $repo"
git clone --quiet --bare --single-branch --branch "$branch" "$source_url" "$repo"
G() { git -C "$repo" "$@"; }

old=$(G rev-parse "refs/heads/$branch")
echo "== $branch is at $old"

# Signing through the agent with the public key literal; no private key file.
G config gpg.format ssh
G config user.signingkey "key::$pubkey"
G config commit.gpgsign false
printf '* namespaces="git" %s\n' "$pubkey" > "$work/allowed_signers"
G config gpg.ssh.allowedSignersFile "$work/allowed_signers"

commits=$(G rev-list --reverse --topo-order "$old")
total=$(wc -l <<<"$commits")
merges=$(G rev-list --min-parents=2 --count "$old")
roots=$(G rev-list --max-parents=0 "$old" | wc -l)
echo "== $total commits, $merges merges, $roots roots"
if [ "$merges" -ne 0 ] || [ "$roots" -ne 1 ]; then
  echo "   history is not linear: merges keep their parent order, roots stay roots"
fi

# Only the headers tree/parent/author/committer and a gpgsig may appear;
# anything else (encoding, mergetag, …) would be lost by commit-tree.
odd=$(for c in $commits; do G cat-file commit "$c" | sed '/^$/q' ; done \
  | grep -vE '^(tree|parent|author|committer|gpgsig) |^ |^$' | sort -u || true)
[ -z "$odd" ] || die "commits carry headers commit-tree cannot keep: $odd"

declare -A map
n=0
for c in $commits; do
  n=$((n + 1))
  # Message: everything after the first empty line, byte for byte.
  G cat-file commit "$c" | sed '1,/^$/d' > "$work/msg"
  # The message of a commit may lack a trailing newline; sed adds none.
  tree=$(G rev-parse "$c^{tree}")
  pargs=()
  for p in $(G rev-list --parents -n1 "$c" | cut -s -d' ' -f2-); do
    [ -n "${map[$p]:-}" ] || die "parent $p of $c not rebuilt yet"
    pargs+=(-p "${map[$p]}")
  done
  IFS=$'\t' read -r an ae ad cn ce cd < <(G log -1 --format='%an%x09%ae%x09%ad%x09%cn%x09%ce%x09%cd' --date=raw "$c")
  new=$(GIT_AUTHOR_NAME=$an GIT_AUTHOR_EMAIL=$ae GIT_AUTHOR_DATE=$ad \
        GIT_COMMITTER_NAME=$cn GIT_COMMITTER_EMAIL=$ce GIT_COMMITTER_DATE=$cd \
        G commit-tree -S "$tree" "${pargs[@]}" -F "$work/msg") \
    || die "signing $c failed"
  map[$c]=$new
  [ $((n % 200)) -eq 0 ] && echo "   $n/$total"
done
newhead=${map[$old]}
G update-ref "refs/resigned/$branch" "$newhead"
echo "== rebuilt: $newhead"

# Checks: a failure ends the script with no push command.
echo "== checks"
newcommits=$(G rev-list --reverse --topo-order "$newhead")
[ "$(wc -l <<<"$newcommits")" -eq "$total" ] || die "commit count differs"
# Unsigned body = the raw object without its gpgsig header; parent lines
# are mapped back to the old hashes. Then old and new must be identical
# byte for byte (tree, author, committer, dates, time zones, message).
strip() { G cat-file commit "$1" | awk '
  done { print; next }
  /^$/ { done = 1; print; next }
  /^gpgsig / { insig = 1; next }
  insig && /^ / { next }
  { insig = 0; print }'; }
fails=0
for c in $commits; do
  nc=${map[$c]}
  a=$(strip "$c"; echo x)
  b=$(strip "$nc"; echo x)
  for p in $(G rev-list --parents -n1 "$c" | cut -s -d' ' -f2-); do
    b=${b/"parent ${map[$p]}"/"parent $p"}
  done
  [ "$a" = "$b" ] || { echo "   differs: $c -> $nc"; fails=$((fails + 1)); }
  [ "$(G rev-parse "$c^{tree}")" = "$(G rev-parse "$nc^{tree}")" ] \
    || { echo "   tree differs: $c"; fails=$((fails + 1)); }
  G verify-commit "$nc" 2>/dev/null \
    || { echo "   signature fails: $nc"; fails=$((fails + 1)); }
done
[ "$fails" -eq 0 ] || die "$fails check(s) failed"
# Order and shape: the n-th old and n-th new commit pair up.
paste -d' ' <(echo "$commits") <(echo "$newcommits") | while read -r o nn; do
  [ "${map[$o]}" = "$nn" ] || die "commit order differs at $o"
done
echo "   $total commits: same count, trees, authors, committers, dates, messages, parents; all signatures verify"

# Refuse if the branch moved while we worked.
now=$(git ls-remote "$source_url" "refs/heads/$branch" | cut -f1)
[ "$now" = "$old" ] || die "$branch moved during the run ($old -> $now); start again"

echo "== done in $(( $(date +%s) - started )) s"
echo
echo "Push (run it yourself; the main ruleset must allow it, e.g. a bypass for you, or push before enabling it):"
echo "  git -C $repo push --force-with-lease=$branch:$old origin $newhead:$branch"
