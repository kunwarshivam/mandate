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
6. **Reads re-verify.** `rows`, `event`, and the events an idempotent retry returns re-run §11
   checks 1 to 5 over the stream from `seq` 1 through the last row returned; an append also
   checks that the head row's hash is the stored hash of the head event. A failure is
   `IntegrityError` naming the check and `seq`, never a result.
7. **Tests.** Each test uses its own schema in the database named by `MANDATE_PG_URL`, migrated
   as the owner role; roles are cluster-wide `NOLOGIN` roles the tests `SET ROLE` to. Without
   `MANDATE_PG_URL` the tests skip, unless `MANDATE_PG_REQUIRED` is set. They run against
   PostgreSQL 18 in CI's `full` job (`cargo xtask ci postgres`, a service container) and against
   17, the supported floor, nightly; not in `fast`, to keep Actions minutes down.

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
- [ ] Tests came first; each touched invariant has a property test whose oracle is independent and
      was shown to fail on a seeded bug.
- [ ] New state changes emit journal events (none: this story stores them).
- [ ] Docs updated where behavior, interfaces, or decisions changed.
- [ ] `cargo xtask check` is green (paste the summary in the PR).
- [ ] The PR description is complete (see the PR template).
