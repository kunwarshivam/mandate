//! J3 (E7-19, DEC-536; journal spec v0.18 §9.1 rules 5 and 7): `DecisionMade`'s `decided_by` may
//! be `policy_overlay`, for an `ask` the policy overlay narrowed or DEC-534's `deny` of an opening,
//! and never on an `auto`. The vectors' additive `policy_overlay` section builds each draft from
//! `agent_stream`'s chain, so this file reads both. Until J3's implementation adds the label, the
//! journal refuses it as `non_canonical`, which is the answer these tests fail on.

use std::path::Path;

use mandate_canon::{Key, Object, Value, parse, to_canonical};
use mandate_journal::Draft;

/// The journal's own fields, which a writer's draft never carries.
const SEALED: [&str; 3] = ["seq", "prev_hash", "recorded_at"];

fn fixture() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    parse(&std::fs::read(path).unwrap()).unwrap()
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
    let value = change.get("value").cloned().unwrap();
    node.insert(Key::new(last).unwrap(), value);
}

/// A case's draft: `agent_stream`'s chain event at its `base_seq`, less the journal's fields, with
/// the case's changes applied, in canonical bytes.
fn draft(fixture: &Value, case: &Value) -> Vec<u8> {
    let seq = case.get("base_seq").and_then(Value::as_int).unwrap();
    let chain = fixture.get("agent_stream").unwrap();
    let entry = list(chain, "chain")
        .iter()
        .find(|entry| entry.get("seq").and_then(Value::as_int) == Some(seq))
        .unwrap();
    let mut body = entry
        .get("body")
        .and_then(Value::as_object)
        .cloned()
        .unwrap();
    for field in SEALED {
        body.remove(field);
    }
    for change in list(case, "changes") {
        apply(&mut body, change);
    }
    to_canonical(&Value::Object(body))
}

fn section(fixture: &Value) -> &Value {
    let section = fixture.get("policy_overlay").unwrap();
    assert_eq!(text(section, "base"), "agent_stream");
    section
}

/// Every valid draft parses: an `ask` the overlay narrowed, suppressed or not, and a `deny` of an
/// opening the agent or a connected client asked for, each `decided_by: policy_overlay`.
#[test]
fn every_policy_overlay_valid_draft_parses() {
    let fixture = fixture();
    let valid = list(section(&fixture), "valid_drafts");
    let mut failed = Vec::new();
    for case in valid {
        if let Err(e) = Draft::parse(&draft(&fixture, case)) {
            failed.push(format!("{}: {e:?}", text(case, "name")));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(
        valid.len() >= 4,
        "{} valid drafts; never fewer",
        valid.len()
    );
    for autonomy in ["ask", "deny"] {
        let named = valid.iter().any(|case| {
            list(case, "changes").iter().any(|change| {
                text(change, "path") == "payload.autonomy" && text(change, "value") == autonomy
            })
        });
        assert!(named, "a valid draft the overlay decided `{autonomy}`");
    }
}

/// Every invalid draft is refused with its reason at its path: the overlay's label on an `auto`
/// (rule 7), with a level appended (rule 5's closed set), and after a denied dry run (rule 5).
#[test]
fn every_policy_overlay_invalid_draft_is_refused_with_its_reason_at_its_path() {
    let fixture = fixture();
    let invalid = list(section(&fixture), "invalid_drafts");
    let mut failed = Vec::new();
    for case in invalid {
        let expect = case.get("expect").unwrap();
        let want = (text(expect, "reason"), text(expect, "path"));
        let got = Draft::parse(&draft(&fixture, case)).err();
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
        invalid.len() >= 3,
        "{} invalid drafts; never fewer",
        invalid.len()
    );
    let on_auto = invalid
        .iter()
        .any(|case| text(case, "name") == "decision_overlay_on_auto");
    assert!(on_auto, "rule 7's clause has its invalid draft");
}
