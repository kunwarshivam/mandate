//! E7-19 slice E1b-P (DEC-859; journal spec v0.33 §9.5 rules 41 and 44): the `ProtectionChanged`
//! shapes the executor's fold needs, through the journal's own draft check against the vectors'
//! `protection_shapes` section. Every base and valid draft parses as `ProtectionChanged` at schema
//! version 1, and every invalid draft is refused with its reason at its path. Until the journal
//! enforces v0.33, it refuses the newly valid shapes at `payload.orders`, `payload.awaiting` or
//! `payload.entry`, which is the answer both tests fail on.

use std::path::Path;

use mandate_canon::{Key, Object, Value, parse, to_canonical};
use mandate_journal::Draft;

fn section() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get("protection_shapes").cloned().unwrap()
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

/// A `{path, value}` change, dotted from the envelope.
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
    assert!(
        node.contains_key(last),
        "`{path}` changes a member every record has (§4.2)"
    );
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

/// The two base drafts and the 11 valid drafts, one for each row of DEC-859 item 1's table, parse
/// as `ProtectionChanged` at schema version 1: a start naming no order, an end or a bound awaiting
/// no cancel, and a re-placement naming its entry and owner with no intent.
#[test]
fn every_protection_shape_base_and_valid_draft_parses() {
    let section = section();
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    let mut parses: Vec<(String, Vec<u8>)> = drafts
        .iter()
        .map(|(name, body)| (format!("base {}", name.as_str()), to_canonical(body)))
        .collect();
    let valid = list(&section, "valid_drafts");
    for case in valid {
        assert_eq!(text(case.get("expect").unwrap(), "outcome"), "Valid");
        parses.push((
            format!("valid {}", text(case, "name")),
            draft(&section, case),
        ));
    }
    let mut failed = Vec::new();
    for (name, bytes) in &parses {
        let parsed = Draft::parse(bytes).map(|d| (d.event_type().to_owned(), d.schema_version()));
        if parsed != Ok(("ProtectionChanged".to_owned(), 1)) {
            failed.push(format!("{name}: {parsed:?}"));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert_eq!((drafts.len(), valid.len()), (2, 11));
    let names: Vec<&str> = valid.iter().map(|c| text(c, "name")).collect();
    for name in [
        "bracket_first_fill_start",
        "bracket_filled_end",
        "nothing_to_cover_end",
        "acknowledged_end",
        "uncovered_end",
        "interval_limit_awaiting_nothing",
        "replacement_before_expiry",
        "passive_replacement_before_expiry",
        "handed_on_passive_start",
        "passive_wait_start",
        "lost_protection_start",
    ] {
        assert!(names.contains(&name), "`{name}` is a valid draft");
    }
}

/// The 14 near misses stay refused as `schema` at the member each names: `placed` and `cancelled`
/// still name an order and an end names none (rule 41); `awaiting` stays empty on the starts and
/// `watchdog` (rule 41); `entry` needs an intent unless `replacing` is true, a re-placement names
/// both its entry and its owner, and an end names neither an entry nor `replacing` (rule 44).
/// v0.32's code refuses four of them at another member, because it refuses their v0.33-valid
/// parts first, which is the answer this test fails on until the journal enforces v0.33.
#[test]
fn every_protection_shape_invalid_draft_is_refused_at_its_path() {
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
    assert_eq!(invalid.len(), 14);
    let paths: Vec<&str> = invalid
        .iter()
        .map(|c| text(c.get("expect").unwrap(), "path"))
        .collect();
    for path in [
        "payload.orders",
        "payload.awaiting",
        "payload.entry",
        "payload.agent_id",
        "payload.replacing",
    ] {
        assert!(
            paths.contains(&path),
            "an invalid draft is refused at `{path}`"
        );
    }
}
