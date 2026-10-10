//! E12-3 (DEC-894; journal spec v0.38 §9.13, §11): `VerificationRun` version 2's closed list of
//! checks gains `segment_rows_mismatch`, a check reported for a range as a whole, so rule 111 has
//! its failure name no `seq`; version 1's closed list is not edited (§8). Driven by the
//! `VerificationRun` drafts of the vectors' `segment_rows` section, each first shown to be the
//! draft its case names. Until the code change lists the check, the journal refuses it at
//! version 2 as it does at version 1, `non_canonical` at `failure.check`, which is the answer the
//! pending tests fail on.

use std::path::Path;

use mandate_canon::{Key, Object, Value, parse, to_canonical};
use mandate_journal::Draft;

const SEGMENT_ROWS: &str = "segment_rows";
const CHECK: &str = "segment_rows_mismatch";
const FAILURE_CHECK: &str = "payload.ranges[0].failure.check";
const FAILURE_SEQ: &str = "payload.ranges[0].failure.seq";

fn section() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get(SEGMENT_ROWS).cloned().unwrap()
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

/// The case `name` of the section's list `cases`, and its draft: its base draft with its changes
/// applied, as a canonical object.
fn case(cases: &str, name: &str) -> (Value, Value) {
    let section = section();
    let found = list(&section, cases)
        .iter()
        .find(|c| text(c, "name") == name)
        .cloned()
        .unwrap();
    let based = section
        .get("drafts")
        .and_then(|d| d.get(text(&found, "base_draft")));
    let mut body = based.and_then(Value::as_object).cloned().unwrap();
    for change in list(&found, "changes") {
        apply(&mut body, change);
    }
    (found, Value::Object(body))
}

/// The draft is a `VerificationRun` at `version` whose first range fails `segment_rows_mismatch`
/// at `seq` and whose other ranges pass, read from its own members rather than from its case.
fn names_the_check(draft: &Value, version: u64, seq: Option<u64>) {
    let header = (
        text(draft, "event_type"),
        draft.get("schema_version").and_then(Value::as_int),
    );
    assert_eq!(header, ("VerificationRun", Some(version)));
    let ranges = list(draft.get("payload").unwrap(), "ranges");
    let failure = ranges[0].get("failure").unwrap();
    assert_eq!(text(failure, "check"), CHECK);
    assert_eq!(failure.get("seq").and_then(Value::as_int), seq);
    let others = ranges[1..].iter().map(|r| r.get("failure"));
    assert!(others.into_iter().all(|f| f == Some(&Value::Null)));
}

/// The draft's refusal, as `(reason, path)`, or `None` when it parses.
fn refusal(draft: &Value) -> Option<(&'static str, String)> {
    let got = Draft::parse(&to_canonical(draft)).err();
    got.map(|e| (e.reason.code(), e.path.as_str().to_owned()))
}

/// §9.13 rule 111, §11: `segment_rows_mismatch` reported for the range with `seq` null parses as
/// a version 2 `VerificationRun`, whose rule 133 result is `fail`.
#[test]
#[ignore = "pending E12-3"]
fn a_version_2_run_reports_the_rows_mismatch_for_the_range() {
    let (found, draft) = case("valid_drafts", "rows_mismatch_for_the_range");
    assert_eq!(
        found.get("expect").map(|e| text(e, "outcome")),
        Some("Valid")
    );
    names_the_check(&draft, 2, None);
    assert_eq!(text(draft.get("payload").unwrap(), "result"), "fail");
    let got = Draft::parse(&to_canonical(&draft));
    let got = got.map(|d| (d.event_type().to_owned(), d.schema_version()));
    assert_eq!(
        got.map_err(|e| (e.reason.code(), e.path.as_str().to_owned())),
        Ok(("VerificationRun".to_owned(), 2)),
        "{CHECK} for the range, seq null, at version 2"
    );
}

/// §9.13 rule 111: the same failure naming a `seq` inside its range is refused `schema` at
/// `failure.seq`, since the check is reported for the range, not at an event.
#[test]
#[ignore = "pending E12-3"]
fn a_version_2_rows_mismatch_naming_a_seq_is_refused_at_its_seq() {
    let (found, draft) = case("invalid_drafts", "rows_mismatch_at_a_seq");
    let expect = found.get("expect").unwrap();
    let want = (text(expect, "reason"), text(expect, "path"));
    assert_eq!(want, ("schema", FAILURE_SEQ));
    names_the_check(&draft, 2, Some(17));
    assert_eq!(
        refusal(&draft),
        Some(("schema", FAILURE_SEQ.to_owned())),
        "{CHECK} at seq 17, at version 2"
    );
}

/// §9.13, §8: version 1's closed list of checks does not gain `segment_rows_mismatch`, so a
/// version 1 run naming it, `seq` null, is refused `non_canonical` at `failure.check`, as rule
/// 111 would not refuse it.
#[test]
fn a_version_1_run_naming_the_rows_mismatch_is_refused_at_its_check() {
    let (found, draft) = case("invalid_drafts", "rows_mismatch_at_version_1");
    let expect = found.get("expect").unwrap();
    let want = (text(expect, "reason"), text(expect, "path"));
    assert_eq!(want, ("non_canonical", FAILURE_CHECK));
    names_the_check(&draft, 1, None);
    assert_eq!(
        refusal(&draft),
        Some(("non_canonical", FAILURE_CHECK.to_owned())),
        "{CHECK} at version 1"
    );
}
