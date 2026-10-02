//! Family E's `lifecycle` op drives `mandate-runtime` itself, reads every member of every case, and
//! compares every member of every expected draft (DEC-85, DEC-292 item 3, DEC-317).
//!
//! DEC-317's first slice interprets the `ask` step and its second the responses, so every case is
//! exercised through its first step, eleven pass whole, and the rest fail naming the slice, the
//! story or the decision that owes what they stop at. Each test has two halves that need each other: a case passes as the
//! fixture states it, which an arm that failed everything could not do, and fails, naming what
//! changed, when a member is edited, dropped, or added, which an arm that compared nothing could not.

use std::path::Path;
use std::sync::Arc;

use mandate_refcases::{Json, mandate, read_fixture};
use serde_json::json;

const UNKNOWN: &str = "a_member_the_harness_does_not_know";

trait Failed {
    fn unwrap_err_or_else(self, id: &str, named: &str) -> String;
}

impl Failed for Result<(), String> {
    fn unwrap_err_or_else(self, id: &str, named: &str) -> String {
        self.err()
            .unwrap_or_else(|| panic!("{id} passed where it must fail naming `{named}`"))
    }
}

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
    let failure = run(fixture, id).unwrap_err_or_else(id, named);
    assert!(failure.contains(named), "{id}: `{named}` not in: {failure}");
}

/// Every one of the twenty-six cases asks through the runtime as its first step states: its request
/// and its `cli_inbox` delivery.
#[test]
fn every_case_asks_as_its_first_step_states() {
    let fixture = fixture();
    let ids = lifecycle_ids(&fixture);
    assert_eq!(ids.len(), 26);
    let asks = asks(&fixture);
    for id in &ids {
        run(asks.clone(), id).unwrap_or_else(|e| panic!("{id}'s ask: {e}"));
    }
}

/// The cases DEC-317's slice 2 passes whole: an ask and its responses, none of them re-validated.
const ANSWERED: [&str; 11] = [
    "MC-E02", "MC-E04", "MC-E07", "MC-E09", "MC-E10", "MC-E11", "MC-E12", "MC-E13", "MC-E14",
    "MC-E15", "MC-E16",
];

/// The cases an admitted grant re-validates, whose `ApprovalResponded` states the quorum the runtime
/// does not record yet (E8-3), and whose re-validation is DEC-317's slice 3.
const QUORUM: [&str; 10] = [
    "MC-E01", "MC-E06", "MC-E17", "MC-E19", "MC-E20", "MC-E21", "MC-E22", "MC-E23", "MC-E24",
    "MC-E29",
];

/// The twenty-six split as slice 2 stands. Eleven pass whole. The ten that re-validate name slice 3
/// for the re-validation, and nine of them fail on the quorum; MC-E22 fails on its draft count
/// first, since its mark is folded only by slice 3. The four with a tick, a fold, a cancellation or a
/// batch name slice 3 at that step. MC-E18 fails because the runtime cancels in a step whose mode is
/// exits-only before it judges the response (§6.4 "Cancellation", DEC-318).
#[test]
fn the_lifecycle_cases_split_as_slice_2_stands() {
    let fixture = fixture();
    for id in ANSWERED {
        run(fixture.clone(), id).unwrap_or_else(|e| panic!("{id}: {e}"));
    }
    for id in QUORUM {
        fails_naming(fixture.clone(), id, "DEC-317's slice 3");
        let named = if id == "MC-E22" {
            "2 drafts expected"
        } else {
            "the runtime records no quorum"
        };
        fails_naming(fixture.clone(), id, named);
    }
    for (id, step) in [
        ("MC-E03", "tick"),
        ("MC-E05", "fold"),
        ("MC-E08", "cancel"),
        ("MC-E31", "batch"),
    ] {
        fails_naming(
            fixture.clone(),
            id,
            &format!("step 2: the `{step}` step, DEC-317's slice 3"),
        );
    }
    fails_naming(
        fixture.clone(),
        "MC-E18",
        r#"["AgentModeChanged", "ApprovalCanceled", "ApprovalResponded"]"#,
    );
    let mut all: Vec<&str> = ANSWERED.iter().chain(&QUORUM).copied().collect();
    all.extend(["MC-E03", "MC-E05", "MC-E08", "MC-E18", "MC-E31"]);
    all.sort_unstable();
    assert_eq!(lifecycle_ids(&fixture), all);
}

/// Every member of every expected draft of every answered case is compared: edited, it fails the
/// case naming the member; dropped, it fails the case. A content hash is a name, which
/// `a_content_hash_is_a_name_bound_at_the_request` covers.
#[test]
fn every_member_of_an_answered_case_is_compared() {
    let fixture = fixture();
    let mut edits = 0;
    for id in ANSWERED {
        let case = case_mut(&mut fixture.clone(), id).clone();
        for (step, expect) in case["expect"]
            .as_array()
            .expect("expectations")
            .iter()
            .enumerate()
        {
            for (index, draft) in expect["drafts"]
                .as_array()
                .expect("drafts")
                .iter()
                .enumerate()
            {
                let path = format!("/expect/{step}/drafts/{index}");
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
                            let named = if name == "type" { "not" } else { name.as_str() };
                            fails_naming(with(&fixture, id, &leaf, value), id, named);
                            edits += 1;
                        }
                    }
                    let mut dropped = fixture.clone();
                    case_mut(&mut dropped, id)
                        .pointer_mut(&path)
                        .and_then(Json::as_object_mut)
                        .expect("a draft")
                        .remove(name);
                    run(dropped, id).expect_err("a dropped member must fail");
                }
            }
            let mut extra = fixture.clone();
            case_mut(&mut extra, id)["expect"][step]["drafts"]
                .as_array_mut()
                .expect("drafts")
                .push(json!({"type": "ApprovalDelivered"}));
            fails_naming(extra, id, "drafts expected");
        }
    }
    assert!(edits > 300, "{edits} edits");
}

/// A case's content hash is a name for the request's, bound when the runtime writes the request:
/// renamed at the request and at the response alike, the case passes; renamed at the request
/// alone, the response repeats a hash the request does not have and is refused.
#[test]
fn a_content_hash_is_a_name_bound_at_the_request() {
    let fixture = fixture();
    let name = json!("sha256:another-name");
    let renamed = with(
        &fixture,
        "MC-E02",
        "/expect/0/drafts/0/content_hash",
        name.clone(),
    );
    run(
        with(&renamed, "MC-E02", "/script/1/response/content_hash", name),
        "MC-E02",
    )
    .unwrap_or_else(|e| panic!("renamed throughout: {e}"));
    fails_naming(renamed, "MC-E02", "content_mismatch");
    let runtime_name = with(
        &fixture,
        "MC-E07",
        "/script/1/response/approval",
        json!("ap2"),
    );
    fails_naming(runtime_name, "MC-E07", "`approval`");
}

/// A response's copy names the control-stream event it copies, not another one the case handed.
#[test]
fn a_copy_names_its_own_source() {
    let fixture = fixture();
    fails_naming(
        with(
            &fixture,
            "MC-E09",
            "/expect/1/drafts/0/source",
            json!("ctl2"),
        ),
        "MC-E09",
        "`source`",
    );
    fails_naming(
        with(
            &fixture,
            "MC-E15",
            "/expect/2/drafts/0/source",
            json!("ctl1"),
        ),
        "MC-E15",
        "`source`",
    );
}

/// A re-tailed `source` hands the event already folded again, and the runtime copies it once: the
/// re-tail writes nothing. A re-tail that is not the same response fails loudly.
#[test]
fn a_retailed_source_is_copied_once() {
    let mut fixture = fixture();
    let case = case_mut(&mut fixture, "MC-E02");
    let response = case["script"][1].clone();
    case["script"]
        .as_array_mut()
        .expect("a script")
        .push(response);
    case["expect"]
        .as_array_mut()
        .expect("expectations")
        .push(json!({"drafts": []}));
    run(fixture.clone(), "MC-E02").unwrap_or_else(|e| panic!("re-tailed: {e}"));
    let edited = with(
        &fixture,
        "MC-E02",
        "/script/2/response/submitted_at",
        json!("2026-09-22T14:00:31.000000000Z"),
    );
    fails_naming(edited, "MC-E02", "re-tailed");
    let mut copied_twice = fixture.clone();
    case_mut(&mut copied_twice, "MC-E02")["expect"][2] =
        case_mut(&mut fixture.clone(), "MC-E02")["expect"][1].clone();
    fails_naming(copied_twice, "MC-E02", "drafts expected");
}

/// The mode `now` states reaches the runtime: it is folded, so an exits-only `now` cancels the
/// approval before the response is judged.
#[test]
fn a_mode_now_states_reaches_the_runtime() {
    fails_naming(
        with(
            &fixture(),
            "MC-E02",
            "/script/1/now/mode",
            json!("exits_only"),
        ),
        "MC-E02",
        r#"["AgentModeChanged", "ApprovalCanceled", "ApprovalResponded"]"#,
    );
}

/// The rest of `now` is read: any classification and dry run the gate can give, a restriction, the
/// universe, a version and a mark are accepted. They are re-validation's inputs, which no step of
/// this slice consults, so a skip, which re-validates nothing, still ends the approval; slice 3 makes
/// them checkable.
#[test]
fn the_rest_of_now_is_read() {
    let fixture = fixture();
    for (path, value) in [
        (
            "/script/1/now/classification",
            json!({"decision": "auto", "by": null}),
        ),
        (
            "/script/1/now/classification",
            json!({"decision": "deny", "by": "rule:no"}),
        ),
        (
            "/script/1/now/dry_run",
            json!({"verdict": "deny", "reason": "max_order_usd"}),
        ),
        ("/script/1/now/instrument_restricted", json!(true)),
        ("/script/1/now/in_working_universe", json!(false)),
        ("/script/1/now/mandate_version", json!("sha256:v2")),
        ("/script/1/now/mark", json!("156")),
    ] {
        run(with(&fixture, "MC-E02", path, value), "MC-E02")
            .unwrap_or_else(|e| panic!("{path}: {e}"));
    }
}
/// Each value an expected member can take edited to another of its shape.
fn edited(value: &Json) -> Json {
    match value {
        Json::Null => json!("edited"),
        Json::Bool(b) => json!(!b),
        Json::Number(n) => json!(n.as_u64().expect("a count") + 1),
        Json::String(s) if s.ends_with('Z') => {
            let (minute, second) = s.rsplit_once(':').expect("an instant");
            let (whole, fraction) = second.split_once('.').expect("nanoseconds");
            let moved = (whole.parse::<u32>().expect("a second") + 1) % 60;
            json!(format!("{minute}:{moved:02}.{fraction}"))
        }
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

/// Every key is read (DEC-85): a member the arm does not know, added anywhere an ask or a response
/// has an object, fails the case naming it.
#[test]
fn every_member_of_an_ask_and_a_response_is_read() {
    let fixture = fixture();
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
        ("MC-E02", "/script/1"),
        ("MC-E02", "/script/1/response"),
        ("MC-E07", "/script/1/response/step_up"),
        ("MC-E02", "/script/1/now"),
        ("MC-E02", "/script/1/now/classification"),
        ("MC-E02", "/script/1/now/dry_run"),
        ("MC-E07", "/expect/1/drafts/0/step_up"),
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
            "head",
        ),
        (
            "MC-E02",
            "/script/1/response/actor_kind",
            json!("robot"),
            "robot",
        ),
        (
            "MC-E02",
            "/script/1/response/verdict",
            json!("denied"),
            "denied",
        ),
        ("MC-E02", "/script/1/now/mode", json!("frozen"), "frozen"),
        (
            "MC-E02",
            "/script/1/now/classification/decision",
            json!("maybe"),
            "maybe",
        ),
        (
            "MC-E02",
            "/script/1/now/dry_run/verdict",
            json!("warn"),
            "warn",
        ),
        (
            "MC-E02",
            "/script/1/now/dry_run/reason",
            json!("max_order_usd"),
            "max_order_usd",
        ),
        ("MC-E02", "/script/1/now/mark", json!(null), "no mark"),
        (
            "MC-E02",
            "/script/1/now/in_working_universe",
            json!("yes"),
            "boolean",
        ),
        (
            "MC-E02",
            "/script/1/now/classification/by",
            json!(7),
            "string or null",
        ),
        ("MC-E02", "/script/1/kind", json!("tick"), "slice 3"),
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
