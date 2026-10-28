//! The journal port over a real Postgres journal (E21-29, E21-30; DEC-1012): ownership fences, and
//! a draft the journal accepts reads back as written. The suite's PG class: it skips unless
//! `MANDATE_PG_URL` is set, and fails without it where `MANDATE_PG_REQUIRED` is (DEC-109,
//! DEC-519). The journal is a local test schema and nothing here carries a credential
//! (`AGENTS.md` rules 7 and 8).

#[path = "../../mandate-journal/tests/common/mod.rs"]
#[allow(
    unused_imports,
    reason = "only the journal's support needs these; the suite runs in `mandate-journal-pg`"
)]
mod common;
#[path = "../../mandate-journal/tests/conformance/mod.rs"]
#[allow(
    unused_imports,
    unused_macros,
    reason = "only `support`'s backend trait is used here; the suite runs in `mandate-journal-pg`"
)]
mod conformance;
#[path = "../../mandate-journal-pg/tests/support/mod.rs"]
mod support;

use mandate_canon::{Object, parse};
use mandate_journal::{AppendOutcome, Environment, StreamId};
use mandate_journal_pg::APP_ROLE;
use mandate_store::envelope::{Envelope, Fields, draft_bytes, object, text};
use mandate_store::{PgStore, Store};
use mandate_time::UtcNanos;

/// 2026-10-16T13:00:00Z.
const T: i64 = 1_792_155_600;

/// The DSN of the test's own schema, acting as the journal's application role.
fn dsn(db: &support::TestDb) -> String {
    let url = std::env::var(support::URL_VAR).unwrap();
    let joiner = if url.contains('?') { '&' } else { '?' };
    let options = format!("-c%20search_path%3D{}%20-c%20role%3D{APP_ROLE}", db.schema);
    format!("{url}{joiner}options={options}")
}

fn stream() -> StreamId {
    StreamId::parse("clock:ws_1").unwrap()
}

/// The scheduler stream's `StreamOpened` at seq 1 under `epoch` (journal spec §9.20).
fn opened(epoch: u64) -> Vec<u8> {
    let payload = object(vec![
        ("stream_type", text("scheduler")),
        ("workspace_id", text("ws_1")),
    ])
    .unwrap();
    let event_id = format!("2{epoch:05}{:014}{:06}", 0, 0);
    draft_bytes(
        &Envelope {
            stream: "clock:ws_1",
            environment: Environment::Paper,
            actor_kind: "system",
            actor_id: "scheduler",
            build: "mandate-store-tests/0",
            version: "0",
            event_time: UtcNanos::from_parts(T, 0).unwrap(),
        },
        &Fields {
            event_id: &event_id,
            event_type: "StreamOpened",
            schema_version: 1,
            causation_id: None,
            correlation_id: None,
            config_refs: &Object::new(),
            payload: &payload,
        },
    )
    .unwrap()
}

#[test]
fn each_owner_takes_a_newer_epoch_than_the_last() {
    let Some(db) = support::TestDb::new() else {
        return;
    };
    let mut first = PgStore::connect(&dsn(&db)).unwrap();
    let mut second = PgStore::connect(&dsn(&db)).unwrap();
    let taken = first.take_ownership(&stream()).unwrap();
    let newer = second.take_ownership(&stream()).unwrap();
    assert!(taken >= 1, "an owner holds epoch {taken}");
    assert!(newer > taken, "{newer} fences {taken}");
}

#[test]
fn a_draft_the_journal_accepts_reads_back_as_written() {
    let Some(db) = support::TestDb::new() else {
        return;
    };
    let mut writer = PgStore::connect(&dsn(&db)).unwrap();
    let epoch = writer.take_ownership(&stream()).unwrap();
    let draft = opened(epoch);
    let at = UtcNanos::from_parts(T, 0).unwrap();
    let outcome = writer.append(&stream(), 0, epoch, at, std::slice::from_ref(&draft));
    assert!(
        matches!(outcome, AppendOutcome::Committed(_)),
        "{outcome:?}"
    );
    let reader = PgStore::connect(&dsn(&db)).unwrap();
    let rows = reader.rows(&stream()).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].event_type, "StreamOpened");
    let (stored, drafted) = (parse(&rows[0].body).unwrap(), parse(&draft).unwrap());
    for member in ["event_id", "payload", "actor", "event_time"] {
        assert_eq!(stored.get(member), drafted.get(member), "{member}");
    }
}

/// A store holds at most one connection, however many calls it makes: each long-running process
/// drives its stores from one thread, so a pool of sqlx's default ten per store held about twenty
/// idle connections per runtime, and four runtimes took every slot of the cell's journal, refusing
/// its migration (lane DR's cell run, 2026-10-26).
#[test]
fn a_store_holds_one_connection_however_many_calls_it_makes() {
    let Some(db) = support::TestDb::new() else {
        return;
    };
    let name = format!("{}_store", db.schema);
    let mut store = PgStore::connect(&format!("{}&application_name={name}", dsn(&db))).unwrap();
    let epoch = store.take_ownership(&stream()).unwrap();
    let at = UtcNanos::from_parts(T, 0).unwrap();
    let outcome = store.append(&stream(), 0, epoch, at, &[opened(epoch)]);
    assert!(
        matches!(outcome, AppendOutcome::Committed(_)),
        "{outcome:?}"
    );
    for _ in 0..30 {
        assert_eq!(store.rows(&stream()).map(|rows| rows.len()), Some(1));
    }
    let held: i64 = db
        .block_on(
            sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE application_name = $1")
                .bind(&name)
                .fetch_one(&db.admin),
        )
        .unwrap();
    assert_eq!(held, 1, "the store's connections");
}

/// A reader past a head it holds reads only what follows it, checked from its hash: nothing past
/// the head, the whole stream from genesis, and nothing at all for a hash that is not the chain's.
#[test]
fn rows_from_a_held_head_read_only_what_follows_it() {
    let Some(db) = support::TestDb::new() else {
        return;
    };
    let mut writer = PgStore::connect(&dsn(&db)).unwrap();
    let epoch = writer.take_ownership(&stream()).unwrap();
    let at = UtcNanos::from_parts(T, 0).unwrap();
    let outcome = writer.append(&stream(), 0, epoch, at, &[opened(epoch)]);
    assert!(
        matches!(outcome, AppendOutcome::Committed(_)),
        "{outcome:?}"
    );
    let reader = PgStore::connect(&dsn(&db)).unwrap();
    let all = reader.rows(&stream()).unwrap();
    assert_eq!(
        reader.rows_from(&stream(), mandate_journal::TrustedStart::GENESIS),
        Some(all.clone())
    );
    let past = mandate_journal::TrustedStart {
        from_seq: all[0].seq + 1,
        prev_hash: all[0].hash,
    };
    assert_eq!(reader.rows_from(&stream(), past), Some(Vec::new()));
    let forked = mandate_journal::TrustedStart {
        from_seq: 1,
        prev_hash: mandate_canon::Digest::of(b"another chain"),
    };
    assert_eq!(reader.rows_from(&stream(), forked), None);
}
