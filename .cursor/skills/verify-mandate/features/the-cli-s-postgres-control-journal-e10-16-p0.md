# The CLI's Postgres control journal (E10-16, P0)

- **Spec:** `docs/specs/journal.md` §5.1, §6.1, §11 check 6; the first paper trade brief
  (`docs/project/tasks/first-paper-trade.md`, P0 and X-12); DEC-510, DEC-520.
- **Code:** `crates/mandate-cli/src/postgres.rs` (`JournalArgs`, the `--journal` and `--store`
  options D1, D2 and V0 flatten; `PgControlJournal`, `mandate-journal-pg` behind `ControlJournal`,
  appending through J0's artifact-aware append with the `mandate-artifacts-fs` store; the DSN a
  `SecretString` exposed only to `PgJournal::from_dsn`).
- **Tests:** `crates/mandate-cli/tests/postgres.rs` (the control-stream vectors byte for byte, a
  registration refused until its object is in the store, as `MemoryJournal` answers, and
  ownership, fencing and retries as the CLI tests' journal answers; each starts with a
  database-free DSN property; the options parse and hide the DSN), and the in-module
  `waiting_sleeps_for_the_whole_delay`.
- **`journal export` (V0, DEC-522):** `crates/mandate-cli/src/journal/export.rs` (one stream to
  the §6.2 segment `journal verify` reads, written whole to `<out>.tmp` and published by a hard
  link that never replaces a file), and `main`, synchronous so the journal's runtime is the only
  one; its tests are the four export tests in `crates/mandate-cli/tests/postgres.rs` (the vectors'
  segment, line for line, verified with its store; an existing file and an empty stream refused;
  the binary exporting and verifying; no refusal naming the DSN or creating a file), each running
  without a database until it reaches the journal.
- **Run:** `MANDATE_PG_URL=postgres://… cargo nextest run -p mandate-cli --test postgres`, or
  `cargo xtask ci postgres`, which runs `mandate-cli` beside `mandate-journal-pg`.
