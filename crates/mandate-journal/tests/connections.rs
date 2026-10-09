//! E7-17 (DEC-800; journal spec v0.20 §9.8): the connection's records on the control and account
//! streams, through the journal's own draft check against the vectors' `connections` section.
//! Every base and valid draft parses to its event type and version, and every invalid draft is
//! refused with its reason at its path: the closed members, their types, and rules 19 and 54 to
//! 65. The owner's fold (rules 66 to 68) and §11 are not `append`'s, so they are not here. Until
//! E7-17's implementation registers these records, the journal refuses them as not catalogued, not
//! registered, or on the wrong stream, which is the answer these tests fail on. Journal spec v0.32
//! (DEC-699) adds `ConnectionRequested`, checked against the `connection_requests` section.

use std::path::Path;

use mandate_canon::{Key, Object, Value, parse, to_canonical};
use mandate_journal::Draft;

fn section() -> Value {
    section_named("connections")
}

fn section_named(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get(name).cloned().unwrap()
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
fn every_connection_base_and_valid_draft_parses() {
    let section = section();
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    let mut failed = Vec::new();
    let mut check = |name: String, bytes: Vec<u8>| {
        let written = parse(&bytes).unwrap();
        let want = (
            text(&written, "event_type").to_owned(),
            written
                .get("schema_version")
                .and_then(Value::as_int)
                .unwrap_or_default(),
        );
        let parsed = Draft::parse(&bytes).map(|d| (d.event_type().to_owned(), d.schema_version()));
        if parsed.as_ref() != Ok(&want) {
            failed.push(format!("{name}: {parsed:?}"));
        }
    };
    for (name, body) in drafts {
        check(format!("base {}", name.as_str()), to_canonical(body));
    }
    let valid = list(&section, "valid_drafts");
    for case in valid {
        check(
            format!("valid {}", text(case, "name")),
            draft(&section, case),
        );
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert_eq!(drafts.len(), 8, "one base draft per record and copy");
    assert!(
        valid.len() >= 16,
        "{} valid drafts; never fewer",
        valid.len()
    );
}

#[test]
fn every_invalid_connection_draft_is_refused_with_its_reason_at_its_path() {
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
        invalid.len() >= 159,
        "{} invalid drafts; never fewer",
        invalid.len()
    );
    let clauses: Vec<&str> = invalid.iter().map(|c| text(c, "clause")).collect();
    for rule in (54..=65).chain([19]) {
        let named = format!("rule {rule}");
        assert!(
            clauses.iter().any(|clause| clause.starts_with(&named)),
            "{named} has an invalid draft"
        );
    }
}

/// `ConnectionRequested` (journal spec v0.32 §9.8, DEC-699) is registered on the control stream at
/// schema version 1, closed: the base request and its live twin parse, and each of the 23 invalid
/// drafts of the `connection_requests` section is refused with its reason at its path, among them
/// a member the record does not have (`note`), the authorization code, and the account fingerprint,
/// which no member can carry (CN-1, CN-10).
#[test]
#[ignore = "pending E7-17"]
fn the_connection_request_is_registered_closed_and_secret_free() {
    let section = section_named("connection_requests");
    let mut failed = Vec::new();
    let requested = to_canonical(
        section
            .get("drafts")
            .and_then(|d| d.get("requested"))
            .unwrap(),
    );
    let mut parses = vec![("base requested".to_owned(), requested)];
    for case in list(&section, "valid_drafts") {
        parses.push((
            format!("valid {}", text(case, "name")),
            draft(&section, case),
        ));
    }
    for (name, bytes) in &parses {
        let parsed = Draft::parse(bytes).map(|d| (d.event_type().to_owned(), d.schema_version()));
        if parsed != Ok(("ConnectionRequested".to_owned(), 1)) {
            failed.push(format!("{name}: {parsed:?}"));
        }
    }
    let invalid = list(&section, "invalid_drafts");
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
    assert_eq!(parses.len(), 2, "the base request and `requested_live`");
    assert!(
        invalid.len() >= 23,
        "{} invalid drafts; never fewer",
        invalid.len()
    );
    let names: Vec<&str> = invalid.iter().map(|c| text(c, "name")).collect();
    for name in [
        "requested.extra",
        "requested_carries_the_code",
        "requested_carries_the_fingerprint",
    ] {
        assert!(names.contains(&name), "`{name}` is an invalid draft");
    }
}
