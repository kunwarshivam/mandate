//! DEC-942 item 2: `ApprovalRequested` at schema version 2 is version 1 with one more content
//! member, `execution`, the order builder's plan for the asked order, covered by the content hash.
//! Version 1 keeps its closed shape, so a plan is refused there, and version 2 requires one.

use mandate_canon::{Digest, Key, Object, Value, to_canonical};
use mandate_journal::{Draft, InvalidReason};

fn text(s: &str) -> Value {
    Value::Str(s.to_owned())
}

fn object(members: &[(&str, Value)]) -> Value {
    let mut out = Object::new();
    for (key, value) in members {
        out.insert(Key::new(key).unwrap(), value.clone());
    }
    Value::Object(out)
}

const VERSION: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const DEADLINE: u64 = 1_790_000_100;

fn plan(asset_class: &str) -> Value {
    object(&[
        ("asset_class", text(asset_class)),
        ("tif", text("gtc")),
        ("protection_required", Value::Bool(true)),
        (
            "protection",
            object(&[("stop", text("147.25")), ("take_profit", text("170.5"))]),
        ),
    ])
}

fn content(execution: Option<Value>) -> Value {
    let mut members = vec![
        (
            "action",
            object(&[
                ("instrument", text("AAPL")),
                ("asset_class", text("us_equity")),
                ("side", text("buy")),
                ("qty", text("10")),
                ("limit", text("155")),
                ("order_usd", text("1550")),
                ("purpose", text("open")),
            ]),
        ),
        (
            "trigger",
            object(&[
                ("mandate_version", text(VERSION)),
                ("decided_by", text("rule:large_order")),
            ]),
        ),
        (
            "evidence",
            object(&[
                (
                    "combined_score",
                    object(&[
                        (
                            "label",
                            text("combined model score, not a probability of profit"),
                        ),
                        ("value", text("0.5")),
                    ]),
                ),
                ("outputs", Value::Array(Vec::new())),
            ]),
        ),
        ("risk_impact", Value::Array(Vec::new())),
        ("reference_mark", Value::Null),
        ("deadline", text("2026-09-21T14:15:00.000000000Z")),
        ("default", text("If you do nothing, this action is skipped")),
        ("choices", Value::Array(vec![text("approve"), text("skip")])),
        (
            "approvers",
            object(&[
                ("required", Value::Int(mandate_canon::Int::new(1).unwrap())),
                ("independent", Value::Bool(false)),
            ]),
        ),
    ];
    if let Some(execution) = execution {
        members.push(("execution", execution));
    }
    object(&members)
}

fn int(n: u64) -> Value {
    Value::Int(mandate_canon::Int::new(n).unwrap())
}

fn payload(content: Value) -> Value {
    let hash = format!("sha256:{}", Digest::of(&to_canonical(&content)).to_hex());
    object(&[
        ("instrument", text("AAPL")),
        ("asset_class", text("us_equity")),
        ("side", text("buy")),
        ("qty", text("10")),
        ("limit", text("155")),
        ("purpose", text("open")),
        ("mandate_version", text(VERSION)),
        ("decided_by", text("rule:large_order")),
        ("combined_score", text("0.5")),
        ("reference_mark", Value::Null),
        ("approvers_required", int(1)),
        ("independent_required", Value::Bool(false)),
        ("deadline", int(DEADLINE)),
        ("timeout_s", int(300)),
        ("on_timeout", text("skip")),
        ("content", content),
        ("content_hash", text(&hash)),
    ])
}

/// The draft's envelope around `payload`, listing the payload's two references as its artifacts
/// (journal spec §4.4).
fn draft(schema_version: u64, payload: Value) -> Vec<u8> {
    let hash = payload
        .get("content_hash")
        .and_then(Value::as_str)
        .unwrap()
        .to_owned();
    let refs: std::collections::BTreeSet<String> = [VERSION.to_owned(), hash].into();
    let artifact_refs = Value::Array(refs.iter().map(|r| text(r)).collect());
    to_canonical(&object(&[
        ("envelope_version", int(1)),
        ("environment", text("paper")),
        ("event_id", text("00000100000000000000000001")),
        ("stream_id", text("agent:ws1:ag1")),
        ("event_type", text("ApprovalRequested")),
        ("schema_version", int(schema_version)),
        ("event_time", text("2026-09-21T14:10:00.000000000Z")),
        ("clock_source", text("local")),
        ("causation_id", text("00000100000000000000000000")),
        ("correlation_id", Value::Null),
        (
            "actor",
            object(&[
                ("kind", text("agent")),
                ("id", text("ag1")),
                ("version", text("0.1.0")),
                ("build", text(VERSION)),
            ]),
        ),
        ("config_refs", object(&[("mandate_version", text(VERSION))])),
        ("payload", payload),
        ("artifact_refs", artifact_refs),
        ("pii_refs", Value::Array(Vec::new())),
    ]))
}

#[test]
fn version_one_without_a_plan_is_accepted() {
    Draft::parse(&draft(1, payload(content(None)))).unwrap();
}

#[test]
fn version_two_with_a_plan_is_accepted() {
    Draft::parse(&draft(2, payload(content(Some(plan("us_equity")))))).unwrap();
}

#[test]
fn version_two_with_a_plan_without_protection_is_accepted() {
    let unprotected = object(&[
        ("asset_class", text("us_equity")),
        ("tif", text("day")),
        ("protection_required", Value::Bool(false)),
        ("protection", Value::Null),
    ]);
    Draft::parse(&draft(2, payload(content(Some(unprotected))))).unwrap();
    let no_take_profit = object(&[
        ("asset_class", text("us_equity")),
        ("tif", text("ioc")),
        ("protection_required", Value::Bool(true)),
        (
            "protection",
            object(&[("stop", text("147.25")), ("take_profit", Value::Null)]),
        ),
    ]);
    Draft::parse(&draft(2, payload(content(Some(no_take_profit))))).unwrap();
}

#[test]
fn version_one_refuses_a_plan() {
    let refusal = Draft::parse(&draft(1, payload(content(Some(plan("us_equity")))))).unwrap_err();
    assert_eq!(refusal.reason, InvalidReason::Schema);
    assert_eq!(refusal.path, "payload.content.execution");
}

#[test]
fn version_two_requires_a_plan() {
    let refusal = Draft::parse(&draft(2, payload(content(None)))).unwrap_err();
    assert_eq!(refusal.reason, InvalidReason::Schema);
    assert_eq!(refusal.path, "payload.content.execution");
}

#[test]
fn a_plan_for_another_asset_class_than_the_order_is_refused() {
    let refusal = Draft::parse(&draft(2, payload(content(Some(plan("crypto")))))).unwrap_err();
    assert_eq!(refusal.reason, InvalidReason::Schema);
    assert_eq!(refusal.path, "payload.content.execution.asset_class");
}

#[test]
fn a_plan_member_of_the_wrong_type_is_refused() {
    let cases = [
        ("tif", text("fok"), InvalidReason::NonCanonical),
        ("protection_required", text("true"), InvalidReason::Schema),
        ("protection", text("147.25"), InvalidReason::Schema),
    ];
    for (member, value, reason) in cases {
        let mut execution = plan("us_equity").as_object().cloned().unwrap();
        execution.insert(Key::new(member).unwrap(), value);
        let refusal =
            Draft::parse(&draft(2, payload(content(Some(Value::Object(execution)))))).unwrap_err();
        assert_eq!(refusal.reason, reason, "{member}");
        assert_eq!(refusal.path, format!("payload.content.execution.{member}"));
    }
}

#[test]
fn a_plan_whose_hash_is_stale_is_refused() {
    let mut stale = payload(content(Some(plan("us_equity"))))
        .as_object()
        .cloned()
        .unwrap();
    let mut moved = plan("us_equity").as_object().cloned().unwrap();
    moved.insert(
        Key::new("protection").unwrap(),
        object(&[("stop", text("140")), ("take_profit", text("170.5"))]),
    );
    stale.insert(
        Key::new("content").unwrap(),
        content(Some(Value::Object(moved))),
    );
    let refusal = Draft::parse(&draft(2, Value::Object(stale))).unwrap_err();
    assert_eq!(refusal.reason, InvalidReason::Schema);
    assert_eq!(refusal.path, "payload.content_hash");
}
