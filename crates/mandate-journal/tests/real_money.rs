//! Journal spec v0.89 §9.35 (DEC-2134): the customer real-money gate's `RealMoneyGateChanged` and
//! `RealMoneyRefused` on the control stream at schema version 1, and rules 273 and 274. Through
//! [`Draft::parse`] against the vectors' `real_money` section: the base and every valid draft parse
//! as their record, every invalid draft is refused with its reason at its path, and every record of
//! every setting stream parses and [`check_stream`] refuses none. Before the registration the
//! journal refuses both records as unknown event types, the answer these tests fail on.

use std::path::Path;

use mandate_canon::{Key, Object, Value, parse, to_canonical};
use mandate_journal::{Draft, check_stream};

const EVENT_TYPES: [&str; 2] = ["RealMoneyGateChanged", "RealMoneyRefused"];

fn section() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get("real_money").cloned().unwrap()
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

/// The vectors' section is §9.35's, with every part this file reads.
#[test]
fn the_real_money_section_holds_every_part() {
    let section = section();
    assert_eq!(
        text(&section, "spec"),
        "docs/specs/journal.md v0.89 §9.35 (DEC-2134)"
    );
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    let names: Vec<&str> = drafts.keys().map(|k| k.as_str()).collect();
    assert_eq!(
        names,
        ["real_money_gate_changed", "real_money_refused"],
        "one base draft of each record"
    );
    for (part, at_least) in [("invalid_drafts", 40), ("valid_drafts", 8), ("streams", 6)] {
        let n = list(&section, part).len();
        assert!(n >= at_least, "{n} {part}; never fewer than {at_least}");
    }
}

#[test]
fn the_base_and_every_valid_real_money_draft_parse_as_their_record() {
    let section = section();
    let mut failed = Vec::new();
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    for (name, body) in drafts {
        match Draft::parse(&to_canonical(body)) {
            Ok(d) if Some(d.event_type()) == body.get("event_type").and_then(Value::as_str) => {}
            got => failed.push(format!("base {}: {:?}", name.as_str(), got.err())),
        }
    }
    for case in list(&section, "valid_drafts") {
        match Draft::parse(&draft(&section, case)) {
            Ok(d) if EVENT_TYPES.contains(&d.event_type()) => {}
            got => failed.push(format!("valid {}: {:?}", text(case, "name"), got.err())),
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

#[test]
fn every_invalid_real_money_draft_is_refused_with_its_reason_at_its_path() {
    let section = section();
    let mut failed = Vec::new();
    for case in list(&section, "invalid_drafts") {
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
}

/// The gate's setting is a fold its reader computes (§9.35), not a stream rule: every record of
/// every setting stream parses, and [`check_stream`] refuses none of them.
#[test]
fn every_setting_stream_record_parses_and_no_stream_rule_refuses_it() {
    let section = section();
    let mut failed = Vec::new();
    for case in list(&section, "streams") {
        let name = text(case, "name");
        let mut parsed = Vec::new();
        for (i, record) in list(case, "records").iter().enumerate() {
            match Draft::parse(&to_canonical(record)) {
                Ok(d) => parsed.push(d),
                Err(e) => failed.push(format!("{name} record {i}: {e:?}")),
            }
        }
        if let Err((i, e)) = check_stream(&[], &parsed) {
            failed.push(format!("{name}: check_stream refused record {i}: {e:?}"));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}
