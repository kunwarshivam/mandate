//! Journal spec v0.108 §9.1 (DEC-2845): the `intent` type on every account-stream record the
//! executor's own exits write, through the journal's own checks against the vectors'
//! `exit_intents` section. Every base and valid draft parses; every near miss is refused with its
//! reason at its path. Before the type the journal refused a kill switch's flatten
//! (`k-{switch}-{n}`) and the triggered-stop watchdog's exit (`w-{event}`) as `non_canonical`, so the
//! batch that received, gated or protected one, the `KillSwitchActivated` with it, never committed.

use std::path::Path;

use mandate_canon::{Key, Object, Value, parse, to_canonical};
use mandate_journal::Draft;

const RECORDS: [(&str, u64); 7] = [
    ("IntentReceived", 1),
    ("IntentReceived", 2),
    ("GateDecided", 1),
    ("GateDecided", 2),
    ("OrderRequestRecorded", 1),
    ("ProtectionChanged", 1),
    ("ProtectionChanged", 2),
];

fn section() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get("exit_intents").cloned().unwrap()
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
fn the_section_holds_each_record_version_and_every_part() {
    let section = section();
    assert!(text(&section, "spec").contains("DEC-2845"));
    assert!(text(&section, "spec").contains("DEC-2857"));
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    let mut versions: Vec<(String, u64)> = drafts
        .values()
        .map(|body| {
            let version = body.get("schema_version").and_then(Value::as_int).unwrap();
            (text(body, "event_type").to_owned(), version)
        })
        .collect();
    versions.sort();
    let mut want: Vec<(String, u64)> = RECORDS
        .iter()
        .map(|(event_type, version)| ((*event_type).to_owned(), *version))
        .collect();
    want.sort();
    assert_eq!(versions, want);
    for (part, at_least) in [("invalid_drafts", 175), ("valid_drafts", 28)] {
        let n = list(&section, part).len();
        assert!(n >= at_least, "{n} {part}; never fewer than {at_least}");
    }
}

#[test]
fn each_base_and_every_executor_exit_intent_parses() {
    let section = section();
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    let mut failed = Vec::new();
    for (name, body) in drafts {
        if let Err(e) = Draft::parse(&to_canonical(body)) {
            failed.push(format!("base {}: {e:?}", name.as_str()));
        }
    }
    for case in list(&section, "valid_drafts") {
        if remainder(case) {
            continue;
        }
        if let Err(e) = Draft::parse(&draft(&section, case)) {
            failed.push(format!("valid {}: {e:?}", text(case, "name")));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// Whether a valid draft names a flatten's remainder, `k-{ulid}-{n}r`, the form journal spec
/// v0.114 adds (DEC-2857): [`a_flattens_remainder_parses_on_every_record_version`] reads those.
fn remainder(case: &Value) -> bool {
    text(case, "name").contains("_remainder_")
}

/// v0.114 (DEC-2857): an agent-scoped kill switch's flatten remainder, `k-{ulid}-{n}r`, on each
/// record version, as the first, a middle and the last ordinal. Before it the journal refuses the
/// remainder's `IntentReceived` as `non_canonical`, so the batch that raises it never commits.
#[test]
#[ignore = "pending E7-4"]
fn a_flattens_remainder_parses_on_every_record_version() {
    let section = section();
    let cases: Vec<&Value> = list(&section, "valid_drafts")
        .iter()
        .filter(|case| remainder(case))
        .collect();
    assert_eq!(
        cases.len(),
        3 * RECORDS.len(),
        "three ordinals on each version"
    );
    let failed: Vec<String> = cases
        .into_iter()
        .filter_map(|case| {
            Draft::parse(&draft(&section, case))
                .err()
                .map(|e| format!("valid {}: {e:?}", text(case, "name")))
        })
        .collect();
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// Every near miss of the two forms, on each record version, refused at `intent_id`.
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
