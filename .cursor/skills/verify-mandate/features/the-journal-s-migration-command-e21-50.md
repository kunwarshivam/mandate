# The journal's migration command (E21-50, lane K0)

- **Spec:** ADR-0001 ES-08; `docs/design/infrastructure.md` OPS-13 and §7.2; journal spec §6.1;
  the managed-cells plan's lane K0; DEC-994.
- **Code:** `crates/mandate-journal-pg/src/migrate.rs` (the runner: `migrations`, the pure ledger
  check `applied_prefix`, `migrate`, `migrate_dsn`, `migrate_known`; roles, schema, ledger and the
  `deploy.migrations` takeover in one locked transaction); `crates/mandate-cli/src/db.rs`
  (`mandate db migrate --journal <DSN> [--schema <name>]`); `clippy.toml` (the
  `disallowed-methods` entries that keep every other caller off the runner); the guard around
  `deploy/bootstrap.sh`'s migration loop.
- **Tests:** `crates/mandate-journal-pg/tests/migrate.rs` (the ledger check against an
  independent oracle, forward steps, a downgrade, a failing file rolling the run back, a changed
  or out-of-order ledger, an unknown or foreign schema, a bad schema name, concurrent runs, a role
  that cannot act as the owner); `crates/mandate-journal-pg/tests/pg.rs`
  (`migrations_are_embedded_in_order_with_no_down_migrations`,
  `applied_migrations_are_checked_and_never_rerun`); `crates/mandate-cli/tests/db_migrate.rs`
  (the binary on a fresh database, the takeover of a database `deploy/bootstrap.sh`'s own SQL
  migrated, refusals, no part of the DSN shown); `deploy/tests/test_invariants.py`
  (`migrations_hand_over`).
- **Run:** `MANDATE_PG_URL=postgres://… cargo nextest run -p mandate-journal-pg -p mandate-cli
  -E 'binary(migrate) | binary(db_migrate) | binary(pg)'`, or `cargo xtask ci postgres`.
