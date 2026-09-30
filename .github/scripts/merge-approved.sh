#!/usr/bin/env bash
# Squash-merges one pull request the coordinator approved (DEC-175), or does nothing.
#
# The approval is bound to a commit: the pull request carries the `coordinator-approved` label and
# its description holds a line `Coordinator-approved-head: <40-hex sha>` naming the head the
# independent review passed. Builders cannot edit a description (they have no pull request tool), so
# only the coordinator or the founder writes that line. A push moves the head away from it, which
# withdraws the approval with nothing to cancel or retry.
#
# A pull request merges only when all of these hold, read here from GitHub and never from the event:
# it is open, not a draft, targets `main`, is mergeable, carries the label, names its current head
# as approved, and the latest `ci` run on that head succeeded (so `fast` and `full` did), plus the
# latest `web` run when it touches `web/` (DEC-112 lets `fast` and `full` skip the web checks). The
# merge names the head (`sha`), so a push after these reads makes GitHub refuse it. The commit
# message is the description's own body with any `Co-authored-by` line removed, never GitHub's
# default, which copies every commit message and its trailers onto `main` (ship playbook, step 5).
#
# Usage: merge-approved.sh <pull request number>   (needs GH_TOKEN and GITHUB_REPOSITORY;
# MERGE_DRY_RUN=1 reads everything and prints the merge instead of making it)
set -euo pipefail

pr=${1:?pull request number}
repo=${GITHUB_REPOSITORY:?}
label=coordinator-approved

view=$(gh pr view "$pr" --repo "$repo" \
  --json number,state,isDraft,baseRefName,headRefOid,mergeable,labels,title,body)

skip() {
  echo "#$pr $1; not merging"
  exit 0
}

[ "$(jq -r .state <<<"$view")" = OPEN ] || skip "is not open"
[ "$(jq -r .isDraft <<<"$view")" != true ] || skip "is a draft"
[ "$(jq -r .baseRefName <<<"$view")" = main ] || skip "does not target main"
jq -e --arg l "$label" '.labels | any(.name == $l)' <<<"$view" >/dev/null ||
  skip "does not carry $label"

sha=$(jq -r .headRefOid <<<"$view")
body=$(jq -r '.body // ""' <<<"$view" | tr -d '\r')
approved=$(sed -nE 's/^[[:space:]]*Coordinator-approved-head:[[:space:]]*([0-9a-f]{40})[[:space:]]*$/\1/p' <<<"$body" | tail -n 1)
[ -n "$approved" ] || skip "names no Coordinator-approved-head"
[ "$approved" = "$sha" ] || skip "was approved at $approved, but its head is $sha"

mergeable=$(jq -r .mergeable <<<"$view")
[ "$mergeable" = MERGEABLE ] || skip "is not mergeable yet ($mergeable)"

latest_run() {
  gh api "repos/$repo/actions/workflows/$1/runs?head_sha=$sha&event=pull_request&per_page=100" \
    --jq '.workflow_runs | if length == 0 then "missing"
          else (max_by(.id) | if .status == "completed" then .conclusion else .status end) end'
}

workflows=(ci.yml)
if gh api "repos/$repo/pulls/$pr/files?per_page=100" --paginate --jq '.[].filename' |
  grep -qE '^(web/|\.github/workflows/web\.yml$)'; then
  workflows+=(web.yml)
fi
for workflow in "${workflows[@]}"; do
  conclusion=$(latest_run "$workflow")
  [ "$conclusion" = success ] || skip "has $workflow at $conclusion on $sha"
done

title="$(jq -r .title <<<"$view") (#$pr)"
kept=$(awk '
  /^<!-- CURSOR_AGENT_PR_BODY_BEGIN -->$/ { inside = 1; marked = 1; next }
  /^<!-- CURSOR_AGENT_PR_BODY_END -->$/ { inside = 0; next }
  { line[++n] = $0; kept[n] = inside }
  END { for (i = 1; i <= n; i++) if (!marked || kept[i]) print line[i] }
' <<<"$body")
message=$(grep -viE '^[[:space:]]*co-authored-by:' <<<"$kept" || true)
[ -n "$(tr -d '[:space:]' <<<"$message")" ] || skip "has an empty description, which would give GitHub's default message"

echo "#$pr: approved at its head $sha, ${workflows[*]} green; squash-merging"
if [ -n "${MERGE_DRY_RUN:-}" ]; then
  printf 'dry run: would merge %s at %s\n--- title\n%s\n--- message\n%s\n' "$pr" "$sha" "$title" "$message"
  exit 0
fi
gh api -X PUT "repos/$repo/pulls/$pr/merge" \
  -f merge_method=squash \
  -f sha="$sha" \
  -f commit_title="$title" \
  -f commit_message="$message" \
  --jq '"merged as \(.sha)"'
