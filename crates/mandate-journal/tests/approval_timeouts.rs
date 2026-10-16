//! Journal spec v0.41 §9.7 (DEC-944; DEC-940 item 7): the runtime's `ApprovalTimedOut`, closed at
//! schema version 1 as `{approval, on_timeout: skip}` with rule 135 (`causation_id` is the ask it
//! ends). Through [`MemoryJournal::append`], which runs the journal's own draft check: a timed-out
//! ask commits and its stream verifies, every malformed draft is refused at its path with nothing
//! committed, and the committed rows replay identically. Before DEC-944 the journal refused every
//! `ApprovalTimedOut` as `unknown_schema`, the answer these tests fail on.

mod common;

use std::collections::BTreeMap;

use common::{edit, now};
use mandate_canon::Digest;
use mandate_journal::{
    AppendOutcome, InvalidReason, MemoryJournal, StoredEvent, StreamId, TrustedStart,
    verify_agent_stream, verify_events,
};

const AGENT: &str = "agent:ws_1:agent_a";
const T: &str = "2026-09-21T14:05:00.000000000Z";
const OPENED: &str = "01J8Z3P0A000000000000000Q0";
const APPROVAL: &str = "01J8Z3P0A000000000000000Q1";
const TIMED_OUT: &str = "01J8Z3P4A000000000000000Q5";
const OTHER: &str = "01J8Z3P2A000000000000000Q3";
/// The bytes of the mandate version the ask was bound to.
const MANDATE: &[u8] = br#"{"mandate":"bound"}"#;

fn agent() -> StreamId {
    StreamId::parse(AGENT).unwrap()
}

fn envelope(
    event_id: &str,
    event_type: &str,
    causation: &str,
    refs: &str,
    payload: &str,
) -> Vec<u8> {
    format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{event_id}","stream_id":"{AGENT}",
        "event_type":"{event_type}","schema_version":1,"event_time":"{T}","clock_source":"local",
        "causation_id":{causation},"correlation_id":null,
        "actor":{{"kind":"agent","id":"agent_a","version":"0.1.0","build":"sha256:{}"}},
        "config_refs":{refs},"payload":{payload},"artifact_refs":[],"pii_refs":[]}}"#,
        "c".repeat(64)
    )
    .into_bytes()
}

fn opened() -> Vec<u8> {
    envelope(
        OPENED,
        "StreamOpened",
        "null",
        "{}",
        r#"{"stream_type":"agent","workspace_id":"ws_1","agent_id":"agent_a"}"#,
    )
}

/// The runtime's record that `APPROVAL`'s deadline passed: caused by the ask, its default `skip`.
fn timed_out() -> Vec<u8> {
    envelope(
        TIMED_OUT,
        "ApprovalTimedOut",
        &format!(r#""{APPROVAL}""#),
        &format!(
            r#"{{"mandate_version":"sha256:{}"}}"#,
            Digest::of(MANDATE).to_hex()
        ),
        &format!(r#"{{"approval":"{APPROVAL}","on_timeout":"skip"}}"#),
    )
}

/// A journal whose agent stream holds its `StreamOpened`, with the writer's epoch.
fn opened_journal() -> (MemoryJournal, u64) {
    let mut journal = MemoryJournal::new();
    let epoch = journal.take_ownership(&agent());
    let outcome = journal.append(&agent(), 0, epoch, now(), &[&opened()]);
    assert!(
        matches!(outcome, AppendOutcome::Committed(_)),
        "{outcome:?}"
    );
    (journal, epoch)
}

fn timed_out_journal() -> MemoryJournal {
    let (mut journal, epoch) = opened_journal();
    let outcome = journal.append(&agent(), 1, epoch, now(), &[&timed_out()]);
    assert!(
        matches!(outcome, AppendOutcome::Committed(_)),
        "a timed-out ask is journaled: {outcome:?}"
    );
    journal
}

/// The artifact store holding the mandate version the timeout names, so §11's artifact check
/// re-hashes it.
fn artifacts() -> BTreeMap<Digest, Vec<u8>> {
    BTreeMap::from([(Digest::of(MANDATE), MANDATE.to_vec())])
}

#[test]
fn a_timed_out_ask_appends_and_its_stream_verifies() {
    let journal = timed_out_journal();
    let rows = journal.rows(&agent());
    let types: Vec<&str> = rows.iter().map(|r| r.event_type.as_str()).collect();
    assert_eq!(types, ["StreamOpened", "ApprovalTimedOut"]);
    assert_eq!(rows[1].event_id, TIMED_OUT);
    assert_eq!(
        verify_events(rows, TrustedStart::GENESIS, &artifacts()).map(|v| v.next_seq),
        Ok(3),
        "§11's per-event checks pass over the timed-out record"
    );
    assert_eq!(verify_agent_stream(rows, TrustedStart::GENESIS), Ok(()));
}

#[test]
fn a_malformed_timeout_is_refused_at_its_path_and_commits_nothing() {
    let base = timed_out();
    let cases: &[(&str, Option<&str>, InvalidReason, &str)] = &[
        (
            "causation_id",
            Some("null"),
            InvalidReason::Schema,
            "causation_id",
        ),
        (
            "causation_id",
            Some(&format!(r#""{OTHER}""#)),
            InvalidReason::Schema,
            "causation_id",
        ),
        (
            "payload.approval",
            Some(&format!(r#""{OTHER}""#)),
            InvalidReason::Schema,
            "causation_id",
        ),
        (
            "payload.approval",
            Some("7"),
            InvalidReason::Schema,
            "payload.approval",
        ),
        (
            "payload.approval",
            Some(r#""not-a-ulid""#),
            InvalidReason::NonCanonical,
            "payload.approval",
        ),
        (
            "payload.approval",
            None,
            InvalidReason::Schema,
            "payload.approval",
        ),
        (
            "payload.on_timeout",
            Some(r#""act""#),
            InvalidReason::NonCanonical,
            "payload.on_timeout",
        ),
        (
            "payload.on_timeout",
            Some("null"),
            InvalidReason::Schema,
            "payload.on_timeout",
        ),
        (
            "payload.on_timeout",
            None,
            InvalidReason::Schema,
            "payload.on_timeout",
        ),
        (
            "payload.deadline",
            Some("1790000300"),
            InvalidReason::Schema,
            "payload.deadline",
        ),
        (
            "config_refs",
            Some("{}"),
            InvalidReason::MissingConfigRef,
            "config_refs.mandate_version",
        ),
        (
            "schema_version",
            Some("2"),
            InvalidReason::UnknownSchema,
            "payload",
        ),
    ];
    let mut failed = Vec::new();
    for (path, value, reason, at) in cases {
        let (mut journal, epoch) = opened_journal();
        let draft = edit(&base, path, *value);
        let got = match journal.append(&agent(), 1, epoch, now(), &[&draft]) {
            AppendOutcome::Invalid { draft: 0, error } => Some((error.reason, error.path)),
            _ => None,
        };
        if got != Some((*reason, (*at).to_owned())) {
            failed.push(format!(
                "{path} = {value:?}: expected {reason:?} at {at}, got {got:?}"
            ));
        }
        if journal.head(&agent()).seq != 1 {
            failed.push(format!("{path} = {value:?}: a refused draft committed"));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

#[test]
fn a_committed_timeout_replays_identically() {
    let journal = timed_out_journal();
    let again = timed_out_journal();
    let rows: &[StoredEvent] = journal.rows(&agent());
    assert_eq!(
        rows,
        again.rows(&agent()),
        "the same drafts seal the same chain"
    );
    let mut replayed = timed_out_journal();
    let epoch = replayed.head(&agent()).writer_epoch;
    let outcome = replayed.append(&agent(), 2, epoch, now(), &[&timed_out()]);
    assert!(
        matches!(&outcome, AppendOutcome::AlreadyCommitted(sealed) if sealed.as_slice() == &rows[1..]),
        "a retried timeout is its committed row, not a second record: {outcome:?}"
    );
    assert_eq!(replayed.rows(&agent()), rows);
    assert_eq!(
        verify_events(replayed.rows(&agent()), TrustedStart::GENESIS, &artifacts())
            .map(|v| v.last_hash),
        Ok(rows[1].hash)
    );
}
