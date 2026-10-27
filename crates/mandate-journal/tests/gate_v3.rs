//! Journal spec v0.109 §9.5 (DEC-2848): `GateDecided` at `schema_version` 3, version 2's members
//! with the executor's own `sized_qty`, `parked` and `held_long`, through the journal's own checks
//! against the vectors' `gate_v3` section. Every base and valid draft parses, and every near miss
//! is refused with its reason at its path. Before version 3 the journal refused a sized
//! discretionary exit's, a park's and a long hold's decision, and with it the whole batch.

use std::path::Path;

use mandate_canon::{Key, Object, Value, parse, to_canonical};
use mandate_journal::Draft;

fn section() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get("gate_v3").cloned().unwrap()
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

/// A `{path, value}` change, or a `{path, delete}` one, dotted from the envelope.
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
        return;
    }
    node.insert(
        Key::new(last).unwrap(),
        change.get("value").cloned().unwrap(),
    );
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
fn the_section_holds_the_four_bases_and_every_part() {
    let section = section();
    assert!(text(&section, "spec").contains("DEC-2848"));
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    assert_eq!(drafts.len(), 4);
    for body in drafts.values() {
        assert_eq!(text(body, "event_type"), "GateDecided");
        assert_eq!(body.get("schema_version").and_then(Value::as_int), Some(3));
    }
    for (part, at_least) in [("invalid_drafts", 21), ("valid_drafts", 6)] {
        let n = list(&section, part).len();
        assert!(n >= at_least, "{n} {part}; never fewer than {at_least}");
    }
}

#[test]
fn each_base_and_every_valid_version_3_decision_parses() {
    let section = section();
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    let mut failed = Vec::new();
    for (name, body) in drafts {
        if let Err(e) = Draft::parse(&to_canonical(body)) {
            failed.push(format!("base {}: {e:?}", name.as_str()));
        }
    }
    for case in list(&section, "valid_drafts") {
        if let Err(e) = Draft::parse(&draft(&section, case)) {
            failed.push(format!("valid {}: {e:?}", text(case, "name")));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// Every near miss of rules 313 to 315 and of the closed schema, refused with its reason at its
/// path.
#[test]
fn every_near_miss_is_refused_with_its_reason_at_its_path() {
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
