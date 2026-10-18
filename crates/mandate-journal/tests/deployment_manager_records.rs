//! Journal spec v0.48 §9.21 (DEC-1170; E21-33): the deployment manager's starts, stops, and crash
//! loops, closed at schema version 1 on the workspace control stream and written by workspace
//! control services as a `system` actor. Through [`MemoryJournal::append`], which runs the journal's
//! own draft check: each record commits after the control stream's `StreamOpened`, and the stream
//! verifies. Then through [`Draft::parse`] against the vectors' `deployment_manager` section: every
//! base and valid draft parses as its record, and every invalid draft is refused with its reason at
//! its path. Before DEC-1170 the journal refused every one of them as `unknown_event_type`, the
//! answer these tests fail on.

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use common::now;
use mandate_canon::{Key, Object, Value, parse, to_canonical};
use mandate_journal::{
    AppendOutcome, Draft, InvalidReason, MemoryJournal, StreamId, TrustedStart, verify_events,
};

const CONTROL: &str = "ctl:ws_01J8Z2";
const T: &str = "2026-10-17T15:00:00.000000000Z";
const OPENED: &str = "01J8Z4DN000000000000000000";

/// The three records §9.21 closes, in its order.
const RECORDS: [&str; 3] = [
    "ProcessStartIssued",
    "ProcessStopIssued",
    "ProcessCrashLooped",
];

fn control() -> StreamId {
    StreamId::parse(CONTROL).unwrap()
}

fn opened() -> Vec<u8> {
    format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{OPENED}","stream_id":"{CONTROL}",
        "event_type":"StreamOpened","schema_version":1,"event_time":"{T}","clock_source":"local",
        "causation_id":null,"correlation_id":null,
        "actor":{{"kind":"system","id":"control_services","version":"0.1.0","build":"sha256:{}"}},
        "config_refs":{{}},"payload":{{"stream_type":"control","workspace_id":"ws_01J8Z2"}},
        "artifact_refs":[],"pii_refs":[]}}"#,
        "d".repeat(64)
    )
    .into_bytes()
}

fn section() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get("deployment_manager").cloned().unwrap()
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

/// A record's base-draft key: §4.1's key grammar admits no capital letter.
fn snake(record: &str) -> String {
    let mut out = String::new();
    for (i, c) in record.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

fn base(section: &Value, record: &str) -> Vec<u8> {
    let body = section
        .get("drafts")
        .and_then(|d| d.get(&snake(record)))
        .unwrap();
    to_canonical(body)
}

#[test]
fn every_deployment_manager_record_appends_on_the_control_stream_and_the_stream_verifies() {
    let section = section();
    let mut journal = MemoryJournal::new();
    let epoch = journal.take_ownership(&control());
    let outcome = journal.append(&control(), 0, epoch, now(), &[&opened()]);
    assert!(
        matches!(outcome, AppendOutcome::Committed(_)),
        "{outcome:?}"
    );
    for (i, record) in RECORDS.iter().enumerate() {
        let outcome = journal.append(
            &control(),
            i as u64 + 1,
            epoch,
            now(),
            &[&base(&section, record)],
        );
        assert!(
            matches!(outcome, AppendOutcome::Committed(_)),
            "{record} is journaled on the control stream: {outcome:?}"
        );
    }
    let rows = journal.rows(&control());
    let types: Vec<&str> = rows.iter().skip(1).map(|r| r.event_type.as_str()).collect();
    assert_eq!(types, RECORDS);
    assert_eq!(
        verify_events(rows, TrustedStart::GENESIS, &BTreeMap::new()).map(|v| v.next_seq),
        Ok(5),
        "§11's per-event checks pass over the deployment manager's records"
    );
}

#[test]
fn every_deployment_manager_record_is_refused_off_the_control_stream() {
    let section = section();
    for record in RECORDS {
        for stream in [
            "acct:ws_01J8Z2:ACCT1",
            "agent:ws_01J8Z2:agent_a",
            "clock:ws_01J8Z2",
        ] {
            let mut body = section
                .get("drafts")
                .and_then(|d| d.get(&snake(record)))
                .and_then(Value::as_object)
                .cloned()
                .unwrap();
            body.insert(
                Key::new("stream_id").unwrap(),
                Value::Str(stream.to_owned()),
            );
            let e = Draft::parse(&to_canonical(&Value::Object(body))).unwrap_err();
            assert_eq!(
                (e.reason, e.path.as_str()),
                (InvalidReason::WrongStream, "event_type"),
                "{record} on {stream}"
            );
        }
    }
}

#[test]
fn a_deployment_manager_record_by_a_user_is_refused_and_nothing_commits() {
    let section = section();
    let mut journal = MemoryJournal::new();
    let epoch = journal.take_ownership(&control());
    let outcome = journal.append(&control(), 0, epoch, now(), &[&opened()]);
    assert!(matches!(outcome, AppendOutcome::Committed(_)));
    let mut body = section
        .get("drafts")
        .and_then(|d| d.get("process_start_issued"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap();
    let user =
        parse(br#"{"kind":"user","id":"user_owner_01","version":"1","build":null}"#).unwrap();
    body.insert(Key::new("actor").unwrap(), user);
    let outcome = journal.append(
        &control(),
        1,
        epoch,
        now(),
        &[&to_canonical(&Value::Object(body))],
    );
    assert!(
        matches!(&outcome, AppendOutcome::Invalid { draft: 0, error } if error.path == "actor.kind"),
        "rule 165: {outcome:?}"
    );
    assert_eq!(journal.head(&control()).seq, 1, "nothing committed");
}

#[test]
fn every_deployment_manager_base_and_valid_draft_parses() {
    let section = section();
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    let mut failed = Vec::new();
    for (name, body) in drafts {
        let parsed = Draft::parse(&to_canonical(body)).map(|d| d.event_type().to_owned());
        let record = RECORDS.iter().find(|r| snake(r) == name.as_str());
        if parsed.as_deref().ok() != record.copied() {
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
    assert_eq!(drafts.len(), RECORDS.len(), "one base draft per record");
    assert!(
        valid.len() >= 8,
        "{} valid drafts; never fewer",
        valid.len()
    );
}

#[test]
fn every_invalid_deployment_manager_draft_is_refused_with_its_reason_at_its_path() {
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
        invalid.len() >= 57,
        "{} invalid drafts; never fewer",
        invalid.len()
    );
}
