# Playbook: implement a backlog story

1. Read the story in `docs/project/06-backlog-v1.md`, its specs, and the decisions that apply.
   Write the task brief from `docs/project/templates/task.md` into `docs/project/tasks/`, including
   the interpretations you made and the decisions you took.
2. Check the stop conditions (AGENTS.md "How to work", ES-15). For each, record a decision per
   `SKILL.md` ("Decide, record, continue") and keep going.
3. Name the data shapes. For an API that crosses a crate boundary, write the caller's usage first,
   then the types and signatures as stubs (`SKILL.md`, "Skills to reach for").
4. Write the tests first: one named test per invariant or "never/always" clause, each oracle
   computing the answer its own way. Reference cases stay pending.
5. Implement until the tests and pending reference cases pass
   (`cargo test -p mandate-refcases -- --include-ignored`).
6. Verify with the `verify-mandate` skill: `cargo xtask check`, zero missed mutants, and a planted
   bug for every new oracle or harness case.
7. For safety-critical code, run the `interrogate` skill on the diff and fix what it finds.
8. Deliver the DEC-77 sequence: tests PR (API stubs, pending markers), then implementation PR
   (test files change only by deleting `#[ignore = "pending <story>"]` lines), then status PR
   (`status.toml` only). Check each commit range with `cargo xtask check` and
   `MANDATE_BASE_REF=<parent> cargo xtask ci spec-guard` before opening anything. Every pending test
   must fail on the stubs: the `fast` check runs every test marked pending in the workspace
   (`cargo xtask ci pending`, DEC-110) and names any that passes or does not run.
9. Ship each PR with `playbooks/ship.md`, from `.github/pull_request_template.md`, one at a time
   against `main`. Report per `SKILL.md`.
