#!/bin/bash
# Verify a main artifact's immutable source and that its shared/part checks and build passed.
# ci-job: main.yml Plan and shared checks
# ci-job: main.yml apple / iOS client (check)
# ci-job: main.yml web / Web app (check)
# ci-job: main.yml hub / Hub (check)
# ci-job: main.yml connector / Connector (check)
# ci-job: main.yml site / Site (test)
set -euo pipefail
case ${1:-} in
  apple) job='apple / iOS client (check)' ;;
  web) job='web / Web app (check)' ;;
  hub) job='hub / Hub (check)' ;;
  connector) job='connector / Connector (check)' ;;
  site) job='site / Site (test)' ;;
  *) echo 'main_release_gate.sh: unknown part' >&2; exit 2 ;;
esac
for name in GH_TOKEN GITHUB_REPOSITORY RUN_ID COMMIT RUNNER_TEMP; do
  [ -n "${!name:-}" ] || { echo "::error::$name is not set"; exit 1; }
done
[[ $RUN_ID =~ ^[1-9][0-9]{0,19}$ && $COMMIT =~ ^[0-9a-f]{40}$ ]] || exit 1
gh api "repos/$GITHUB_REPOSITORY/actions/runs/$RUN_ID" > "$RUNNER_TEMP/main-gate-run.json"
jq -e --arg r "$GITHUB_REPOSITORY" --arg c "$COMMIT" \
  '.path == ".github/workflows/main.yml" and .head_branch == "main" and (.event == "push" or .event == "workflow_dispatch")
   and .repository.full_name == $r and .head_repository.full_name == $r and .head_sha == $c' \
  "$RUNNER_TEMP/main-gate-run.json" > /dev/null || { echo '::error::artifact source is not this main build'; exit 1; }
gh api --paginate "repos/$GITHUB_REPOSITORY/actions/runs/$RUN_ID/jobs?filter=latest&per_page=100" \
  --jq '.jobs[] | "\(.name)\t\(.conclusion)"' > "$RUNNER_TEMP/main-gate-jobs.txt"
wanted_jobs=("$job")
[ "$1" = site ] || wanted_jobs+=('Plan and shared checks')
for wanted in "${wanted_jobs[@]}"; do
  result=$(awk -F'\t' -v name="$wanted" '$1 == name {print $2; exit}' "$RUNNER_TEMP/main-gate-jobs.txt")
  [ "$result" = success ] || { echo "::error::$wanted did not pass for run $RUN_ID"; exit 1; }
done
