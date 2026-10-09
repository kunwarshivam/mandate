# Audit journal reads (E12-6)

- **Spec:** `docs/specs/workspace-api.md` §4.8.1 (streams and pages, the one 404), API-9, API-15,
  AU-1, AU-2; DEC-760 (bounds, cursor, head, the 404, the stream list); DEC-770 (the readings
  §4.8.1 leaves open: whole-segment ownership, never-written streams, the stream-list cursor).
- **Code:** `mandate-audit`: `crates/mandate-audit/src/lib.rs` (`JournalRead`, scoped to the
  caller's workspace, and `MemoryRead` over `MemoryJournal`). Pure and read-only: it writes no
  journal.
- **Tests:** `crates/mandate-audit/tests/pages.rs` (pending E12-6 until slice A1's
  implementation: page bounds, the cursor, the head and `at_head`, and a chain check over a whole
  stream).
- **Run:** `cargo nextest run -p mandate-audit --run-ignored all`.
