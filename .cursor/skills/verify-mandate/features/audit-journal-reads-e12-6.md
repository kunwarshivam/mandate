# Audit journal reads (E12-6)

- **Spec:** `docs/specs/workspace-api.md` §4.8.1 (streams and pages, the one 404), API-9, API-15,
  AU-1, AU-2; DEC-760 (bounds, cursor, head, the 404, the stream list); DEC-770 (the readings
  §4.8.1 leaves open: whole-segment ownership, never-written streams, the stream-list cursor, the
  workspace taken only from a `Tenant`).
- **Code:** `mandate-audit`: `crates/mandate-audit/src/lib.rs` (`JournalRead`, scoped to the
  workspace of the caller's `mandate_identity::Tenant`, and `MemoryRead` over `MemoryJournal`,
  which lists streams with `MemoryJournal::stream_ids`). Pure and read-only: it writes no journal.
- **Tests:** `crates/mandate-audit/tests/pages.rs` (page bounds, the cursor, the head and
  `at_head`, and paging under concurrent appends with an independent chain check),
  `crates/mandate-audit/tests/scope.rs` (the stream list and its paging, and foreign, malformed and
  absent ids reading the same, as real tenant contexts), `crates/mandate-journal/tests/append.rs`
  (`stream_ids_lists_written_streams_in_byte_order`).
- **Run:** `cargo nextest run -p mandate-audit`.
