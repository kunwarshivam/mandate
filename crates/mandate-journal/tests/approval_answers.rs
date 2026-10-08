//! J2 (E8-3, DEC-533; journal spec v0.17 §9.7): the owner's approval answer and the runtime's two
//! records of it, through the journal's own draft check against the vectors' `approval_answers`
//! section. Every base and valid draft parses, and every invalid draft is refused with its reason
//! at its path: the closed members, their types, rules 46 to 53, and each record's stream and
//! configuration. Until J2's implementation registers them, the journal refuses all three as
//! uncatalogued or unregistered, which is the answer these tests fail on.

use std::path::Path;

use mandate_canon::{Key, Object, Value, parse, to_canonical};
use mandate_journal::Draft;

fn section() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get("approval_answers").cloned().unwrap()
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
#[ignore = "pending E8-3"]
fn every_approval_answer_base_and_valid_draft_parses() {
    let section = section();
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    let mut failed = Vec::new();
    for (name, body) in drafts {
        let parsed = Draft::parse(&to_canonical(body)).map(|d| d.event_type().to_owned());
        if parsed.as_deref() != Ok(text(body, "event_type")) {
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
    assert_eq!(drafts.len(), 3, "one base draft per record");
    assert!(
        valid.len() >= 11,
        "{} valid drafts; never fewer",
        valid.len()
    );
}

#[test]
#[ignore = "pending E8-3"]
fn every_invalid_approval_answer_is_refused_with_its_reason_at_its_path() {
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
        invalid.len() >= 114,
        "{} invalid drafts; never fewer",
        invalid.len()
    );
    let rules: Vec<&str> = invalid.iter().map(|c| text(c, "clause")).collect();
    for rule in 46..=53 {
        let named = format!("rule {rule}");
        assert!(
            rules.iter().any(|clause| clause.starts_with(&named)),
            "{named} has an invalid draft"
        );
    }
}
