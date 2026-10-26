#!/usr/bin/env bash
# Refuses every tracked file with a line that starts with one of the two lines git writes around a
# merge conflict, each followed by a space and the side's name: a conflict resolved by hand that
# kept them lands both sides and git's markup, as deploy/cell/README.md did after #2622. A bare
# `=======` is left out, since Markdown underlines a heading with it. Binary files are searched
# too. The docs-only path (`docs-checks.sh`) and the lint job run this same script.
# Usage: conflict-markers.sh   (from the repository root)
set -euo pipefail
echo "    conflict-markers: checking that no tracked file keeps a merge conflict's markers"
status=0
hits=$(git grep -naE '^(<{7}|>{7}) ' -- .) || status=$?
if [ "$status" -eq 0 ]; then
  count=0
  while IFS=: read -r path line _; do
    echo "$path:$line: a merge conflict marker; resolve the conflict and remove the markup" >&2
    count=$((count + 1))
  done <<<"$hits"
  echo "conflict-markers: $count problem(s)" >&2
  exit 1
fi
if [ "$status" -ne 1 ]; then
  echo "conflict-markers: git grep failed with status $status" >&2
  exit "$status"
fi
