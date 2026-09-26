//! What only the Postgres journal can get wrong (journal spec §5.1, §5.3, §6.1, §11; ADR-0001
//! ES-08): versioned forward-only migrations, append-only enforcement in the database itself,
//! concurrent appenders, atomicity across a failed or lost commit, and re-verification of stored
//! bytes on read. The protocol itself is the conformance suite (`pg_conformance.rs`).

#[path = "../../mandate-journal/tests/common/mod.rs"]
mod common;
#[path = "../../mandate-journal/tests/conformance/mod.rs"]
#[allow(
    unused_imports,
    unused_macros,
    reason = "only the fixture helpers are used here; the suite itself runs in `pg_conformance.rs`"
)]
mod conformance;
mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use common::{edit, event_id, mark_draft, now, opened_draft, stream};
use conformance::{append_chain, artifact_mark_draft, fixture, get, int, list, text};
use mandate_canon::Digest;
use mandate_journal::{
    Anchor, AnchorLeaf, AppendOutcome, Draft, EventCheck, EventFailure, Head, StoredEvent,
    StreamId, TrustedStart, seal, verify_anchor, verify_events,
};
use mandate_journal_pg::{IntegrityError, PgError, PgJournal, migrator};
use sqlx::migrate::{MigrateError, MigrationType};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{AssertSqlSafe, Executor, PgPool};
use support::{PgBackend, TestDb};

const INSUFFICIENT_PRIVILEGE: &str = "42501";
const CHECK_VIOLATION: &str = "23514";
const UNIQUE_VIOLATION: &str = "23505";
const FOREIGN_KEY_VIOLATION: &str = "23503";
/// What the journal's own triggers raise: an append-only or chain rule was broken.
const RESTRICT_VIOLATION: &str = "23001";

fn sqlstate(result: Result<impl std::fmt::Debug, sqlx::Error>) -> String {
    match result {
        Ok(ok) => panic!("expected a database error, got {ok:?}"),
        Err(e) => e
            .as_database_error()
            .and_then(|d| d.code())
            .map(|c| c.into_owned())
            .unwrap_or_else(|| panic!("not a database error: {e}")),
    }
}

async fn run(pool: &PgPool, sql: &str) -> Result<(), sqlx::Error> {
    pool.execute(AssertSqlSafe(sql.to_owned()))
        .await
        .map(|_| ())
}

fn committed(outcome: Result<AppendOutcome, IntegrityError>) -> Vec<StoredEvent> {
    match outcome {
        Ok(AppendOutcome::Committed(rows)) => rows,
        other => panic!("expected Committed, got {other:?}"),
    }
}

/// A draft of the suite's stream: a MarkUpdated with event ID `n`.
fn mark(n: u64) -> Vec<u8> {
    mark_draft(n, "1")
}

/// Opens the suite's stream as its first owner and appends marks 1 to `marks`.
async fn opened(journal: &PgJournal, marks: u64) -> (StreamId, u64) {
    let s = stream();
    let epoch = journal.take_ownership(&s).await.unwrap();
    committed(
        journal
            .append(&s, 0, epoch, now(), &[&opened_draft("paper")])
            .await,
    );
    for n in 1..=marks {
        committed(journal.append(&s, n, epoch, now(), &[&mark(n)]).await);
    }
    (s, epoch)
}

/// Inserts `row` directly into `events` and `event_ids` in one transaction on `pool`.
async fn insert_row(pool: &PgPool, row: &StoredEvent) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO events (stream_id, seq, event_id, event_type, schema_version, environment, \
         recorded_at, prev_hash, hash, body) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(&row.stream_id)
    .bind(i64::try_from(row.seq).unwrap())
    .bind(&row.event_id)
    .bind(&row.event_type)
    .bind(i64::try_from(row.schema_version).unwrap())
    .bind(&row.environment)
    .bind(&row.recorded_at)
    .bind(row.prev_hash.as_bytes().as_slice())
    .bind(row.hash.as_bytes().as_slice())
    .bind(&row.body)
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO event_ids (event_id, stream_id, seq) VALUES ($1, $2, $3)")
        .bind(&row.event_id)
        .bind(&row.stream_id)
        .bind(i64::try_from(row.seq).unwrap())
        .execute(&mut *tx)
        .await?;
    tx.commit().await
}

fn sealed(draft: &[u8], seq: u64, prev_hash: Digest) -> StoredEvent {
    seal(&Draft::parse(draft).unwrap(), seq, prev_hash, now()).unwrap()
}

#[test]
fn migrations_are_embedded_in_order_with_no_down_migrations() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../migrations");
    let mut files: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    files.sort();
    assert!(!files.is_empty());
    let m = migrator();
    let embedded: Vec<_> = m.iter().collect();
    assert_eq!(
        embedded.len(),
        files.len(),
        "every file in migrations/ is embedded, and nothing else: {files:?}"
    );
    for ((i, migration), file) in (1i64..).zip(&embedded).zip(&files) {
        assert_eq!(migration.version, i, "versions are 1, 2, 3, … in order");
        assert_eq!(
            migration.migration_type,
            MigrationType::Simple,
            "forward-only: no down migrations"
        );
        assert_eq!(
            file,
            &format!("{:04}_{}.sql", migration.version, migration.description),
            "file names are NNNN_description.sql"
        );
        let text = std::fs::read_to_string(format!("{dir}/{file}")).unwrap();
        assert_eq!(
            migration.sql.as_str(),
            text,
            "{file} is embedded as written"
        );
    }
}

#[test]
fn applied_migrations_are_checked_and_never_rerun() {
    let Some(db) = TestDb::new() else { return };
    db.block_on(async {
        migrator()
            .run(&db.owner)
            .await
            .expect("running the migrations again is a no-op");
        let applied: Vec<(i64, bool)> =
            sqlx::query_as("SELECT version, success FROM _sqlx_migrations ORDER BY version")
                .fetch_all(&db.admin)
                .await
                .unwrap();
        let known: Vec<(i64, bool)> = migrator().iter().map(|m| (m.version, true)).collect();
        assert_eq!(applied, known);

        run(
            &db.admin,
            "UPDATE _sqlx_migrations SET checksum = '\\x00' WHERE version = 1",
        )
        .await
        .unwrap();
        assert!(
            matches!(
                migrator().run(&db.owner).await,
                Err(MigrateError::VersionMismatch(1))
            ),
            "an applied migration that changed is refused"
        );
    });
    drop(db);
    let Some(db) = TestDb::new() else { return };
    db.block_on(async {
        run(
            &db.admin,
            "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
             VALUES (9999, 'from a newer binary', true, '\\x00', 0)",
        )
        .await
        .unwrap();
        assert!(
            matches!(
                migrator().run(&db.owner).await,
                Err(MigrateError::VersionMissing(9999))
            ),
            "a database migrated by a newer binary is refused, never rolled back"
        );
    });
}

#[test]
fn the_application_role_can_only_insert_and_select_events() {
    let Some(db) = TestDb::new() else { return };
    db.block_on(async {
        opened(&db.journal(), 2).await;
        for sql in [
            "UPDATE events SET event_type = 'X'",
            "DELETE FROM events",
            "TRUNCATE events CASCADE",
            "UPDATE event_ids SET seq = 9",
            "DELETE FROM event_ids",
            "TRUNCATE event_ids CASCADE",
            "DELETE FROM stream_heads",
            "TRUNCATE stream_heads",
            "UPDATE stream_heads SET stream_id = 'acct:ws_1:X'",
            "ALTER TABLE events DISABLE TRIGGER USER",
            "DROP TABLE events CASCADE",
            "CREATE TABLE sneaky (x int)",
        ] {
            assert_eq!(
                sqlstate(run(&db.app, sql).await),
                INSUFFICIENT_PRIVILEGE,
                "{sql}"
            );
        }
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM events")
            .fetch_one(&db.app)
            .await
            .unwrap();
        assert_eq!(count, 3);
    });
}

#[test]
fn triggers_reject_changes_even_from_the_owner_and_superusers() {
    let Some(db) = TestDb::new() else { return };
    db.block_on(async {
        opened(&db.journal(), 2).await;
        for pool in [&db.owner, &db.admin] {
            for sql in [
                "UPDATE events SET event_type = event_type",
                "DELETE FROM events WHERE seq = 3",
                "TRUNCATE events CASCADE",
                "UPDATE event_ids SET seq = seq",
                "DELETE FROM event_ids",
                "TRUNCATE event_ids CASCADE",
                "DELETE FROM stream_heads",
                "TRUNCATE stream_heads",
            ] {
                assert_eq!(sqlstate(run(pool, sql).await), RESTRICT_VIOLATION, "{sql}");
            }
        }
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM events")
            .fetch_one(&db.admin)
            .await
            .unwrap();
        assert_eq!(count, 3);
    });
}

#[test]
fn the_database_rejects_bad_rows_forks_gaps_and_head_rollbacks() {
    let Some(db) = TestDb::new() else { return };
    db.block_on(async {
        let journal = db.journal();
        let (s, _) = opened(&journal, 2).await;
        let head = journal.head(&s).await.unwrap();

        let mut wrong_hash = sealed(&mark(3), 4, head.hash);
        wrong_hash.hash = Digest::of(b"not the body");
        assert_eq!(
            sqlstate(insert_row(&db.app, &wrong_hash).await),
            CHECK_VIOLATION,
            "hash must be sha256(body)"
        );
        let seq_2 = journal.rows(&s).await.unwrap()[1].hash;
        assert_eq!(
            sqlstate(insert_row(&db.app, &sealed(&mark(3), 3, seq_2)).await),
            UNIQUE_VIOLATION,
            "a second event at a stored seq (a fork)"
        );
        assert_eq!(
            sqlstate(insert_row(&db.app, &sealed(&mark(3), 4, Digest::of(b"elsewhere"))).await),
            RESTRICT_VIOLATION,
            "prev_hash must be the predecessor's hash"
        );
        assert_eq!(
            sqlstate(insert_row(&db.app, &sealed(&mark(3), 5, head.hash)).await),
            RESTRICT_VIOLATION,
            "no gaps"
        );
        let other = edit(
            &edit(
                &opened_draft("paper"),
                "stream_id",
                Some("\"acct:ws_1:OTHER\""),
            ),
            "payload.account_ref",
            Some("\"OTHER\""),
        );
        let other = edit(&other, "event_id", Some(&format!("\"{}\"", event_id(90))));
        assert_eq!(
            sqlstate(insert_row(&db.app, &sealed(&other, 1, Digest::of(b"x"))).await),
            RESTRICT_VIOLATION,
            "seq 1 chains from 64 zeros"
        );

        let orphan = sealed(&mark(3), 4, head.hash);
        let mut tx = db.app.begin().await.unwrap();
        sqlx::query(
            "INSERT INTO events (stream_id, seq, event_id, event_type, schema_version, \
             environment, recorded_at, prev_hash, hash, body) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(&orphan.stream_id)
        .bind(4i64)
        .bind(&orphan.event_id)
        .bind(&orphan.event_type)
        .bind(1i64)
        .bind(&orphan.environment)
        .bind(&orphan.recorded_at)
        .bind(orphan.prev_hash.as_bytes().as_slice())
        .bind(orphan.hash.as_bytes().as_slice())
        .bind(&orphan.body)
        .execute(&mut *tx)
        .await
        .unwrap();
        assert_eq!(
            sqlstate(tx.commit().await),
            FOREIGN_KEY_VIOLATION,
            "every event is registered in event_ids by commit"
        );

        let hex = |d: Digest| d.to_hex();
        for (sql, why) in [
            (
                "UPDATE stream_heads SET seq = 2, hash = (SELECT hash FROM events WHERE seq = 2)"
                    .to_owned(),
                "a head never moves back",
            ),
            (
                format!(
                    "UPDATE stream_heads SET hash = '\\x{}'",
                    hex(Digest::of(b"x"))
                ),
                "a head's hash is its event's hash",
            ),
            (
                "UPDATE stream_heads SET seq = 9".to_owned(),
                "a head only moves to a stored event",
            ),
            (
                "UPDATE stream_heads SET writer_epoch = 0".to_owned(),
                "writer epochs never decrease",
            ),
            (
                "UPDATE stream_heads SET risk_clock = '2026-09-21T13:00:00.000000000Z'".to_owned(),
                "the stream's risk clock never decreases",
            ),
            (
                "UPDATE stream_heads SET risk_clock = NULL".to_owned(),
                "the stream's risk clock is never cleared",
            ),
            (
                format!(
                    "INSERT INTO stream_heads (stream_id, seq, hash, writer_epoch) \
                     VALUES ('acct:ws_1:NEW', 5, '\\x{}', 0)",
                    hex(Digest::ZERO)
                ),
                "a new head starts at seq 0",
            ),
        ] {
            assert_eq!(
                sqlstate(run(&db.app, &sql).await),
                RESTRICT_VIOLATION,
                "{why}"
            );
        }
        assert_eq!(journal.head(&s).await.unwrap(), head);
        assert_eq!(journal.rows(&s).await.unwrap().len(), 3);
    });
}

#[test]
fn concurrent_appenders_never_fork_or_reuse_a_seq() {
    let Some(db) = TestDb::new() else { return };
    let journal = db.journal();
    let (s, epoch) = db.block_on(opened(&journal, 0));
    const WRITERS: u64 = 8;
    const EACH: u64 = 10;
    let tasks: Vec<_> = (0..WRITERS)
        .map(|w| {
            let (journal, s) = (journal.clone(), s.clone());
            db.rt.spawn(async move {
                let mut mismatches = 0u64;
                for i in 0..EACH {
                    let draft = mark(1 + w * EACH + i);
                    loop {
                        let head = journal.head(&s).await.unwrap();
                        match journal.append(&s, head.seq, epoch, now(), &[&draft]).await {
                            Ok(AppendOutcome::Committed(_)) => break,
                            Ok(AppendOutcome::HeadMismatch { .. }) => mismatches += 1,
                            other => panic!("writer {w}: {other:?}"),
                        }
                    }
                }
                mismatches
            })
        })
        .collect();
    let mismatches: u64 = tasks.into_iter().map(|t| db.block_on(t).unwrap()).sum();
    let rows = db.block_on(journal.rows(&s)).unwrap();
    let total = 1 + WRITERS * EACH;
    assert_eq!(
        rows.iter().map(|r| r.seq).collect::<Vec<_>>(),
        (1..=total).collect::<Vec<_>>(),
        "gapless, one event per seq"
    );
    let ids: BTreeSet<&str> = rows.iter().map(|r| r.event_id.as_str()).collect();
    assert_eq!(ids.len(), rows.len(), "every event stored once");
    assert!(
        verify_events(
            &rows,
            TrustedStart::GENESIS,
            &std::collections::BTreeMap::new()
        )
        .is_ok()
    );
    assert_eq!(
        db.block_on(journal.head(&s)).unwrap(),
        Head {
            seq: total,
            hash: rows.last().unwrap().hash,
            writer_epoch: epoch
        }
    );
    eprintln!("{mismatches} head mismatches were retried");

    let head = db.block_on(journal.head(&s)).unwrap();
    let racers: Vec<_> = (0..WRITERS)
        .map(|w| {
            let (journal, s) = (journal.clone(), s.clone());
            db.rt.spawn(async move {
                journal
                    .append(&s, head.seq, epoch, now(), &[&mark(1000 + w)])
                    .await
                    .unwrap()
            })
        })
        .collect();
    let outcomes: Vec<AppendOutcome> = racers
        .into_iter()
        .map(|t| db.block_on(t).unwrap())
        .collect();
    let winners = outcomes
        .iter()
        .filter(|o| matches!(o, AppendOutcome::Committed(_)))
        .count();
    assert_eq!(winners, 1, "one append per expected head: {outcomes:?}");
    for outcome in &outcomes {
        match outcome {
            AppendOutcome::Committed(_) => {}
            AppendOutcome::HeadMismatch { actual_seq, .. } => {
                assert_eq!(*actual_seq, head.seq + 1, "losers see the winner's head");
            }
            other => panic!("a racer neither won nor lost cleanly: {other:?}"),
        }
    }
}

#[test]
fn concurrent_owners_get_distinct_epochs() {
    let Some(db) = TestDb::new() else { return };
    let journal = db.journal();
    let s = stream();
    let tasks: Vec<_> = (0..16)
        .map(|_| {
            let (journal, s) = (journal.clone(), s.clone());
            db.rt
                .spawn(async move { journal.take_ownership(&s).await.unwrap() })
        })
        .collect();
    let epochs: BTreeSet<u64> = tasks.into_iter().map(|t| db.block_on(t).unwrap()).collect();
    assert_eq!(epochs, (1..=16).collect());
    assert_eq!(db.block_on(journal.head(&s)).unwrap().writer_epoch, 16);
}

#[test]
fn racing_retries_and_shared_event_ids_commit_once() {
    let Some(db) = TestDb::new() else { return };
    let journal = db.journal();
    let (s, epoch) = db.block_on(opened(&journal, 0));
    let batch = vec![mark(1), mark(2)];
    let tasks: Vec<_> = (0..8)
        .map(|_| {
            let (journal, s, batch) = (journal.clone(), s.clone(), batch.clone());
            db.rt.spawn(async move {
                let refs: Vec<&[u8]> = batch.iter().map(Vec::as_slice).collect();
                journal.append(&s, 1, epoch, now(), &refs).await.unwrap()
            })
        })
        .collect();
    let outcomes: Vec<AppendOutcome> = tasks.into_iter().map(|t| db.block_on(t).unwrap()).collect();
    let stored = db.block_on(journal.rows(&s)).unwrap()[1..].to_vec();
    assert_eq!(stored.len(), 2);
    for outcome in &outcomes {
        match outcome {
            AppendOutcome::Committed(rows) | AppendOutcome::AlreadyCommitted(rows) => {
                assert_eq!(rows, &stored);
            }
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(
        outcomes
            .iter()
            .filter(|o| matches!(o, AppendOutcome::Committed(_)))
            .count(),
        1
    );

    let other = StreamId::parse("acct:ws_1:OTHER").unwrap();
    let opened_other = edit(
        &edit(
            &opened_draft("paper"),
            "stream_id",
            Some("\"acct:ws_1:OTHER\""),
        ),
        "payload.account_ref",
        Some("\"OTHER\""),
    );
    let opened_other = edit(
        &opened_other,
        "event_id",
        Some(&format!("\"{}\"", event_id(500))),
    );
    committed(db.block_on(journal.append(&other, 0, 0, now(), &[&opened_other])));
    for round in 0..10u64 {
        let id = 600 + round;
        let here = mark(id);
        let there = edit(&mark(id), "stream_id", Some("\"acct:ws_1:OTHER\""));
        let a = {
            let (journal, s) = (journal.clone(), s.clone());
            db.rt.spawn(async move {
                let head = journal.head(&s).await.unwrap();
                journal
                    .append(&s, head.seq, epoch, now(), &[&here])
                    .await
                    .unwrap()
            })
        };
        let b = {
            let (journal, other) = (journal.clone(), other.clone());
            db.rt.spawn(async move {
                let head = journal.head(&other).await.unwrap();
                journal
                    .append(&other, head.seq, 0, now(), &[&there])
                    .await
                    .unwrap()
            })
        };
        let outcomes = [db.block_on(a).unwrap(), db.block_on(b).unwrap()];
        assert_eq!(
            outcomes
                .iter()
                .filter(|o| matches!(o, AppendOutcome::Committed(_)))
                .count(),
            1,
            "round {round}: {outcomes:?}"
        );
        assert!(
            outcomes
                .iter()
                .any(|o| matches!(o, AppendOutcome::IdempotencyConflict { .. })),
            "round {round}: {outcomes:?}"
        );
    }
}

/// Makes every commit that inserted events run `body` first (a deferred constraint trigger).
fn plant_commit_fault(db: &TestDb, body: &str) {
    db.admin_execute(&format!(
        "CREATE FUNCTION commit_fault() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN {body}; RETURN NULL; END $$;
         CREATE CONSTRAINT TRIGGER commit_fault AFTER INSERT ON events
           DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION commit_fault()"
    ));
}

fn remove_commit_fault(db: &TestDb) {
    db.admin_execute("DROP TRIGGER commit_fault ON events; DROP FUNCTION commit_fault()");
}

#[test]
fn a_failed_commit_leaves_no_partial_event() {
    let Some(db) = TestDb::new() else { return };
    let journal = db.journal();
    let (s, epoch) = db.block_on(opened(&journal, 1));
    let head = db.block_on(journal.head(&s)).unwrap();
    plant_commit_fault(&db, "RAISE EXCEPTION 'injected commit failure'");
    assert_eq!(
        db.block_on(journal.append(&s, 2, epoch, now(), &[&mark(2), &mark(3)])),
        Ok(AppendOutcome::Unavailable),
        "the commit failed and said so: nothing was written, retry with the same drafts"
    );
    assert_eq!(db.block_on(journal.head(&s)).unwrap(), head);
    assert_eq!(db.block_on(journal.rows(&s)).unwrap().len(), 2);
    assert_eq!(db.block_on(journal.event(&event_id(2))).unwrap(), None);
    remove_commit_fault(&db);
    let rows = committed(db.block_on(journal.append(&s, 2, epoch, now(), &[&mark(2), &mark(3)])));
    assert_eq!(
        rows.iter()
            .map(|r| (r.seq, r.prev_hash))
            .collect::<Vec<_>>(),
        [(3, head.hash), (4, rows[0].hash)],
        "the retry commits at the seqs the failed attempt would have used"
    );
}

#[test]
fn a_connection_lost_during_commit_is_ambiguous_and_atomic() {
    let Some(db) = TestDb::new() else { return };
    let journal = db.journal();
    let (s, epoch) = db.block_on(opened(&journal, 1));
    let head = db.block_on(journal.head(&s)).unwrap();
    plant_commit_fault(&db, "PERFORM pg_sleep(60)");
    let append = {
        let (journal, s) = (journal.clone(), s.clone());
        db.rt.spawn(async move {
            journal
                .append(&s, 2, epoch, now(), &[&mark(2), &mark(3)])
                .await
        })
    };
    let pid: i32 = db.block_on(async {
        for _ in 0..400 {
            let found: Option<i32> = sqlx::query_scalar(
                "SELECT pid FROM pg_stat_activity \
                 WHERE application_name = $1 AND wait_event = 'PgSleep'",
            )
            .bind(&db.schema)
            .fetch_optional(&db.admin)
            .await
            .unwrap();
            if let Some(pid) = found {
                return pid;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("the append never reached its commit")
    });
    db.block_on(
        sqlx::query("SELECT pg_terminate_backend($1)")
            .bind(pid)
            .execute(&db.admin),
    )
    .unwrap();
    assert_eq!(
        db.block_on(append).unwrap(),
        Ok(AppendOutcome::Ambiguous),
        "the connection died during the commit: the writer must re-query before acting"
    );
    remove_commit_fault(&db);
    assert_eq!(db.block_on(journal.event(&event_id(2))).unwrap(), None);
    assert_eq!(db.block_on(journal.event(&event_id(3))).unwrap(), None);
    assert_eq!(db.block_on(journal.head(&s)).unwrap(), head);
    let rows = committed(db.block_on(journal.append(&s, 2, epoch, now(), &[&mark(2), &mark(3)])));
    assert_eq!(rows[0].seq, 3);
}

#[test]
#[ignore = "pending E5-3"]
fn an_error_while_appending_is_unavailable_and_not_retried() {
    let Some(db) = TestDb::new() else { return };
    let journal = db.journal();
    let (s, epoch) = db.block_on(opened(&journal, 1));
    db.admin_execute(
        "CREATE SEQUENCE append_attempts;
         CREATE FUNCTION append_fault() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER AS $$
         BEGIN
           IF nextval('append_attempts') = 1 THEN RAISE EXCEPTION 'injected failure'; END IF;
           RETURN NEW;
         END $$;
         CREATE TRIGGER append_fault BEFORE INSERT ON events
           FOR EACH ROW EXECUTE FUNCTION append_fault()",
    );
    assert_eq!(
        db.block_on(journal.append(&s, 2, epoch, now(), &[&mark(2)])),
        Ok(AppendOutcome::Unavailable),
        "only a lost race is rerun; any other error ends the append, even one a rerun would pass"
    );
    let attempts: i64 = db
        .block_on(sqlx::query_scalar("SELECT last_value FROM append_attempts").fetch_one(&db.admin))
        .unwrap();
    assert_eq!(attempts, 1);
    assert_eq!(db.block_on(journal.rows(&s)).unwrap().len(), 2);
    let rows = committed(db.block_on(journal.append(&s, 2, epoch, now(), &[&mark(2)])));
    assert_eq!(rows[0].seq, 3);
}

/// Changes stored rows as a superuser with the journal's triggers switched off for the session,
/// the way an operator with direct database access could; CHECK constraints still apply.
async fn tamper(db: &TestDb, sql: &str) {
    let mut conn = db.admin.acquire().await.unwrap();
    conn.execute(AssertSqlSafe(format!(
        "SET session_replication_role = replica; {sql}; SET session_replication_role = DEFAULT"
    )))
    .await
    .unwrap();
}

/// Drops `CHECK (hash = sha256(body))`, as an operator with DDL rights could, so that a stored hash
/// can stop matching its body.
async fn drop_hash_check(db: &TestDb) {
    let constraint: String = sqlx::query_scalar(
        "SELECT conname::text FROM pg_constraint \
         WHERE conrelid = 'events'::regclass AND contype = 'c' \
         AND pg_get_constraintdef(oid) LIKE '%sha256%'",
    )
    .fetch_one(&db.admin)
    .await
    .unwrap();
    tamper(
        db,
        &format!("ALTER TABLE events DROP CONSTRAINT {constraint}"),
    )
    .await;
}

/// The SQL expression for `body` with `from` replaced by `to` once, as bytes.
fn replaced(from: &str, to: &str) -> String {
    format!("convert_to(replace(convert_from(body, 'UTF8'), '{from}', '{to}'), 'UTF8')")
}

fn integrity(stream: &StreamId, seq: u64, check: EventCheck) -> IntegrityError {
    IntegrityError {
        stream_id: stream.as_str().to_owned(),
        failure: EventFailure { seq, check },
    }
}

fn read_failure<T: std::fmt::Debug>(result: Result<T, PgError>) -> IntegrityError {
    match result {
        Err(PgError::Integrity(e)) => e,
        other => panic!("expected an integrity failure, got {other:?}"),
    }
}

#[test]
fn stored_bytes_are_reverified_on_read() {
    let price = |p: &str| format!(r#""price":"{p}""#);
    {
        let Some(db) = TestDb::new() else { return };
        let journal = db.journal();
        let (s, _) = db.block_on(opened(&journal, 3));
        db.block_on(tamper(
            &db,
            &format!(
                "UPDATE events SET body = {b}, hash = sha256({b}) WHERE seq = 3",
                b = replaced(&price("1"), &price("2"))
            ),
        ));
        assert_eq!(
            read_failure(db.block_on(journal.rows(&s))),
            integrity(&s, 4, EventCheck::PrevHashMismatch),
            "a rewritten, rehashed event breaks the chain at its successor"
        );
    }
    {
        let Some(db) = TestDb::new() else { return };
        let journal = db.journal();
        let (s, _) = db.block_on(opened(&journal, 3));
        db.block_on(tamper(
            &db,
            &format!(
                "UPDATE events SET body = {b}, hash = sha256({b}) WHERE seq = 3",
                b = replaced(&price("1"), r#""price": "1""#)
            ),
        ));
        assert_eq!(
            read_failure(db.block_on(journal.rows(&s))),
            integrity(&s, 3, EventCheck::NonCanonical)
        );
        assert_eq!(
            read_failure(db.block_on(journal.event(&event_id(2)))),
            integrity(&s, 3, EventCheck::NonCanonical)
        );
    }
    {
        let Some(db) = TestDb::new() else { return };
        let journal = db.journal();
        let (s, epoch) = db.block_on(opened(&journal, 3));
        db.block_on(tamper(
            &db,
            "UPDATE events SET event_type = 'FillApplied' WHERE seq = 3",
        ));
        assert_eq!(
            read_failure(db.block_on(journal.rows(&s))),
            integrity(&s, 3, EventCheck::ColumnMismatch)
        );
        assert_eq!(
            read_failure(db.block_on(journal.event(&event_id(2)))),
            integrity(&s, 3, EventCheck::ColumnMismatch)
        );
        assert_eq!(
            db.block_on(journal.append(&s, 1, epoch, now(), &[&mark(2)])),
            Err(integrity(&s, 3, EventCheck::ColumnMismatch)),
            "a retry never returns an event that fails verification"
        );
    }
    {
        let Some(db) = TestDb::new() else { return };
        let journal = db.journal();
        let (s, _) = db.block_on(opened(&journal, 3));
        db.block_on(drop_hash_check(&db));
        db.block_on(tamper(
            &db,
            "UPDATE events SET hash = sha256('x') WHERE seq = 2",
        ));
        assert_eq!(
            read_failure(db.block_on(journal.rows(&s))),
            integrity(&s, 2, EventCheck::RehashMismatch)
        );
        assert_eq!(
            read_failure(db.block_on(journal.event(&event_id(1)))),
            integrity(&s, 2, EventCheck::RehashMismatch)
        );
    }
    {
        let Some(db) = TestDb::new() else { return };
        let journal = db.journal();
        let (s, _) = db.block_on(opened(&journal, 3));
        db.block_on(tamper(
            &db,
            "DELETE FROM event_ids WHERE seq = 3; DELETE FROM events WHERE seq = 3",
        ));
        assert_eq!(
            read_failure(db.block_on(journal.rows(&s))),
            integrity(&s, 4, EventCheck::SeqGap)
        );
    }
    {
        let Some(db) = TestDb::new() else { return };
        let journal = db.journal();
        let (s, epoch) = db.block_on(opened(&journal, 3));
        db.block_on(tamper(
            &db,
            &format!(
                "UPDATE events SET body = {b}, hash = sha256({b}) WHERE seq = 4",
                b = replaced(&price("1"), &price("2"))
            ),
        ));
        assert_eq!(
            db.block_on(journal.append(&s, 4, epoch, now(), &[&mark(4)])),
            Err(integrity(&s, 4, EventCheck::RehashMismatch)),
            "an append never chains onto a head event whose hash is not the head's"
        );
        assert_eq!(db.block_on(journal.event(&event_id(4))).unwrap(), None);
    }
}

/// Opens the suite's stream, then appends artifact-referencing marks 1 and 2 (seq 2 and 3) and plain
/// marks 3 and 4 (seq 4 and 5).
async fn opened_with_artifact_marks(journal: &PgJournal) -> StreamId {
    let (s, epoch) = opened(journal, 0).await;
    let artifacts = [artifact_mark_draft(1, 'a'), artifact_mark_draft(2, 'b')];
    let refs: Vec<&[u8]> = artifacts.iter().map(Vec::as_slice).collect();
    committed(journal.append(&s, 1, epoch, now(), &refs).await);
    committed(
        journal
            .append(&s, 3, epoch, now(), &[&mark(3), &mark(4)])
            .await,
    );
    s
}

#[test]
fn reads_pass_over_artifact_checks_and_verify_every_other_check() {
    let rewrite = |seq: u64| {
        format!(
            "UPDATE events SET body = {b}, hash = sha256({b}) WHERE seq = {seq}",
            b = replaced(r#""price":"1""#, r#""price":"2""#)
        )
    };
    let cases = [
        (
            rewrite(2),
            3,
            EventCheck::PrevHashMismatch,
            "the chain is checked across consecutive artifact events",
        ),
        (
            rewrite(4),
            5,
            EventCheck::PrevHashMismatch,
            "and after them",
        ),
        (
            "UPDATE events SET event_type = 'FillApplied' WHERE seq = 3".to_owned(),
            3,
            EventCheck::ColumnMismatch,
            "and an artifact event's own columns are checked",
        ),
    ];
    for (sql, seq, check, why) in cases {
        let Some(db) = TestDb::new() else { return };
        let journal = db.journal();
        let s = db.block_on(opened_with_artifact_marks(&journal));
        assert_eq!(db.block_on(journal.rows(&s)).unwrap().len(), 5);
        db.block_on(tamper(&db, &sql));
        assert_eq!(
            read_failure(db.block_on(journal.rows(&s))),
            integrity(&s, seq, check),
            "{why}"
        );
        if check == EventCheck::ColumnMismatch {
            assert_eq!(
                read_failure(db.block_on(journal.event(&event_id(2)))),
                integrity(&s, seq, check)
            );
        }
    }
}

/// SQL that points row `seq`'s `prev_hash`, in its body and its column, at `hash` (a SQL `bytea`).
fn repoint(seq: u64, hash: &str) -> String {
    format!(
        "UPDATE events SET body = convert_to(replace(convert_from(body, 'UTF8'), \
         encode(prev_hash, 'hex'), encode({hash}, 'hex')), 'UTF8'), prev_hash = {hash} \
         WHERE seq = {seq}"
    )
}

fn rehash(seq: u64) -> String {
    format!("UPDATE events SET hash = sha256(body) WHERE seq = {seq}")
}

const FOREIGN_HASH: &str =
    "'\\xabababababababababababababababababababababababababababababababab'::bytea";

/// Each tamper vector's change (`fixtures/refcases/journal.json`, `tamper_cases`), as SQL on the
/// stored chain: (name, the vector's description, the statements).
fn tamper_sql(name: &str) -> (&'static str, Vec<String>) {
    let verdict = replaced(r#""verdict":"allow""#, r#""verdict":"deny""#);
    let delete = |seq: u64| {
        format!("DELETE FROM event_ids WHERE seq = {seq}; DELETE FROM events WHERE seq = {seq}")
    };
    let hash_of = |seq: u64| format!("(SELECT hash FROM events WHERE seq = {seq})");
    match name {
        "payload_modified" => (
            "seq 3 body: payload.verdict 'allow' -> 'deny'; stored hash unchanged",
            vec![format!("UPDATE events SET body = {verdict} WHERE seq = 3")],
        ),
        "event_deleted" => ("row seq 3 deleted", vec![delete(3)]),
        "seq_values_swapped" => (
            "the seq members inside bodies 2 and 3 are swapped; rows and columns stay in seq order",
            vec![
                format!(
                    "UPDATE events SET body = {} WHERE seq = 2",
                    replaced(r#""seq":2,"#, r#""seq":3,"#)
                ),
                format!(
                    "UPDATE events SET body = {} WHERE seq = 3",
                    replaced(r#""seq":3,"#, r#""seq":2,"#)
                ),
            ],
        ),
        "prev_hash_changed_without_rehash" => (
            "seq 4: prev_hash replaced in body and column; stored hash unchanged",
            vec![repoint(4, FOREIGN_HASH)],
        ),
        "prev_hash_rewritten_and_rehashed" => (
            "seq 4: prev_hash replaced in body and column; hash recomputed",
            vec![repoint(4, FOREIGN_HASH), rehash(4)],
        ),
        "whitespace_inserted_and_rehashed" => (
            "seq 2: a space inserted after the opening brace of the body; hash recomputed",
            vec![
                "UPDATE events SET body = '\\x7b20'::bytea || substring(body FROM 2) WHERE seq = 2"
                    .to_owned(),
                rehash(2),
            ],
        ),
        "column_altered" => (
            "seq 5: event_type column changed to 'FillReversed'",
            vec!["UPDATE events SET event_type = 'FillReversed' WHERE seq = 5".to_owned()],
        ),
        "tail_truncated_after_anchor" => (
            "anchor covers the account stream at seq 5 (merkle leaf 0); row seq 5 deleted and head reset to 4",
            vec![
                delete(5),
                format!("UPDATE stream_heads SET seq = 4, hash = {}", hash_of(4)),
            ],
        ),
        "chain_rewritten_from_seq_3" => (
            "events 3-5 rewritten with recomputed hashes and columns",
            vec![
                format!("UPDATE events SET body = {verdict} WHERE seq = 3"),
                rehash(3),
                repoint(4, &hash_of(3)),
                rehash(4),
                repoint(5, &hash_of(4)),
                rehash(5),
                format!("UPDATE stream_heads SET hash = {}", hash_of(5)),
            ],
        ),
        other => panic!("no interpretation for tamper case `{other}`; add one here"),
    }
}

#[test]
fn tamper_vectors_are_caught_when_the_stored_rows_are_read() {
    let fx = fixture();
    let anchor = Anchor {
        leaves: list(&fx, "merkle.leaves")
            .iter()
            .map(|leaf| AnchorLeaf {
                stream_id: text(leaf, "stream_id").to_owned(),
                seq: int(leaf, "seq"),
                hash: Digest::from_hex(text(leaf, "hash")).unwrap(),
            })
            .collect(),
        root: Digest::from_hex(text(&fx, "merkle.root")).unwrap(),
    };
    let cases = list(&fx, "tamper_cases");
    assert_eq!(
        cases.len(),
        9,
        "a new tamper vector needs an interpretation here"
    );
    for case in cases {
        let name = text(case, "name");
        let (change, statements) = tamper_sql(name);
        assert_eq!(
            text(case, "change"),
            change,
            "{name}: the vector changed; re-read it"
        );
        let Some(mut pg) = PgBackend::fresh() else {
            return;
        };
        let s = append_chain(&mut pg, &fx);
        let db = &pg.db;
        db.block_on(drop_hash_check(db));
        db.block_on(tamper(db, &statements.join("; ")));
        let read = db.block_on(pg.journal.rows(&s));
        match get(case, "expect").get("range_check") {
            Some(range_check) => {
                assert_eq!(text(case, "expect.per_event"), "pass");
                let rows = read.unwrap_or_else(|e| panic!("{name}: {e}"));
                assert!(verify_events(&rows, TrustedStart::GENESIS, &BTreeMap::new()).is_ok());
                assert_eq!(
                    verify_anchor(&anchor, &s, &rows).map_err(|c| c.code()),
                    Err(range_check.as_str().unwrap()),
                    "{name}"
                );
            }
            None => {
                let failure = read_failure(read).failure;
                assert_eq!(
                    (failure.seq, failure.check.code()),
                    (int(case, "expect.seq"), text(case, "expect.check")),
                    "{name}"
                );
            }
        }
    }
}

#[test]
fn an_unreachable_database_is_unavailable_and_changes_nothing() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let pool = PgPoolOptions::new()
            .acquire_timeout(Duration::from_secs(2))
            .connect_lazy_with(PgConnectOptions::new().host("127.0.0.1").port(1));
        let journal = PgJournal::new(pool);
        let s = stream();
        assert!(matches!(
            journal.take_ownership(&s).await,
            Err(PgError::Unavailable(_))
        ));
        assert!(matches!(
            journal.head(&s).await,
            Err(PgError::Unavailable(_))
        ));
        assert!(matches!(
            journal.rows(&s).await,
            Err(PgError::Unavailable(_))
        ));
        assert!(matches!(
            journal.event(&event_id(1)).await,
            Err(PgError::Unavailable(_))
        ));
        assert_eq!(
            journal
                .append(&s, 0, 0, now(), &[&opened_draft("paper")])
                .await,
            Ok(AppendOutcome::Unavailable)
        );
        assert_eq!(
            journal.append(&s, 0, 0, now(), &[]).await,
            Ok(AppendOutcome::Invalid {
                draft: 0,
                error: mandate_journal::Invalid {
                    reason: mandate_journal::InvalidReason::EmptyBatch,
                    path: String::new()
                }
            }),
            "drafts are validated before the database is asked"
        );
    });
}

#[test]
fn error_codes_are_stable() {
    let e = integrity(&stream(), 7, EventCheck::PrevHashMismatch);
    assert_eq!(e.code(), "prev_hash_mismatch");
    assert_eq!(
        e.to_string(),
        "stored event acct:ws_1:ACCT1 seq 7 fails `prev_hash_mismatch`"
    );
    assert_eq!(PgError::from(e).code(), "integrity");
    assert_eq!(
        PgError::Unavailable(sqlx::Error::PoolClosed).code(),
        "unavailable"
    );
}
