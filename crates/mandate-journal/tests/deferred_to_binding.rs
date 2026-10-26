//! Journal spec v0.85 (DEC-2388 item 10; §9.1 rules 269 and 270): `DecisionMade` version 3, the
//! cell's runtime's advisory dry run, names every check it deferred to the binding gate and the
//! stored bars' figures it read. The vectors' additive `deferred_to_binding` section builds each
//! draft from `agent_stream`'s chain, so this file reads both.

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

/// A `{path, value}` change or a `{path, delete}` removal, dotted from the envelope.
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
        return;
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
    let section = fixture.get("deferred_to_binding").unwrap();
    assert_eq!(text(section, "base"), "agent_stream");
    section
}

/// Every valid draft parses at version 3: every part deferred and asked, the bars read and asked,
/// a denial on the figures read and one before any read, and a deferred exit deferring nothing.
#[test]
fn every_deferred_to_binding_valid_draft_parses_at_version_3() {
    let fixture = fixture();
    let valid = list(section(&fixture), "valid_drafts");
    let mut failed = Vec::new();
    for case in valid {
        let bytes = draft(&fixture, case);
        match Draft::parse(&bytes) {
            Ok(draft) if draft.schema_version() == 3 => {}
            Ok(draft) => failed.push(format!(
                "{}: parsed at version {}",
                text(case, "name"),
                draft.schema_version()
            )),
            Err(e) => failed.push(format!("{}: {e:?}", text(case, "name"))),
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(
        valid.len() >= 5,
        "{} valid drafts; never fewer",
        valid.len()
    );
}

/// Every invalid draft is refused with its reason at its path: each clause of rules 269 and 270,
/// an unknown or untyped member, a version 1 carrying a deferral, and an unknown version 4.
#[test]
fn every_deferred_to_binding_invalid_draft_is_refused_with_its_reason_at_its_path() {
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
        invalid.len() >= 15,
        "{} invalid drafts; never fewer",
        invalid.len()
    );
    for clause in ["§9.1 rule 269", "§9.1 rule 270"] {
        assert!(
            invalid.iter().any(|case| text(case, "clause") == clause),
            "{clause} has its invalid drafts"
        );
    }
}

/// The section names the fourteen parts in byte order, the order rule 269 requires a writer to
/// sort them into, whatever order the runtime's own type declares them in.
#[test]
fn the_section_names_the_fourteen_parts_in_byte_order() {
    let fixture = fixture();
    let parts: Vec<&str> = list(section(&fixture), "parts")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(parts.len(), 14);
    let mut sorted = parts.clone();
    sorted.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    assert_eq!(parts, sorted);
}

/// The valid draft every other probe below starts from: every part deferred, and asked.
fn every_part_deferred(fixture: &Value) -> Object {
    let case = list(section(fixture), "valid_drafts")
        .iter()
        .find(|case| text(case, "name") == "every_part_deferred_and_asked")
        .unwrap();
    match parse(&draft(fixture, case)).unwrap() {
        Value::Object(body) => body,
        other => panic!("a draft is an object: {other:?}"),
    }
}

fn parsed(body: Object) -> Result<u64, (String, String)> {
    Draft::parse(&to_canonical(&Value::Object(body)))
        .map(|draft| draft.schema_version())
        .map_err(|e| (e.reason.code().to_owned(), e.path.clone()))
}

/// Version 3's references are version 1's: a policy set or a model registry, version 2's stricter
/// references, refuses it as an unknown schema, exactly as it refuses a version 1, so neither is
/// ever accepted unchecked (DEC-2388 item 10).
#[test]
fn version_3_refuses_version_2_s_references() {
    let fixture = fixture();
    assert_eq!(parsed(every_part_deferred(&fixture)), Ok(3));
    for kind in ["policy_set", "model_registry"] {
        let mut body = every_part_deferred(&fixture);
        if let Some(Value::Object(refs)) = body.get_mut("config_refs") {
            refs.insert(
                Key::new(kind).unwrap(),
                Value::Str(format!("sha256:{}", "a".repeat(64))),
            );
        }
        assert_eq!(
            parsed(body),
            Err(("unknown_schema".to_owned(), "payload".to_owned())),
            "{kind}"
        );
    }
}

/// The journal names exactly the section's fourteen parts: each alone is a well-typed name, and a
/// fifteenth after them is refused at its index.
#[test]
fn the_journal_admits_exactly_the_fourteen_parts() {
    let fixture = fixture();
    let parts: Vec<Value> = list(section(&fixture), "parts").to_vec();
    let mut extended = parts.clone();
    extended.push(Value::Str("unseen_fact".to_owned()));
    let mut body = every_part_deferred(&fixture);
    if let Some(Value::Object(payload)) = body.get_mut("payload") {
        payload.insert(
            Key::new("deferred_to_binding").unwrap(),
            Value::Array(extended),
        );
    }
    assert_eq!(
        parsed(body),
        Err((
            "non_canonical".to_owned(),
            "payload.deferred_to_binding[14]".to_owned()
        ))
    );
    let mut body = every_part_deferred(&fixture);
    if let Some(Value::Object(payload)) = body.get_mut("payload") {
        payload.insert(
            Key::new("deferred_to_binding").unwrap(),
            Value::Array(parts),
        );
    }
    assert_eq!(parsed(body), Ok(3), "the fourteen, in the section's order");
}
