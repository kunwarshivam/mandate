//! Journal spec v0.117 (DEC-2858; §9.1 rule 8, §9.6 rule 319): an owner's or a client's request
//! is decided with no §8.3 evaluation, so its `DecisionMade` carries no conviction, score, output
//! or weight while its clips stay listed, and an `ApprovalRequested` asking for it carries no
//! score and cites no output. Rule 319's batch clause refuses a request whose score does not
//! match who asked for its decision. The vectors' `owner_requests` section holds whole drafts, so
//! this file needs no other section.

use std::path::Path;

use mandate_canon::{Key, Object, Value, parse, to_canonical};
use mandate_journal::{Draft, check_batch};

fn fixture() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    parse(&std::fs::read(path).unwrap()).unwrap()
}

fn section(fixture: &Value) -> &Value {
    fixture.get("owner_requests").unwrap()
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
    node.insert(
        Key::new(last).unwrap(),
        change.get("value").cloned().unwrap(),
    );
}

fn object(value: &Value) -> Object {
    value.as_object().cloned().unwrap()
}

/// One of the section's base drafts, with `changes` applied.
fn drafted(section: &Value, base: &str, changes: &[Value]) -> Object {
    let mut draft = object(section.get("drafts").and_then(|d| d.get(base)).unwrap());
    for change in changes {
        apply(&mut draft, change);
    }
    draft
}

fn bytes(draft: &Object) -> Vec<u8> {
    to_canonical(&Value::Object(draft.clone()))
}

fn parsed(draft: &Object) -> Draft {
    Draft::parse(&bytes(draft)).unwrap_or_else(|e| panic!("{e:?}: {draft:?}"))
}

fn batch(section: &Value, case: &Value) -> Vec<Draft> {
    let decision = object(
        section
            .get("decisions")
            .and_then(|d| d.get(text(case, "decision")))
            .unwrap(),
    );
    let request_case = case.get("request").unwrap();
    let mut request = drafted(section, text(request_case, "base_draft"), &[]);
    request.insert(
        Key::new("causation_id").unwrap(),
        decision.get("event_id").cloned().unwrap(),
    );
    for change in list(request_case, "changes") {
        apply(&mut request, change);
    }
    let mut pair = vec![parsed(&decision), parsed(&request)];
    if text(case, "order") == "request_first" {
        pair.reverse();
    }
    pair
}

/// The judgement one case expects, as `(reason, path)`, or `None` for a valid draft.
fn expected(case: &Value) -> Option<(&str, &str)> {
    let expect = case.get("expect").unwrap();
    (text(expect, "outcome") == "Invalid").then(|| (text(expect, "reason"), text(expect, "path")))
}

fn judged(draft: &Object) -> Option<(&'static str, String)> {
    Draft::parse(&bytes(draft))
        .err()
        .map(|e| (e.reason.code(), e.path))
}

fn mismatches(cases: &[Value], draft: impl Fn(&Value) -> Object) -> Vec<String> {
    cases
        .iter()
        .filter_map(|case| {
            let want = expected(case);
            let got = judged(&draft(case));
            let same = match (&want, &got) {
                (None, None) => true,
                (Some((reason, path)), Some((r, p))) => reason == r && path == p,
                _ => false,
            };
            (!same).then(|| format!("{}: expected {want:?}, got {got:?}", text(case, "name")))
        })
        .collect()
}

#[test]
fn every_decision_case_is_judged_as_rule_8_says() {
    let fixture = fixture();
    let section = section(&fixture);
    let cases = list(section, "decision_cases");
    let failed = mismatches(cases, |case| {
        let mut decision = object(
            section
                .get("decisions")
                .and_then(|d| d.get(text(case, "decision")))
                .unwrap(),
        );
        for change in list(case, "changes") {
            apply(&mut decision, change);
        }
        decision
    });
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(
        cases.len() >= 14,
        "{} decision cases; never fewer",
        cases.len()
    );
}

#[test]
fn every_request_draft_is_judged_as_rule_319_says() {
    let fixture = fixture();
    let section = section(&fixture);
    let invalid = list(section, "invalid_drafts");
    let valid = list(section, "valid_drafts");
    let draft = |case: &Value| drafted(section, text(case, "base_draft"), list(case, "changes"));
    let mut failed = mismatches(invalid, draft);
    failed.extend(mismatches(valid, draft));
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(invalid.len() >= 6 && valid.len() >= 3, "never fewer cases");
}

#[test]
fn every_batch_is_judged_as_rule_319_says() {
    let fixture = fixture();
    let section = section(&fixture);
    let cases = list(section, "batches");
    let mut failed = Vec::new();
    for case in cases {
        let expect = case.get("expect").unwrap();
        let got = check_batch(&batch(section, case));
        let ok = match (text(expect, "outcome"), &got) {
            ("Valid", Ok(())) => true,
            ("Invalid", Err((index, invalid))) => {
                Some(*index)
                    == expect
                        .get("index")
                        .and_then(Value::as_int)
                        .and_then(|i| usize::try_from(i).ok())
                    && invalid.reason.code() == text(expect, "reason")
                    && invalid.path == text(expect, "path")
            }
            _ => false,
        };
        if !ok {
            failed.push(format!(
                "{}: expected {expect:?}, got {got:?}",
                text(case, "name")
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(cases.len() >= 8, "{} batches; never fewer", cases.len());
}
