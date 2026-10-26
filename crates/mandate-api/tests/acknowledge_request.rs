//! An acknowledgment's optional members, ahead of the schema that names them (journal spec §9.28
//! version 2, DEC-2523; the struct first, as DEC-2126 and DEC-2128 order it): `subject` is one of
//! `limit`, `restriction`, and `reconciliation`; `request` is an event id; and `step_up_assertion`
//! is carried only beside `passkey` evidence (DEC-2128 item 1), else one violation at
//! `/step_up_assertion`. Each is read as `null` when absent, so the body the schema serves today
//! decodes as before.
//!
//! The base body is the schema's example; every expected outcome is written here by hand.

use mandate_api::envelope::{StepUpAssertion, StepUpMethod};
use mandate_api::problem::Violation;
use mandate_api::requests::{AcknowledgeRequest, AcknowledgeSubject};
use mandate_api::wire::{EventId, Refused, decode};
use serde_json::{Value, json};

const PASSKEY_ID: &str = "AAECAwQFBgcICQoLDA0ODw";
const REQUEST: &str = "01JBH3BTZ8K2M4N6P8R0S2T4V7";

fn base() -> Value {
    serde_json::from_str(include_str!(
        "../../../schemas/workspace-api/examples/commands.acknowledge-request.json"
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

/// The base body with `member` set to `value` (`step_up`'s method when `member` is `method`).
fn with(changes: &[(&str, Value)]) -> Vec<u8> {
    let mut body = base();
    for (member, value) in changes {
        if *member == "method" {
            body["step_up"]["method"] = value.clone();
        } else {
            body[*member] = value.clone();
        }
    }
    serde_json::to_vec(&body).expect("JSON")
}

/// The `(path, code)` of each violation `bytes` is refused with, or `None` when it decodes.
fn refused(bytes: &[u8]) -> Option<Vec<(String, String)>> {
    match decode::<AcknowledgeRequest>(bytes) {
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

fn event(id: &str) -> EventId {
    EventId::try_from(id.to_owned()).expect("an event id")
}

fn one(path: &str, code: &str) -> Option<Vec<(String, String)>> {
    Some(vec![(path.to_owned(), code.to_owned())])
}

#[test]
fn the_schema_s_body_decodes_as_before_with_every_new_member_null() {
    let request = decode::<AcknowledgeRequest>(&with(&[])).expect("decodes");
    assert_eq!(
        (request.subject, request.request, request.step_up_assertion),
        (None, None, None)
    );
    assert_eq!(request.acknowledged, event("01JBH3BTZ8K2M4N6P8R0S2T4V6"));
    let nulls = [
        ("subject", Value::Null),
        ("request", Value::Null),
        ("step_up_assertion", Value::Null),
    ];
    let request = decode::<AcknowledgeRequest>(&with(&nulls)).expect("decodes");
    assert_eq!(
        (request.subject, request.request, request.step_up_assertion),
        (None, None, None)
    );
}

#[test]
fn each_subject_decodes_as_its_variant_and_no_other_name_does() {
    let subjects = [
        ("limit", AcknowledgeSubject::Limit),
        ("restriction", AcknowledgeSubject::Restriction),
        ("reconciliation", AcknowledgeSubject::Reconciliation),
    ];
    for (name, subject) in subjects {
        let request = decode::<AcknowledgeRequest>(&with(&[("subject", json!(name))])).expect(name);
        assert_eq!(request.subject, Some(subject), "{name}");
    }
    for name in [
        json!("Limit"),
        json!("hold"),
        json!(""),
        json!(1),
        json!(["limit"]),
    ] {
        assert!(
            refused(&with(&[("subject", name.clone())])).is_some(),
            "{name}"
        );
    }
}

#[test]
fn a_request_is_an_event_id_and_nothing_else() {
    let request =
        decode::<AcknowledgeRequest>(&with(&[("request", json!(REQUEST))])).expect("decodes");
    assert_eq!(request.request, Some(event(REQUEST)));
    for id in [
        json!("not-an-event"),
        json!(""),
        json!(7),
        json!({"event_id": REQUEST}),
    ] {
        assert!(refused(&with(&[("request", id.clone())])).is_some(), "{id}");
    }
}

#[test]
fn a_passkey_acknowledgment_carries_its_assertion_member_for_member() {
    let request = decode::<AcknowledgeRequest>(&with(&[("step_up_assertion", assertion())]))
        .expect("decodes");
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
fn an_assertion_resting_on_no_passkey_is_one_violation_at_its_member() {
    let cli = [
        ("method", json!("cli_confirm")),
        ("step_up_assertion", assertion()),
    ];
    assert_eq!(refused(&with(&cli)), one("/step_up_assertion", "condition"));
    let none = [("step_up", Value::Null), ("step_up_assertion", assertion())];
    assert_eq!(
        refused(&with(&none)),
        one("/step_up_assertion", "condition")
    );
    assert_eq!(refused(&with(&[("method", json!("cli_confirm"))])), None);
    assert_eq!(refused(&with(&[("step_up", Value::Null)])), None);
}

#[test]
fn an_assertion_s_own_members_are_checked() {
    let mut carried = assertion();
    carried["signature"] = json!("MEUC/Q");
    assert_eq!(
        refused(&with(&[("step_up_assertion", carried)])),
        one("/step_up_assertion/signature", "pattern")
    );
    let mut carried = assertion();
    carried["user_handle"] = json!("AAAA");
    assert!(refused(&with(&[("step_up_assertion", carried)])).is_some());
}

#[test]
fn a_member_the_body_does_not_name_is_still_refused() {
    for member in ["requested_by", "user", "independent_required"] {
        assert!(
            refused(&with(&[(member, json!("usr_x"))])).is_some(),
            "{member}"
        );
    }
}
