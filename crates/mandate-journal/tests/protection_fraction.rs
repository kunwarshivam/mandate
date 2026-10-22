//! E7-4's fractional slice (DEC-2312; journal spec v0.72 §9.5 rule 44): `ProtectionChanged` version
//! 2, which discloses a fractional equity position's unprotected fraction, through the journal's own
//! draft check against the vectors' `protection_fraction` section. Every version-2 base and valid
//! draft parses as `ProtectionChanged` at schema version 2, the version-1 base still parses at 1, and
//! every invalid draft is refused with its reason at its path. Until the journal registers version
//! 2 it refuses every version-2 draft as an unknown schema, which is the answer both tests fail on.

use std::path::Path;

use mandate_canon::{Key, Object, Value, parse, to_canonical};
use mandate_journal::Draft;

fn section() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get("protection_fraction").cloned().unwrap()
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

/// A `{path, value}` change, dotted from the envelope. A change may add `unprotected_fraction` to
/// the version-1 base, which is the member version 1 refuses.
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

/// The version-2 bases and the 7 valid drafts parse as `ProtectionChanged` at schema version 2: a
/// fractional entry's first-fill start, its whole shares' placement and its interval's end
/// (DEC-2315) each disclose a fraction strictly between 0 and 1, an exponent form is normalized on
/// the way in (§4.6), and a whole position discloses none. The version-1 base still parses at
/// version 1 (§8: version 1 is never edited and stays registered).
#[test]
#[ignore = "pending E7-4"]
fn every_version_2_base_and_valid_draft_parses_at_version_2() {
    let section = section();
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    let mut parses: Vec<(String, Vec<u8>, u64)> = drafts
        .iter()
        .map(|(name, body)| {
            let version = if name.as_str() == "placed_v1" { 1 } else { 2 };
            (
                format!("base {}", name.as_str()),
                to_canonical(body),
                version,
            )
        })
        .collect();
    let valid = list(&section, "valid_drafts");
    for case in valid {
        assert_eq!(text(case.get("expect").unwrap(), "outcome"), "Valid");
        parses.push((
            format!("valid {}", text(case, "name")),
            draft(&section, case),
            2,
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
    assert_eq!((drafts.len(), valid.len()), (3, 7));
    let names: Vec<&str> = valid.iter().map(|c| text(c, "name")).collect();
    for name in [
        "fraction_on_first_fill_start",
        "fraction_on_placed",
        "fraction_smallest",
        "fraction_largest",
        "no_fraction_on_a_whole_position",
        "fraction_in_exponent_form",
        "fraction_on_the_end",
    ] {
        assert!(names.contains(&name), "`{name}` is a valid draft");
    }
}

/// The 17 invalid drafts are refused at `payload.unprotected_fraction`, with the reason each names: a
/// fraction on each of the nine actions that disclose none, a fraction of 0, below 0, of 1, of 1 in
/// exponent form or above 1 (rule 44), a JSON number or text that is no decimal (§9.1), and a
/// fraction on version 1, which has no such member (§8).
#[test]
#[ignore = "pending E7-4"]
fn every_invalid_fraction_draft_is_refused_at_its_path() {
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
    assert_eq!(invalid.len(), 17);
    assert!(
        invalid
            .iter()
            .all(|c| text(c.get("expect").unwrap(), "path") == "payload.unprotected_fraction"),
        "every invalid draft is refused at the fraction"
    );
}
