#!/usr/bin/env bash
# The short path's checks for a documentation-only change (DEC-112), mirroring xtask's
# `spec-guard` and `commit-trailers` parts so a docs PR needs no Rust toolchain:
#   1. `typos` over the repository (the same binary and config `xtask ci lint` uses);
#   2. protected paths (ES-22) changed only with a DEC-<n> cited in the PR description or a commit;
#   3. no `Co-authored-by` trailer on any commit.
# Usage: MANDATE_PR_BODY=<body> docs-checks.sh   (the base is `base-ref.sh`'s, as xtask chooses it)
set -euo pipefail
base=$("$(dirname "$0")/base-ref.sh")
problems=0
echo "    $ typos"
typos || problems=1
if [ -z "$base" ]; then
  echo "    spec-guard, commit-trailers: HEAD is the base; nothing to check"
  [ "$problems" -eq 0 ] && echo "    docs checks passed" || exit 1
  exit 0
fi
merge_base=$(git merge-base "$base" HEAD)
protected=$(git diff --name-only "$merge_base" HEAD | grep -E '^(docs/specs/|schemas/|reference/|fixtures/refcases/|crates/mandate-refcases/status\.toml)' || true)
if [ -n "$protected" ]; then
  messages="${MANDATE_PR_BODY:-}
$(git log --format=%B "$merge_base..HEAD")"
  if ! grep -Eq 'DEC-[0-9]+' <<<"$messages"; then
    echo "    spec-guard: protected paths changed but no DEC-<n> is cited in the PR description or commits" >&2
    problems=1
  else
    echo "    spec-guard: protected paths changed with a DEC cited"
  fi
else
  echo "    spec-guard: no protected paths changed"
fi
while IFS= read -r line; do
  sha=${line%% *}
  trailer=${line#* }
  if [ -n "$(echo "$trailer" | tr -d '[:space:]')" ] && [ "$trailer" != "$sha" ]; then
    echo "    commit-trailers: commit $sha has a Co-authored-by trailer; remove it" >&2
    problems=1
  fi
done < <(git log --format='%h %(trailers:key=Co-authored-by,valueonly,separator=%x2C)' "$merge_base..HEAD")
[ "$problems" -eq 0 ] && echo "    docs checks passed" || exit 1
