//! The API-7 operations' lenient decoder, part 1 (workspace API spec §5, DEC-682 item 27, DEC-886),
//! pending E10-10: every server case of `schemas/workspace-api/examples/lenient/api7.json`, a body
//! that is not a JSON object, and API-4's comparison over the members kept. The kept value's oracle
//! is the test's own: the body with each listed pointer removed, `null` members read as absent.

use mandate_api::lenient::{Api7, ApprovalAnswer, decode_lenient};
use mandate_api::requests::{
    EndDelegationRequest, HoldRequest, KillSwitchRequest, OwnerExitRequest, PauseRequest,
};
use mandate_api::wire::Refused;
use serde_json::{Value, json};

const CASES: &str = include_str!("../../../schemas/workspace-api/examples/lenient/api7.json");

const HASH: &str = "sha256:abababababababababababababababababababababababababababababababab";
const BUILD: &str = "sha256:cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";
const ASSET: &str = "7b4a1c2e-1111-4a2b-9c3d-000000000001";

/// What the decoder kept, as JSON, and what it listed.
type Outcome = Result<(Value, Vec<String>), Refused>;

fn run<T: Api7>(body: &[u8]) -> Outcome {
    let kept = decode_lenient::<T>(body)?;
    let value = serde_json::to_value(&kept.value).expect("a kept value serializes");
    Ok((value, kept.dropped))
}

/// `operation`'s lenient decode of `body`, by the name the server cases use.
fn lenient(operation: &str, body: &[u8]) -> Outcome {
    match operation {
        "pause" => run::<PauseRequest>(body),
        "hold" => run::<HoldRequest>(body),
        "end_delegation" => run::<EndDelegationRequest>(body),
        "kill_switch" => run::<KillSwitchRequest>(body),
        "owner_exit" => run::<OwnerExitRequest>(body),
        "respond_approval" => run::<ApprovalAnswer>(body),
        other => panic!("not an API-7 lenient operation: {other}"),
    }
}

/// `value` with every `null` member of every object removed: an absent member and a `null` one
/// both keep nothing.
fn without_nulls(value: Value) -> Value {
    match value {
        Value::Object(members) => Value::Object(
            members
                .into_iter()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| (k, without_nulls(v)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(without_nulls).collect()),
        other => other,
    }
}

/// `body` with the member at each pointer removed; `""` leaves `{}`.
fn removed(mut body: Value, pointers: &[String]) -> Value {
    for pointer in pointers {
        if pointer.is_empty() {
            return json!({});
        }
        let (parent, name) = pointer.rsplit_once('/').unwrap_or_default();
        let name = name.replace("~1", "/").replace("~0", "~");
        match body.pointer_mut(parent).and_then(Value::as_object_mut) {
            Some(object) => assert!(object.remove(&name).is_some(), "{pointer} is in the body"),
            None => panic!("{pointer}'s parent is an object in the body"),
        }
    }
    body
}

/// The kept value `outcome` must hold for `body` with `dropped` listed.
fn assert_kept(label: &str, body: Value, dropped: &[&str], outcome: Outcome) -> Result<(), String> {
    let (kept, listed) = outcome.map_err(|e| format!("{label}: {e:?}"))?;
    assert_eq!(listed, dropped, "{label}: dropped");
    let dropped: Vec<String> = dropped.iter().map(|p| (*p).to_owned()).collect();
    let expected = without_nulls(removed(body, &dropped));
    assert_eq!(without_nulls(kept), expected, "{label}: kept value");
    Ok(())
}

fn assert_refused(label: &str, outcome: Outcome) {
    match outcome {
        Err(Refused::Invalid { violations }) if !violations.is_empty() => {}
        other => panic!("{label}: want invalid, got {other:?}"),
    }
}

/// Each case's body as the file spells it, its members in the file's order, which `Value` sorts:
/// `dropped` lists pointers in body order (DEC-886 item 11).
fn sent_bodies() -> Vec<&'static str> {
    let key = "\"body\": ";
    let starts = CASES.match_indices(key).map(|(at, _)| at + key.len());
    starts
        .map(|at| {
            let mut values = serde_json::Deserializer::from_str(&CASES[at..]).into_iter::<Value>();
            assert!(matches!(values.next(), Some(Ok(_))), "a body at {at}");
            &CASES[at..at + values.byte_offset()]
        })
        .collect()
}

#[test]
#[ignore = "pending E10-10"]
fn every_server_case_is_kept_and_listed_or_refused_as_it_says() -> Result<(), String> {
    let cases: Value = serde_json::from_str(CASES).map_err(|e| e.to_string())?;
    let cases = cases["cases"].as_array().ok_or("cases is an array")?;
    let sent = sent_bodies();
    assert_eq!(cases.len(), 28, "every server case is read");
    assert_eq!(sent.len(), cases.len(), "every body is found in the file");
    for (index, (case, sent)) in cases.iter().zip(sent).enumerate() {
        let operation = case["operation"].as_str().ok_or("operation is text")?;
        let label = format!("case {index} ({operation})");
        let (bytes, body) = match &case["body"] {
            Value::String(raw) => (raw.as_bytes().to_vec(), json!({})),
            body => (sent.as_bytes().to_vec(), body.clone()),
        };
        let outcome = lenient(operation, &bytes);
        match (case.get("accept"), case.get("refuse")) {
            (Some(Value::Array(list)), None) => {
                let list: Vec<&str> = list.iter().filter_map(Value::as_str).collect();
                assert_kept(&label, body, &list, outcome)?;
            }
            (None, Some(code)) => {
                assert_eq!(code, "invalid", "{label}: the only refusal is invalid");
                assert_refused(&label, outcome);
            }
            _ => panic!("{label}: neither accept nor refuse"),
        }
    }
    Ok(())
}

/// Bodies that are not a JSON object: not UTF-8, not JSON, two values, and each other JSON type.
const NOT_OBJECTS: [&[u8]; 10] = [
    b"",
    b"not json",
    b"\xff\xfe",
    b"{",
    b"{} {}",
    b"[]",
    b"[{\"record\": null}]",
    b"5",
    b"null",
    b"\"{}\"",
];

#[test]
#[ignore = "pending E10-10"]
fn a_pause_or_hold_body_that_is_not_an_object_is_read_as_empty() -> Result<(), String> {
    for operation in ["pause", "hold"] {
        for body in NOT_OBJECTS {
            let label = format!("{operation} {}", String::from_utf8_lossy(body));
            assert_kept(&label, json!({}), &[""], lenient(operation, body))?;
        }
    }
    Ok(())
}

#[test]
#[ignore = "pending E10-10"]
fn the_other_four_refuse_a_body_that_is_not_an_object() {
    for operation in [
        "end_delegation",
        "kill_switch",
        "owner_exit",
        "respond_approval",
    ] {
        for body in NOT_OBJECTS {
            let label = format!("{operation} {}", String::from_utf8_lossy(body));
            assert_refused(&label, lenient(operation, body));
        }
    }
}

/// Each operation's well-formed body, and members whose values are dropped: unknown, wrong-typed,
/// garbage, or `requested_by` (API-6).
fn idempotency_cases() -> Vec<(&'static str, Value, Value)> {
    let skip = json!({"verdict": "skipped", "content_hash": HASH});
    let delegation = json!({"preview_id": "p"});
    vec![
        (
            "pause",
            json!({}),
            json!({"record": "x", "requested_by": "owner"}),
        ),
        ("hold", json!({}), json!({"workspace": "ws_2", "x": [1]})),
        (
            "end_delegation",
            json!({}),
            json!({"record": {"artifact": HASH}}),
        ),
        (
            "kill_switch",
            json!({"scope": {"kind": "agent", "id": "a"}}),
            json!({"step_up": 7}),
        ),
        (
            "owner_exit",
            json!({"instrument": ASSET}),
            json!({"bid": 101.5, "record": 5}),
        ),
        (
            "respond_approval",
            skip,
            json!({"delegation": delegation, "step_up": "x"}),
        ),
    ]
}

#[test]
#[ignore = "pending E10-10"]
fn bodies_differing_only_in_dropped_members_keep_equal_values() -> Result<(), String> {
    for (operation, base, extra) in idempotency_cases() {
        let (want, none) = lenient(operation, base.to_string().as_bytes())
            .map_err(|e| format!("{operation}: {e:?}"))?;
        assert!(none.is_empty(), "{operation}: the base drops nothing");
        let mut body = base.clone();
        let (Some(into), Some(from)) = (body.as_object_mut(), extra.as_object()) else {
            panic!("{operation}: the bodies are objects");
        };
        into.extend(from.clone());
        let (got, listed) = lenient(operation, body.to_string().as_bytes())
            .map_err(|e| format!("{operation}: {e:?}"))?;
        assert_eq!(got, want, "{operation}: API-4 compares the members kept");
        let names: Vec<String> = from.keys().map(|k| format!("/{k}")).collect();
        assert_eq!(
            listed, names,
            "{operation}: every extra member is listed, in body order"
        );
    }
    Ok(())
}

#[test]
#[ignore = "pending E10-10"]
fn a_valid_value_for_a_member_once_dropped_is_a_different_call() -> Result<(), String> {
    let record = json!({"artifact": HASH, "ui_build": BUILD});
    for operation in ["pause", "hold", "end_delegation"] {
        let garbage = json!({"record": "garbage"}).to_string();
        let valid = json!({"record": record}).to_string();
        let (dropped, _) = lenient(operation, garbage.as_bytes()).map_err(|e| format!("{e:?}"))?;
        let (kept, listed) = lenient(operation, valid.as_bytes()).map_err(|e| format!("{e:?}"))?;
        assert!(listed.is_empty(), "{operation}: a valid record is kept");
        assert_ne!(
            dropped, kept,
            "{operation}: the fix differs in a kept member"
        );
    }
    Ok(())
}
