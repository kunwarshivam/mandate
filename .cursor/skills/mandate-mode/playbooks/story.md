# Playbook: implement a backlog story

1. Read the story in `docs/project/06-backlog-v1.md`, its specs, and the decisions that apply.
   Write the task brief from `docs/project/templates/task.md` into `docs/project/tasks/`, including
   the interpretations the founder should confirm and the decisions needed.
2. Check the stop conditions (AGENTS.md "How to work", ES-15). A blocking founder decision becomes
   a DEC proposal; do the parts it does not block.
3. Name the data shapes. For an API that crosses a crate boundary, run pstack `architect`.
4. Write the tests first: one named test per invariant or "never/always" clause, each oracle
   computing the answer its own way. Reference cases stay pending.
5. Implement until the tests and pending reference cases pass
   (`cargo test -p mandate-refcases -- --include-ignored`).
6. Verify with the `verify-mandate` skill: `cargo xtask check`, zero missed mutants, and a planted
   bug for every new oracle or harness case.
7. For safety-critical code, run pstack `interrogate` on the diff and fix what it finds.
8. Split into the DEC-77 stack: tests PR (API stubs, pending markers), implementation PR (test
   files change only by deleting `#[ignore = "pending <story>"]` lines), status PR
   (`status.toml` only). Check each commit range with `cargo xtask check` and
   `MANDATE_BASE_REF=<parent> cargo xtask ci spec-guard`.
9. Push and open draft PRs from `.github/pull_request_template.md`, each based on the one below.
   Report per `SKILL.md`.
