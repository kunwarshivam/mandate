//! Journal spec v0.102 and v0.104 §9.28 (DEC-2824 item 1, DEC-2830): `OwnerAcknowledged` and
//! `OwnerAcknowledgmentRequested` at schema version 2, version 1's members and `subject` (`limit`,
//! `restriction` or `reconciliation`), on the control stream and as the executor's account-stream
//! copies, rule 239 judged as at version 1. Through [`Draft::parse`] against the vectors'
//! `restriction_lifts` section's acknowledgment drafts, and every acknowledgment record of the
//! `reconciliation_acknowledgments` section's executor vectors. The section's
//! `AccountRestrictionLifted` drafts are E7-2y slice B's and are not read here. Before the
//! registration the journal refuses every version-2 record as `unknown_schema`.

use std::path::Path;

use mandate_canon::{Key, Object, Value, parse, to_canonical};
use mandate_journal::Draft;

/// The section's version-2 acknowledgment base drafts, and the record each is.
const BASES: [(&str, &str); 3] = [
    ("acknowledged_v2", "OwnerAcknowledged"),
    ("acknowledged_v2_copy", "OwnerAcknowledged"),
    ("requested_v2", "OwnerAcknowledgmentRequested"),
];

fn fixture() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    parse(&std::fs::read(path).unwrap()).unwrap()
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

/// The cases of `part` whose base draft is one of [`BASES`].
fn acknowledgment_cases<'a>(section: &'a Value, part: &str) -> Vec<&'a Value> {
    list(section, part)
        .iter()
        .filter(|case| BASES.iter().any(|(b, _)| *b == text(case, "base_draft")))
        .collect()
}

#[test]
fn every_version_2_acknowledgment_base_and_valid_draft_parses_as_its_record() {
    let fixture = fixture();
    let section = fixture.get("restriction_lifts").cloned().unwrap();
    let mut failed = Vec::new();
    for (name, record) in BASES {
        let body = section.get("drafts").and_then(|d| d.get(name)).unwrap();
        match Draft::parse(&to_canonical(body)) {
            Ok(parsed) if parsed.event_type() == record => {}
            other => failed.push(format!("base {name}: {other:?}")),
        }
    }
    let valid = acknowledgment_cases(&section, "valid_drafts");
    assert!(
        valid.len() >= 5,
        "{} valid version-2 acknowledgment cases",
        valid.len()
    );
    for case in valid {
        if let Err(e) = Draft::parse(&draft(&section, case)) {
            failed.push(format!("valid {}: {e:?}", text(case, "name")));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

#[test]
fn every_invalid_version_2_acknowledgment_draft_is_refused_at_its_path() {
    let fixture = fixture();
    let section = fixture.get("restriction_lifts").cloned().unwrap();
    let invalid = acknowledgment_cases(&section, "invalid_drafts");
    assert!(
        invalid.len() >= 14,
        "{} invalid version-2 acknowledgment cases",
        invalid.len()
    );
    let mut failed = Vec::new();
    for case in invalid {
        let expect = case.get("expect").unwrap();
        let got = Draft::parse(&draft(&section, case)).err();
        let got = got.as_ref().map(|e| (e.reason.code(), e.path.as_str()));
        if got != Some((text(expect, "reason"), text(expect, "path"))) {
            failed.push(format!(
                "{}: expected {expect:?}, got {got:?}",
                text(case, "name")
            ));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

#[test]
fn every_acknowledgment_of_a_reconciliation_hold_parses() {
    let fixture = fixture();
    let section = fixture
        .get("reconciliation_acknowledgments")
        .cloned()
        .unwrap();
    let mut seen = 0usize;
    let mut failed = Vec::new();
    for vector in list(&section, "executor") {
        for stream in ["control", "account"] {
            for record in list(vector, stream) {
                let kind = text(record, "event_type");
                if kind != "OwnerAcknowledged" && kind != "OwnerAcknowledgmentRequested" {
                    continue;
                }
                seen = seen.saturating_add(1);
                if let Err(e) = Draft::parse(&to_canonical(record)) {
                    failed.push(format!("{} {stream}: {e:?}", text(vector, "name")));
                }
            }
        }
    }
    assert!(seen >= 30, "{seen} acknowledgment records");
    assert!(failed.is_empty(), "{failed:#?}");
}

/// Version 2 is registered on the control and account streams only, as version 1 is: on an agent
/// stream each record is refused `wrong_stream`.
#[test]
fn version_2_is_refused_off_the_control_and_account_streams() {
    let fixture = fixture();
    let section = fixture.get("restriction_lifts").cloned().unwrap();
    for (name, _) in BASES {
        let mut body = section
            .get("drafts")
            .and_then(|d| d.get(name))
            .and_then(Value::as_object)
            .cloned()
            .unwrap();
        body.insert(
            Key::new("stream_id").unwrap(),
            Value::Str("agent:ws_01J8Z2:agent_a".to_owned()),
        );
        let got = Draft::parse(&to_canonical(&Value::Object(body))).err();
        let got = got.as_ref().map(|e| (e.reason.code(), e.path.as_str()));
        assert_eq!(got, Some(("wrong_stream", "event_type")), "{name}");
    }
}
