//! The connect check of a paper Alpaca connection (DEC-2007 item 4; connections spec §8.1; journal
//! spec §9.8), on the in-memory journal, which judges every record it is given against the
//! registered schemas, and the fixture broker through `mandate-alpaca`'s client.

use mandate_canon::{Value, parse};
use mandate_domain::{CapabilityProfile, ProfileError};
use mandate_executor::{
    AccountRef, AccountScope, BrokerOutcome, BrokerRequest, ConnectorError, WorkspaceId,
};
use mandate_executord::Broker;
use mandate_executord::alpaca::AlpacaBroker;
use mandate_executord::connect::connect_check;
use mandate_executord::fixture::Ids;
use mandate_executord::paper::FixturePaper;
use mandate_journal::StreamId;
use mandate_store::{MemoryStore, Store};
use mandate_time::UtcNanos;

type Error = Box<dyn std::error::Error>;

const ACCOUNT: &str = "01J8Z2ACCT0000000000000001";
const STREAM: &str = "acct:ws1:01J8Z2ACCT0000000000000001";
/// A personal-data vault reference: `pii_` and a ULID (journal spec §6.4).
const PII: &str = "pii_01J8Z2PQ000000000000000001";

fn scope() -> AccountScope {
    AccountScope {
        account: AccountRef(ACCOUNT.to_owned()),
        workspace: WorkspaceId("ws1".to_owned()),
    }
}

fn now() -> Result<UtcNanos, Error> {
    Ok(UtcNanos::parse_rfc3339("2026-09-21T15:00:00.5Z")?)
}

fn payloads(store: &MemoryStore, event_type: &str) -> Result<Vec<(Value, Value)>, Error> {
    let stream = StreamId::parse(STREAM).ok_or("stream")?;
    let rows = store.rows(&stream).ok_or("rows")?;
    rows.iter()
        .filter(|row| row.event_type == event_type)
        .map(|row| {
            let body = parse(&row.body)?;
            Ok((
                body.get("payload").cloned().ok_or("payload")?,
                body.get("pii_refs").cloned().ok_or("pii_refs")?,
            ))
        })
        .collect()
}

/// A passing check opens the new account stream and records every check passed, ascending by
/// check, with the account's opaque reference in the payload and the envelope's `pii_refs`, and
/// the journal admits both records. The event id it answers is the record's.
#[test]
fn a_passing_check_opens_the_stream_and_records_every_check() -> Result<(), Error> {
    let mut store = MemoryStore::default();
    let mut broker = AlpacaBroker::new(FixturePaper::new(now()?))?;
    let checked = connect_check(
        &mut store,
        &mut broker,
        &Ids::of(&scope()),
        &scope(),
        "conn1",
        PII,
        now()?,
    )?;
    assert!(checked.passed);
    let stream = StreamId::parse(STREAM).ok_or("stream")?;
    let rows = store.rows(&stream).ok_or("rows")?;
    let types: Vec<&str> = rows.iter().map(|r| r.event_type.as_str()).collect();
    assert_eq!(types, ["StreamOpened", "ConnectionChecked"]);
    assert_eq!(
        rows.last().map(|r| r.event_id.as_str()),
        Some(checked.event_id.as_str())
    );
    let records = payloads(&store, "ConnectionChecked")?;
    let [(payload, pii)] = records.as_slice() else {
        return Err("one record".into());
    };
    let text = |v: &Value, m: &str| v.get(m).and_then(Value::as_str).map(str::to_owned);
    assert_eq!(text(payload, "account_pii_ref").as_deref(), Some(PII));
    assert_eq!(text(payload, "connection_id").as_deref(), Some("conn1"));
    assert_eq!(text(payload, "occasion").as_deref(), Some("connect"));
    assert_eq!(
        text(payload, "risk_clock").as_deref(),
        Some("2026-09-21T15:00:00.000000000Z")
    );
    let results: Vec<(Option<String>, Option<String>)> = match payload.get("results") {
        Some(Value::Array(results)) => results
            .iter()
            .map(|r| (text(r, "check"), text(r, "result")))
            .collect(),
        _ => return Err("results".into()),
    };
    let passed = |check: &str| (Some(check.to_owned()), Some("passed".to_owned()));
    assert_eq!(
        results,
        [passed("account"), passed("environment"), passed("scope")]
    );
    assert_eq!(*pii, Value::Array(vec![Value::Str(PII.to_owned())]));
    Ok(())
}

/// A check on a stream already opened appends only its record, never a second `StreamOpened`.
#[test]
fn a_second_check_appends_only_its_record() -> Result<(), Error> {
    let mut store = MemoryStore::default();
    let mut broker = AlpacaBroker::new(FixturePaper::new(now()?))?;
    let first = connect_check(
        &mut store,
        &mut broker,
        &Ids::of(&scope()),
        &scope(),
        "conn1",
        PII,
        now()?,
    )?;
    let second = connect_check(
        &mut store,
        &mut broker,
        &Ids::of(&scope()),
        &scope(),
        "conn1",
        PII,
        now()?,
    )?;
    assert_ne!(first.event_id, second.event_id);
    let stream = StreamId::parse(STREAM).ok_or("stream")?;
    let types: Vec<String> = store
        .rows(&stream)
        .ok_or("rows")?
        .into_iter()
        .map(|r| r.event_type)
        .collect();
    assert_eq!(
        types,
        ["StreamOpened", "ConnectionChecked", "ConnectionChecked"]
    );
    Ok(())
}

/// A broker whose account cannot be read fails the account check: the failure is journaled with
/// its reason, no personal-data reference is recorded, and the check does not pass.
#[test]
fn an_unreadable_account_fails_the_check_and_is_journaled() -> Result<(), Error> {
    struct Unreadable;
    impl Broker for Unreadable {
        fn profile(&self) -> Result<CapabilityProfile, ProfileError> {
            mandate_alpaca::alpaca_profile()
        }
        fn call(&mut self, _request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError> {
            Err(ConnectorError::NotSent {
                code: "unreachable",
            })
        }
    }
    let mut store = MemoryStore::default();
    let checked = connect_check(
        &mut store,
        &mut Unreadable,
        &Ids::of(&scope()),
        &scope(),
        "conn1",
        PII,
        now()?,
    )?;
    assert!(!checked.passed);
    let records = payloads(&store, "ConnectionChecked")?;
    let [(payload, pii)] = records.as_slice() else {
        return Err("one record".into());
    };
    assert_eq!(payload.get("account_pii_ref"), Some(&Value::Null));
    assert_eq!(*pii, Value::Array(Vec::new()));
    let account = match payload.get("results") {
        Some(Value::Array(results)) => results
            .iter()
            .find(|r| r.get("check").and_then(Value::as_str) == Some("account"))
            .cloned(),
        _ => None,
    }
    .ok_or("the account result")?;
    assert_eq!(
        account.get("result").and_then(Value::as_str),
        Some("failed")
    );
    assert!(account.get("reason").and_then(Value::as_str).is_some());
    Ok(())
}

/// Ids that do not read as identifiers are refused before anything is taken or written.
#[test]
fn an_invalid_connection_or_reference_writes_nothing() -> Result<(), Error> {
    let mut store = MemoryStore::default();
    let mut broker = AlpacaBroker::new(FixturePaper::new(now()?))?;
    assert_eq!(
        connect_check(
            &mut store,
            &mut broker,
            &Ids::of(&scope()),
            &scope(),
            "conn 1",
            PII,
            now()?
        ),
        Err("connection_id_invalid")
    );
    for reference in [
        "pii ref",
        "not-a-reference",
        "pii_01J8Z2",
        "01J8Z2PQ000000000000000001",
    ] {
        assert_eq!(
            connect_check(
                &mut store,
                &mut broker,
                &Ids::of(&scope()),
                &scope(),
                "conn1",
                reference,
                now()?
            ),
            Err("pii_ref_invalid"),
            "{reference}"
        );
    }
    let stream = StreamId::parse(STREAM).ok_or("stream")?;
    assert!(store.rows(&stream).ok_or("rows")?.is_empty());
    Ok(())
}
