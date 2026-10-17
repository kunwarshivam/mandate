#!/bin/bash
# usage: reapprove.sh PR "why text"  -> approves the PR's current head, prints PR:SHA
P=$1; WHY=$2; S=$(dirname "$0")
H=$(gh api repos/kunwarshivam/mandate/pulls/$P --jq .head.sha)
gh api repos/kunwarshivam/mandate/pulls/$P --jq .body > $S/b$P.md
python3 - "$S/b$P.md" "$H" "$WHY" <<'EOF'
import sys,re
p,h,why=sys.argv[1:4]
s=open(p).read()
s=re.sub(r"\n*Coordinator check:.*?\n\nCoordinator-approved-head: [0-9a-f]+\n","\n",s,flags=re.S)
add=f"\n\nCoordinator check: {why} Merges once fast and full are green at this head.\n\nCoordinator-approved-head: {h}\n"
s=s.replace("\n🤖 Generated", add+"\n🤖 Generated",1) if "\n🤖 Generated" in s else s+add
open(p,'w').write(s)
EOF
gh api -X PATCH repos/kunwarshivam/mandate/pulls/$P -F body=@$S/b$P.md --jq .number >/dev/null && gh api -X POST repos/kunwarshivam/mandate/issues/$P/labels -f 'labels[]=coordinator-approved' --jq '.[].name' >/dev/null && echo "$P:$H"
