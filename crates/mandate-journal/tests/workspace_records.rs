//! E10-15 (DEC-670 to DEC-672; journal spec v0.19 to v0.21): the records the workspace API commits,
//! the `client` actor, and the hold on new openings, through the journal's own draft and batch
//! checks against the vectors' `workspace_api`, `client_actor` and `hold` sections. Every base and
//! valid draft parses, every invalid draft is refused with its reason at its path, and rule 69's
//! batches commit or fail at their draft. Until E10-15's implementation registers them, the journal
//! refuses these records as not catalogued or not registered, which is the answer these tests fail
//! on.

use std::path::Path;

use mandate_canon::{Key, Object, Value, parse, to_canonical};
use mandate_journal::{Draft, check_batch};

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
        if let Err(e) = Draft::parse(&draft(&section, case)) {
            failed.push(format!("valid {}: {e:?}", text(case, "name")));
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
    for case in list(&section, "valid_batches") {
        if let Err(e) = check_batch(&batch(case)) {
            failed.push(format!("valid {}: {e:?}", text(case, "name")));
        }
    }
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
