//! Journal spec v0.43 §9.7 (DEC-950; DEC-944 item 5): the runtime's `ApprovalCanceled`, closed at
//! schema version 1 as `{approval, reason}` with the five cancel reasons the runtime writes and no
//! legacy `rebound`. Through [`MemoryJournal::append`], which runs the journal's own draft check:
//! a canceled ask commits and its stream verifies, and every reason commits. Then through
//! [`Draft::parse`] against the vectors' `approval_cancellations` section: every base and valid
//! draft parses, and every invalid draft is refused with its reason at its path. Before DEC-950 the
//! journal refused every `ApprovalCanceled` as `unknown_schema`, the answer these tests fail on.

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use common::{edit, now};
use mandate_canon::{Digest, Key, Object, Value, parse, to_canonical};
use mandate_journal::{
    AppendOutcome, Draft, MemoryJournal, StreamId, TrustedStart, verify_agent_stream, verify_events,
};

const AGENT: &str = "agent:ws_1:agent_a";
const T: &str = "2026-09-21T14:05:00.000000000Z";
const OPENED: &str = "01J8Z3P0A000000000000000Q0";
const APPROVAL: &str = "01J8Z3P0A000000000000000Q1";
const CANCELED: &str = "01J8Z3P4A000000000000000Q6";
/// The bytes of the mandate version the ask was bound to.
const MANDATE: &[u8] = br#"{"mandate":"bound"}"#;
/// Every cancel reason the runtime writes (mandate spec §2.2, §5.9, §6.4).
const REASONS: [&str; 5] = [
    "version_applied",
    "mode_tightened",
    "owner_pause",
    "owner_stop",
    "kill_switch",
];

fn agent() -> StreamId {
    StreamId::parse(AGENT).unwrap()
}

fn envelope(event_id: &str, event_type: &str, refs: &str, payload: &str) -> Vec<u8> {
    format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{event_id}","stream_id":"{AGENT}",
        "event_type":"{event_type}","schema_version":1,"event_time":"{T}","clock_source":"local",
        "causation_id":null,"correlation_id":null,
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
        "{}",
        r#"{"stream_type":"agent","workspace_id":"ws_1","agent_id":"agent_a"}"#,
    )
}

/// The runtime's record that a version applied canceled `APPROVAL`.
fn canceled() -> Vec<u8> {
    envelope(
        CANCELED,
        "ApprovalCanceled",
        &format!(
            r#"{{"mandate_version":"sha256:{}"}}"#,
            Digest::of(MANDATE).to_hex()
        ),
        &format!(r#"{{"approval":"{APPROVAL}","reason":"version_applied"}}"#),
    )
}

/// `draft` appended after the agent stream's `StreamOpened`.
fn appended(draft: &[u8]) -> (MemoryJournal, AppendOutcome) {
    let mut journal = MemoryJournal::new();
    let epoch = journal.take_ownership(&agent());
    let outcome = journal.append(&agent(), 0, epoch, now(), &[&opened()]);
    assert!(
        matches!(outcome, AppendOutcome::Committed(_)),
        "{outcome:?}"
    );
    let outcome = journal.append(&agent(), 1, epoch, now(), &[draft]);
    (journal, outcome)
}

#[test]
fn a_canceled_ask_appends_and_its_stream_verifies() {
    let (journal, outcome) = appended(&canceled());
    assert!(
        matches!(outcome, AppendOutcome::Committed(_)),
        "a canceled ask is journaled: {outcome:?}"
    );
    let rows = journal.rows(&agent());
    let types: Vec<&str> = rows.iter().map(|r| r.event_type.as_str()).collect();
    assert_eq!(types, ["StreamOpened", "ApprovalCanceled"]);
    let artifacts = BTreeMap::from([(Digest::of(MANDATE), MANDATE.to_vec())]);
    assert_eq!(
        verify_events(rows, TrustedStart::GENESIS, &artifacts).map(|v| v.next_seq),
        Ok(3),
        "§11's per-event checks pass over the canceled record"
    );
    assert_eq!(verify_agent_stream(rows, TrustedStart::GENESIS), Ok(()));
}

#[test]
fn every_cancel_reason_commits_and_the_legacy_rebound_does_not() {
    for reason in REASONS {
        let draft = edit(
            &canceled(),
            "payload.reason",
            Some(&format!(r#""{reason}""#)),
        );
        let (_, outcome) = appended(&draft);
        assert!(
            matches!(outcome, AppendOutcome::Committed(_)),
            "{reason}: {outcome:?}"
        );
    }
    let rebound = edit(&canceled(), "payload.reason", Some(r#""rebound""#));
    let (journal, outcome) = appended(&rebound);
    assert!(
        matches!(&outcome, AppendOutcome::Invalid { draft: 0, error } if error.path == "payload.reason"),
        "{outcome:?}"
    );
    assert_eq!(journal.head(&agent()).seq, 1, "nothing committed");
}

fn section() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get("approval_cancellations").cloned().unwrap()
}

fn list<'a>(value: &'a Value, name: &str) -> &'a [Value] {
    value
        .get(name)
        .and_then(Value::as_array)
        .unwrap_or_default()
}

fn text<'a>(value: &'a Value, name: &str) -> &'a str {
    value.get(name).and_then(Value::as_str).unwrap_or_default()
}

/// A `{path, value}` or `{path, delete: true}` change, dotted from the envelope.
fn apply(draft: &mut Object, change: &Value) {
    let path = text(change, "path");
    let mut names: Vec<&str> = path.split('.').collect();
    let last = names.pop().unwrap();
    let mut node = draft;
    for name in names {
        node = match node.get_mut(name) {
            Some(Value::Object(inner)) => inner,
            other => panic!("`{path}` has no object `{name}`: {other:?}"),
        };
    }
    if change.get("delete") == Some(&Value::Bool(true)) {
        assert!(node.remove(last).is_some(), "`{path}` deletes nothing");
    } else {
        node.insert(
            Key::new(last).unwrap(),
            change.get("value").cloned().unwrap(),
        );
    }
}

/// A case's draft: its named base draft with its changes applied, in canonical bytes.
fn draft(section: &Value, case: &Value) -> Vec<u8> {
    let base = section
        .get("drafts")
        .and_then(|d| d.get(text(case, "base_draft")));
    let mut body = base.and_then(Value::as_object).cloned().unwrap();
    for change in list(case, "changes") {
        apply(&mut body, change);
    }
    to_canonical(&Value::Object(body))
}

#[test]
fn every_approval_cancel_base_and_valid_draft_parses() {
    let section = section();
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    let mut failed = Vec::new();
    for (name, body) in drafts {
        let parsed = Draft::parse(&to_canonical(body)).map(|d| d.event_type().to_owned());
        if parsed.as_deref() != Ok("ApprovalCanceled") {
            failed.push(format!("base {}: {parsed:?}", name.as_str()));
        }
    }
    let valid = list(&section, "valid_drafts");
    for case in valid {
        if let Err(e) = Draft::parse(&draft(&section, case)) {
            failed.push(format!("valid {}: {e:?}", text(case, "name")));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert_eq!(drafts.len(), 1, "one base draft");
    assert!(
        valid.len() >= 7,
        "{} valid drafts; never fewer",
        valid.len()
    );
}

#[test]
fn every_invalid_approval_cancel_is_refused_with_its_reason_at_its_path() {
    let section = section();
    let invalid = list(&section, "invalid_drafts");
    let mut failed = Vec::new();
    for case in invalid {
        let expect = case.get("expect").unwrap();
        let want = (text(expect, "reason"), text(expect, "path"));
        let got = Draft::parse(&draft(&section, case)).err();
        let got = got.as_ref().map(|e| (e.reason.code(), e.path.as_str()));
        if got != Some(want) {
            failed.push(format!(
                "{}: expected {want:?}, got {got:?}",
                text(case, "name")
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(
        invalid.len() >= 11,
        "{} invalid drafts; never fewer",
        invalid.len()
    );
}
