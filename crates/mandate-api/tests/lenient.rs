//! The API-7 operations' lenient decoder (workspace API spec §5, DEC-682 item 27, DEC-886), pending
//! E10-10: every server case of `schemas/workspace-api/examples/lenient/api7.json`, a body that is
//! not a JSON object, API-4's comparison over the members kept, each member of each operation
//! corrupted against a hard-member list typed from the spec, workspace members (#1155), duplicates,
//! and the bodies DEC-886 reads. The kept value's oracle is the test's own: the body with each
//! listed pointer removed, `null` members read as absent.

use mandate_api::lenient::{Api7, ApprovalAnswer, decode_lenient};
use mandate_api::problem::Violation;
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

/// Each operation's hard body members, typed from §5's text ("only their hard members are strict:
/// the path ids, the kill switch's `scope`, an owner exit's `instrument`, and a Skip's `verdict` and
/// `content_hash`"), never read from the crate.
fn hard(operation: &str) -> &'static [&'static str] {
    match operation {
        "kill_switch" => &["scope"],
        "owner_exit" => &["instrument"],
        "respond_approval" => &["verdict", "content_hash"],
        _ => &[],
    }
}

/// An owner exit's bid confirmation, all or nothing (§5, DEC-682 item 27).
const BID: [&str; 4] = ["bid", "bid_size", "quoted_at", "floor"];

/// Each operation's well-formed body, every member it names present and valid.
fn examples() -> Vec<(&'static str, Value)> {
    let record = json!({"artifact": HASH, "ui_build": BUILD});
    let at = "2026-10-08T14:30:00.000000000Z";
    let step_up = json!({"assertion_id": "01ARZ3NDEKTSV4RRFFQ69G5FAV", "authenticated_at": at,
        "method": "passkey"});
    let bid = json!({"asset_id": ASSET, "bid": "101.5", "bid_size": "10", "quoted_at": at,
        "floor": "99.5"});
    vec![
        ("pause", json!({"record": record})),
        ("hold", json!({"record": record})),
        ("end_delegation", json!({"record": record})),
        (
            "kill_switch",
            json!({"scope": {"kind": "agent", "id": "agt_1"}, "record": record,
            "environment_shown": "paper", "owner_exit": [bid], "step_up": step_up}),
        ),
        (
            "owner_exit",
            json!({"instrument": ASSET, "bid": "101.5", "bid_size": "10",
            "quoted_at": at, "floor": "99.5", "record": record, "step_up": step_up}),
        ),
        (
            "respond_approval",
            json!({"verdict": "skipped", "content_hash": HASH, "record": record,
            "step_up": step_up, "delegation": null}),
        ),
    ]
}

/// Values no member of these bodies takes: the wrong type, garbage text, and an object naming an
/// unknown member (DEC-886 item 3).
fn corruptions() -> [Value; 3] {
    [json!(5), json!("garbage"), json!({"zz": 1})]
}

/// The pointers a corruption of `member` drops: the whole confirmation for a bid member, in the
/// order the sent body names them, else the member alone.
fn dropped_for(operation: &str, body: &Value, member: &str) -> Vec<String> {
    let group = operation == "owner_exit" && BID.contains(&member);
    let names = body.as_object().into_iter().flat_map(|o| o.keys());
    let names = names.filter(|n| *n == member || group && BID.contains(&n.as_str()));
    names.map(|n| format!("/{n}")).collect()
}

#[test]
#[ignore = "pending E10-10"]
fn a_corrupt_non_hard_member_is_dropped_and_listed_and_the_rest_kept() -> Result<(), String> {
    for (operation, example) in examples() {
        let label = format!("{operation} example");
        assert_kept(
            &label,
            example.clone(),
            &[],
            lenient(operation, example.to_string().as_bytes()),
        )?;
        let mut members: Vec<String> = example
            .as_object()
            .into_iter()
            .flat_map(|o| o.keys().cloned())
            .collect();
        members.push("zz_unknown".to_owned());
        for member in members
            .iter()
            .filter(|m| !hard(operation).contains(&m.as_str()))
        {
            for bad in corruptions() {
                let mut body = example.clone();
                body[member.as_str()] = bad.clone();
                let label = format!("{operation} {member} = {bad}");
                let dropped = dropped_for(operation, &body, member);
                let dropped: Vec<&str> = dropped.iter().map(String::as_str).collect();
                let outcome = lenient(operation, body.to_string().as_bytes());
                assert_kept(&label, body, &dropped, outcome)?;
            }
        }
    }
    Ok(())
}

#[test]
#[ignore = "pending E10-10"]
fn a_corrupt_or_missing_hard_member_is_refused() {
    for (operation, example) in examples() {
        for member in hard(operation) {
            let mut missing = example.clone();
            missing.as_object_mut().map(|o| o.remove(*member));
            let label = format!("{operation} without {member}");
            assert_refused(&label, lenient(operation, missing.to_string().as_bytes()));
            for bad in corruptions().into_iter().chain([json!(null), json!({})]) {
                let mut body = example.clone();
                body[*member] = bad.clone();
                let label = format!("{operation} {member} = {bad}");
                assert_refused(&label, lenient(operation, body.to_string().as_bytes()));
            }
        }
    }
    for scope in [
        json!({"kind": "agent"}),
        json!({"kind": "connection", "id": "bad id!"}),
    ] {
        let body = json!({"scope": scope}).to_string();
        assert_refused(&body, lenient("kill_switch", body.as_bytes()));
    }
}

const WORKSPACE_MEMBERS: [&str; 3] = ["workspace", "workspace_id", "ws"];

#[test]
#[ignore = "pending E10-10"]
fn a_workspace_member_is_dropped_and_listed_and_never_applied() -> Result<(), String> {
    for (operation, example) in examples() {
        for name in WORKSPACE_MEMBERS {
            let mut body = example.clone();
            body[name] = json!("ws_other");
            let label = format!("{operation} /{name}");
            let pointer = format!("/{name}");
            let outcome = lenient(operation, body.to_string().as_bytes());
            assert_kept(&label, body, &[pointer.as_str()], outcome)?;
            let mut nested = example.clone();
            nested["record"][name] = json!("ws_other");
            let label = format!("{operation} /record/{name}");
            let outcome = lenient(operation, nested.to_string().as_bytes());
            assert_kept(&label, nested, &["/record"], outcome)?;
        }
    }
    for name in WORKSPACE_MEMBERS {
        scope_member_is_dropped_and_the_scope_kept(name)?;
    }
    Ok(())
}

/// The scope kinds, each with a valid id.
fn scopes() -> [(&'static str, Value); 3] {
    [
        ("agent", json!("agt_1")),
        ("connection", json!("con_1")),
        ("workspace", json!(null)),
    ]
}

/// DEC-886 item 1: `name` inside the hard `scope` of each kind is dropped and listed at
/// `/scope/<name>`, and the kept scope is the scope without it, so the stop never moves.
fn scope_member_is_dropped_and_the_scope_kept(name: &str) -> Result<(), String> {
    for (kind, id) in scopes() {
        let body = json!({"scope": {"kind": kind, "id": id, name: "ws_other"}});
        let label = format!("kill switch {kind} scope naming {name}");
        let (kept, listed) = lenient("kill_switch", body.to_string().as_bytes())
            .map_err(|e| format!("{label}: {e:?}"))?;
        assert_eq!(listed, [format!("/scope/{name}")], "{label}: dropped");
        let scope = without_nulls(json!({"kind": kind, "id": id}));
        assert_eq!(
            without_nulls(kept)["scope"],
            scope,
            "{label}: the stop never moves"
        );
    }
    Ok(())
}

#[test]
#[ignore = "pending E10-10"]
fn any_unknown_member_inside_the_scope_is_dropped_and_the_scope_kept() -> Result<(), String> {
    for name in ["zz", "label", "org"] {
        scope_member_is_dropped_and_the_scope_kept(name)?;
    }
    Ok(())
}

/// `names` as pointers, in the order `text` first names each of them: body order, read from the
/// text sent rather than from `Value`'s member order (DEC-886 item 11).
fn in_text_order(text: &str, names: &[&str]) -> Vec<String> {
    let mut found: Vec<(usize, String)> = names
        .iter()
        .map(|n| {
            (
                text.find(&format!("\"{n}\"")).unwrap_or(usize::MAX),
                format!("/{n}"),
            )
        })
        .collect();
    found.sort();
    found.into_iter().map(|(_, pointer)| pointer).collect()
}

/// `text` decoded and checked against the pointers `dropped` lists, in text order.
fn assert_text_kept(operation: &str, text: &str, dropped: &[&str]) -> Result<(), String> {
    let body: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let order = in_text_order(text, dropped);
    let order: Vec<&str> = order.iter().map(String::as_str).collect();
    assert_kept(text, body, &order, lenient(operation, text.as_bytes()))
}

#[test]
#[ignore = "pending E10-10"]
fn a_duplicate_inside_the_scope_is_refused_and_a_duplicate_bid_drops_the_group()
-> Result<(), String> {
    for scope in [
        r#"{"kind": "agent", "id": "agt_1", "id": "agt_2"}"#,
        r#"{"kind": "workspace", "kind": "agent", "id": "agt_1"}"#,
        r#"{"kind": "workspace", "id": null, "id": null}"#,
    ] {
        let body = format!(r#"{{"scope": {scope}}}"#);
        let code = first_code(lenient("kill_switch", body.as_bytes()));
        assert_eq!(code, "duplicate_member", "{body}");
    }
    let at = "2026-10-08T14:30:00.000000000Z";
    let text = format!(
        r#"{{"floor": "99.5", "instrument": "{ASSET}", "bid": "101.5", "quoted_at": "{at}",
        "bid_size": "10", "bid": "102"}}"#
    );
    assert_text_kept("owner_exit", &text, &BID)?;
    let text = format!(r#"{{"instrument": "{ASSET}", "bid": "1", "bid": "2"}}"#);
    assert_text_kept("owner_exit", &text, &["bid"])
}

/// The first violation's code, or what the decoder answered instead.
fn first_code(outcome: Outcome) -> String {
    match outcome {
        Err(Refused::Invalid { violations }) => match violations.first() {
            Some(Violation::Schema { code, .. }) => code.clone(),
            other => format!("{other:?}"),
        },
        other => format!("{other:?}"),
    }
}

#[test]
#[ignore = "pending E10-10"]
fn a_duplicate_non_hard_member_is_dropped_once_and_a_hard_one_refused() -> Result<(), String> {
    let record = json!({"artifact": HASH, "ui_build": BUILD}).to_string();
    let twice = format!(r#"{{"record": {record}, "x": 1, "record": {record}, "x": 2}}"#);
    for operation in ["pause", "hold", "end_delegation"] {
        assert_kept(
            operation,
            json!({"record": 0, "x": 0}),
            &["/record", "/x"],
            lenient(operation, twice.as_bytes()),
        )?;
    }
    let inner = format!(
        r#"{{"record": {{"artifact": "{HASH}", "artifact": "{HASH}", "ui_build": "{BUILD}"}}}}"#
    );
    assert_kept(
        "a nested duplicate",
        json!({"record": 0}),
        &["/record"],
        lenient("pause", inner.as_bytes()),
    )?;
    let scope = r#"{"kind": "agent", "id": "agt_1"}"#;
    let skip = format!(r#""verdict": "skipped", "content_hash": "{HASH}""#);
    let refused = [
        (
            "kill_switch",
            format!(r#"{{"scope": {scope}, "scope": {scope}}}"#),
        ),
        (
            "owner_exit",
            format!(r#"{{"instrument": "{ASSET}", "instrument": "{ASSET}"}}"#),
        ),
        (
            "respond_approval",
            format!(r#"{{{skip}, "verdict": "skipped"}}"#),
        ),
        (
            "respond_approval",
            format!(r#"{{{skip}, "content_hash": "{HASH}"}}"#),
        ),
    ];
    for (operation, body) in refused {
        let code = first_code(lenient(operation, body.as_bytes()));
        assert_eq!(code, "duplicate_member", "{operation} {body}");
    }
    Ok(())
}

#[test]
#[ignore = "pending E10-10"]
fn the_open_bodies_are_read_as_dec_886_says() -> Result<(), String> {
    let at = "2026-10-08T14:30:00.000000000Z";
    let workspace = json!({"scope": {"kind": "workspace"}});
    assert_kept(
        "item 2",
        workspace.clone(),
        &[],
        lenient("kill_switch", workspace.to_string().as_bytes()),
    )?;
    let mixed = format!(
        r#"{{"instrument": "{ASSET}", "quoted_at": "{at}", "bid": "101.5", "floor": "99.5",
        "bid_size": null}}"#
    );
    assert_text_kept("owner_exit", &mixed, &BID)?;
    let nulls = json!({"instrument": ASSET, "bid": null, "bid_size": null, "quoted_at": null,
        "floor": null});
    assert_kept(
        "item 4, all null",
        nulls.clone(),
        &[],
        lenient("owner_exit", nulls.to_string().as_bytes()),
    )?;
    let empty = json!({"scope": {"kind": "agent", "id": "agt_1"}, "owner_exit": []});
    assert_kept(
        "item 9",
        empty.clone(),
        &[],
        lenient("kill_switch", empty.to_string().as_bytes()),
    )?;
    Ok(())
}
