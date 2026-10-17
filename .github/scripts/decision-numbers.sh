#!/usr/bin/env bash
# Refuses two decision files that take one DEC number anywhere in the tree, and a decision file
# whose `# DEC-<n>:` heading names a number other than its file name's (DEC-2600 item 3). A file is
# a decision file when its name is `DEC-<digits>` then anything then `.md`, and it exists; the number is read as
# an integer, so `DEC-01010.md` and `DEC-1010-draft.md` both take 1010. It reads the files git
# lists, tracked or not, from the current directory, so a lane's new reservation counts before its
# first commit. The docs-only path (`docs-checks.sh`) and the lint job run this same script.
# Usage: decision-numbers.sh   (from the repository root)
set -euo pipefail
problems=0
count=0
while IFS= read -r path; do
  name=${path##*/}
  case $name in
    DEC-[0-9]*.md) ;;
    *) continue ;;
  esac
  [ -f "$path" ] || continue
  digits=${name#DEC-}
  digits=${digits%%[!0-9]*}
  number=$((10#$digits))
  slot=taken_$number
  if [ -n "${!slot:-}" ]; then
    echo "    decisions: DEC-$number is taken by both ${!slot} and $path; renumber the later one" >&2
    problems=1
  else
    printf -v "$slot" '%s' "$path"
    count=$((count + 1))
  fi
  heading=$(head -n 1 "$path")
  case $heading in
    "# DEC-"[0-9]*:*)
      named=${heading#"# DEC-"}
      named=${named%%[!0-9]*}
      if [ "$((10#$named))" -ne "$number" ]; then
        echo "    decisions: $path is headed DEC-$named; its heading and file name must name one number" >&2
        problems=1
      fi
      ;;
  esac
done < <(git ls-files --cached --others --exclude-standard | sort)
[ "$problems" -eq 0 ] && echo "    decisions: $count decision files, each number taken once" || exit 1
