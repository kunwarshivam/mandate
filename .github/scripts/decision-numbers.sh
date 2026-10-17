#!/usr/bin/env bash
# Refuses two decision files that take one DEC number anywhere in the tree, and a decision file
# whose `# DEC-<n>:` heading names a number other than its file name's (DEC-2600 item 3). A file is
# a decision file when its name is `DEC-<digits>` then anything then `.md`; the number is read as
# an integer, so `DEC-01010.md` and `DEC-1010-draft.md` both take 1010. It reads the files git
# lists, tracked or not, from the current directory, so a lane's new reservation counts before its
# first commit. The docs-only path (`docs-checks.sh`) and the lint job run this same script.
# Usage: decision-numbers.sh   (from the repository root)
set -euo pipefail
problems=0
declare -A taken
while IFS= read -r path; do
  name=${path##*/}
  [[ $name =~ ^DEC-([0-9]+).*\.md$ ]] || continue
  number=$((10#${BASH_REMATCH[1]}))
  if [ -n "${taken[$number]:-}" ]; then
    echo "    decisions: DEC-$number is taken by both ${taken[$number]} and $path; renumber the later one" >&2
    problems=1
  else
    taken[$number]=$path
  fi
  heading=$(head -n 1 "$path")
  if [[ $heading =~ ^#\ DEC-([0-9]+): ]] && [ "$((10#${BASH_REMATCH[1]}))" -ne "$number" ]; then
    echo "    decisions: $path is headed DEC-${BASH_REMATCH[1]}; its heading and file name must name one number" >&2
    problems=1
  fi
done < <(git ls-files --cached --others --exclude-standard | sort)
[ "$problems" -eq 0 ] && echo "    decisions: ${#taken[@]} decision files, each number taken once" || exit 1
