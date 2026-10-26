//! Journal spec v0.92 (DEC-2515; §9.6 rules 291 and 292): `ApprovalRequested` version 3 names,
//! inside its hashed content, the checks its dry run deferred to the binding gate. Rule 291 judges
//! the list on the request; rule 292 refuses a batch whose request names other deferrals than the
//! `DecisionMade` it names. The vectors' additive `approval_deferrals` section holds whole drafts,
//! so this file needs no other section. Journal spec v0.103 (DEC-2522) adds §11's
//! `approval_deferral_mismatch`, rule 292 across batches, whose ranges the section also holds.

use std::path::Path;

use mandate_canon::{Digest, Key, Object, Value, parse, to_canonical};
use mandate_journal::{
    AgentStreamCheck, Draft, StoredEvent, TrustedStart, check_batch, verify_agent_stream,
};

fn fixture() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    parse(&std::fs::read(path).unwrap()).unwrap()
}

fn section(fixture: &Value) -> &Value {
    fixture.get("approval_deferrals").unwrap()
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

#[test]
fn each_base_parses_at_its_version() {
    let fixture = fixture();
    let section = section(&fixture);
    for (base, version) in [
        ("requested_v1", 1),
        ("requested_v2", 2),
        ("requested_v3", 3),
    ] {
        assert_eq!(
            parsed(&drafted(section, base, &[])).schema_version(),
            version,
            "{base}"
        );
    }
    for name in ["plain", "deferring"] {
        let decision = object(section.get("decisions").and_then(|d| d.get(name)).unwrap());
        assert_eq!(parsed(&decision).event_type(), "DecisionMade", "{name}");
    }
}

#[test]
fn every_invalid_draft_is_refused_with_its_reason_at_its_path() {
    let fixture = fixture();
    let section = section(&fixture);
    let cases = list(section, "invalid_drafts");
    let mut failed = Vec::new();
    for case in cases {
        let draft = drafted(section, text(case, "base_draft"), list(case, "changes"));
        let expect = case.get("expect").unwrap();
        let want = (text(expect, "reason"), text(expect, "path"));
        let got = Draft::parse(&bytes(&draft)).err();
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
        cases.len() >= 10,
        "{} invalid drafts; never fewer",
        cases.len()
    );
    for clause in [
        "rule 291: ascending by bytes",
        "rule 291: never empty",
        "rule 291: the liquidity checks together",
    ] {
        assert!(
            cases.iter().any(|case| text(case, "clause") == clause),
            "{clause} has its invalid draft"
        );
    }
}

#[test]
fn every_valid_draft_parses_at_version_3() {
    let fixture = fixture();
    let section = section(&fixture);
    let cases = list(section, "valid_drafts");
    for case in cases {
        let draft = drafted(section, text(case, "base_draft"), list(case, "changes"));
        assert_eq!(parsed(&draft).schema_version(), 3, "{}", text(case, "name"));
    }
    assert!(
        cases.len() >= 3,
        "{} valid drafts; never fewer",
        cases.len()
    );
}

/// The case's two drafts in its order: the decision it names, and its request naming that decision
/// unless its changes name another.
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

#[test]
fn every_batch_is_judged_as_rule_292_says() {
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
    assert!(
        cases
            .iter()
            .any(|case| text(case, "name") == "v2_request_hides_a_deferral"),
        "the batch the cell wrote before version 3 is refused"
    );
}

/// Mandate spec v0.12 §6.4's sentence, which the web app reads from here (DEC-2515 item 5).
#[test]
fn the_section_carries_the_approver_s_sentence() {
    let fixture = fixture();
    assert_eq!(
        text(section(&fixture), "approver_sentence"),
        "These checks have not been run yet. They run when the order is placed, and may still refuse it:"
    );
}

const STREAM: &str = "agent:ws_01J8Z2:agent_a";

/// A range's event as stored at `seq`: its id, type, version, cause and payload as the case gives
/// them, on one agent stream.
fn stored(seq: u64, event: &Value) -> StoredEvent {
    let mut body = Object::new();
    for (name, value) in [
        ("event_id", event.get("event_id").cloned().unwrap()),
        ("event_type", event.get("event_type").cloned().unwrap()),
        ("causation_id", event.get("causation_id").cloned().unwrap()),
        ("payload", event.get("payload").cloned().unwrap()),
        ("stream_id", Value::Str(STREAM.to_owned())),
    ] {
        body.insert(Key::new(name).unwrap(), value);
    }
    let body = to_canonical(&Value::Object(body));
    StoredEvent {
        stream_id: STREAM.to_owned(),
        seq,
        event_id: text(event, "event_id").to_owned(),
        event_type: text(event, "event_type").to_owned(),
        schema_version: event.get("schema_version").and_then(Value::as_int).unwrap(),
        environment: "paper".to_owned(),
        recorded_at: "2026-10-26T16:00:00.000000000Z".to_owned(),
        prev_hash: Digest::of(b""),
        hash: Digest::of(&body),
        body,
    }
}

/// §11's `approval_deferral_mismatch` (v0.103, DEC-2522) over every range of the section: the first
/// request the case reports fails at its `seq`, with that code, and a range it reports nothing in
/// passes. Each clause is in some case: a hidden deferral, an invented one, no cause, a cause that
/// is not a decision, a decision after its request, and a cause outside a later range.
#[test]
fn every_range_reports_its_first_request_that_breaks_rule_292_across_batches() {
    let fixture = fixture();
    let ranges = list(section(&fixture), "ranges");
    assert!(ranges.len() >= 13, "{} ranges; never fewer", ranges.len());
    let mut failed = Vec::new();
    for case in ranges {
        let from = case.get("from_seq").and_then(Value::as_int).unwrap();
        let rows: Vec<StoredEvent> = list(case, "events")
            .iter()
            .zip(from..)
            .map(|(event, seq)| stored(seq, event))
            .collect();
        let start = TrustedStart {
            from_seq: from,
            prev_hash: Digest::of(b""),
        };
        let want = list(case, "expect")
            .first()
            .and_then(Value::as_int)
            .map(|i| (from + i, AgentStreamCheck::ApprovalDeferralMismatch));
        let got = verify_agent_stream(&rows, start)
            .err()
            .map(|f| (f.seq, f.check));
        if got != want {
            failed.push(format!(
                "{}: expected {want:?}, got {got:?}",
                text(case, "name")
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert_eq!(
        AgentStreamCheck::ApprovalDeferralMismatch.code(),
        "approval_deferral_mismatch"
    );
}
