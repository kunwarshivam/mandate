#!/bin/bash
# usage: mw.sh PR:SHA ...  merges each when fast+full succeed at SHA; drops it if the head moves or it conflicts
declare -A todo; for a in "$@"; do todo[${a%%:*}]=${a##*:}; done
for i in $(seq 1 110); do
  for P in "${!todo[@]}"; do H=${todo[$P]}
    cur=$(gh api repos/kunwarshivam/mandate/pulls/$P --jq '"\(.head.sha) \(.merged) \(.mergeable_state)"' 2>/dev/null) || continue
    set -- $cur
    if [ "$2" = true ]; then echo "#$P merged (by other)"; unset "todo[$P]"; continue; fi
    if [ "$1" != "$H" ]; then echo "#$P head moved to $1, skipped"; unset "todo[$P]"; continue; fi
    if [ "$3" = dirty ]; then echo "#$P conflicts with main"; unset "todo[$P]"; continue; fi
    r=$(gh api "repos/kunwarshivam/mandate/commits/$H/check-runs?per_page=100" --jq '[.check_runs[] | select(.name=="fast" or .name=="full")] | map("\(.name)=\(.conclusion)") | sort | join(",")')
    if [ "$r" = "fast=success,full=success" ]; then
      out=$(gh api -X PUT repos/kunwarshivam/mandate/pulls/$P/merge -f merge_method=squash -f sha=$H --jq '.sha' 2>&1)
      if echo "$out" | grep -q "^[0-9a-f]\{40\}$"; then echo "#$P merged $out"; unset "todo[$P]";
      elif echo "$out" | grep -qi "conflict"; then echo "#$P conflicts: $out"; unset "todo[$P]";
      else echo "#$P merge retry later: $(echo $out | head -c 120)"; fi
    elif echo "$r" | grep -q "failure\|cancelled"; then echo "#$P checks: $r"; unset "todo[$P]"; fi
  done
  [ ${#todo[@]} -eq 0 ] && exit 0; sleep 60
done
echo "timeout: ${!todo[@]}"
