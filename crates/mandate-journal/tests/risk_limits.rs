//! Journal spec v0.58 §9.27 (DEC-2301; E6-15): the account stream's seven risk-limit records,
//! `RiskDayStarted`, `RiskLimitTriggered`, `RiskLimitLifted`, `HighWaterMarkReset`,
//! `PositionReleased`, `InstrumentRestrictionChanged`, and `GoalCompleted`, closed at schema version
//! 1 with rules 230 to 238. Through [`Draft::parse`] against the vectors' `risk_limits` section:
//! every base and valid draft parses as its record, and every invalid draft is refused with its
//! reason at its path. Through [`MemoryJournal::append`]: each record commits on the account stream
//! after its `StreamOpened`, and the stream verifies. Before E6-15's registration the journal
//! refuses every one of them as `unknown_schema` at `payload`, the answer these tests fail on.

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use common::{edit, now};
use mandate_canon::{Digest, Key, Object, ParseErrorKind, Value, parse, to_canonical};
use mandate_journal::{
    AppendOutcome, Draft, InvalidReason, MemoryJournal, StreamId, TrustedStart, verify_events,
};

const ACCOUNT: &str = "acct:ws_01J8Z2:01J8Z2ACCT00000000000000A1";
const T: &str = "2026-09-22T14:30:00.000000000Z";
const OPENED: &str = "01J8Z4R0A000000000000000Z0";
/// The mandate version the appended drafts name, stored so that §11 check 6 re-hashes it: the
/// vectors' own `mandate_version` is a placeholder digest no stored object hashes to.
const MANDATE: &[u8] = br#"{"mandate":"bound"}"#;

/// Each base draft of the section, by name, and the record it is.
const BASES: [(&str, &str); 9] = [
    ("day_started", "RiskDayStarted"),
    ("rung_triggered", "RiskLimitTriggered"),
    ("tripwire_triggered", "RiskLimitTriggered"),
    ("rung_lifted", "RiskLimitLifted"),
    ("tripwire_lifted", "RiskLimitLifted"),
    ("mark_reset", "HighWaterMarkReset"),
    ("owner_released", "PositionReleased"),
    ("mark_stale", "InstrumentRestrictionChanged"),
    ("goal_completed", "GoalCompleted"),
];

/// The base drafts appended in one batch: every one but the owner's release, whose `warning_ref`
/// names an artifact the in-memory journal does not hold.
const APPENDED: [&str; 8] = [
    "day_started",
    "rung_triggered",
    "tripwire_triggered",
    "rung_lifted",
    "tripwire_lifted",
    "mark_reset",
    "mark_stale",
    "goal_completed",
];

fn account() -> StreamId {
    StreamId::parse(ACCOUNT).unwrap()
}

fn opened() -> Vec<u8> {
    format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{OPENED}","stream_id":"{ACCOUNT}",
        "event_type":"StreamOpened","schema_version":1,"event_time":"{T}","clock_source":"local",
        "causation_id":null,"correlation_id":null,
        "actor":{{"kind":"system","id":"executor","version":"0.1.0","build":"sha256:{}"}},
        "config_refs":{{}},"payload":{{"stream_type":"account","workspace_id":"ws_01J8Z2",
        "broker":"alpaca","account_ref":"01J8Z2ACCT00000000000000A1"}},"artifact_refs":[],"pii_refs":[]}}"#,
        "3".repeat(64)
    )
    .into_bytes()
}

fn section() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get("risk_limits").cloned().unwrap()
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

fn base(section: &Value, name: &str) -> Vec<u8> {
    to_canonical(section.get("drafts").and_then(|d| d.get(name)).unwrap())
}

#[test]
fn every_risk_limit_base_and_valid_draft_parses() {
    let section = section();
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    let mut failed = Vec::new();
    for (name, body) in drafts {
        let parsed = Draft::parse(&to_canonical(body)).map(|d| d.event_type().to_owned());
        let record = BASES
            .iter()
            .find(|(n, _)| *n == name.as_str())
            .map(|(_, r)| *r);
        if parsed.as_deref().ok() != record {
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
    assert_eq!(drafts.len(), BASES.len(), "one base draft per shape");
    assert!(
        valid.len() >= 25,
        "{} valid drafts; never fewer",
        valid.len()
    );
}

#[test]
fn every_invalid_risk_limit_draft_is_refused_with_its_reason_at_its_path() {
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
        invalid.len() >= 173,
        "{} invalid drafts; never fewer",
        invalid.len()
    );
}

#[test]
fn the_risk_limit_records_append_on_the_account_stream_and_the_stream_verifies() {
    let section = section();
    let mut journal = MemoryJournal::new();
    let epoch = journal.take_ownership(&account());
    let outcome = journal.append(&account(), 0, epoch, now(), &[&opened()]);
    assert!(
        matches!(outcome, AppendOutcome::Committed(_)),
        "{outcome:?}"
    );
    let bound = format!(
        r#"{{"mandate_version":"sha256:{}"}}"#,
        Digest::of(MANDATE).to_hex()
    );
    let drafts: Vec<Vec<u8>> = APPENDED
        .iter()
        .map(|n| edit(&base(&section, n), "config_refs", Some(&bound)))
        .collect();
    let refs: Vec<&[u8]> = drafts.iter().map(Vec::as_slice).collect();
    let outcome = journal.append(&account(), 1, epoch, now(), &refs);
    assert!(
        matches!(outcome, AppendOutcome::Committed(_)),
        "§9.27's records commit on the account stream: {outcome:?}"
    );
    let rows = journal.rows(&account());
    let artifacts = BTreeMap::from([(Digest::of(MANDATE), MANDATE.to_vec())]);
    let types: Vec<&str> = rows.iter().skip(1).map(|r| r.event_type.as_str()).collect();
    let want: Vec<&str> = APPENDED
        .iter()
        .map(|n| BASES.iter().find(|(b, _)| b == n).unwrap().1)
        .collect();
    assert_eq!(types, want);
    assert_eq!(
        verify_events(rows, TrustedStart::GENESIS, &artifacts).map(|v| v.next_seq),
        Ok(APPENDED.len() as u64 + 2),
        "§11's per-event checks pass over the risk-limit records"
    );
}

/// §4.4 keeps every integer non-negative, so a negative `breach_time_s` is refused when the draft
/// is decoded, before §9.27 is read: the vectors cannot carry one, since the fixture file would not
/// parse (the A1a review's minor, DEC-2301).
#[test]
fn a_negative_breach_time_is_refused_before_any_rule_reads_it() {
    let rung = base(&section(), "rung_triggered");
    let text = String::from_utf8(rung).unwrap();
    let negative = text.replace("\"breach_time_s\":300", "\"breach_time_s\":-300");
    assert_ne!(
        negative, text,
        "the base rung trigger carries breach_time_s 300"
    );
    let e = Draft::parse(negative.as_bytes()).unwrap_err();
    assert_eq!(
        (e.reason, e.path.as_str()),
        (InvalidReason::Json(ParseErrorKind::IntegerRange), ""),
        "refused at decode, before the payload schema"
    );
}

/// Rule 233 reports `payload.threshold`, then `payload.value`: a counted tripwire whose threshold
/// is 0 and whose value is fractional breaks both, and is refused at its threshold (the A3
/// review's nit, DEC-2301).
#[test]
fn rule_233_reports_the_threshold_before_the_value() {
    let tripwire = String::from_utf8(base(&section(), "tripwire_triggered")).unwrap();
    let both = tripwire
        .replace("\"threshold\":\"2\"", "\"threshold\":\"0\"")
        .replace("\"value\":\"2\"", "\"value\":\"2.5\"");
    assert!(
        tripwire.contains("\"threshold\":\"2\"") && tripwire.contains("\"value\":\"2\""),
        "the base tripwire trigger counts fills with threshold 2 and value 2"
    );
    let e = Draft::parse(both.as_bytes()).unwrap_err();
    assert_eq!(
        (e.reason, e.path.as_str()),
        (InvalidReason::Schema, "payload.threshold"),
        "the threshold is reported first"
    );
    let value_only = tripwire.replace("\"value\":\"2\"", "\"value\":\"2.5\"");
    let e = Draft::parse(value_only.as_bytes()).unwrap_err();
    assert_eq!(
        (e.reason, e.path.as_str()),
        (InvalidReason::Schema, "payload.value"),
        "a sound threshold leaves the value to be reported"
    );
}
