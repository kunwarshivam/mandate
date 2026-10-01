//! Rows and drafts for the cold-store tests, in the journal tests' own shape.
#![allow(dead_code, reason = "the cold tests use a subset of these builders")]

use mandate_canon::{Key, Value, parse, to_canonical};
use mandate_journal::{AppendOutcome, MemoryJournal, StreamId};
use mandate_time::UtcNanos;

pub const STREAM: &str = "acct:ws_1:ACCT1";
pub const T: &str = "2026-09-21T14:00:00.000000000Z";

pub fn event_id(n: u64) -> String {
    format!("01J8Z3M4{n:018}")
}

pub fn opened_draft() -> Vec<u8> {
    format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{}","stream_id":"{STREAM}",
        "event_type":"StreamOpened","schema_version":1,"event_time":"{T}","clock_source":"local",
        "causation_id":null,"correlation_id":null,
        "actor":{{"kind":"system","id":"executor","version":"0.1.0","build":"sha256:{}"}},
        "config_refs":{{}},"payload":{{"stream_type":"account","workspace_id":"ws_1","broker":"alpaca",
        "account_ref":"ACCT1"}},"artifact_refs":[],"pii_refs":[]}}"#,
        event_id(0),
        "3".repeat(64)
    )
    .into_bytes()
}

pub fn mark_draft(n: u64) -> Vec<u8> {
    format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{}","stream_id":"{STREAM}",
        "event_type":"MarkUpdated","schema_version":1,"event_time":"{T}","clock_source":"local",
        "causation_id":null,"correlation_id":null,
        "actor":{{"kind":"system","id":"executor","version":"0.1.0","build":"sha256:{}"}},
        "config_refs":{{}},"payload":{{"instrument_id":"inst","price":"1","source":"quote",
        "feed":"iex","risk_clock":"{T}"}},"artifact_refs":[],"pii_refs":[]}}"#,
        event_id(n),
        "3".repeat(64)
    )
    .into_bytes()
}

pub fn stream() -> StreamId {
    StreamId::parse(STREAM).unwrap()
}

pub fn now() -> UtcNanos {
    UtcNanos::parse(T).unwrap()
}

/// A journal whose stream holds StreamOpened and `marks` MarkUpdated events, with writer epoch 1:
/// `marks + 1` rows, `seq` 1 to `marks + 1`.
pub fn journal_with(marks: u64) -> MemoryJournal {
    let mut j = MemoryJournal::new();
    let epoch = j.take_ownership(&stream());
    assert!(matches!(
        j.append(&stream(), 0, epoch, now(), &[&opened_draft()]),
        AppendOutcome::Committed(_)
    ));
    for n in 1..=marks {
        let outcome = j.append(&stream(), n, epoch, now(), &[&mark_draft(n)]);
        assert!(
            matches!(outcome, AppendOutcome::Committed(_)),
            "{outcome:?}"
        );
    }
    j
}

/// `value` re-canonicalized, for building a manifest's edited bytes inside a test's own oracle.
pub fn canonical(value: &Value) -> Vec<u8> {
    to_canonical(value)
}

/// The six-field manifest object §6.2 names, as the test's own oracle builds it.
pub fn manifest_value(
    stream: &str,
    first_seq: u64,
    last_seq: u64,
    first_prev_hash: &str,
    last_hash: &str,
    file_sha256: &str,
) -> Value {
    let mut object = mandate_canon::Object::new();
    object.insert(Key::new("stream").unwrap(), Value::Str(stream.to_owned()));
    object.insert(
        Key::new("first_seq").unwrap(),
        Value::Int(mandate_canon::Int::new(first_seq).unwrap()),
    );
    object.insert(
        Key::new("last_seq").unwrap(),
        Value::Int(mandate_canon::Int::new(last_seq).unwrap()),
    );
    object.insert(
        Key::new("first_prev_hash").unwrap(),
        Value::Str(first_prev_hash.to_owned()),
    );
    object.insert(
        Key::new("last_hash").unwrap(),
        Value::Str(last_hash.to_owned()),
    );
    object.insert(
        Key::new("file_sha256").unwrap(),
        Value::Str(file_sha256.to_owned()),
    );
    Value::Object(object)
}

/// Parses canonical bytes back to a value, for a test's own edits.
pub fn parsed(bytes: &[u8]) -> Value {
    parse(bytes).unwrap()
}
