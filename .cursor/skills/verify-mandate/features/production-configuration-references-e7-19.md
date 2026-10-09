# Production configuration references (E7-19)

- **Spec:** `docs/specs/journal.md` §9's `config_refs` table and its append-time reference check
  for `policy_set` and `model_registry` — the object is present, canonical, and carries the
  expected top-level `kind`, and nothing beyond that; DEC-484.
- **Code:** `crates/mandate-journal/src/lib.rs` (`validate_config_artifacts` and
  `referenced_config_object`, reached through `config_artifact_refusal` by
  `MemoryJournal::append_with_config_artifacts` and `PgJournal::append_with_config_artifacts`),
  `crates/mandate-journal/src/draft.rs` (`Draft::config_artifact_path`, the version-2 records a
  plain append cannot check, named so both stores fail closed the same way),
  `crates/mandate-journal-pg/src/lib.rs` (`missing_config_artifact`, applied after §5.1 step 1's
  idempotency so an identical retry still answers `AlreadyCommitted`).
- **Tests:** `crates/mandate-refcases/tests/production_config_refs.rs` (the four reference tests:
  version-2 agent records and registrations bind their objects, every invalid reference is
  refused as specified, and every new reference requires its stored object),
  `crates/mandate-journal/src/lib.rs`'s `production_config_tests` and
  `crates/mandate-journal-pg/src/lib.rs`'s
  `a_version_two_configuration_draft_passes_validation_and_is_refused_after_idempotency` (the
  plain-append refusal, which no external oracle covers).
  J0 (DEC-510): `crates/mandate-journal-pg/tests/pg_config_artifacts.rs` (the Postgres
  artifact-aware append, `PgJournal::append_with_config_artifacts`, gives `MemoryJournal`'s
  outcomes, heads, and rows; refusals write nothing; §5.1's order; a property against an outcome
  built from each step). Both journals call `mandate_journal::config_artifact_refusal`, one
  predicate (DEC-510 item 4). Each test starts with a database-free property, so without Postgres
  that property still runs.
- **Reference cases:** the `production_config_refs` section of `fixtures/refcases/journal.json`.
- **Not covered here:** DEC-484 item 4's object shape (`policy_set_version`, level order and
  uniqueness, sorted unique models and params) is not checked at append, because the spec's
  append-time check is only absence and a differing top-level `kind`; the validated production
  input owns it, as the E7-19 backlog row says.
- **Run:** `cargo nextest run -p mandate-journal -p mandate-journal-pg -p mandate-refcases -E
  'binary(production_config_refs) | package(mandate-journal) | package(mandate-journal-pg)'`. J0's
  Postgres tests: `MANDATE_PG_URL=postgres://… cargo nextest run -p mandate-journal-pg --test
  pg_config_artifacts`.
