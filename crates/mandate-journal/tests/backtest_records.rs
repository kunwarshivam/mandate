//! Journal spec v0.48 §9.18 (DEC-1001, DEC-1002, DEC-1142; E4-5, E17-8): `BacktestRunRecorded`, one backtest
//! of a mandate version, and `EvaluationRunRecorded`, one forward-paper evaluation, closed at
//! schema version 1 on the workspace control stream, with rules 143 to 145 and DEC-1141's writer
//! (a `user` who asked or a `system` runner). Through [`MemoryJournal::append`], which runs the
//! journal's own draft check: each record commits after the control stream's `StreamOpened`, and
//! the stream verifies. Then through [`Draft::parse`] against the vectors' `backtest_records`
//! section: every base and valid draft parses as its record, and every invalid draft is refused
//! with its reason at its path. Before E4-5 the journal refused a backtest record as
//! `unknown_schema` and an evaluation record as `unknown_event_type`, the answers these tests fail
//! on.

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use common::now;
use mandate_canon::{Digest, Key, Object, Value, parse, to_canonical};
use mandate_journal::{
    AppendOutcome, ArtifactRef, Draft, EventCheck, EventFailure, InvalidReason, MemoryJournal,
    StreamId, TrustedStart, verify_events,
};

const CONTROL: &str = "ctl:ws_01J8Z2";
const T: &str = "2026-10-17T15:30:00.000000000Z";
const OPENED: &str = "01J8Z5A0000000000000000000";

/// The two records §9.18 closes, in its order.
const RECORDS: [&str; 2] = ["BacktestRunRecorded", "EvaluationRunRecorded"];

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
    fixture.get("backtest_records").cloned().unwrap()
}

/// The section's stored objects by digest, which §11 check 6 reads every named reference from.
fn artifacts(section: &Value) -> BTreeMap<Digest, Vec<u8>> {
    list(section, "artifacts")
        .iter()
        .map(|artifact| {
            let reference = ArtifactRef::parse(text(artifact, "ref")).unwrap();
            (
                reference.digest(),
                text(artifact, "canonical").as_bytes().to_vec(),
            )
        })
        .collect()
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

fn base_object(section: &Value, record: &str) -> Object {
    section
        .get("drafts")
        .and_then(|d| d.get(&snake(record)))
        .and_then(Value::as_object)
        .cloned()
        .unwrap()
}

fn case<'a>(section: &'a Value, kind: &str, name: &str) -> &'a Value {
    list(section, kind)
        .iter()
        .find(|c| text(c, "name") == name)
        .unwrap_or_else(|| panic!("no {kind} case {name}"))
}

fn opened_journal() -> (MemoryJournal, u64) {
    let mut journal = MemoryJournal::new();
    let epoch = journal.take_ownership(&control());
    let outcome = journal.append(&control(), 0, epoch, now(), &[&opened()]);
    assert!(
        matches!(outcome, AppendOutcome::Committed(_)),
        "{outcome:?}"
    );
    (journal, epoch)
}

#[test]
fn both_records_append_on_the_control_stream_and_the_stream_verifies() {
    let section = section();
    let (mut journal, epoch) = opened_journal();
    let refused = draft(
        &section,
        case(&section, "valid_drafts", "BacktestRunRecorded.refused"),
    );
    let mut bodies: Vec<Vec<u8>> = RECORDS
        .iter()
        .map(|r| to_canonical(&Value::Object(base_object(&section, r))))
        .collect();
    let mut refused_body = parse(&refused).unwrap();
    if let Value::Object(object) = &mut refused_body {
        object.insert(
            Key::new("event_id").unwrap(),
            Value::Str("01J8Z5BT00000000000000BT02".to_owned()),
        );
    }
    bodies.push(to_canonical(&refused_body));
    for (i, body) in bodies.iter().enumerate() {
        let outcome = journal.append(&control(), i as u64 + 1, epoch, now(), &[body]);
        assert!(
            matches!(outcome, AppendOutcome::Committed(_)),
            "draft {i} is journaled on the control stream: {outcome:?}"
        );
    }
    let rows = journal.rows(&control());
    let types: Vec<&str> = rows.iter().skip(1).map(|r| r.event_type.as_str()).collect();
    assert_eq!(
        types,
        [RECORDS[0], RECORDS[1], RECORDS[0]],
        "a completed and a refused backtest, and an evaluation"
    );
    assert_eq!(
        verify_events(rows, TrustedStart::GENESIS, &BTreeMap::new()).map(|v| v.next_seq),
        Err(EventFailure {
            seq: 2,
            check: EventCheck::ArtifactMissing,
        }),
        "a report the store does not hold fails §11 check 6"
    );
    assert_eq!(
        verify_events(rows, TrustedStart::GENESIS, &artifacts(&section)).map(|v| v.next_seq),
        Ok(5),
        "§11's per-event checks pass over the backtest and evaluation records"
    );
}

#[test]
fn both_records_are_refused_off_the_control_stream() {
    let section = section();
    for record in RECORDS {
        for stream in [
            "acct:ws_01J8Z2:ACCT1",
            "agent:ws_01J8Z2:agent_a",
            "clock:ws_01J8Z2",
            "ntf:ws_01J8Z2",
        ] {
            let mut body = base_object(&section, record);
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
fn a_backtest_by_an_agent_is_refused_and_nothing_commits() {
    let section = section();
    let (mut journal, epoch) = opened_journal();
    let mut body = base_object(&section, RECORDS[0]);
    let agent = parse(
        br#"{"kind":"agent","id":"agent_a","version":"0.1.0","build":"sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"}"#,
    )
    .unwrap();
    body.insert(Key::new("actor").unwrap(), agent);
    let outcome = journal.append(
        &control(),
        1,
        epoch,
        now(),
        &[&to_canonical(&Value::Object(body))],
    );
    assert!(
        matches!(&outcome, AppendOutcome::Invalid { draft: 0, error } if error.path == "actor.kind"),
        "DEC-1141: {outcome:?}"
    );
    assert_eq!(journal.head(&control()).seq, 1, "nothing committed");
}

#[test]
fn a_backtest_bound_to_another_version_is_refused_and_nothing_commits() {
    let section = section();
    let (mut journal, epoch) = opened_journal();
    let body = draft(
        &section,
        case(
            &section,
            "invalid_drafts",
            "BacktestRunRecorded.bound_to_another_version",
        ),
    );
    let outcome = journal.append(&control(), 1, epoch, now(), &[&body]);
    assert!(
        matches!(&outcome, AppendOutcome::Invalid { draft: 0, error }
            if (error.reason, error.path.as_str()) == (InvalidReason::Schema, "payload.mandate_version")),
        "rule 144: {outcome:?}"
    );
    assert_eq!(journal.head(&control()).seq, 1, "nothing committed");
}

#[test]
fn every_backtest_and_evaluation_base_and_valid_draft_parses() {
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
        valid.len() >= 15,
        "{} valid drafts; never fewer",
        valid.len()
    );
}

#[test]
fn every_invalid_backtest_and_evaluation_draft_is_refused_with_its_reason_at_its_path() {
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
        invalid.len() >= 111,
        "{} invalid drafts; never fewer",
        invalid.len()
    );
}
