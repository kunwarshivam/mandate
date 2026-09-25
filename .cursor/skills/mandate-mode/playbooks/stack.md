# Playbook: land a stack of PRs

The founder merges. Agents prepare, verify, and keep the stack reviewable.

1. Confirm each PR's CI (`fast`, `full`) and that each PR's diff is only its own step.
2. After the founder squash-merges the bottom PR, the next branch still carries the original
   commits. Rebase it onto `main` (`git rebase --onto origin/main <old-base-branch> <branch>`),
   check that its tree equals the pre-rebase tree plus nothing else, and retarget its PR to `main`.
3. The rebase needs a force push of an agent branch. Ask the founder before the first one in a
   session; after a yes, use `--force-with-lease` and never touch `main`.
4. Repeat bottom-up. Never merge, never enable auto-merge.
