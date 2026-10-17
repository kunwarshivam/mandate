//! The scheduler's Postgres store over a real Postgres journal (E21-29, DEC-1010): ownership
//! fences, rows read back as written, and a scheduler that starts and ticks on it. The suite's PG
//! class: it skips unless `MANDATE_PG_URL` is set, and fails without it where
//! `MANDATE_PG_REQUIRED` is (DEC-109, DEC-519). The journal is a local test schema and nothing
//! here carries a credential (`AGENTS.md` rules 7 and 8).

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

use std::time::Duration;

use mandate_journal::{Environment, StreamId};
use mandate_journal_pg::APP_ROLE;
use mandate_process::State;
use mandate_scheduler::store::PgStore;
use mandate_scheduler::{ClockStore, Config, Sample, Scheduler, TimeServers};
use mandate_time::{ExchangeCalendar, UtcNanos};

/// 2026-10-16T13:00:00Z, a Friday, 09:00 in New York.
const FRIDAY_0900: i64 = 1_792_155_600;

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

struct InTolerance;

impl TimeServers for InTolerance {
    fn ask(&mut self) -> Vec<Sample> {
        vec![Sample {
            server: "time0.example".into(),
            stratum: 2,
            offset_ns: 1_000_000,
            delay_ns: 1_000_000,
        }]
    }
}

#[test]
fn each_owner_of_the_clock_stream_takes_a_newer_epoch_than_the_last() {
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
fn a_scheduler_on_postgres_reads_back_the_records_it_wrote() {
    let Some(db) = support::TestDb::new() else {
        return;
    };
    let config = Config {
        workspace: "ws_1".into(),
        environment: Environment::Paper,
        tolerance_ns: 50_000_000,
        liveness: Duration::from_secs(10),
    };
    let store = PgStore::connect(&dsn(&db)).unwrap();
    let start = UtcNanos::from_parts(FRIDAY_0900, 0).unwrap();
    let mut scheduler = Scheduler::start(
        store,
        &config,
        ExchangeCalendar::us_equities().unwrap(),
        start,
    );
    let tick = UtcNanos::from_parts(FRIDAY_0900 + 1, 0).unwrap();
    scheduler.step(tick, 1, &mut InTolerance);
    assert_eq!(scheduler.lifecycle().state(), State::Ready);
    let reader = PgStore::connect(&dsn(&db)).unwrap();
    let kinds: Vec<String> = reader
        .rows(&stream())
        .unwrap()
        .into_iter()
        .map(|row| row.event_type)
        .collect();
    assert_eq!(kinds.first().map(String::as_str), Some("StreamOpened"));
    assert!(kinds.iter().any(|k| k == "ClockAdvanced"), "{kinds:?}");
    assert!(
        kinds.iter().any(|k| k == "ClockOffsetRecorded"),
        "{kinds:?}"
    );
}
