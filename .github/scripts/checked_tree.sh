#!/bin/sh
# Main only (build.yml, job `changes`): which parts' checks already passed on the pull request this commit came from,
# for the very same tree of files. Prints `core=`, `web=`, `connector=`, `hub=` lines (true or false) for
# $GITHUB_OUTPUT. A part marked true does not run its tests and clippy on main again: the same files, the same
# workflow (it is part of the tree) and the same tests passed minutes before.
#
# The evidence, all of it from GitHub itself, nothing taken on a file's word:
#   1. the pull request the commit was merged from (GitHub's commit -> pull request link): its head commit
#   2. the artifact `checked-tree-<tree>` that build.yml's job `changes` uploads on every pull request run; its name
#      says which tree that run checked out (the merge of the pull request into its base, as GitHub made it)
#   3. that artifact's run: build.yml, a pull_request run, of this repository (no fork), of that very head commit
#   4. the jobs of that run (their latest attempts): a part counts only when its check job ended in success
# Anything missing or not as expected: every part is false and main runs all checks, as before. A pull request whose
# base moved on before it merged was checked on another tree; then nothing matches and main runs them all too.
#
#   GH_TOKEN, GITHUB_REPOSITORY, GITHUB_SHA: as in a workflow (the token needs actions: read and contents: read)
set -u
none() {
  [ -n "${1:-}" ] && echo "checked_tree.sh: $1" >&2
  printf 'core=false\nweb=false\nconnector=false\nhub=false\n'
  exit 0
}
repo=$GITHUB_REPOSITORY
tree=$(git rev-parse "$GITHUB_SHA^{tree}" 2>/dev/null) || none "no tree for $GITHUB_SHA"
printf '%s' "$tree" | grep -Eq '^[0-9a-f]{40}$' || none "not a tree: $tree"
repo_id=$(gh api "repos/$repo" --jq .id) || none "the repository could not be read"
head=$(gh api "repos/$repo/commits/$GITHUB_SHA/pulls" \
  --jq '[.[] | select(.merged_at != null and .base.repo.full_name == "'"$repo"'" and .head.repo.full_name == "'"$repo"'")] | .[0].head.sha // ""') \
  || none "the pull request of $GITHUB_SHA could not be read"
printf '%s' "$head" | grep -Eq '^[0-9a-f]{40}$' || none "$GITHUB_SHA came from no merged pull request of a branch of this repository"
echo "checked_tree.sh: $GITHUB_SHA is tree $tree, merged from the pull request head $head" >&2
runs=$(gh api "repos/$repo/actions/artifacts?name=checked-tree-$tree&per_page=100" \
  --jq '.artifacts[] | select(.workflow_run.head_sha == "'"$head"'" and .workflow_run.head_repository_id == '"$repo_id"') | .workflow_run.id' | sort -u) \
  || none "the artifacts could not be listed"
[ -n "$runs" ] || none "no pull request run checked tree $tree at $head"
passed=
for run in $runs; do
  printf '%s' "$run" | grep -Eq '^[1-9][0-9]{0,19}$' || continue
  gh api "repos/$repo/actions/runs/$run" \
    --jq 'select(.path == ".github/workflows/build.yml" and .event == "pull_request" and .head_sha == "'"$head"'" and .head_repository.full_name == "'"$repo"'" and .repository.full_name == "'"$repo"'") | .id' \
    | grep -qx "$run" || { echo "checked_tree.sh: run $run is not build.yml on a pull request of this repository at $head" >&2; continue; }
  names=$(gh api --paginate "repos/$repo/actions/runs/$run/jobs?filter=latest&per_page=100" --jq '.jobs[] | select(.conclusion == "success") | .name') || continue
  passed=$(printf '%s\n%s\n' "$passed" "$names")
  echo "checked_tree.sh: run $run checked tree $tree" >&2
done
say() { if printf '%s\n' "$passed" | grep -Fxq "$2"; then echo "$1=true"; else echo "$1=false"; fi; }
say core 'Core (check)'
say web 'Web app (check)'
say connector 'Connector (check)'
say hub 'Hub (check)'
