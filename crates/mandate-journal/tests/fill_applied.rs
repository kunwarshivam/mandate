//! Journal spec v0.94 §9.5 (DEC-2809; X10c): `FillApplied` at schema version 2, whose
//! `client_order_id` is `null` for a fill that names no order of ours, with rule 299, through the
//! journal's own checks against the vectors' `fill_applied` section. Every base and valid draft
//! parses; every invalid draft is refused with its reason at its path. Before version 2 the journal
//! refused every unattributed fill, so the executor's batch for the first fill it did not originate
//! was refused whole (DEC-2805).

use std::path::Path;

use mandate_canon::{Key, Object, Value, parse, to_canonical};
use mandate_journal::{Draft, StreamType, closes};
use proptest::prelude::*;

fn section() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get("fill_applied").cloned().unwrap()
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
fn the_fill_section_holds_every_part() {
    let section = section();
    assert!(text(&section, "spec").contains("rule 299"));
    for (part, at_least) in [("invalid_drafts", 28), ("valid_drafts", 12)] {
        let n = list(&section, part).len();
        assert!(n >= at_least, "{n} {part}; never fewer than {at_least}");
    }
}

#[test]
fn each_base_and_every_valid_fill_parses() {
    let section = section();
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    let mut failed = Vec::new();
    for (name, body) in drafts {
        match Draft::parse(&to_canonical(body)) {
            Ok(d) if ["FillApplied", "LateFillApplied"].contains(&d.event_type()) => {}
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

/// Rule 299 and every member type, each refused at its reason and path.
#[test]
fn every_invalid_fill_is_refused_with_its_reason_at_its_path() {
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

/// `FillApplied` closes on the account stream at versions 1 and 2, and `LateFillApplied` at 1 only.
#[test]
fn the_catalogue_closes_fill_applied_at_versions_1_and_2_on_the_account_stream() {
    for (event_type, versions) in [("FillApplied", 1..=2), ("LateFillApplied", 1..=1)] {
        for version in 1..=3 {
            assert_eq!(
                closes(event_type, version, StreamType::Account),
                versions.contains(&version),
                "{event_type} {version}"
            );
        }
        for stream in [StreamType::Agent, StreamType::Control] {
            assert!(!closes(event_type, 1, stream), "{event_type} {stream:?}");
        }
    }
}

/// Trading spec §2.3 read directly, without the journal: `md-`, then one or more ASCII letters,
/// digits or hyphens, at most 128 bytes in all.
fn ours(named: &str) -> bool {
    named.len() <= 128
        && named.len() > 3
        && named.as_bytes()[..3] == *b"md-"
        && named.as_bytes()[3..]
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'-')
}

/// The unattributed base draft naming `named`, in canonical bytes.
fn naming(section: &Value, named: &str) -> Vec<u8> {
    let base = section.get("drafts").and_then(|d| d.get("unattributed"));
    let mut body = base.and_then(Value::as_object).cloned().unwrap();
    if let Some(Value::Object(payload)) = body.get_mut("payload") {
        payload.insert(
            Key::new("client_order_id").unwrap(),
            Value::Str(named.to_owned()),
        );
    }
    to_canonical(&Value::Object(body))
}

proptest! {
    /// Rule 299 admits exactly the ids §2.3 states, around every edge of the form: the prefix, each
    /// kind of character, and the length.
    #[test]
    fn rule_299_admits_exactly_our_ids(
        prefix in prop::sample::select(vec!["md-", "MD-", "md", "md_", "m-", "", "xmd-"]),
        body in "[A-Za-z0-9_. é-]{0,130}",
    ) {
        let section = section();
        let named = format!("{prefix}{body}");
        prop_assume!(!named.is_empty());
        let got = Draft::parse(&naming(&section, &named));
        let refused_here = got
            .as_ref()
            .err()
            .is_some_and(|e| e.path == "payload.client_order_id");
        prop_assert_eq!(!refused_here, ours(&named), "{:?}: {:?}", named, got.err());
    }
}
