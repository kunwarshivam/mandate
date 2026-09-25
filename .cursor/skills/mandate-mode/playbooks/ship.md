# Playbook: ship a PR

Agents land their own work (DEC-79). Green CI is necessary, not sufficient: an independent review
decides.

1. Open every PR against `main`. Never base a PR on another PR's branch: a merge into that branch
   never reaches `main`. When work is sequenced (tests, implementation, status), open the next PR
   only after the previous one has merged, from the updated `main`.
2. Wait until both required checks (`fast`, `full`) are green on the PR's final head.
3. Spawn a review agent on a different model from the author's, in a fresh cloud agent. Give it
   the PR number, the story or DEC, and this checklist; it must not have written the change:
   - check out the PR head and run `cargo xtask check`;
   - read the diff against the story's acceptance criteria, the specs it cites, and `AGENTS.md`;
   - for safety-critical crates, confirm zero missed mutants in the `full` log and run the
     `interrogate` skill on the diff;
   - return PASS or FAIL with file and line evidence for each finding.
4. On FAIL, fix the findings in the same PR and repeat from step 2.
5. On PASS, squash-merge through the GitHub API with the story ID in the title and an explicit
   `commit_message` (the PR description). GitHub's default squash message copies every commit
   message, including `Co-authored-by` trailers, which must not reach `main`. Never force-push
   `main`, never enable auto-merge on a PR that has not passed step 3.
6. Report the merge, the review verdict, and anything the founder should look at after the fact.
