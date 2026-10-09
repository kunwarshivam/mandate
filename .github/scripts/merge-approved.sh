#!/usr/bin/env bash
# Squash-merges one pull request the coordinator approved (DEC-175), or does nothing.
#
# An approval has two parts. The `coordinator-approved` label is the act of approval: GitHub lets
# only people and apps with triage access or above apply it. The description line
# `Coordinator-approved-head: <40-hex sha>` binds that approval to the head the independent review
# passed, so a push moves the head off it and withdraws the approval with nothing to cancel. The
# line's integrity does not rest on a GitHub permission alone: anyone with write access can apply
# the label and edit a description. So the script also requires that the label's most recent
# application, and the description's most recent edit (or, if it was never edited, its author),
# came from an approver: a login in MERGE_APPROVERS (space-separated), by default the repository's
# owner, whose account the coordinator acts through. A collaborator who labels a pull request, or
# moves its approved head by editing the description, therefore withdraws the approval rather than
# granting one.
#
# A pull request merges only when all of these hold, read here from GitHub and never from the event:
# it is open, not a draft, targets `main`, and carries the label; its description names exactly one
# approved head, outside any code fence, and that head is its current head; GitHub reports it
# mergeable; and the latest `ci` run for that head from a `pull_request` event succeeded with its
# `fast` and `full` jobs run and green, not skipped as on a draft (DEC-610), plus the latest `web`
# run when it touches `web/`, since DEC-112 lets `fast` and `full` skip the web checks. The merge names the head (`sha`), so a push after these reads makes
# GitHub refuse it. The commit message is the description's own body with any `Co-authored-by`
# line removed, never GitHub's default, which copies every commit message and its trailers onto
# `main` (ship playbook, step 5).
#
# Usage: merge-approved.sh <pull request number>   (needs GH_TOKEN and GITHUB_REPOSITORY;
# MERGE_DRY_RUN=1 reads everything and prints the merge instead of making it;
# MERGE_RETRY_S sets the wait between reads while GitHub is still computing mergeability)
set -euo pipefail

pr=${1:?pull request number}
repo=${GITHUB_REPOSITORY:?}
label=coordinator-approved
approvers=" ${MERGE_APPROVERS:-${repo%%/*}} "

skip() {
  echo "#$pr $1; not merging"
  exit 0
}

for attempt in 1 2 3; do
  view=$(gh pr view "$pr" --repo "$repo" \
    --json number,state,isDraft,baseRefName,headRefOid,mergeable,labels,title,body)
  [ "$(jq -r .mergeable <<<"$view")" = UNKNOWN ] || break
  [ "$attempt" -eq 3 ] || sleep "${MERGE_RETRY_S:-5}"
done

[ "$(jq -r .state <<<"$view")" = OPEN ] || skip "is not open"
[ "$(jq -r .isDraft <<<"$view")" != true ] || skip "is a draft"
[ "$(jq -r .baseRefName <<<"$view")" = main ] || skip "does not target main"
jq -e --arg l "$label" '.labels | any(.name == $l)' <<<"$view" >/dev/null ||
  skip "does not carry $label"

sha=$(jq -r .headRefOid <<<"$view")
body=$(jq -r '.body // ""' <<<"$view" | tr -d '\r')
approvals=$(awk '/^[[:space:]]*(```|~~~)/ { fenced = !fenced; next } !fenced' <<<"$body" |
  sed -nE 's/^[[:space:]]*([-*][[:space:]]+)?Coordinator-approved-head:[[:space:]]*([0-9a-fA-F]{40})[[:space:]]*$/\2/p' |
  tr 'A-F' 'a-f')
count=$(grep -c . <<<"$approvals" || true)
[ "$count" -gt 0 ] || skip "names no Coordinator-approved-head"
[ "$count" -eq 1 ] || skip "names more than one Coordinator-approved-head"
[ "$approvals" = "$sha" ] || skip "was approved at $approvals, but its head is $sha"

labeled_by=$(gh api "repos/$repo/issues/$pr/events?per_page=100" --paginate \
  --jq ".[] | select(.event == \"labeled\" and .label.name == \"$label\") | .actor.login" | tail -n 1)
[[ "$approvers" == *" $labeled_by "* ]] ||
  skip "carries $label applied by ${labeled_by:-nobody}, who is not an approver"
owner=${repo%%/*}
name=${repo#*/}
# The GraphQL variables are GitHub's, not the shell's, so the query stays in single quotes.
# shellcheck disable=SC2016
written_by=$(gh api graphql -F owner="$owner" -F name="$name" -F number="$pr" -f query='
  query($owner: String!, $name: String!, $number: Int!) {
    repository(owner: $owner, name: $name) {
      pullRequest(number: $number) { author { login } editor { login } }
    }
  }' --jq '.data.repository.pullRequest | (.editor.login // .author.login // "")')
[[ "$approvers" == *" $written_by "* ]] ||
  skip "has its description last written by ${written_by:-nobody}, who is not an approver"

mergeable=$(jq -r .mergeable <<<"$view")
[ "$mergeable" = MERGEABLE ] || skip "is not mergeable yet ($mergeable)"

# The latest run of a workflow for the head, as its conclusion (or status, while it runs) and its
# id: "missing 0" when there is none.
latest_run() {
  gh api "repos/$repo/actions/workflows/$1/runs?head_sha=$sha&event=pull_request&per_page=100" \
    --jq '.workflow_runs | if length == 0 then "missing 0"
          else (max_by(.id) | "\(if .status == "completed" then .conclusion else .status end) \(.id)") end'
}

# The conclusions of a run's `fast` and `full` jobs, as "fast=<conclusion> full=<conclusion>". `ci`
# skips every job while a pull request is a draft (DEC-610), and a run whose jobs were all skipped
# can conclude `success`; only a run in which both required jobs ran and passed counts.
required_jobs() {
  gh api "repos/$repo/actions/runs/$1/jobs?per_page=100" --paginate \
    --jq '.jobs[] | select(.name == "fast" or .name == "full") | "\(.name)=\(.conclusion)"' |
    sort | paste -sd ' ' -
}

workflows=(ci.yml)
files=$(gh api "repos/$repo/pulls/$pr/files?per_page=100" --paginate --jq '.[].filename')
if grep -qE '^(web/|\.github/workflows/web\.yml$)' <<<"$files"; then
  workflows+=(web.yml)
fi
for workflow in "${workflows[@]}"; do
  read -r conclusion run <<<"$(latest_run "$workflow")"
  [ "$conclusion" = success ] || skip "has $workflow at $conclusion on $sha"
  if [ "$workflow" = ci.yml ]; then
    jobs=$(required_jobs "$run")
    [ "$jobs" = "fast=success full=success" ] ||
      skip "has ci.yml run $run on $sha without fast and full both run and green (${jobs:-neither found}); a run skipped on a draft does not count"
  fi
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
