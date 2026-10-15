#!/bin/bash
# Which parts changed (pr-check.yml on a pull request, the part's ship-*.yml on main). Writes <part>=true|false to GITHUB_OUTPUT.
# Environment: EVENT (pull_request or push), PR_BASE (pull request), PARTS (push: the part whose checks run), WORKFLOW (push:
# the calling workflow's file; the core's changes are counted since its newest green run on main), GH_TOKEN, RUNNER_TEMP.
set -eu
base=
if [ "$EVENT" = pull_request ]; then
  base=$(git merge-base "$PR_BASE" HEAD || true)
else
  # called by a ship-*.yml on main: since its newest run on main that passed
  base=$(gh api "repos/$GITHUB_REPOSITORY/actions/workflows/$WORKFLOW/runs?branch=main&status=success&per_page=1" \
    --jq '.workflow_runs[0].head_sha // ""' 2>/dev/null) || base=
fi
if [ -n "$base" ] && git cat-file -e "$base^{commit}" 2>/dev/null; then
  all=0
  git diff --name-only "$base" HEAD > "$RUNNER_TEMP/files"
  echo "compared with $base: $(wc -l < "$RUNNER_TEMP/files") file(s) changed"
else
  all=1
  : > "$RUNNER_TEMP/files"
  echo "nothing to compare with: every part counts as changed"
fi
# what every Rust part and app is built on
shared='core/|client/|Cargo\.toml$|\.config/nextest\.toml$|Cargo\.lock$|rust-toolchain\.toml$|\.github/workflows/pr-check\.yml|\.github/actions/|\.github/scripts/changed_parts\.sh$|\.github/scripts/(inputs|source_tree|rust_toolchain)\.sh$'
# part <name> '<its own paths>': on a pull request, whether its paths (or the shared ones) changed; called,
# only the named part (always) and the core (when it changed)
part() {
  if [ "$all" = 1 ] || grep -Eq "^($2|$shared)" "$RUNNER_TEMP/files"; then value=true; else value=false; fi
  if [ "$EVENT" != pull_request ]; then
    case "$1" in
      "$PARTS") value=true ;;
      # the website does not build on the core
      core) [ "$PARTS" != site ] || value=false ;;
      *) value=false ;;
    esac
  fi
  echo "$1=$value" >> "$GITHUB_OUTPUT"
  echo "- $1: $value" | tee -a "$GITHUB_STEP_SUMMARY"
}
part core 'tests/core/|tests/src/|tests/bindings/|tests/Cargo\.toml$|spec/vectors/|spec/account-vectors\.json$|THIRD-PARTY\.md$|\.github/scripts/third_party\.mjs$'
part web 'app/web/|tests/web/|connector/tools\.json$|connector/prompt\.md$|demo/|\.node-version$|\.github/scripts/(web_release\.mjs|web_smoke\.sh|deployed\.sh|release_notes\.mjs)$|\.github/tools/wrangler/|\.github/workflows/ship-web\.yml$'
part connector 'connector/|release/|install\.sh$|\.claude-plugin/|tests/connector/|tests/Cargo\.toml$|\.github/workflows/(ship-connector|sign-release)\.yml$'
part hub 'hub/|release/|tests/hub/|tests/deploy/|tests/Cargo\.toml$|\.github/workflows/(ship-hub|sign-release)\.yml$|\.github/scripts/(hub_compat|deployed|deploy_issue)\.sh$'
part apple 'app/ios/|tests/ios/|demo/|\.github/scripts/(ios_|mac_)|\.github/workflows/ship-apple\.yml$'
# (the window draws with the web app's pen and fonts)
part linux 'app/linux/|connector/|hub/|tests/connector/common/|app/web/public/ui\.mjs$|app/web/public/fonts/|\.github/workflows/ship-linux\.yml$'
part site 'site/|tests/site/|\.github/workflows/ship-site\.yml$'
