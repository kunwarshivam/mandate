//! Journal spec v0.88 §9.5 (DEC-2137; E7-2u): the account stream's `AccountRestrictionChanged` at
//! schema version 1, with rules 279 and 280, through the journal's own checks against the vectors'
//! `account_restrictions` section. Every base and valid draft parses; every invalid draft is
//! refused with its reason at its path. Before the registration the journal refused the record as
//! `unknown_schema`, so the executor's batch for a restricted account was refused whole.

use std::path::Path;

use mandate_canon::{Key, Object, Value, parse, to_canonical};
use mandate_journal::{Draft, StreamType, closes};

fn section() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get("account_restrictions").cloned().unwrap()
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
fn the_restriction_section_holds_every_part() {
    let section = section();
    assert!(text(&section, "spec").contains("279"));
    for (part, at_least) in [("invalid_drafts", 21), ("valid_drafts", 4)] {
        let n = list(&section, part).len();
        assert!(n >= at_least, "{n} {part}; never fewer than {at_least}");
    }
}

#[test]
fn each_base_and_every_valid_restriction_parses() {
    let section = section();
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    let mut failed = Vec::new();
    for (name, body) in drafts {
        match Draft::parse(&to_canonical(body)) {
            Ok(d) if d.event_type() == "AccountRestrictionChanged" => {}
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

/// Rules 279 and 280 and every member type, each refused at its reason and path.
#[test]
fn every_invalid_restriction_is_refused_with_its_reason_at_its_path() {
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

/// Closed on the account stream at version 1, and nowhere else (v0.88).
#[test]
fn the_catalogue_closes_the_restriction_on_the_account_stream_at_version_1() {
    assert!(closes("AccountRestrictionChanged", 1, StreamType::Account));
    assert!(!closes("AccountRestrictionChanged", 2, StreamType::Account));
    for stream in [
        StreamType::Agent,
        StreamType::Control,
        StreamType::Scheduler,
        StreamType::Notice,
        StreamType::Meter,
    ] {
        assert!(
            !closes("AccountRestrictionChanged", 1, stream),
            "{stream:?}"
        );
    }
}
