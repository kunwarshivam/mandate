#!/usr/bin/env bash
# Classifies a pull request's change for CI (DEC-112). Prints `docs_only=true` when every changed
# file is documentation or the web app, so the required checks can take their short path (typos,
# the spec guard, the trailer check) without a Rust toolchain; anything else prints
# `docs_only=false`.
#
# Documentation: Markdown anywhere, `docs/` (except the reference-case YAML, which the fixtures and
# reference jobs check), `CODEOWNERS`, and `LICENSE*`. The verification skill's feature map is
# code: `cargo xtask feature-map` checks it against the workspace. The web app is everything under
# `web/`, which `.github/workflows/web.yml` checks (DEC-200).
#
# Usage: docs-only.sh <base-ref>   (the merge base with HEAD is computed here, as xtask does)
set -euo pipefail
base="$1"
merge_base=$(git merge-base "$base" HEAD)
docs_only=true
while IFS= read -r file; do
  [ -z "$file" ] && continue
  case "$file" in
    .cursor/skills/verify-mandate/feature-map.md) docs_only=false ;;
    docs/specs/reference-cases/*) docs_only=false ;;
    *.md | docs/* | CODEOWNERS | LICENSE*) ;;
    web/*) ;;
    *) docs_only=false ;;
  esac
done < <(git diff --name-only "$merge_base" HEAD)
echo "docs_only=$docs_only"
