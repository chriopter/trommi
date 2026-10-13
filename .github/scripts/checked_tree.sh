#!/bin/sh
# Main only (build.yml, job `changes`): which parts' checks already passed before the merge for the very same tree of
# files, in the merge queue or on the pull request. Prints `core=`, `web=`, `connector=`, `hub=` lines (true or false)
# for $GITHUB_OUTPUT. A part marked true does not run its tests and clippy on main again: the same files, the same
# workflow (it is part of the tree) and the same tests passed minutes before.
#
# The evidence, all of it from GitHub itself, nothing taken on a file's word:
#   1. the artifact `checked-tree-<tree>` that build.yml's job `changes` uploads on every run of the merge queue and of
#      a pull request; its name says which tree that run checked out
#   2. that artifact's run: build.yml, of this repository, and either
#        a merge_group run on a branch of main's merge queue (gh-readonly-queue/main/...): the commit the queue made
#        to land on main, or
#        a pull_request run of a branch of this repository (no fork) at the head commit of the pull request this commit
#        was merged from (GitHub's commit -> pull request link)
#   3. the jobs of that run (their latest attempts): a part counts only when its check job ended in success
# Anything missing or not as expected: every part is false and main runs all checks, as before. A pull request whose
# base moved on before it merged was checked on another tree; then nothing matches and main runs them all too (the
# merge queue avoids that: it checks the commit that lands).
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
# the head of the pull request this commit was merged from, when there is one of a branch of this repository
head=$(gh api "repos/$repo/commits/$GITHUB_SHA/pulls" \
  --jq '[.[] | select(.merged_at != null and .base.repo.full_name == "'"$repo"'" and .head.repo.full_name == "'"$repo"'")] | .[0].head.sha // ""' 2>/dev/null) || head=
printf '%s' "$head" | grep -Eq '^[0-9a-f]{40}$' || head=
echo "checked_tree.sh: $GITHUB_SHA is tree $tree${head:+, merged from the pull request head $head}" >&2
runs=$(gh api "repos/$repo/actions/artifacts?name=checked-tree-$tree&per_page=100" \
  --jq '.artifacts[] | select(.workflow_run.head_repository_id == '"$repo_id"') | .workflow_run.id' | sort -u) \
  || none "the artifacts could not be listed"
[ -n "$runs" ] || none "no run of the merge queue or of a pull request checked tree $tree"
passed=
for run in $runs; do
  printf '%s' "$run" | grep -Eq '^[1-9][0-9]{0,19}$' || continue
  gh api "repos/$repo/actions/runs/$run" \
    --jq 'select(.path == ".github/workflows/build.yml" and .repository.full_name == "'"$repo"'" and .head_repository.full_name == "'"$repo"'"
      and ((.event == "merge_group" and (.head_branch | startswith("gh-readonly-queue/main/")))
        or (.event == "pull_request" and "'"$head"'" != "" and .head_sha == "'"$head"'"))) | .id' \
    | grep -qx "$run" || { echo "checked_tree.sh: run $run is neither main's merge queue nor the merged pull request at its head" >&2; continue; }
  names=$(gh api --paginate "repos/$repo/actions/runs/$run/jobs?filter=latest&per_page=100" --jq '.jobs[] | select(.conclusion == "success") | .name') || continue
  passed=$(printf '%s\n%s\n' "$passed" "$names")
  echo "checked_tree.sh: run $run checked tree $tree" >&2
done
say() { if printf '%s\n' "$passed" | grep -Fxq "$2"; then echo "$1=true"; else echo "$1=false"; fi; }
say core 'Core (check)'
say web 'Web app (check)'
say connector 'Connector (check)'
say hub 'Hub (check)'
