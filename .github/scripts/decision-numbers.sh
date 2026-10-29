#!/usr/bin/env bash
# Refuses a decision number taken twice, wherever the tree holds it (DEC-2600 item 3, DEC-2650).
# A number is taken by a per-decision file — its name is `DEC-<digits>` then anything then `.md`,
# the number read as an integer, so `DEC-01010.md` and `DEC-1010-draft.md` both take 1010 — or by
# a `## DEC-<n>:` section of a category file under docs/project/decisions/ (a `.md` there whose
# name is not `DEC-` anything and not the README), which must carry the matching
# `<a id="dec-<n>">` anchor above its heading. A per-decision file's first line must head the
# number its name takes. It reads the files git lists, tracked or not, from the current directory,
# so a lane's new reservation counts before its first commit. The docs-only path
# (`docs-checks.sh`) and the lint job run this same script.
# Usage: decision-numbers.sh   (from the repository root)
set -euo pipefail
problems=0
count=0
sections=0
while IFS= read -r path; do
  name=${path##*/}
  case $name in
    DEC-[0-9]*.md) ;;
    *) continue ;;
  esac
  [ -f "$path" ] || continue
  digits=${name#DEC-}
  digits=${digits%%[!0-9]*}
  [ -n "$digits" ] || continue
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
    '# DEC-'[0-9]*:*)
      named=${heading#'# DEC-'}
      named=${named%%[!0-9]*}
      if [ "$((10#$named))" -ne "$number" ]; then
        echo "    decisions: $path is headed DEC-$named; its heading and file name must name one number" >&2
        problems=1
      fi
      ;;
  esac
done < <(git ls-files --cached --others --exclude-standard | sort)
while IFS= read -r path; do
  name=${path##*/}
  case $name in
    DEC-*.md | README.md) continue ;;
  esac
  [ -f "$path" ] || continue
  section_number=
  while IFS= read -r line; do
    case $line in
      '<a id="dec-'*'"></a>')
        digits=${line#'<a id="dec-'}
        digits=${digits%%[!0-9]*}
        if [ -n "$digits" ]; then
          section_number=$((10#$digits))
        else
          section_number=
        fi
        ;;
      '## DEC-'[0-9]*:*)
        digits=${line#'## DEC-'}
        digits=${digits%%[!0-9]*}
        [ -n "$digits" ] || continue
        number=$((10#$digits))
        if [ -z "${section_number:-}" ] || [ "$section_number" -ne "$number" ]; then
          echo "    decisions: $path heads DEC-$number with no matching anchor dec-$number above it" >&2
          problems=1
        fi
        slot=taken_$number
        if [ -n "${!slot:-}" ]; then
          echo "    decisions: DEC-$number is taken by both ${!slot} and the DEC-$number section of $path; renumber the later one" >&2
          problems=1
        else
          printf -v "$slot" '%s' "the DEC-$number section of $path"
          sections=$((sections + 1))
        fi
        section_number=
        ;;
    esac
  done < "$path"
done < <(git ls-files --cached --others --exclude-standard -- 'docs/project/decisions/*.md' | sort)
[ "$problems" -eq 0 ] && echo "    decisions: $count decision files and $sections folded sections, each number taken once" || exit 1
