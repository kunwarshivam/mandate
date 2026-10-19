//! Journal spec v0.68 (E7-27, DEC-2621): `ConnectionRequested` version 2 and rule 255, through
//! the journal's own checks against the vectors' `connection_requested` section. Every base and
//! valid draft parses, at both versions; every invalid draft is refused with its reason at its
//! path; and rule 255 reports its first failing clause.

use std::path::Path;

use mandate_canon::{Key, Object, Value, parse, to_canonical};
use mandate_journal::{Draft, StreamType, closes};

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

/// The `oauth` and `api_key` requests at version 2, version 1's request, and the values rule 255
/// might be misread to refuse (an API-key request for live or for another broker, a later
/// disclosure version), parse.
#[test]
fn every_connection_request_base_and_valid_draft_parses() {
    let failed = parses("connection_requested");
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// The closed version-2 schema and rule 255, refused at the member named, and version 1 kept
/// closed at its six members.
#[test]
fn every_invalid_connection_request_is_refused_with_its_reason_at_its_path() {
    let (count, failed) = refusals("connection_requested");
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(count >= 15, "{count} invalid drafts; never fewer");
}

/// Rule 255's order: `breadth` before `broker`, `broker` before `environment`, and `environment`
/// before the disclosure's document, each the first clause that fails.
#[test]
fn rule_255_reports_its_first_failing_clause() {
    let section = section("connection_requested");
    let cases: [(&[(&str, Value)], &str); 4] = [
        (
            &[
                ("payload.breadth", Value::Null),
                ("payload.broker", Value::Str("robinhood".to_owned())),
            ],
            "payload.breadth",
        ),
        (
            &[
                ("payload.broker", Value::Str("robinhood".to_owned())),
                ("payload.environment", Value::Str("live".to_owned())),
            ],
            "payload.broker",
        ),
        (
            &[
                ("payload.environment", Value::Str("live".to_owned())),
                (
                    "payload.breadth.disclosure.document",
                    Value::Str("leveraged_etp".to_owned()),
                ),
            ],
            "payload.environment",
        ),
        (
            &[(
                "payload.breadth.disclosure.document",
                Value::Str("leveraged_etp".to_owned()),
            )],
            "payload.breadth.disclosure.document",
        ),
    ];
    for (changes, path) in cases {
        let mut body = section
            .get("drafts")
            .and_then(|d| d.get("oauth_v2"))
            .and_then(Value::as_object)
            .cloned()
            .unwrap();
        for (at, value) in changes {
            let mut change = Object::new();
            change.insert(Key::new("path").unwrap(), Value::Str((*at).to_owned()));
            change.insert(Key::new("value").unwrap(), value.clone());
            apply(&mut body, &Value::Object(change));
        }
        let got = Draft::parse(&to_canonical(&Value::Object(body))).err();
        let got = got.as_ref().map(|e| (e.reason.code(), e.path.as_str()));
        assert_eq!(got, Some(("schema", path)), "{changes:?}");
    }
}

/// The request is closed on the control stream at versions 1 and 2, and nowhere else.
#[test]
fn the_catalogue_closes_connection_requested_on_the_control_stream_at_both_versions() {
    assert!(closes("ConnectionRequested", 1, StreamType::Control));
    assert!(closes("ConnectionRequested", 2, StreamType::Control));
    assert!(!closes("ConnectionRequested", 3, StreamType::Control));
    for stream in [
        StreamType::Account,
        StreamType::Agent,
        StreamType::Scheduler,
        StreamType::Notice,
        StreamType::Meter,
    ] {
        for version in [1, 2] {
            assert!(
                !closes("ConnectionRequested", version, stream),
                "{stream:?} {version}"
            );
        }
    }
}
