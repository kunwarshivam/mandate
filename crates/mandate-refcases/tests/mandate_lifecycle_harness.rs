//! Family E's `lifecycle` op drives `mandate-runtime` itself, reads every member of every case, and
//! compares every member of every expected draft (DEC-85, DEC-292 item 3, DEC-317).
//!
//! DEC-317's first slice interprets the `ask` step, so every case is exercised through its first
//! step: the case cut to that step passes, and the whole case fails at its second step naming the
//! slice that interprets it. Each test has two halves that need each other: a case passes as the
//! fixture states it, which an arm that failed everything could not do, and fails, naming what
//! changed, when a member is edited, dropped, or added, which an arm that compared nothing could not.

use std::path::Path;
use std::sync::Arc;

use mandate_refcases::{Json, mandate, read_fixture};
use serde_json::json;

const UNKNOWN: &str = "a_member_the_harness_does_not_know";

fn fixture() -> Json {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases");
    Arc::unwrap_or_clone(read_fixture(&dir, "mandate.json").expect("the mandate fixture"))
}

fn run(fixture: Json, id: &str) -> Result<(), String> {
    let wanted = format!("mandate::{id}");
    let case = mandate::cases(&Arc::new(fixture))
        .into_iter()
        .find(|c| c.id == wanted)
        .unwrap_or_else(|| panic!("no case {wanted}"));
    (case.run)()
}

fn case_mut<'a>(fixture: &'a mut Json, id: &str) -> &'a mut Json {
    fixture["cases"]
        .as_array_mut()
        .expect("a case list")
        .iter_mut()
        .find(|c| c["id"] == id)
        .unwrap_or_else(|| panic!("no case {id}"))
}

fn lifecycle_ids(fixture: &Json) -> Vec<String> {
    fixture["cases"]
        .as_array()
        .expect("a case list")
        .iter()
        .filter(|c| c["op"] == "lifecycle")
        .map(|c| c["id"].as_str().expect("an id").to_owned())
        .collect()
}

/// `fixture` with every lifecycle case cut to its first step, the ask.
fn asks(fixture: &Json) -> Json {
    let mut doctored = fixture.clone();
    for id in lifecycle_ids(fixture) {
        let case = case_mut(&mut doctored, &id);
        for list in ["script", "expect"] {
            case[list].as_array_mut().expect("a list").truncate(1);
        }
    }
    doctored
}

/// `fixture` with the member at the JSON pointer `path` of case `id` replaced by `value`.
fn with(fixture: &Json, id: &str, path: &str, value: Json) -> Json {
    let mut doctored = fixture.clone();
    *case_mut(&mut doctored, id)
        .pointer_mut(path)
        .unwrap_or_else(|| panic!("{id} has no {path}")) = value;
    doctored
}

fn fails_naming(fixture: Json, id: &str, named: &str) {
    let failure = run(fixture, id).expect_err("the doctored case must fail");
    assert!(failure.contains(named), "{id}: `{named}` not in: {failure}");
}

/// Every one of the twenty-six cases asks through the runtime as its first step states: its request
/// and its `cli_inbox` delivery. Whole, each fails at its second step naming the slice that
/// interprets that step, so none can pass on an arm that never ran it.
#[test]
fn every_case_asks_as_its_first_step_states() {
    let fixture = fixture();
    let ids = lifecycle_ids(&fixture);
    assert_eq!(ids.len(), 26);
    let asks = asks(&fixture);
    for id in &ids {
        run(asks.clone(), id).unwrap_or_else(|e| panic!("{id}'s ask: {e}"));
        let failure = run(fixture.clone(), id).expect_err("a whole case cannot pass yet");
        assert!(failure.starts_with("step 2: the `"), "{id}: {failure}");
        assert!(failure.contains("DEC-317's slice"), "{id}: {failure}");
    }
}

/// Each value an expected member can take edited to another of its shape.
fn edited(value: &Json) -> Json {
    match value {
        Json::Null => json!("edited"),
        Json::Bool(b) => json!(!b),
        Json::Number(n) => json!(n.as_u64().expect("a count") + 1),
        Json::String(s) if s.ends_with('Z') => json!(s.replace(":00.000000000Z", ":01.000000000Z")),
        Json::String(s) if s.chars().all(|c| c.is_ascii_digit() || c == '.') => {
            json!(format!("{s}1"))
        }
        Json::String(s) => json!(format!("{s}-edited")),
        other => panic!("no edit for {other}"),
    }
}

/// Every member of both expected drafts of every ask is compared: edited, it fails the case naming
/// the member; dropped, it fails the case. The content hash is a name the ask binds, which a later
/// step's response repeats, so a renamed hash alone changes nothing here.
#[test]
fn every_member_of_an_expected_request_and_delivery_is_compared() {
    let fixture = asks(&fixture());
    let mut edits = 0;
    for id in lifecycle_ids(&fixture) {
        for index in 0..2 {
            let path = format!("/expect/0/drafts/{index}");
            let draft = case_mut(&mut fixture.clone(), &id)
                .pointer(&path)
                .expect("a draft")
                .clone();
            for (name, value) in draft.as_object().expect("a draft") {
                let leaves: Vec<(String, Json)> = match value {
                    Json::Object(inner) => inner
                        .iter()
                        .map(|(key, leaf)| (format!("{path}/{name}/{key}"), edited(leaf)))
                        .collect(),
                    leaf => vec![(format!("{path}/{name}"), edited(leaf))],
                };
                if name != "content_hash" {
                    for (leaf, value) in leaves {
                        let named = if name == "type" {
                            "not a draft type"
                        } else {
                            name.as_str()
                        };
                        fails_naming(with(&fixture, &id, &leaf, value), &id, named);
                        edits += 1;
                    }
                }
                let mut dropped = fixture.clone();
                case_mut(&mut dropped, &id)
                    .pointer_mut(&path)
                    .and_then(Json::as_object_mut)
                    .expect("a draft")
                    .remove(name);
                run(dropped, &id).expect_err("a dropped member must fail");
            }
        }
        let mut extra = fixture.clone();
        case_mut(&mut extra, &id)["expect"][0]["drafts"]
            .as_array_mut()
            .expect("drafts")
            .push(json!({"type": "ApprovalDelivered"}));
        fails_naming(extra, &id, "drafts expected");
    }
    assert!(edits > 400, "{edits} edits");
}

/// The bound order is read against the request the runtime wrote: a bound member the runtime does
/// not take as an input fails the case when it differs from the request's.
#[test]
fn the_bound_order_is_read_against_the_request() {
    let fixture = asks(&fixture());
    for (path, value, named) in [
        (
            "/script/0/bound/approvers_required",
            json!(2),
            "bound `approvers_required`",
        ),
        (
            "/script/0/bound/independent_required",
            json!(true),
            "bound `independent_required`",
        ),
        ("/script/0/bound/timeout_s", json!(301), "bound `timeout_s`"),
        ("/script/0/approval", json!("ap9"), "`approval`"),
    ] {
        fails_naming(with(&fixture, "MC-E02", path, value), "MC-E02", named);
    }
}

/// Every key is read (DEC-85): a member the arm does not know, added anywhere an ask has an
/// object, fails the case naming it.
#[test]
fn every_member_of_an_ask_is_read() {
    let fixture = asks(&fixture());
    for (id, path) in [
        ("MC-E02", ""),
        ("MC-E02", "/context"),
        ("MC-E29", "/context/quiet_hours"),
        ("MC-E02", "/script/0"),
        ("MC-E02", "/script/0/bound"),
        ("MC-E02", "/script/0/bound/reference_mark"),
        ("MC-E02", "/expect/0"),
        ("MC-E02", "/expect/0/drafts/0"),
        ("MC-E02", "/expect/0/drafts/1"),
        ("MC-E02", "/expect/0/drafts/0/reference_mark"),
    ] {
        let mut doctored = fixture.clone();
        case_mut(&mut doctored, id)
            .pointer_mut(path)
            .and_then(Json::as_object_mut)
            .unwrap_or_else(|| panic!("{id} has no object at {path:?}"))
            .insert(UNKNOWN.to_owned(), json!(1));
        fails_naming(doctored, id, UNKNOWN);
    }
}

/// A value the arm does not know, or one the runtime cannot be driven to, fails the case naming
/// it: an unknown step, environment, asset class, side, purpose or draft type; a push channel
/// (E8-4) or a second inbox; a fractional second; a quiet-hours bound that is no wall time; and a
/// reference mark at a seq the account stream has already passed.
#[test]
fn what_the_runtime_cannot_be_driven_to_fails() {
    let fixture = fixture();
    let asks = asks(&fixture);
    for (id, path, value, named) in [
        ("MC-E02", "/script/1/kind", json!("answer"), "answer"),
        (
            "MC-E02",
            "/context/environment",
            json!("backtest"),
            "backtest",
        ),
        (
            "MC-E02",
            "/script/0/bound/asset_class",
            json!("option"),
            "option",
        ),
        ("MC-E02", "/script/0/bound/side", json!("sell"), "sell"),
        (
            "MC-E02",
            "/script/0/bound/purpose",
            json!("risk_exit"),
            "risk_exit",
        ),
        (
            "MC-E02",
            "/expect/0/drafts/1/type",
            json!("ApprovalWithdrawn"),
            "ApprovalWithdrawn",
        ),
        (
            "MC-E02",
            "/context/push_channels",
            json!(["web_push"]),
            "push",
        ),
        ("MC-E02", "/context/inbox", json!(2), "cli_inbox"),
        (
            "MC-E02",
            "/start",
            json!("2026-09-22T14:00:00.500000000Z"),
            "whole second",
        ),
        ("MC-E29", "/context/quiet_hours/end", json!("25:00"), "end"),
        (
            "MC-E02",
            "/script/0/bound/reference_mark/seq",
            json!(1),
            "bound `reference_mark`",
        ),
    ] {
        fails_naming(with(&fixture, id, path, value.clone()), id, named);
        if !path.starts_with("/script/1") {
            fails_naming(with(&asks, id, path, value), id, named);
        }
    }
    let mut empty = asks.clone();
    for list in ["script", "expect"] {
        case_mut(&mut empty, "MC-E02")[list] = json!([]);
    }
    fails_naming(empty, "MC-E02", "0 steps");
}

/// An `increase` is asked as an `open` is, and a crypto pair as an equity is.
#[test]
fn an_ask_binds_an_open_or_an_increase_of_either_class() {
    let fixture = asks(&fixture());
    let mut increase = with(
        &fixture,
        "MC-E02",
        "/script/0/bound/purpose",
        json!("increase"),
    );
    case_mut(&mut increase, "MC-E02")["expect"][0]["drafts"][0]["purpose"] = json!("increase");
    run(increase, "MC-E02").unwrap_or_else(|e| panic!("increase: {e}"));
    run(fixture, "MC-E24").unwrap_or_else(|e| panic!("crypto: {e}"));
}
