# The dispatcher's fallible reads, random source, and Postgres journal (E8-10, slice D10a)

- **Spec:** [notifications spec](../../../../docs/specs/notifications.md) v0.8 §5.9 ("Where it
  sits", "Its reads can fail"), NT-8, NT-9; DEC-2500 items 2, 3, and 8.
- **Code:** `crates/mandate-dispatcher/src/step.rs` (`Journal::committed` returns a `Result`; a
  failed read ends the step, at the tests PR's stub as `Unimplemented`, after it as the closed
  `DispatchError::JournalUnavailable`); `crates/mandate-dispatcher/src/os.rs` (`OsRandom`, the
  kernel's generator through `ring` behind both `mandate_notify::SecureRandom` and
  `mandate_webpush::SecureRandom`; `EntropyUnavailable`); `crates/mandate-dispatcher/src/pg.rs`
  (`PgNotices`, the `Journal` over `PgJournal`: its notice stream's writer epoch, short reads under
  `READ_DEADLINE`, appends at that epoch); `xtask/layers.toml` (layer 9, `pure = false`,
  `allowed_dependents = []`, `mandate-agentd` and `mandate-api` forbidden). Stubs in the tests PR
  (DEC-77).
- **Tests:** `crates/mandate-dispatcher/src/step/tests/unread.rs`, over `MemoryJournal` with a
  failing mode: a failed read at each of a step's four reads ends the step with nothing tried after
  it, no issue before the issue batch and no send, and a later clean step satisfies NT-8's oracle
  (live); the step answers `JournalUnavailable`; and a property over random failing reads and
  restarts in which no send precedes its committed `NoticeIssued` and NT-8's oracle holds at the
  end. `crates/mandate-dispatcher/src/os/tests.rs`: the kernel's source fills and never repeats a
  draw; a source that cannot answer is `EntropyUnavailable` on both traits (and, live, never yields
  a draw). `crates/mandate-dispatcher/tests/pg.rs` (PG class: skips without `MANDATE_PG_URL`,
  required under `MANDATE_PG_REQUIRED`; each first checks, with no database, that a DSN that does
  not parse is refused closed): random cause sequences give byte-identical notice streams and
  sends on Postgres and in memory; a second dispatcher fences the first before its send; a read
  refused by Postgres is `JournalUnavailable` and sends and appends nothing.
- **Run:** `cargo nextest run -p mandate-dispatcher`; with a database,
  `MANDATE_PG_URL=... cargo nextest run -p mandate-dispatcher --test pg` (the D10a tests are
  pending until the code PR; `cargo nextest run -p mandate-dispatcher --run-ignored ignored-only`
  shows them failing).
