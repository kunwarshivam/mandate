#!/bin/bash
# usage: safemerge.sh PR [PROTECTED_PR ...]
# Merges PR at its current head only if: a Sonnet PASS line names that head, fast and full are green,
# GitHub says it is mergeable, and merging it would not conflict with any protected PR's head.
cd /home/user/mandate
R=kunwarshivam/mandate; P=$1; shift
H=$(gh api repos/$R/pulls/$P --jq .head.sha)
ms=$(gh api repos/$R/pulls/$P --jq .mergeable_state)
gh api repos/$R/pulls/$P --jq .body | grep -q "${H:0:8}" || { echo "#$P: no review line naming ${H:0:8}"; exit 1; }
r=$(gh api "repos/$R/commits/$H/check-runs?per_page=100" --jq '[.check_runs[] | select(.name=="fast" or .name=="full")] | map("\(.name)=\(.conclusion)") | sort | join(",")')
[ "$r" = "fast=success,full=success" ] || { echo "#$P: checks $r"; exit 1; }
[ "$ms" = dirty ] && { echo "#$P: conflicts with main"; exit 1; }
git fetch -q origin main "pull/$P/head:sm$P" || exit 1
t=$(git merge-tree --write-tree origin/main sm$P | head -1); c=$(git commit-tree "$t" -p origin/main -p sm$P -m probe) || exit 1
for Q in "$@"; do
  git fetch -q origin "pull/$Q/head:sm$Q" || continue
  if ! git merge-tree --write-tree --name-only $c sm$Q >/dev/null 2>&1; then echo "#$P: would conflict with protected #$Q; held"; exit 1; fi
done
gh api -X PUT repos/$R/pulls/$P/merge -f merge_method=squash -f sha=$H --jq '"#'$P' merged \(.merged)"'
