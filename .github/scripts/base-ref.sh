#!/usr/bin/env bash
# Prints the commit a change is compared against, the choice xtask's `choose_base` makes, so the
# short path (DEC-112) diffs the same range as `xtask ci`:
#   1. `MANDATE_BASE_REF` when it is set; all zeros means there is no base;
#   2. on a `pull_request` event, the first parent of the merge commit CI checks out: main's tip
#      the PR was merged onto, not the PR's `base.sha`, which misses main's later changes;
#   3. otherwise the merge base with `origin/main`, and nothing when that is HEAD itself.
# Prints nothing when there is no base, except on a `pull_request` event: a pull request always has
# a base, so there it fails, as a shallow checkout would otherwise pass every check unchecked.
set -euo pipefail
base=""
read -r -a commits <<<"$(git rev-list --parents -n 1 HEAD)"
if [ -n "${MANDATE_BASE_REF:-}" ]; then
  if [[ ! "$MANDATE_BASE_REF" =~ ^0+$ ]]; then
    base="$MANDATE_BASE_REF"
  fi
elif [ "${GITHUB_EVENT_NAME:-}" = pull_request ] && [ "${#commits[@]}" -eq 3 ]; then
  base="${commits[1]}"
else
  merge_base=$(git merge-base HEAD origin/main 2>/dev/null || true)
  if [ -n "$merge_base" ] && [ "$merge_base" != "${commits[0]}" ]; then
    base="$merge_base"
  fi
fi
if [ -z "$base" ] && [ "${GITHUB_EVENT_NAME:-}" = pull_request ]; then
  echo "base-ref: a pull_request run resolved no diff base: HEAD has $((${#commits[@]} - 1)) parent(s) and origin/main gives none; check out with fetch-depth: 0" >&2
  exit 1
fi
if [ -n "$base" ]; then
  echo "$base"
fi
