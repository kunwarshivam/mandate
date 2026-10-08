//! E10-15 (DEC-670 to DEC-672; journal spec v0.19 to v0.21): the records the workspace API commits,
//! the `client` actor, and the hold on new openings, through the journal's own draft and batch
//! checks against the vectors' `workspace_api`, `client_actor` and `hold` sections. Every base and
//! valid draft parses, every invalid draft is refused with its reason at its path, and rule 69's
//! batches commit or fail at their draft. Until E10-15's implementation registers them, the journal
//! refuses these records as not catalogued or not registered, which is the answer these tests fail
//! on.

use std::path::Path;

use mandate_canon::{Digest, Key, Object, Value, parse, to_canonical};
use mandate_journal::{Draft, StoredEvent, TrustedStart, check_batch, verify_agent_stream};

fn section(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get(name).cloned().unwrap()
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

/// A body with a case's changes applied, in canonical bytes.
fn changed(base: Option<&Value>, case: &Value) -> Vec<u8> {
    let mut body = base.and_then(Value::as_object).cloned().unwrap();
    for change in list(case, "changes") {
        apply(&mut body, change);
    }
    to_canonical(&Value::Object(body))
}

/// A case's draft: its named base draft with its changes applied.
fn draft(section: &Value, case: &Value) -> Vec<u8> {
    let base = section
        .get("drafts")
        .and_then(|d| d.get(text(case, "base_draft")));
    changed(base, case)
}

/// Every base and valid draft of `name` parses; answers the failures and the number of valid
/// drafts.
fn unrefused(name: &str) -> (Vec<String>, usize) {
    let section = section(name);
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    let mut failed = Vec::new();
    for (base, body) in drafts {
        let parsed = Draft::parse(&to_canonical(body)).map(|d| d.event_type().to_owned());
        if parsed.as_deref() != Ok(text(body, "event_type")) {
            failed.push(format!("base {}: {parsed:?}", base.as_str()));
        }
    }
    let valid = list(&section, "valid_drafts");
    for case in valid {
        let bytes = draft(&section, case);
        let written = parse(&bytes).unwrap();
        let parsed = Draft::parse(&bytes).map(|d| (d.event_type().to_owned(), d.schema_version()));
        let want = (
            text(&written, "event_type").to_owned(),
            written
                .get("schema_version")
                .and_then(Value::as_int)
                .unwrap_or_default(),
        );
        if parsed.as_ref() != Ok(&want) {
            failed.push(format!("valid {}: {parsed:?}", text(case, "name")));
        }
    }
    (failed, valid.len())
}

/// Every invalid draft of `name` is refused with its reason at its path; answers the failures and
/// the clauses the cases cite.
fn refused(name: &str) -> (Vec<String>, Vec<String>) {
    let section = section(name);
    let invalid = list(&section, "invalid_drafts");
    let mut failed = Vec::new();
    for case in invalid {
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
    let clauses = invalid
        .iter()
        .map(|c| text(c, "clause").to_owned())
        .collect();
    (failed, clauses)
}

fn assert_rules_cited(clauses: &[String], rules: std::ops::RangeInclusive<u32>) {
    for rule in rules {
        let named = format!("rule {rule}");
        assert!(
            clauses.iter().any(|clause| clause.starts_with(&named)),
            "{named} has an invalid draft"
        );
    }
}

#[test]
fn every_workspace_api_base_and_valid_draft_parses() {
    let (failed, valid) = unrefused("workspace_api");
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(valid >= 13, "{valid} valid drafts; never fewer");
}

#[test]
fn every_invalid_workspace_api_draft_is_refused_with_its_reason_at_its_path() {
    let (failed, clauses) = refused("workspace_api");
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(
        clauses.len() >= 147,
        "{} invalid drafts; never fewer",
        clauses.len()
    );
    assert_rules_cited(&clauses, 54..=65);
}

#[test]
fn every_client_actor_base_and_valid_draft_parses() {
    let (failed, valid) = unrefused("client_actor");
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(valid >= 8, "{valid} valid drafts; never fewer");
}

#[test]
fn every_invalid_client_actor_draft_is_refused_with_its_reason_at_its_path() {
    let (failed, clauses) = refused("client_actor");
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(
        clauses.len() >= 68,
        "{} invalid drafts; never fewer",
        clauses.len()
    );
    assert_rules_cited(&clauses, 66..=74);
}

/// Rule 69's batch clause: a compromised revocation names, earlier in its batch, the kill switch at
/// its connection's scope. Each valid batch commits, and each invalid one fails at its draft.
#[test]
fn a_compromised_revocation_follows_its_connections_kill_switch_in_its_batch() {
    let section = section("client_actor");
    let batch = |case: &Value| -> Vec<Draft> {
        list(case, "drafts")
            .iter()
            .map(|member| {
                let base = if member.get("kill_switch") == Some(&Value::Bool(true)) {
                    section.get("kill_switch")
                } else {
                    section
                        .get("drafts")
                        .and_then(|d| d.get(text(member, "base_draft")))
                };
                Draft::parse(&changed(base, member)).unwrap()
            })
            .collect()
    };
    let mut failed = Vec::new();
    let valid = list(&section, "valid_batches");
    for case in valid {
        let drafts = batch(case);
        if let Err(e) = check_batch(&drafts) {
            failed.push(format!("valid {}: {e:?}", text(case, "name")));
        }
        let unrelated = section
            .get("drafts")
            .and_then(|d| d.get("client_connected"))
            .map(to_canonical)
            .and_then(|bytes| Draft::parse(&bytes).ok())
            .unwrap();
        let mut apart = drafts.clone();
        apart.insert(1, unrelated);
        if let Err(e) = check_batch(&apart) {
            failed.push(format!(
                "valid {} with an unrelated draft between: {e:?}",
                text(case, "name")
            ));
        }
    }
    assert!(!valid.is_empty(), "at least one valid batch");
    let invalid = list(&section, "invalid_batches");
    for case in invalid {
        let expect = case.get("expect").unwrap();
        let index = expect
            .get("draft_index")
            .and_then(Value::as_int)
            .and_then(|i| usize::try_from(i).ok());
        let want = (index, text(expect, "reason"), text(expect, "path"));
        let got = check_batch(&batch(case)).err();
        let got = got
            .as_ref()
            .map(|(i, e)| (Some(*i), e.reason.code(), e.path.as_str()));
        if got != Some(want) {
            failed.push(format!(
                "{}: expected {want:?}, got {got:?}",
                text(case, "name")
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(
        invalid.len() >= 6,
        "{} invalid batches; never fewer",
        invalid.len()
    );
}

#[test]
fn every_hold_base_and_valid_draft_parses() {
    let (failed, valid) = unrefused("hold");
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(valid >= 11, "{valid} valid drafts; never fewer");
}

#[test]
fn every_invalid_hold_draft_is_refused_with_its_reason_at_its_path() {
    let (failed, clauses) = refused("hold");
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(
        clauses.len() >= 69,
        "{} invalid drafts; never fewer",
        clauses.len()
    );
    assert_rules_cited(&clauses, 75..=80);
}

/// One stored agent-stream row at `seq` of an `event_type` with `payload`.
fn stored(seq: u64, event_type: &str, version: u64, payload: &Value) -> StoredEvent {
    let body = format!(
        r#"{{"event_type":"{event_type}","schema_version":{version},"payload":{}}}"#,
        String::from_utf8(to_canonical(payload)).unwrap()
    );
    StoredEvent {
        stream_id: "agent:ws_01J8Z2:agent_a".to_owned(),
        seq,
        event_id: format!("01J8Z6R0A{seq:017}"),
        event_type: event_type.to_owned(),
        schema_version: version,
        environment: "paper".to_owned(),
        recorded_at: "2026-10-08T16:00:00.000000000Z".to_owned(),
        prev_hash: Digest::of(b""),
        hash: Digest::of(b""),
        body: body.into_bytes(),
    }
}

/// §11's `held_mismatch` (§9.10, DEC-672) over each of the `hold` section's ranges. A case's
/// `anchor`, the stored chain before a later range, is given to the verifier as the version-2 hold
/// or lift that set it, one `seq` before the range, so the check anchors on the chain rather than
/// on the range's first record; a case with no anchor has none before its start and fails closed.
/// The first failing record is reported at its `seq`, and only it: a case lists every violation
/// the reference finds, and the verifier stops at the first.
#[test]
fn the_owners_hold_is_carried_through_every_range() {
    let section = section("hold");
    let cases = list(&section, "range_verification");
    let mut failed = Vec::new();
    for case in cases {
        let from = case.get("from_seq").and_then(Value::as_int).unwrap();
        let mut rows = Vec::new();
        let mut first = from;
        if let Some(anchor) = case.get("anchor").and_then(Value::as_object) {
            let held = anchor.get("held") == Some(&Value::Bool(true));
            let reason = if held {
                "owner_hold"
            } else {
                "owner_lift_hold"
            };
            let payload = parse(
                format!(
                    r#"{{"from":"normal","held":{held},"lifecycle":"normal","reason":"{reason}","to":"{}"}}"#,
                    if held { "exits_only" } else { "normal" }
                )
                .as_bytes(),
            )
            .unwrap();
            first = from - 1;
            rows.push(stored(first, "AgentModeChanged", 2, &payload));
        }
        for (event, seq) in list(case, "events").iter().zip(from..) {
            let version = event.get("schema_version").and_then(Value::as_int).unwrap();
            let payload = event.get("payload").unwrap();
            rows.push(stored(seq, text(event, "event_type"), version, payload));
        }
        let start = TrustedStart {
            from_seq: first,
            prev_hash: Digest::of(b""),
        };
        let expect = list(case, "expect");
        let want = expect
            .first()
            .and_then(Value::as_int)
            .map(|i| (from + i, "held_mismatch"));
        let got = verify_agent_stream(&rows, start)
            .err()
            .map(|f| (f.seq, f.check.code()));
        if got != want {
            failed.push(format!(
                "{}: expected the first of {expect:?} from seq {from}, got {got:?}",
                text(case, "name")
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(
        cases.len() >= 17,
        "{} range cases; never fewer",
        cases.len()
    );
    assert!(
        cases.iter().any(|c| list(c, "expect").len() >= 2),
        "a case with two violations shows only the first is reported"
    );
}
