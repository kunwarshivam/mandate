//! The executor process as the binary runs it (DEC-2007): the fixture wiring and the fixture paper
//! broker behind `mandate-alpaca`'s client, on the in-memory journal, which judges every record it
//! is given against the registered schemas. With a scheduler tick on the clock stream it opens its
//! account stream, reconciles against the fixture account, and is ready; a restart is ready again
//! and opens nothing twice.

use std::time::Duration;

use mandate_canon::Object;
use mandate_executor::{AccountRef, AccountScope, WorkspaceId};
use mandate_executord::alpaca::AlpacaBroker;
use mandate_executord::paper::FixturePaper;
use mandate_executord::{Config, ExecutorProcess, fixture};
use mandate_journal::{AppendOutcome, Environment, StreamId};
use mandate_store::envelope::{Envelope, Fields, draft_bytes, object, text};
use mandate_store::{MemoryStore, Store};
use mandate_time::UtcNanos;

type Error = Box<dyn std::error::Error>;

const ACCOUNT: &str = "01J8Z2ACCT0000000000000001";
const STREAM: &str = "acct:ws1:01J8Z2ACCT0000000000000001";

fn noon() -> Result<UtcNanos, Error> {
    Ok(UtcNanos::parse_rfc3339("2026-09-21T15:00:00Z")?)
}

/// The scheduler's clock stream: opened, then one tick at `at` (journal spec §9.20).
fn ticked(store: &mut MemoryStore, at: UtcNanos) -> Result<(), Error> {
    let clock = StreamId::parse("clock:ws1").ok_or("stream")?;
    let epoch = store.take_ownership(&clock).ok_or("epoch")?;
    let envelope = Envelope {
        stream: clock.as_str(),
        environment: Environment::Paper,
        actor_kind: "system",
        actor_id: "scheduler",
        build: "mandate-scheduler/0.0.0",
        version: "0.0.0",
        event_time: at,
    };
    let opened = object(vec![
        ("stream_type", text("scheduler")),
        ("workspace_id", text("ws1")),
    ])
    .ok_or("payload")?;
    let tick = object(vec![("clock", text(&at.to_string()))]).ok_or("payload")?;
    let refs = Object::new();
    let draft = |id: &str, event_type: &str, payload| {
        draft_bytes(
            &envelope,
            &Fields {
                event_id: id,
                event_type,
                schema_version: 1,
                causation_id: None,
                correlation_id: None,
                config_refs: &refs,
                payload,
            },
        )
    };
    let drafts = vec![
        draft("20000100000000000000000000", "StreamOpened", &opened).ok_or("draft")?,
        draft("20000100000000000000000001", "ClockAdvanced", &tick).ok_or("draft")?,
    ];
    match store.append(&clock, 0, epoch, at, &drafts) {
        AppendOutcome::Committed(_) => Ok(()),
        other => Err(format!("the clock stream: {}", other.name()).into()),
    }
}

fn config() -> Config {
    Config {
        scope: AccountScope {
            account: AccountRef(ACCOUNT.to_owned()),
            workspace: WorkspaceId("ws1".to_owned()),
        },
        environment: Environment::Paper,
        liveness: Duration::from_secs(10),
        max_chain: 64,
    }
}

#[test]
fn the_binarys_process_reconciles_against_the_fixture_and_is_ready() -> Result<(), Error> {
    let mut store = MemoryStore::default();
    ticked(&mut store, noon()?)?;
    let broker = AlpacaBroker::new(FixturePaper::new(noon()?))?;
    let mut process = ExecutorProcess::start(
        store.clone(),
        broker,
        fixture::wiring(&config().scope)?,
        &config(),
        noon()?,
    );
    process.step(noon()?);
    assert!(process.ready(), "{:?}", process.lifecycle().state());
    let account = StreamId::parse(STREAM).ok_or("stream")?;
    let types = |store: &MemoryStore| -> Result<Vec<String>, Error> {
        Ok(store
            .rows(&account)
            .ok_or("rows")?
            .into_iter()
            .map(|row| row.event_type)
            .collect())
    };
    let first = types(&store)?;
    assert_eq!(first.first().map(String::as_str), Some("StreamOpened"));
    assert!(first.iter().any(|t| t == "ReconciliationRun"), "{first:?}");

    let broker = AlpacaBroker::new(FixturePaper::new(noon()?))?;
    let mut restarted = ExecutorProcess::start(
        store.clone(),
        broker,
        fixture::wiring(&config().scope)?,
        &config(),
        noon()?,
    );
    restarted.step(noon()?);
    assert!(restarted.ready(), "{:?}", restarted.lifecycle().state());
    let again = types(&store)?;
    assert_eq!(again.iter().filter(|t| *t == "StreamOpened").count(), 1);
    Ok(())
}

/// Lane DR's X9: two accounts' executors on one journal, each on the binary's fixture wiring,
/// both open their account streams and are ready. The journal refuses an `event_id` another
/// stream already holds (journal spec §3), so ids derived from the epoch, head and ordinal alone
/// stopped the second account's executor at its first batch.
#[test]
fn two_accounts_executors_on_one_journal_both_open_their_streams() -> Result<(), Error> {
    let mut store = MemoryStore::default();
    ticked(&mut store, noon()?)?;
    let other = "01J8Z2ACCT0000000000000002";
    let mut config_b = config();
    config_b.scope.account = AccountRef(other.to_owned());
    let mut first = ExecutorProcess::start(
        store.clone(),
        AlpacaBroker::new(FixturePaper::new(noon()?))?,
        fixture::wiring(&config().scope)?,
        &config(),
        noon()?,
    );
    let mut second = ExecutorProcess::start(
        store.clone(),
        AlpacaBroker::new(FixturePaper::new(noon()?))?,
        fixture::wiring(&config_b.scope)?,
        &config_b,
        noon()?,
    );
    first.step(noon()?);
    second.step(noon()?);
    assert!(first.ready(), "{:?}", first.lifecycle().state());
    assert!(second.ready(), "{:?}", second.lifecycle().state());
    for stream in [STREAM.to_owned(), format!("acct:ws1:{other}")] {
        let rows = store
            .rows(&StreamId::parse(&stream).ok_or("stream")?)
            .ok_or("rows")?;
        assert_eq!(
            rows.first().map(|row| row.event_type.as_str()),
            Some("StreamOpened"),
            "{stream}"
        );
    }
    Ok(())
}
