//! Journal spec v0.93 §9.5 (DEC-2803; X10): the account stream's `ExternalActivityIngested`,
//! `RejectObserved` and `OrderAbandoned` at schema version 1, with rules 297 and 298, through the
//! journal's own checks against the vectors' `account_activity` section. Every base and valid draft
//! parses; every invalid draft is refused with its reason at its path. Before the registration the
//! journal refused each as `unknown_schema`, so the executor's first external order, broker reject
//! or abandoned intent refused its batch whole and stopped the process.

use std::path::Path;

use mandate_canon::{Key, Object, Value, parse, to_canonical};
use mandate_journal::{Draft, StreamType, closes};

const ACTIVITY: [&str; 3] = [
    "ExternalActivityIngested",
    "RejectObserved",
    "OrderAbandoned",
];

fn section() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get("account_activity").cloned().unwrap()
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

/// A `{path, value}` change, dotted from the envelope, or `{path, delete}` removing the member.
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
        node.remove(last);
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
fn the_activity_section_holds_every_part() {
    let section = section();
    assert!(text(&section, "spec").contains("297, 298 and 303"));
    for (part, at_least) in [("invalid_drafts", 45), ("valid_drafts", 15)] {
        let n = list(&section, part).len();
        assert!(n >= at_least, "{n} {part}; never fewer than {at_least}");
    }
}

#[test]
fn each_base_and_every_valid_activity_record_parses() {
    let section = section();
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    let mut failed = Vec::new();
    for (name, body) in drafts {
        match Draft::parse(&to_canonical(body)) {
            Ok(d) if ACTIVITY.contains(&d.event_type()) => {}
            got => failed.push(format!("base {}: {:?}", name.as_str(), got.err())),
        }
    }
    for case in list(&section, "valid_drafts") {
        if let Err(e) = Draft::parse(&draft(&section, case)) {
            failed.push(format!("valid {}: {e:?}", text(case, "name")));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// Rules 297 and 298 and every member type, each refused at its reason and path.
#[test]
fn every_invalid_activity_record_is_refused_with_its_reason_at_its_path() {
    let section = section();
    let mut failed = Vec::new();
    for case in list(&section, "invalid_drafts") {
        let expect = case.get("expect").unwrap();
        let want = (text(expect, "reason"), text(expect, "path"));
        let got = Draft::parse(&draft(&section, case)).err();
        let got = got.as_ref().map(|e| (e.reason.code(), e.path.as_str()));
        if got != Some(want) {
            failed.push(format!(
                "{} ({}): expected {want:?}, got {got:?}",
                text(case, "name"),
                text(case, "clause")
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// Each closed on the account stream at version 1, and nowhere else (v0.90).
#[test]
fn the_catalogue_closes_each_activity_record_on_the_account_stream_at_version_1() {
    for event_type in ACTIVITY {
        assert!(closes(event_type, 1, StreamType::Account), "{event_type}");
        assert!(!closes(event_type, 2, StreamType::Account), "{event_type}");
        for stream in [
            StreamType::Agent,
            StreamType::Control,
            StreamType::Scheduler,
            StreamType::Notice,
            StreamType::Meter,
        ] {
            assert!(!closes(event_type, 1, stream), "{event_type} {stream:?}");
        }
    }
}
