# Postgres journal

- **Spec:** `docs/specs/journal.md` §5.1, §6.1, §11; ADR-0001 ES-08; DEC-109;
  `docs/project/tasks/E5-3-postgres-journal.md`.
- **Code:** `mandate-journal-pg`: `crates/mandate-journal-pg/src/lib.rs` (`PgJournal`: the append
  protocol in one transaction holding the head row, reads that re-verify stored bytes, `migrator`);
  the schema, roles, and triggers in `migrations/`.
- **Tests:** `crates/mandate-journal/tests/conformance/mod.rs` (the append suite every journal
  backend runs: stream rules, fencing, `risk_clock`, retries, rejections, the chain and append
  vectors, and a differential property against `MemoryJournal`), run by
  `crates/mandate-journal/tests/memory_conformance.rs` and
  `crates/mandate-journal-pg/tests/pg_conformance.rs`; `crates/mandate-journal-pg/tests/pg.rs`
  (migrations, privileges and triggers, database-enforced chain rules, concurrent appenders and
  owners, failed and lost commits, re-verification on read, the tamper vectors replayed on stored
  rows). Setup: `crates/mandate-journal-pg/README.md`.
- **Run:** `MANDATE_PG_URL=postgres://… cargo nextest run -p mandate-journal-pg`, or
  `cargo xtask ci postgres`; without `MANDATE_PG_URL` the Postgres tests skip.
