# Task: E5-3 Hardened Postgres journal

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). One task implements one
story. Fill every section; write "none" rather than deleting one.

## Story

- **Story:** E5-3 ([backlog](../06-backlog-v1.md#e5-journal))
- **Acceptance criteria (verbatim):** "As an operator, I want the Postgres journal hardened (body
  stored as exact canonical bytes with a hash check, append-only roles and triggers including
  TRUNCATE, stream heads with writer fencing) so that records cannot be altered by application
  code."
- **PRD / HLD / spec anchors:** PRD 6.7 (FR-7.1 to FR-7.7); HLD "Record before acting"; journal spec
  §1, §2 (`risk_clock`), §5.1 (append protocol), §5.3 (outcomes), §6.1 (hot store), §11
  (verification).
- **Decisions that apply (DEC-NN):** DEC-72 (ADR-0001: ES-02 layer 6 and safety-critical, ES-08
  Postgres, ES-09 error codes, ES-11 tests, ES-12 CI, ES-14 dependencies), DEC-76 (`fast` and
  `full`), DEC-77 (tests PR, then implementation PR), DEC-81 (`risk_clock`), DEC-109 (this
  story's choices, below).

## Scope

- **Reference cases that must move from pending to passing:** none. The 46 journal cases already
  pass on `MemoryJournal`; this story replays the chain, append, and tamper vectors against
  Postgres in its own tests (ES-08), so there is no status PR.
- **Invariants touched** (each a named test; `C::` is the conformance suite in
  `crates/mandate-journal/tests/conformance/mod.rs`, run by both backends; the rest are in
  `crates/mandate-journal-pg/tests/pg.rs`):

  | Clause | Test |
  |---|---|
  | §5.1, §5.3: identical outcomes and rejection reasons on both backends | `C::matches_the_memory_journal_on_random_sequences` (differential, `MemoryJournal` as the oracle), `C::append_vectors`, `C::stream_rules` |
  | §6.1 exact canonical bytes; the chain vectors byte for byte | `C::chain_vectors_byte_for_byte` |
  | §1.3 fencing; one owner per epoch | `C::ownership_and_fencing`, `concurrent_owners_get_distinct_epochs` |
  | §2 `risk_clock` never decreases along a stream | `C::risk_clock_never_decreases_along_a_stream` |
  | §5.1 idempotency first; retries return the stored events | `C::identical_retries_return_the_stored_events`, `racing_retries_and_shared_event_ids_commit_once` |
  | A rejected batch writes nothing and uses no `seq` | `C::rejected_batches_write_nothing_and_use_no_seq` |
  | Transactional append with the head locked: no fork, no `seq` reuse | `concurrent_appenders_never_fork_or_reuse_a_seq` |
  | A crash between write and commit leaves no partial event | `a_failed_commit_leaves_no_partial_event` (`Unavailable`), `a_connection_lost_during_commit_is_ambiguous_and_atomic` (`Ambiguous`) |
  | Stored bytes are re-verified on read (§11 checks 1 to 5) | `stored_bytes_are_reverified_on_read`, `tamper_vectors_are_caught_when_the_stored_rows_are_read` |
  | Reads do not need the artifact store (§11 checks 6 and 7) and still check everything else | `C::events_with_artifact_references_are_stored_and_read_back`, `reads_pass_over_artifact_checks_and_verify_every_other_check` |
  | §6.1 append-only in the database: privileges | `the_application_role_can_only_insert_and_select_events` |
  | §6.1 append-only in the database: triggers, including TRUNCATE, binding owner and superuser | `triggers_reject_changes_even_from_the_owner_and_superusers` |
  | §6.1 `CHECK (hash = sha256(body))`, chain links, gapless, heads only advance | `the_database_rejects_bad_rows_forks_gaps_and_head_rollbacks` |
  | ES-08 versioned, forward-only migrations | `migrations_are_embedded_in_order_with_no_down_migrations`, `applied_migrations_are_checked_and_never_rerun` |
  | Database unreachable: nothing changes, and drafts are still validated first | `an_unreachable_database_is_unavailable_and_changes_nothing` |

- **Crates in scope:** `mandate-journal-pg` (new; layer 6, safety-critical). In `mandate-journal`,
  only new test files (the shared suite and its memory runner); no source change.
- **Crates out of scope:** `mandate-journal` sources (E5-2 is changing them), `mandate-cli` (the
  `db migrate` command; see "Not done").
- **New dependencies allowed:** `sqlx` (ES-08; the features in DEC-109). `tokio` becomes a dev
  dependency of this crate (tests drive the async API from sync `#[test]`s).
- **Safety-critical:** yes. This brief ships with the **tests PR** (stubs, pending markers, CI
  wiring); the **implementation PR** adds `migrations/0001_journal.sql` and the real `PgJournal`,
  and changes tests only by deleting `#[ignore = "pending E5-3"]` lines.
- **Size budget:** 400 non-generated lines per PR (ES-13), exceeded by the tests PR: the
  conformance suite is shared by both backends and most of it is the append vectors ported from
  the reference-case harness. The PR description gives the split.

## Interpretations (DEC-109)

1. **Client.** sqlx 0.9 with `postgres`, `runtime-tokio`, `migrate`, and
   `tls-rustls-ring-native-roots`, and **no query macros**: builds need no database and no
   committed `.sqlx` metadata, and the journal's handful of statements are each executed by the
   per-PR Postgres tests instead of checked at compile time. This amends ES-08's
   "macros … offline query metadata committed (`cargo sqlx prepare --check` in CI)".
2. **Schema** (§6.1). `events (stream_id, seq)` primary key, `event_id` unique, `seq`,
   `schema_version`, and `writer_epoch` as `bigint` with range checks, `recorded_at` as the
   canonical nanosecond text (Postgres timestamps hold microseconds), `prev_hash`, `hash`, and
   `body` as `bytea` with `CHECK (octet_length(hash) = 32 AND hash = sha256(body))`. `event_ids`
   is referenced by `events` through a foreign key deferred to commit, so every event is
   registered in the same transaction. `stream_heads` adds `risk_clock` (the latest `risk_clock`
   on the stream, NULL until the first), so §2's rule is checked on the locked head row rather
   than by scanning the stream.
3. **What the database enforces.** A `BEFORE INSERT` trigger on `events` requires `seq` = the
   stream's last `seq` + 1 and `prev_hash` = that event's hash (64 zeros at `seq` 1). A trigger on
   `stream_heads` lets a head only advance, to a stored event and its hash, never lowers
   `writer_epoch` or `risk_clock` or clears `risk_clock`, and creates heads at `seq` 0.
   `BEFORE UPDATE`, `DELETE`, and `TRUNCATE` triggers on `events` and `event_ids`, and `DELETE`
   and `TRUNCATE` on `stream_heads`, raise SQLSTATE 23001 (`restrict_violation`), which binds the
   owner and superusers too. The application role has INSERT and SELECT on `events` and
   `event_ids`, and SELECT, INSERT, and UPDATE on `stream_heads` (bounded by the head trigger).
4. **Append.** One transaction: validate the drafts (no database call), create the head row if
   missing and lock it (`SELECT … FOR UPDATE`), then §5.1 in order (idempotency, fencing and head
   check, stream rules and `risk_clock`, seal, insert, advance the head). A unique violation on
   `event_ids` (the same `event_id` appended concurrently to another stream) or a serialization
   failure reruns the whole transaction, a bounded number of times.
5. **Outcomes on failure.** An error before COMMIT, or an error response to COMMIT, is
   `Unavailable`: Postgres rolled the transaction back, so the same drafts can be retried. A
   connection lost or terminated during COMMIT is `Ambiguous`: the writer must re-query before
   acting (§5.3).
6. **Reads re-verify.** `rows` re-runs §11 checks 1 to 5 over the stream from `seq` 1. `event`,
   and each event an idempotent retry returns, re-runs checks 1, 2, and 4 on that row. An append
   runs the same three on the head event and checks that its hash is the head row's. A failure is
   `IntegrityError` naming the check and `seq`, never a result. Checks 6 and 7 need the artifact
   store, which is not this crate's, so reads pass over an artifact failure and check the rest.
7. **Tests.** Each test uses its own schema in the database named by `MANDATE_PG_URL`, migrated
   as the owner role; roles are cluster-wide `NOLOGIN` roles the tests `SET ROLE` to. Without
   `MANDATE_PG_URL` the tests skip, unless `MANDATE_PG_REQUIRED` is set. They run against
   PostgreSQL 18 in CI's `full` job (`cargo xtask ci postgres`, a service container) and against
   17, the supported floor, nightly; not in `fast`, to keep Actions minutes down.

## Planted bugs

On the stubs every pending test fails at the same place (the harness's "embeds no migrations"
guard, or the stub itself), so that run shows only that each test is wired in. Each test's own
assertion is shown here instead. Every bug below was planted on its own in the implementation
(`cursor/e5-3-pg-impl-e15e`, with the tests of this PR merged), and the crate's 26 tests were run
against a local PostgreSQL 18.6 (`MANDATE_PG_URL`, `MANDATE_PG_REQUIRED=1`,
`cargo nextest run -p mandate-journal-pg --no-fail-fast`). The bug was then reverted. The first
column is the test the bug was aimed at.
All 28 bugs were caught, and each of the 26 tests failed on at least one of them. The aimed-at
test failed in 27 of the 28. The exception is the repeated `event_id` bug, which
`C::append_vectors` does not catch, though `C::stream_rules` and the differential do.

| Aimed at | Planted bug | Tests that failed |
|---|---|---|
| `migrations_are_embedded_in_order_with_no_down_migrations` | The migration is registered as reversible (`MigrationType::ReversibleUp`) | that test |
| `applied_migrations_are_checked_and_never_rerun` | The migrator ignores applied migrations it does not know (`set_ignore_missing(true)`) | that test |
| `the_application_role_can_only_insert_and_select_events` | The migration also grants the application role DELETE on the three tables | that test |
| `triggers_reject_changes_even_from_the_owner_and_superusers` | The append-only trigger on `events` omits TRUNCATE | that test |
| `the_database_rejects_bad_rows_forks_gaps_and_head_rollbacks` | The head trigger lets `risk_clock` be cleared to NULL | that test |
| `the_database_rejects_bad_rows_forks_gaps_and_head_rollbacks` | The chain trigger links a new event to the latest earlier one, so a gap is accepted | that test |
| `concurrent_appenders_never_fork_or_reuse_a_seq` | The head lock does not wait (`FOR UPDATE NOWAIT`), so a contended append fails instead of queueing | that test, `racing_retries_and_shared_event_ids_commit_once` |
| `concurrent_owners_get_distinct_epochs` | `take_ownership` reads the epoch, then writes epoch + 1 in a second statement | that test |
| `racing_retries_and_shared_event_ids_commit_once` | A unique violation (the same `event_id` committed to another stream meanwhile) is not rerun | that test |
| `a_failed_commit_leaves_no_partial_event` | An error response to COMMIT is `Ambiguous` instead of `Unavailable` (severity `Fatal` matched instead of `Error`) | that test, `a_connection_lost_during_commit_is_ambiguous_and_atomic` |
| `a_connection_lost_during_commit_is_ambiguous_and_atomic` | A connection lost during COMMIT is `Unavailable` (safe to resend) instead of `Ambiguous` | that test |
| `an_error_while_appending_is_unavailable_and_not_retried` | Every error during an append is treated as a lost race and rerun | that test |
| `stored_bytes_are_reverified_on_read` | An append chains onto the stored head event without comparing its hash with the head row's | that test |
| `stored_bytes_are_reverified_on_read` | `event` returns the stored row without re-checking it | that test, `reads_pass_over_artifact_checks_and_verify_every_other_check` |
| `reads_pass_over_artifact_checks_and_verify_every_other_check` | A read stops checking at the first artifact event and returns every row | that test |
| `C::events_with_artifact_references_are_stored_and_read_back` | After passing over an artifact event, the walk expects the same `seq` again | that test, `reads_pass_over_artifact_checks_and_verify_every_other_check` |
| `tamper_vectors_are_caught_when_the_stored_rows_are_read` | `rows` checks each row on its own (checks 1, 2, 4) instead of walking the chain from `seq` 1 | that test, `stored_bytes_are_reverified_on_read`, `reads_pass_over_artifact_checks_and_verify_every_other_check` |
| `an_unreachable_database_is_unavailable_and_changes_nothing` | Failing to open a transaction is `Ambiguous` | that test |
| `error_codes_are_stable` | `IntegrityError`'s message says "failed" instead of "fails" | that test |
| `C::new_streams_start_empty` | An unknown stream's head reports writer epoch 1 | that test, `C::matches_the_memory_journal_on_random_sequences` |
| `C::stream_rules` | The stream's environment is not taken from its stored head event | that test |
| `C::ownership_and_fencing` | A writer with a newer epoch than the stream's is not fenced (`<` instead of `!=`) | that test, `C::rejected_batches_write_nothing_and_use_no_seq`, `C::matches_the_memory_journal_on_random_sequences` |
| `C::risk_clock_never_decreases_along_a_stream` | The stream's stored `risk_clock` is not passed to the stream rules (the head trigger then rejects the head update, so the outcome is `Unavailable`, not `Invalid`) | that test |
| `C::identical_retries_return_the_stored_events` | A batch that only partly overlaps stored events returns the stored ones as `AlreadyCommitted` | that test, `C::append_vectors`, `C::matches_the_memory_journal_on_random_sequences` |
| `C::rejected_batches_write_nothing_and_use_no_seq` | The risk clock is not carried from draft to draft within a batch | that test, `C::risk_clock_never_decreases_along_a_stream`, `C::matches_the_memory_journal_on_random_sequences`, `the_database_rejects_bad_rows_forks_gaps_and_head_rollbacks` |
| `C::chain_vectors_byte_for_byte` | `recorded_at` is stored truncated to microseconds, as the `timestamptz` column DEC-109 rejects would store it | that test and 19 others: every test that reads stored rows back fails on `column_mismatch` |
| `C::append_vectors` | A batch that repeats an `event_id` is not rejected before the database is asked | `C::stream_rules`, `C::matches_the_memory_journal_on_random_sequences` (`C::append_vectors` fails on the partial-overlap, `recorded_at`, and `HeadMismatch` bugs) |
| `C::matches_the_memory_journal_on_random_sequences` | `HeadMismatch` reports a zero hash instead of the head's | that test, `C::ownership_and_fencing`, `C::append_vectors` |

**Control.** Reading the head without `FOR UPDATE` fails no test, and none should, because it
changes no outcome. A second appender that read the same head blocks on the unique
`(stream_id, seq)` index of `event_ids` until the first commits. It then gets SQLSTATE 23505,
reruns, and sees `HeadMismatch`, which is what the lock would have given it. The lock makes
appenders queue instead of rerunning; the schema's constraints make a fork impossible either way.

**The 10 macro-generated tests.** `cargo xtask ci pending` (DEC-110) finds pending tests by their
`#[ignore = "pending E5-3"]` lines, so it checks the 15 in `pg.rs` but not the 10 that
`conformance_tests!` generates in `pg_conformance.rs`. Those 10 were checked in two ways:

- On this PR's stubs, `cargo nextest run -p mandate-journal-pg --run-ignored only --no-fail-fast`
  gives `25 tests run: 0 passed, 25 failed`, both with `MANDATE_PG_URL` unset and with it set. All
  10 `pg_conformance` tests are among the failures.
- Against the implementation, each of the 10 fails on at least one planted bug above.

## Commands

```bash
cargo xtask check
export MANDATE_PG_URL=postgres://postgres:postgres@localhost:5432/postgres   # README
cargo nextest run -p mandate-journal -p mandate-journal-pg
cargo xtask ci postgres
cargo mutants -p mandate-journal-pg
```

## Stop conditions

Stop and write a DEC proposal instead of continuing if any of these happen:

- the spec is ambiguous or seems wrong;
- a new dependency seems necessary;
- a test would have to be weakened, skipped, or deleted;
- anything would deviate from an accepted decision.

Fired: a new dependency (`sqlx`) and a deviation from ES-08 (no query macros). Both are recorded in
DEC-109 as agent decisions (DEC-79).

## Not done here (with the story that owns each)

- `mandate-cli db migrate`, the only production path that applies migrations (ES-08), and the
  xtask lint that bans migrating at startup: the first story that deploys a database.
- Row-level security by `workspace_id` and hash partitioning of `stream_id` (§6.1, multi-tenant
  deployments): the multi-tenant story.
- Logging DDL, trigger changes, and superuser sessions to write-once storage (§6.1): operations.
- Segment export, eviction, snapshots, and the artifact check (§11 check 6) on stored rows: the
  export and cold-store story, and E5-2 for artifacts.

## Definition of done

- [ ] The cited reference cases pass, and none that passed before now fails.
- [x] Tests came first; each touched invariant has a property test whose oracle is independent and
      was shown to fail on a seeded bug ("Planted bugs": 28 bugs, and each of the 26 tests fails on
      at least one).
- [ ] New state changes emit journal events (none: this story stores them).
- [ ] Docs updated where behavior, interfaces, or decisions changed.
- [ ] `cargo xtask check` is green (paste the summary in the PR).
- [ ] The PR description is complete (see the PR template).
