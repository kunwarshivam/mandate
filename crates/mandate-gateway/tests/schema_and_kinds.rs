//! The output schema language (inference spec §3.2) and the closed vocabularies' codes.

mod common;

use common::json;
use mandate_gateway::schema::{fits, well_formed};
use mandate_gateway::{KeyOwner, Outcome, Purpose};

#[test]
fn each_type_fits_only_its_own_values() {
    let cases = [
        (r#"{"type":"boolean"}"#, "true", true),
        (r#"{"type":"boolean"}"#, "false", true),
        (r#"{"type":"boolean"}"#, r#""true""#, false),
        (r#"{"type":"boolean"}"#, "1", false),
        (
            r#"{"type":"integer","minimum":60,"maximum":100}"#,
            "60",
            true,
        ),
        (
            r#"{"type":"integer","minimum":60,"maximum":100}"#,
            "100",
            true,
        ),
        (
            r#"{"type":"integer","minimum":60,"maximum":100}"#,
            "59",
            false,
        ),
        (
            r#"{"type":"integer","minimum":60,"maximum":100}"#,
            "101",
            false,
        ),
        (
            r#"{"type":"integer","minimum":60,"maximum":100}"#,
            r#""80""#,
            false,
        ),
        (r#"{"type":"decimal"}"#, r#""0.6""#, true),
        (r#"{"type":"decimal"}"#, r#""0.60""#, false),
        (r#"{"type":"decimal"}"#, "1", false),
        (r#"{"type":"string","max_length":3}"#, r#""abc""#, true),
        (r#"{"type":"string","max_length":3}"#, r#""abcd""#, false),
        (r#"{"type":"string","max_length":3}"#, "3", false),
        (r#"{"type":"string","enum":["a","b"]}"#, r#""b""#, true),
        (r#"{"type":"string","enum":["a","b"]}"#, r#""c""#, false),
        (
            r#"{"type":"array","items":{"type":"boolean"},"max_items":2}"#,
            "[true,false]",
            true,
        ),
        (
            r#"{"type":"array","items":{"type":"boolean"},"max_items":2}"#,
            "[true,false,true]",
            false,
        ),
        (
            r#"{"type":"array","items":{"type":"boolean"},"max_items":2}"#,
            "[1]",
            false,
        ),
        (
            r#"{"type":"array","items":{"type":"boolean"},"max_items":2}"#,
            "{}",
            false,
        ),
        (
            r#"{"type":"object","properties":{"a":{"type":"boolean"}}}"#,
            r#"{"a":true}"#,
            true,
        ),
        (
            r#"{"type":"object","properties":{"a":{"type":"boolean"}}}"#,
            r#"{"a":true,"b":true}"#,
            false,
        ),
        (
            r#"{"type":"object","properties":{"a":{"type":"boolean"}}}"#,
            r#"{"b":true}"#,
            false,
        ),
        (
            r#"{"type":"object","properties":{"a":{"type":"boolean"}}}"#,
            r#"{"a":1}"#,
            false,
        ),
        (
            r#"{"type":"object","properties":{"a":{"type":"boolean"}}}"#,
            "[]",
            false,
        ),
        (r#"{"type":"nothing"}"#, "true", false),
    ];
    for (schema, value, want) in cases {
        let schema = json(schema);
        if schema.get("type").and_then(|t| t.as_str()) != Some("nothing") {
            assert!(well_formed(&schema), "{schema:?}");
        }
        assert_eq!(
            fits(&schema, &json(value)),
            want,
            "{value} against {schema:?}"
        );
    }
    assert!(!well_formed(&json(r#"{"type":"nothing"}"#)));
    assert!(well_formed(&json(
        r#"{"type":"integer","minimum":5,"maximum":5}"#
    )));
}

#[test]
fn refusals_are_exactly_the_six_that_send_nothing() {
    let refusals: Vec<&str> = Outcome::ALL
        .into_iter()
        .filter(|o| o.is_refusal())
        .map(Outcome::code)
        .collect();
    assert_eq!(
        refusals,
        [
            "policy_denied",
            "budget_exhausted",
            "rate_limited_local",
            "meter_unavailable",
            "input_rejected",
            "model_withdrawn"
        ]
    );
    let codes: Vec<&str> = Outcome::ALL.into_iter().map(Outcome::code).collect();
    assert_eq!(codes.len(), 14);
    assert_eq!(
        codes[7..],
        [
            "deadline_exceeded",
            "provider_unavailable",
            "rate_limited_provider",
            "credential_invalid",
            "content_refused",
            "schema_invalid",
            "identity_mismatch"
        ]
    );
    assert_eq!(Outcome::Ok.code(), "ok");
    assert_eq!(
        [
            Purpose::Research,
            Purpose::LlmSignal,
            Purpose::FastSignal,
            Purpose::Compiler
        ]
        .map(Purpose::code),
        ["research", "llm_signal", "fast_signal", "compiler"]
    );
    assert_eq!(
        [KeyOwner::Platform, KeyOwner::Customer].map(KeyOwner::code),
        ["platform", "customer"]
    );
}
