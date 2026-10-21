//! Journal spec v0.65 rule 245 (E7-29, DEC-2037) and v0.67 rules 247 and 248 (E7-30, DEC-2040),
//! through the journal's own checks against the vectors' `market_intents` and
//! `owner_confirmations` sections. Every base and valid draft parses; every invalid draft is
//! refused with its reason at its path; and every batch is judged by `check_batch` as it says.

use std::path::Path;

use mandate_canon::{Key, Object, Value, parse, to_canonical};
use mandate_journal::{Draft, StreamType, check_batch, closes};

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

fn parses(name: &str) -> Vec<String> {
    let section = section(name);
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    let mut failed = Vec::new();
    for (base, body) in drafts {
        if let Err(e) = Draft::parse(&to_canonical(body)) {
            failed.push(format!("base {}: {e:?}", base.as_str()));
        }
    }
    for case in list(&section, "valid_drafts") {
        if let Err(e) = Draft::parse(&draft(&section, case)) {
            failed.push(format!("valid {}: {e:?}", text(case, "name")));
        }
    }
    failed
}

fn refusals(name: &str) -> (usize, Vec<String>) {
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
    (invalid.len(), failed)
}

/// Every exit at the market, and the limit orders rule 245 might be misread to refuse, parse as
/// `IntentReceived` at both versions.
#[test]
fn every_market_intent_base_and_valid_draft_parses() {
    let failed = parses("market_intents");
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// Rule 245: `type` is `limit` or `market`, `market` only for an exit, and `limit_price` null
/// exactly for `market`, refused at the member the rule names first.
#[test]
fn every_invalid_market_intent_is_refused_at_the_member_rule_245_names() {
    let (count, failed) = refusals("market_intents");
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(count >= 14, "{count} invalid drafts; never fewer");
}

/// The owner exit at the market, its `OwnerConfirmationReceived`, and the confirmations rule 247
/// might be misread to refuse, parse.
#[test]
fn every_owner_confirmation_base_and_valid_draft_parses() {
    let failed = parses("owner_confirmations");
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// Rule 247 and the closed schema: a price that is not positive, a null or absent member, or an
/// extra one, refused at the member named.
#[test]
fn every_invalid_owner_confirmation_is_refused_with_its_reason_at_its_path() {
    let (count, failed) = refusals("owner_confirmations");
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(count >= 10, "{count} invalid drafts; never fewer");
}

/// Rule 248 over each batch vector: each draft's own rules first, then each confirmation against
/// the record immediately before it, reported by its index in the batch.
#[test]
fn every_owner_confirmation_batch_is_judged_as_its_vector_says() {
    let section = section("owner_confirmations");
    let batches = list(&section, "batches");
    let mut failed = Vec::new();
    for case in batches {
        let drafts: Result<Vec<Draft>, _> = list(case, "drafts")
            .iter()
            .enumerate()
            .map(|(i, record)| Draft::parse(&to_canonical(record)).map_err(|e| (i, e)))
            .collect();
        let got = drafts
            .and_then(|drafts| check_batch(&drafts))
            .err()
            .map(|(i, e)| (i, e.reason.code().to_owned(), e.path));
        let expect = case.get("expect").unwrap();
        let want = (text(expect, "outcome") == "Invalid").then(|| {
            (
                expect
                    .get("index")
                    .and_then(Value::as_int)
                    .and_then(|i| usize::try_from(i).ok())
                    .unwrap_or_default(),
                text(expect, "reason").to_owned(),
                text(expect, "path").to_owned(),
            )
        });
        if got != want {
            failed.push(format!(
                "{}: expected {want:?}, got {got:?}",
                text(case, "name")
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(
        batches.len() >= 13,
        "{} batches; never fewer",
        batches.len()
    );
}

/// The record is closed on the account stream at version 1 and nowhere else.
#[test]
fn the_catalogue_closes_owner_confirmation_received_on_the_account_stream() {
    assert!(closes("OwnerConfirmationReceived", 1, StreamType::Account));
    assert!(!closes("OwnerConfirmationReceived", 2, StreamType::Account));
    for stream in [
        StreamType::Agent,
        StreamType::Control,
        StreamType::Scheduler,
        StreamType::Notice,
        StreamType::Meter,
    ] {
        assert!(
            !closes("OwnerConfirmationReceived", 1, stream),
            "{stream:?}"
        );
    }
}
