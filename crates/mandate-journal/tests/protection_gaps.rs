//! E6-4's protection gap (DEC-2368; journal spec v0.83 §9.5 rule 42): `ProtectionChanged`'s
//! `protection_gap`, the executor's record that a placement of protection after a release may leave
//! shares of the agents' uncovered, through the journal's own draft check against the vectors'
//! `protection_gaps` section. Every base and valid draft parses as `ProtectionChanged` at its
//! schema version, and every invalid draft is refused with its reason at its path.

use std::path::Path;

use mandate_canon::{Key, Object, Value, parse, to_canonical};
use mandate_journal::Draft;

fn section() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get("protection_gaps").cloned().unwrap()
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

/// A `{path, value}` change, dotted from the envelope. A change may add `unprotected_fraction` to a
/// base, which is how the fraction's invalid draft is made.
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

/// The two placement bases and the 3 valid gaps parse as `ProtectionChanged`, at version 1 and 2
/// alike: a gap naming its quantity, at each version and at the smallest gap a whole share leaves.
#[test]
fn every_gap_base_and_valid_draft_parses() {
    let section = section();
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    let version = |base: &str| if base == "placed_v1" { 1 } else { 2 };
    let mut parses: Vec<(String, Vec<u8>, u64)> = drafts
        .iter()
        .map(|(name, body)| {
            (
                format!("base {}", name.as_str()),
                to_canonical(body),
                version(name.as_str()),
            )
        })
        .collect();
    let valid = list(&section, "valid_drafts");
    for case in valid {
        assert_eq!(text(case.get("expect").unwrap(), "outcome"), "Valid");
        parses.push((
            format!("valid {}", text(case, "name")),
            draft(&section, case),
            version(text(case, "base_draft")),
        ));
    }
    let mut failed = Vec::new();
    for (name, bytes, version) in &parses {
        let parsed = Draft::parse(bytes).map(|d| (d.event_type().to_owned(), d.schema_version()));
        if parsed != Ok(("ProtectionChanged".to_owned(), *version)) {
            failed.push(format!("{name}: {parsed:?}"));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert_eq!((drafts.len(), valid.len()), (2, 3));
    let names: Vec<&str> = valid.iter().map(|c| text(c, "name")).collect();
    for name in ["gap_at_version_1", "gap_at_version_2", "gap_of_one_share"] {
        assert!(names.contains(&name), "`{name}` is a valid draft");
    }
}

/// The 5 invalid gaps are refused with the reason and path each names: no quantity at either
/// version (rule 42), orders named (rule 41), a stop (rule 43), and a fraction (rule 44).
#[test]
fn every_invalid_gap_draft_is_refused_at_its_path() {
    let section = section();
    let invalid = list(&section, "invalid_drafts");
    let mut failed = Vec::new();
    for case in invalid {
        let expect = case.get("expect").unwrap();
        assert_eq!(text(expect, "outcome"), "Invalid");
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
    let paths: Vec<&str> = invalid
        .iter()
        .map(|c| text(c.get("expect").unwrap(), "path"))
        .collect();
    assert_eq!(
        paths,
        [
            "payload.qty",
            "payload.qty",
            "payload.orders",
            "payload.stop",
            "payload.unprotected_fraction",
        ]
    );
}
