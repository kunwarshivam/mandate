//! A connect start's `step_up_assertion` ([DEC-2126] item 1, DEC-2108 item 7): optional, read as
//! `null` when absent, and carried only beside `passkey` evidence. One that rests on no passkey is
//! one violation at `/step_up_assertion`, among the body checks, so it is refused before
//! `step_up_required`. Its members are `envelope.schema.json#/$defs/StepUpAssertion`'s.
//!
//! The base body is the schema's example; every expected outcome is written here by hand.
//!
//! [DEC-2126]: ../../../docs/project/decisions/DEC-2126.md

use mandate_api::envelope::{StepUpAssertion, StepUpMethod};
use mandate_api::problem::Violation;
use mandate_api::requests::OauthStartRequest;
use mandate_api::wire::{Refused, decode};
use serde_json::{Value, json};

const PASSKEY_ID: &str = "AAECAwQFBgcICQoLDA0ODw";

fn base() -> Value {
    serde_json::from_str(include_str!(
        "../../../schemas/workspace-api/examples/commands.oauth-start-request.json"
    ))
    .expect("JSON")
}

fn assertion() -> Value {
    json!({
        "passkey_id": PASSKEY_ID,
        "authenticator_data": "SZYN5YgOjGh0NBcPZHZgW4_krrmihjLHmVzzuoMdl2MFAAAACQ",
        "client_data_json": "eyJ0eXBlIjoid2ViYXV0aG4uZ2V0In0",
        "signature": "MEUCIQ",
    })
}

/// The base body with its evidence's method set to `method`, or `null` evidence, and
/// `step_up_assertion` set to `carried` when given.
fn body(method: Option<&str>, carried: Option<Value>) -> Vec<u8> {
    let mut body = base();
    match method {
        Some(method) => body["step_up"]["method"] = json!(method),
        None => body["step_up"] = Value::Null,
    }
    if let Some(carried) = carried {
        body["step_up_assertion"] = carried;
    }
    serde_json::to_vec(&body).expect("JSON")
}

/// The `(path, code)` of each violation `bytes` is refused with, or `None` when it decodes.
fn refused(bytes: &[u8]) -> Option<Vec<(String, String)>> {
    match decode::<OauthStartRequest>(bytes) {
        Ok(_) => None,
        Err(Refused::Invalid { violations }) => Some(
            violations
                .into_iter()
                .map(|v| match v {
                    Violation::Schema { path, code, .. } => (path, code),
                    other => panic!("a body's violation is a schema finding: {other:?}"),
                })
                .collect(),
        ),
        #[allow(
            unreachable_patterns,
            reason = "`Refused::Unimplemented` exists only while the lenient decoder is a stub (DEC-886 item 13)"
        )]
        Err(other) => panic!("the strict decoder refuses only as invalid: {other:?}"),
    }
}

fn one(path: &str, code: &str) -> Option<Vec<(String, String)>> {
    Some(vec![(path.to_owned(), code.to_owned())])
}

#[test]
fn a_start_without_an_assertion_decodes_as_before_and_reads_it_as_null() {
    for method in ["passkey", "cli_confirm"] {
        let absent = decode::<OauthStartRequest>(&body(Some(method), None)).expect("decodes");
        assert_eq!(absent.step_up_assertion, None, "{method}, absent");
        let null = decode::<OauthStartRequest>(&body(Some(method), Some(Value::Null)));
        assert_eq!(null.expect("decodes").step_up_assertion, None, "{method}");
    }
    let unproven = decode::<OauthStartRequest>(&body(None, None)).expect("decodes");
    assert_eq!((unproven.step_up, unproven.step_up_assertion), (None, None));
}

#[test]
fn a_passkey_start_carries_its_assertion_member_for_member() {
    let request =
        decode::<OauthStartRequest>(&body(Some("passkey"), Some(assertion()))).expect("decodes");
    assert_eq!(
        request.step_up.map(|evidence| evidence.method),
        Some(StepUpMethod::Passkey)
    );
    assert_eq!(
        request.step_up_assertion,
        Some(StepUpAssertion {
            passkey_id: PASSKEY_ID.to_owned(),
            authenticator_data: "SZYN5YgOjGh0NBcPZHZgW4_krrmihjLHmVzzuoMdl2MFAAAACQ".to_owned(),
            client_data_json: "eyJ0eXBlIjoid2ViYXV0aG4uZ2V0In0".to_owned(),
            signature: "MEUCIQ".to_owned(),
        })
    );
}

#[test]
fn an_assertion_beside_cli_confirm_evidence_is_one_violation_at_its_member() {
    assert_eq!(
        refused(&body(Some("cli_confirm"), Some(assertion()))),
        one("/step_up_assertion", "condition")
    );
}

#[test]
fn an_assertion_beside_null_evidence_is_one_violation_at_its_member() {
    assert_eq!(
        refused(&body(None, Some(assertion()))),
        one("/step_up_assertion", "condition")
    );
}

#[test]
fn an_assertion_member_outside_its_pattern_is_refused_at_that_member() {
    let cases = [
        ("passkey_id", "A".repeat(21)),
        ("passkey_id", "A".repeat(25)),
        ("passkey_id", "A".repeat(1_365)),
        ("passkey_id", format!("{}=", "A".repeat(23))),
        ("authenticator_data", String::new()),
        ("authenticator_data", "A".repeat(8_193)),
        ("client_data_json", "eyJ0eXBlIjo+".to_owned()),
        ("signature", "MEUC/Q".to_owned()),
    ];
    for (member, text) in cases {
        let mut carried = assertion();
        carried[member] = json!(text);
        assert_eq!(
            refused(&body(Some("passkey"), Some(carried))),
            one(&format!("/step_up_assertion/{member}"), "pattern"),
            "{member} of {} characters",
            text.len()
        );
    }
}

#[test]
fn an_assertion_member_at_its_pattern_s_bounds_is_admitted() {
    let cases = [
        ("passkey_id", "A".repeat(22)),
        ("passkey_id", "A".repeat(24)),
        ("passkey_id", "A".repeat(1_363)),
        ("passkey_id", "A".repeat(1_364)),
        ("authenticator_data", "A".to_owned()),
        ("signature", "-".repeat(8_192)),
    ];
    for (member, text) in cases {
        let mut carried = assertion();
        carried[member] = json!(text);
        assert_eq!(
            refused(&body(Some("passkey"), Some(carried))),
            None,
            "{member} of {} characters",
            text.len()
        );
    }
}

#[test]
fn an_assertion_with_a_member_missing_or_unknown_is_refused() {
    for member in [
        "passkey_id",
        "authenticator_data",
        "client_data_json",
        "signature",
    ] {
        let mut carried = assertion();
        carried.as_object_mut().expect("an object").remove(member);
        assert!(
            refused(&body(Some("passkey"), Some(carried))).is_some(),
            "{member} missing"
        );
    }
    let mut carried = assertion();
    carried["user_handle"] = json!("AAAA");
    assert!(refused(&body(Some("passkey"), Some(carried))).is_some());
    assert!(refused(&body(Some("passkey"), Some(json!([PASSKEY_ID])))).is_some());
}
