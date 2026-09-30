#!/usr/bin/env bash
# Squash-merges one pull request the coordinator approved (DEC-175), or does nothing.
#
# A pull request merges only when all of these hold, checked here and not trusted from the event:
# it is open, not a draft, targets `main`, carries the `coordinator-approved` label, and the
# required checks `fast` and `full` both succeeded on its current head. The merge names that head
# (`sha`), so a push that lands after the checks were read makes GitHub refuse it; a push also
# removes the label (`merge.yml`), so it needs a fresh approval. The commit message is the pull
# request's own description with any `Co-authored-by` line removed, never GitHub's default, which
# copies every commit message and their trailers onto `main` (ship playbook, step 5).
#
# Usage: merge-approved.sh <pull request number>   (needs GH_TOKEN and GITHUB_REPOSITORY;
# MERGE_DRY_RUN=1 reads everything and prints the merge instead of making it)
set -euo pipefail

pr=${1:?pull request number}
repo=${GITHUB_REPOSITORY:?}
label=coordinator-approved

view=$(gh pr view "$pr" --repo "$repo" \
  --json number,state,isDraft,baseRefName,headRefOid,labels,title,body)

if [ "$(jq -r .state <<<"$view")" != OPEN ]; then
  echo "#$pr is not open; nothing to merge"
  exit 0
fi
if [ "$(jq -r .isDraft <<<"$view")" = true ]; then
  echo "#$pr is a draft; not merging"
  exit 0
fi
if [ "$(jq -r .baseRefName <<<"$view")" != main ]; then
  echo "#$pr does not target main; not merging"
  exit 0
fi
if ! jq -e --arg l "$label" '.labels | any(.name == $l)' <<<"$view" >/dev/null; then
  echo "#$pr does not carry $label; not merging"
  exit 0
fi

sha=$(jq -r .headRefOid <<<"$view")
for check in fast full; do
  conclusion=$(gh api "repos/$repo/commits/$sha/check-runs?check_name=$check&per_page=100" \
    --jq '[.check_runs[] | select(.app.slug == "github-actions")]
          | if length == 0 then "missing" else (max_by(.started_at) | .conclusion // "pending") end')
  if [ "$conclusion" != success ]; then
    echo "#$pr: check $check on $sha is $conclusion; not merging yet"
    exit 0
  fi
done

title="$(jq -r .title <<<"$view") (#$pr)"
message=$(jq -r '.body // ""' <<<"$view" | tr -d '\r' | awk '
  /<!-- CURSOR_AGENT_PR_BODY_BEGIN -->/ { inside = 1; marked = 1; next }
  /<!-- CURSOR_AGENT_PR_BODY_END -->/ { inside = 0; next }
  { line[++n] = $0; kept[n] = inside }
  END { for (i = 1; i <= n; i++) if (!marked || kept[i]) print line[i] }
' | grep -viE '^[[:space:]]*co-authored-by:' || true)

echo "#$pr: fast and full green on $sha; squash-merging"
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
