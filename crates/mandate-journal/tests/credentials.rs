//! Journal spec v0.69 §9.31 (DEC-2108; E9-4 S3): `CredentialEnrolled` and `CredentialRemoved` on the
//! control stream at schema version 1, with rules 249 and 250. Through [`Draft::parse`] against the
//! vectors' `credentials` section: each base and every valid draft parse as its record, and every
//! invalid draft is refused with its reason at its path. `append` sees one record and checks rules
//! 249 and 250 only, so stream rule 251 is the writer's (the API's, against its own fold) and
//! [`check_stream`] refuses none of the section's streams. Before the registration the journal
//! refuses both records as unknown event types, the answer these tests fail on.

use std::path::Path;

use mandate_canon::{Key, Object, Value, parse, to_canonical};
use mandate_journal::{Draft, check_stream};

const ENROLLED: &str = "CredentialEnrolled";
const REMOVED: &str = "CredentialRemoved";

fn section() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get("credentials").cloned().unwrap()
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

/// The vectors' section is §9.31's, with every part this file reads.
#[test]
fn the_credential_section_holds_every_part() {
    let section = section();
    assert_eq!(
        text(&section, "spec"),
        "docs/specs/journal.md v0.69 §9.31 (DEC-2108)"
    );
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    let names: Vec<&str> = drafts.keys().map(|k| k.as_str()).collect();
    assert_eq!(
        names,
        ["first_enrolled", "removed", "stepped_up_enrolled"],
        "three base drafts, in key order"
    );
    for (part, at_least) in [("invalid_drafts", 46), ("valid_drafts", 7), ("streams", 8)] {
        let n = list(&section, part).len();
        assert!(n >= at_least, "{n} {part}; never fewer than {at_least}");
    }
}

#[test]
#[ignore = "pending E9-4"]
fn each_base_and_every_valid_credential_draft_parse_as_its_record() {
    let section = section();
    let mut failed = Vec::new();
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    for (name, body) in drafts {
        let want = if name.as_str() == "removed" {
            REMOVED
        } else {
            ENROLLED
        };
        match Draft::parse(&to_canonical(body)) {
            Ok(d) if d.event_type() == want => {}
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

#[test]
#[ignore = "pending E9-4"]
fn every_invalid_credential_draft_is_refused_with_its_reason_at_its_path() {
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

/// `append` checks rules 249 and 250 alone: every record of every stream case parses, the
/// enrolments and removals rule 251 refuses included, and [`check_stream`] refuses none of them,
/// since rule 251 is the API's against its own fold and the verifier's fold fails closed on it
/// (§9.31).
#[test]
#[ignore = "pending E9-4"]
fn append_judges_rules_249_and_250_alone_and_leaves_rule_251_to_the_writer() {
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
