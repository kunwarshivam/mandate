//! The A1b shapes against their JSON Schemas (workspace API spec §3.2, §3.3, §4.2, §4.6, §5;
//! DEC-682, DEC-689). Every oracle is a file under `schemas/workspace-api/`: names come from each schema's
//! `properties`, `enum`, and `const`, and cases from its examples, never from the crate.

use std::collections::BTreeSet;
use std::fmt::Debug;

use mandate_api::envelope::{
    Actor, ApiVersion, Record, StepUpEvidence, StepUpMethod, StepUpStatus, Watermark,
};
use mandate_api::requests::{
    AcknowledgeRequest, EndDelegationRequest, HoldRequest, LiftHoldRequest, OwnerRequest,
    PauseRequest, ResumeRequest, Side,
};
use mandate_api::responses::{
    AlreadyEnded, Applies, ApprovalResponseAccepted, Classification, CommandAccepted,
    ConfirmAccepted, EndDelegationAlreadyEnded, Recorded,
};
use mandate_api::wire::{Refused, Validate, decode, encode};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

macro_rules! file {
    ($path:expr) => {
        serde_json::from_str::<Value>(include_str!(concat!("../../../schemas/", $path)))
            .expect("JSON")
    };
}

/// A request or response schema under `commands/`, by its file's stem.
macro_rules! schema {
    ($name:literal) => {
        file!(concat!("workspace-api/commands/", $name, ".schema.json"))
    };
}

/// A shape's example, its `.valid` cases, and its `.invalid` cases.
macro_rules! cases {
    ($name:literal) => {
        [
            file!(concat!("workspace-api/examples/commands.", $name, ".json")),
            file!(concat!(
                "workspace-api/examples/commands.",
                $name,
                ".valid.json"
            )),
            file!(concat!(
                "workspace-api/examples/commands.",
                $name,
                ".invalid.json"
            )),
        ]
    };
}

/// DEC-683 item 5's in-flight stories: `contract.rs`'s list, and changed with it.
const IN_FLIGHT: &[&str] = &["E10-10"];

/// The names serde lists after "expected" when it refuses `probe`: an enum's variants for an
/// unknown value, or a closed struct's members for an unknown member.
fn serde_names<T: DeserializeOwned + Debug>(probe: Value) -> BTreeSet<String> {
    let refused = serde_json::from_value::<T>(probe).expect_err("an unknown name");
    let text = refused.to_string();
    let listed = text.split("expected").nth(1).unwrap_or_default().split('`');
    listed.skip(1).step_by(2).map(str::to_owned).collect()
}

/// DEC-683 item 3 for one closed set: Rust lists nothing the schema does not, and every value the
/// schema serves unplanned; a planned value Rust lists passes only while its story is in flight.
fn closed_set(what: &str, node: &Value, rust: &BTreeSet<String>) {
    let only = || vec![node["const"].clone()];
    let values = node["enum"].as_array().cloned().unwrap_or_else(only);
    let listed: BTreeSet<_> = values.iter().filter_map(Value::as_str).collect();
    let planned = |v: &str| node["x-planned"].get(v).and_then(Value::as_str);
    for value in rust {
        assert!(listed.contains(value.as_str()), "{what}: {value}");
        let excused = planned(value).is_none_or(|s| IN_FLIGHT.contains(&s));
        assert!(excused, "{what}: {value} is planned, not in flight");
    }
    for value in listed.iter().filter(|v| planned(v).is_none()) {
        assert!(rust.contains(*value), "{what}: no variant for {value}");
    }
}

fn enum_drift<T: DeserializeOwned + Debug>(node: &Value) {
    let rust = serde_names::<T>(json!("no_such_value"));
    closed_set(std::any::type_name::<T>(), node, &rust);
}

#[test]
fn the_closed_enums_are_exactly_the_schemas_values() {
    let defs = &file!("workspace-api/envelope.schema.json")["$defs"];
    enum_drift::<ApiVersion>(&defs["Envelope"]["properties"]["api_version"]);
    enum_drift::<StepUpMethod>(&defs["StepUpEvidence"]["properties"]["method"]);
    enum_drift::<StepUpStatus>(&defs["StepUpStatus"]);
    let accepted = file!("workspace-api/commands/command-accepted.schema.json");
    enum_drift::<Recorded>(&accepted["properties"]["phase"]);
    let approval = schema!("approval-response-accepted");
    enum_drift::<Recorded>(&approval["properties"]["phase"]);
    let confirm = &schema!("confirm-accepted")["properties"];
    enum_drift::<Recorded>(&confirm["phase"]);
    enum_drift::<Classification>(&confirm["classification"]);
    enum_drift::<Applies>(&confirm["applies"]);
    let ended = schema!("end-delegation-already-ended");
    enum_drift::<AlreadyEnded>(&ended["properties"]["outcome"]);
    enum_drift::<Side>(&schema!("owner-request")["properties"]["side"]);
    let branches = defs["Actor"]["oneOf"].as_array().expect("branches");
    let kind = |b: &Value| b["properties"]["kind"]["const"].clone();
    let kinds: Vec<_> = branches.iter().map(kind).collect();
    let rust = serde_names::<Actor>(json!({"kind": "no_such_value"}));
    closed_set("Actor", &json!({ "enum": kinds }), &rust);
}

/// The members the schema objects name, against `T`'s, less a tag `T` is tagged on.
fn members<T: DeserializeOwned + Debug>(objects: &[&Value], tag: Option<(&str, &str)>) {
    let named = |o: &&Value| o["properties"].as_object().cloned();
    let names = objects.iter().filter_map(named).flatten();
    let mut schema: BTreeSet<String> = names.map(|(name, _)| name).collect();
    let mut probe = json!({"no_such_member": 1});
    if let Some((tag, kind)) = tag {
        probe[tag] = json!(kind);
        assert!(schema.remove(tag), "{tag}");
    }
    assert_eq!(serde_names::<T>(probe), schema, "{objects:?}");
}

#[test]
fn every_struct_names_exactly_its_schemas_members() {
    let defs = &file!("workspace-api/envelope.schema.json")["$defs"];
    let watermark = &file!("workspace-api/common.schema.json")["$defs"]["Watermark"];
    members::<Watermark>(&[watermark], None);
    members::<Record>(&[&defs["Record"]], None);
    members::<StepUpEvidence>(&[&defs["StepUpEvidence"]], None);
    members::<Actor>(&[&defs["Actor"]["oneOf"][0]], Some(("kind", "user")));
    members::<Actor>(&[&defs["Actor"]["oneOf"][1]], Some(("kind", "client")));
    let accepted = file!("workspace-api/commands/command-accepted.schema.json");
    members::<CommandAccepted>(&[&defs["Envelope"], &accepted], None);
    let approval = schema!("approval-response-accepted");
    members::<ApprovalResponseAccepted>(&[&defs["Envelope"], &approval], None);
    members::<ConfirmAccepted>(&[&defs["Envelope"], &schema!("confirm-accepted")], None);
    let ended = schema!("end-delegation-already-ended");
    members::<EndDelegationAlreadyEnded>(&[&defs["Envelope"], &ended], None);
    members::<PauseRequest>(&[&schema!("pause-request")], None);
    members::<HoldRequest>(&[&schema!("hold-request")], None);
    members::<EndDelegationRequest>(&[&schema!("end-delegation-request")], None);
    members::<LiftHoldRequest>(&[&schema!("lift-hold-request")], None);
    members::<ResumeRequest>(&[&schema!("resume-request")], None);
    members::<AcknowledgeRequest>(&[&schema!("acknowledge-request")], None);
    members::<OwnerRequest>(&[&schema!("owner-request")], None);
}

/// Each member is `null`-able exactly when its schema says `oneOf` with `{type: null}`, and each
/// `null`-able member may be left out exactly when no schema object requires it. serde names the
/// first missing member in declaration order, so a shape declares its `null`-able members first.
fn nulls<T: DeserializeOwned + Debug>(objects: &[&Value]) {
    let lists = objects.iter().filter_map(|o| o["required"].as_array());
    let required: Vec<&Value> = lists.flatten().collect();
    let refusal = |body: Value| {
        let refused = serde_json::from_value::<T>(body).err();
        refused.map(|e| e.to_string()).unwrap_or_default()
    };
    let props = objects.iter().filter_map(|o| o["properties"].as_object());
    let mut nullable = Vec::new();
    for (name, node) in props.flatten() {
        let branches = node["oneOf"].as_array();
        let null_ok = branches.is_some_and(|b| b.contains(&json!({"type": "null"})));
        let refused = refusal(json!({ name: null })).starts_with("invalid type: null");
        assert_eq!(refused, !null_ok, "{name} null");
        nullable.extend(null_ok.then_some(name));
    }
    for left_out in &nullable {
        let others = nullable.iter().filter(|n| n != &left_out);
        let body = others.map(|n| (n.to_string(), Value::Null)).collect();
        let named = format!("missing field `{left_out}`");
        let missing = refusal(Value::Object(body)).contains(&named);
        assert_eq!(missing, required.contains(&&json!(left_out)), "{left_out}");
    }
}

#[test]
fn every_member_is_null_able_and_optional_exactly_as_its_schema_says() {
    let envelope = &file!("workspace-api/envelope.schema.json")["$defs"]["Envelope"];
    nulls::<CommandAccepted>(&[envelope, &schema!("command-accepted")]);
    nulls::<ApprovalResponseAccepted>(&[envelope, &schema!("approval-response-accepted")]);
    nulls::<ConfirmAccepted>(&[envelope, &schema!("confirm-accepted")]);
    nulls::<EndDelegationAlreadyEnded>(&[envelope, &schema!("end-delegation-already-ended")]);
    nulls::<PauseRequest>(&[&schema!("pause-request")]);
    nulls::<HoldRequest>(&[&schema!("hold-request")]);
    nulls::<EndDelegationRequest>(&[&schema!("end-delegation-request")]);
    nulls::<LiftHoldRequest>(&[&schema!("lift-hold-request")]);
    nulls::<ResumeRequest>(&[&schema!("resume-request")]);
    nulls::<AcknowledgeRequest>(&[&schema!("acknowledge-request")]);
    nulls::<OwnerRequest>(&[&schema!("owner-request")]);
}

/// DEC-682 item 23: no type of the crate flattens another, since serde could not then refuse an
/// unknown member.
#[test]
fn no_serde_attribute_in_the_crate_flattens() {
    let src = std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/src"));
    let files: Vec<_> = src.expect("src").flatten().map(|e| e.path()).collect();
    assert!(files.len() >= 6, "{files:?}");
    for path in files {
        let text = std::fs::read_to_string(&path).expect("a source file");
        for attribute in text.split("#[serde(").skip(1) {
            let attribute = attribute.split(")]").next().unwrap_or_default();
            assert!(!attribute.contains("flatten"), "{path:?}: {attribute}");
        }
    }
}

/// `pointer`'s parent and its last token, unescaped.
fn split(pointer: &str) -> (&str, String) {
    let (parent, last) = pointer.rsplit_once('/').expect("a JSON pointer");
    (parent, last.replace("~1", "/").replace("~0", "~"))
}

/// One `{label, set, remove}` case of `check_examples.py` applied to a copy of `base`; `set` adds
/// a member that is absent.
fn apply(base: &Value, case: &Value) -> Value {
    let mut body = base.clone();
    for (pointer, value) in case["set"].as_object().into_iter().flatten() {
        if let Some(target) = body.pointer_mut(pointer) {
            *target = value.clone();
            continue;
        }
        let (parent, last) = split(pointer);
        let parent = body.pointer_mut(parent).and_then(Value::as_object_mut);
        parent.expect(pointer).insert(last, value.clone());
    }
    let removals = case["remove"].as_array().cloned().unwrap_or_default();
    for pointer in removals.iter().filter_map(Value::as_str) {
        let (parent, last) = split(pointer);
        match body.pointer_mut(parent).expect(pointer) {
            Value::Array(items) => drop(items.remove(last.parse::<usize>().expect(pointer))),
            object => drop(object.as_object_mut().expect(pointer).remove(&last)),
        }
    }
    body
}

#[test]
fn a_case_sets_adds_and_removes_by_pointer() {
    let base = json!({"a": [{"b": 1}, 2], "c~/d": 3});
    let case = json!({"set": {"/a/0/b": 5, "/e": 6}, "remove": ["/a/1", "/c~0~1d"]});
    assert_eq!(apply(&base, &case), json!({"a": [{"b": 5}], "e": 6}));
}

/// Each good instance decodes and encodes back to the same JSON; each bad one is refused.
fn check<T: DeserializeOwned + Serialize + Validate + Debug>(good: &[Value], bad: &[Value]) {
    assert!(!good.is_empty() && !bad.is_empty(), "cases to check");
    for instance in good {
        let value = decode::<T>(instance.to_string().as_bytes());
        let value = value.unwrap_or_else(|e| panic!("{instance}: {e:?}"));
        let written = encode(&value).expect("encodes");
        assert_eq!(
            serde_json::from_slice::<Value>(&written).ok().as_ref(),
            Some(instance)
        );
    }
    refuses::<T>(bad);
}

/// Each instance is refused as `invalid`.
fn refuses<T: DeserializeOwned + Validate + Debug>(bad: &[Value]) {
    for instance in bad {
        let got = decode::<T>(instance.to_string().as_bytes());
        let refused = matches!(got, Err(Refused::Invalid { .. }));
        assert!(refused, "{instance}: {got:?}");
    }
}

/// The instances `files` list under `def`.
fn of(def: &str, files: &[Value]) -> Vec<Value> {
    let all = files.iter().filter_map(|f| f[def].as_array().cloned());
    all.flatten().collect()
}

#[test]
#[ignore = "pending E10-10"]
fn the_envelope_shapes_accept_their_examples_and_refuse_the_invalid() {
    let good = [file!("workspace-api/examples/envelope.json")];
    let bad = [file!("workspace-api/examples/envelope.invalid.json")];
    check::<Actor>(&of("Actor", &good), &of("Actor", &bad));
    check::<Record>(&of("Record", &good), &of("Record", &bad));
    check::<StepUpEvidence>(&of("StepUpEvidence", &good), &of("StepUpEvidence", &bad));
    check::<StepUpStatus>(&of("StepUpStatus", &good), &of("StepUpStatus", &bad));
    let valid = file!("workspace-api/examples/common.valid.json");
    let common = [file!("workspace-api/examples/common.json"), valid];
    let invalid = [file!("workspace-api/examples/common.invalid.json")];
    check::<Watermark>(&of("Watermark", &common), &of("Watermark", &invalid));
}

/// A shape's example with its `.valid` cases applied, and its `.invalid` cases applied.
fn instances(cases: [Value; 3]) -> (Vec<Value>, Vec<Value>) {
    let [base, valid, invalid] = cases;
    let apply_all = |cases: Value| -> Vec<Value> {
        let cases = cases.as_array().cloned().unwrap_or_default();
        cases.iter().map(|case| apply(&base, case)).collect()
    };
    let mut good = apply_all(valid);
    good.push(base.clone());
    (good, apply_all(invalid))
}

/// A response's example and its `.valid` cases are accepted, and its `.invalid` cases refused.
fn response<T: DeserializeOwned + Serialize + Validate + Debug>(cases: [Value; 3]) {
    let (good, bad) = instances(cases);
    check::<T>(&good, &bad);
}

/// A request's example and `.valid` cases decode, and what they encode to decodes to the same
/// value: absent and `null` are one value for an optional `null`-able member, so the spelling may
/// change. Its `.invalid` cases are refused.
fn request<T: DeserializeOwned + Serialize + Validate + PartialEq + Debug>(cases: [Value; 3]) {
    let (good, bad) = instances(cases);
    assert!(good.len() > 1 && !bad.is_empty(), "cases to check");
    for instance in &good {
        let value = decode::<T>(instance.to_string().as_bytes());
        let value = value.unwrap_or_else(|e| panic!("{instance}: {e:?}"));
        let written = encode(&value).expect("encodes");
        assert_eq!(decode::<T>(&written).ok(), Some(value), "{instance}");
    }
    refuses::<T>(&bad);
}

#[test]
#[ignore = "pending E10-10"]
fn command_accepted_matches_its_examples() {
    response::<CommandAccepted>([
        file!("workspace-api/examples/commands.command-accepted.json"),
        file!("workspace-api/examples/commands.command-accepted.valid.json"),
        file!("workspace-api/examples/commands.command-accepted.invalid.json"),
    ]);
}

#[test]
#[ignore = "pending E10-10"]
fn the_plain_responses_match_their_examples() {
    response::<ApprovalResponseAccepted>(cases!("approval-response-accepted"));
    response::<ConfirmAccepted>(cases!("confirm-accepted"));
    response::<EndDelegationAlreadyEnded>(cases!("end-delegation-already-ended"));
}

#[test]
#[ignore = "pending E10-10"]
fn the_plain_requests_match_their_examples() {
    request::<PauseRequest>(cases!("pause-request"));
    request::<HoldRequest>(cases!("hold-request"));
    request::<EndDelegationRequest>(cases!("end-delegation-request"));
    request::<LiftHoldRequest>(cases!("lift-hold-request"));
    request::<ResumeRequest>(cases!("resume-request"));
    request::<AcknowledgeRequest>(cases!("acknowledge-request"));
    request::<OwnerRequest>(cases!("owner-request"));
}
