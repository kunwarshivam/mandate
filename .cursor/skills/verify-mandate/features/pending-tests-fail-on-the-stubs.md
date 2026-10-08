# Pending tests fail on the stubs

- **Spec:** DEC-77 (tests PR, implementation PR, status PR), DEC-110, DEC-137; the story playbook
  step 8. Gap 1 is closed only in part: the gate reads a failure's text, not its cause (E1-3).
- **Code:** `pending_problems`, `pending_tests`, `STUB_MARKERS`, `names_a_stub` and
  `generated_pending_markers` in `xtask/src/main.rs` (markers found on tokens in every tracked or
  untracked `.rs` file, run in one nextest `--run-ignored ignored-only`; each failure must carry
  its stub's own report, and a marker the scan cannot attach to a named function is a `markers`
  failure).
- **Tests:** the `xtask` unit tests (markers in comments, doc comments, strings, raw strings, split
  across lines; a marker inside a `macro_rules!` body, on a `$`-named function, or on no named
  function at all; which bodies are stubs; result matching) and a fixture workspace in a temporary
  git repository in
  which committed, uncommitted, and untracked pending tests pass on the stubs or fail away from
  them.
- **Run:** `cargo nextest run -p xtask`; `cargo xtask ci pending`; `cargo xtask ci mutants`.
