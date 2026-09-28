#!/usr/bin/env bash
# Prints the commit a change is compared against, the choice xtask's `choose_base` makes, so the
# short path (DEC-112) diffs the same range as `cargo xtask ci`:
#   1. `MANDATE_BASE_REF` when it is set; all zeros means there is no base;
#   2. on a `pull_request` event, the first parent of the merge commit CI checks out: main's tip
#      the PR was merged onto, not the PR's `base.sha`, which misses main's later changes;
#   3. otherwise the merge base with `origin/main`, and nothing when that is HEAD itself.
# Prints nothing when there is no base.
set -euo pipefail
if [ -n "${MANDATE_BASE_REF:-}" ]; then
  if [[ ! "$MANDATE_BASE_REF" =~ ^0+$ ]]; then
    echo "$MANDATE_BASE_REF"
  fi
  exit 0
fi
read -r -a commits <<<"$(git rev-list --parents -n 1 HEAD)"
if [ "${GITHUB_EVENT_NAME:-}" = pull_request ] && [ "${#commits[@]}" -eq 3 ]; then
  echo "${commits[1]}"
  exit 0
fi
merge_base=$(git merge-base HEAD origin/main 2>/dev/null || true)
if [ -n "$merge_base" ] && [ "$merge_base" != "${commits[0]}" ]; then
  echo "$merge_base"
fi
