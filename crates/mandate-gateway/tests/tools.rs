//! Tool names, wire names, tool-call validation, the wire schema, `top_p`, and the budget settings
//! (inference spec v0.4 INF-5, §4.1, §6; DEC-2204 item 1).

mod common;

use common::*;
use mandate_gateway::budgets::{
    BudgetError, Budgets, DEPLOYMENT_MONTHLY_VAR, WORKSPACE_MONTHLY_VAR,
};
use mandate_gateway::money::Usd6;
use mandate_gateway::registry::TopP;
use mandate_gateway::tools::{
    CallsRefused, MAX_WIRE_NAME, ToolClass, WireCall, declared_name, validate, wire_name,
    wire_schema,
};
use proptest::prelude::*;

#[test]
fn declared_names_map_to_wire_names_one_to_one() {
    for (name, wire) in [
        ("note", "note"),
        ("market.bars", "market__bars"),
        ("memory.positions", "memory__positions"),
        ("propose_thesis", "propose_thesis"),
        ("filings.get2", "filings__get2"),
        ("a.b_c", "a__b_c"),
    ] {
        assert_eq!(wire_name(name).as_deref(), Some(wire), "{name}");
        assert_eq!(declared_name(wire).as_deref(), Some(name), "{wire}");
    }
    for refused in [
        "",
        ".",
        "a.",
        ".a",
        "a..b",
        "a.b.c",
        "A",
        "Note",
        "1note",
        "_note",
        "note_",
        "no__te",
        "market.__bars",
        "market-bars",
        "market bars",
        "é",
        "a.b_",
    ] {
        assert_eq!(wire_name(refused), None, "{refused:?}");
    }
    let longest = format!("{}.{}", "a".repeat(31), "b".repeat(31));
    assert_eq!(wire_name(&longest).map(|w| w.len()), Some(MAX_WIRE_NAME));
    let over = format!("{}.{}", "a".repeat(32), "b".repeat(31));
    assert_eq!(
        wire_name(&over),
        None,
        "a wire name past {MAX_WIRE_NAME} bytes"
    );
    for refused in [
        "market.bars",
        "market___bars",
        "market____bars",
        "__x",
        "x__",
        "Note",
    ] {
        assert_eq!(declared_name(refused), None, "{refused:?}");
    }
}

proptest! {
    #[test]
    fn the_mapping_round_trips_and_never_collides(
        a in "[a-z][a-z0-9]{0,5}(_[a-z0-9]{1,4}){0,2}",
        b in proptest::option::of("[a-z][a-z0-9]{0,5}(_[a-z0-9]{1,4}){0,2}"),
        c in "[a-z][a-z0-9]{0,5}(_[a-z0-9]{1,4}){0,2}",
        d in proptest::option::of("[a-z][a-z0-9]{0,5}(_[a-z0-9]{1,4}){0,2}"),
    ) {
        let one = b.map_or(a.clone(), |b| format!("{a}.{b}"));
        let two = d.map_or(c.clone(), |d| format!("{c}.{d}"));
        let wire_one = wire_name(&one).unwrap();
        prop_assert!(wire_one.bytes().all(|x| x.is_ascii_alphanumeric() || x == b'_'));
        prop_assert_eq!(declared_name(&wire_one), Some(one.clone()));
        prop_assert_eq!(wire_one == wire_name(&two).unwrap(), one == two);
    }
}

fn wire(name: &str, arguments: &str) -> WireCall {
    WireCall {
        name: Some(name.to_owned()),
        arguments: Some(arguments.to_owned()),
    }
}

#[test]
fn a_replys_calls_are_validated_against_the_offered_tools_in_reply_order() {
    let bars = tool("market.bars", ToolClass::Read, BARS_ARGS, &["ideate"]);
    let note = tool("note", ToolClass::Think, NOTE_ARGS, &["ideate"]);
    let offered = [&bars, &note];
    let calls = validate(
        &[wire("note", NOTE_CALL.1), wire("market__bars", BARS_CALL.1)],
        &offered,
        2,
    )
    .unwrap();
    assert_eq!(
        calls
            .iter()
            .map(|c| (c.index, c.name.as_str()))
            .collect::<Vec<_>>(),
        [(0, "note"), (1, "market.bars")]
    );
    assert_eq!(calls[1].arguments, common::json(BARS_CALL.1));
    assert_eq!(validate(&[], &offered, 2), Ok(vec![]));
    let refused = |calls: &[WireCall],
                   offered: &[&mandate_gateway::tools::ToolDeclaration],
                   per_turn| { validate(calls, offered, per_turn).unwrap_err() };
    assert_eq!(
        refused(&[wire("note", NOTE_CALL.1)], &[], 2),
        CallsRefused::NoneOffered
    );
    assert_eq!(
        refused(
            &[
                wire("note", NOTE_CALL.1),
                wire("note", NOTE_CALL.1),
                wire("note", NOTE_CALL.1)
            ],
            &offered,
            2
        ),
        CallsRefused::TooMany
    );
    assert_eq!(
        refused(&[wire("news__search", "{}")], &offered, 2),
        CallsRefused::NotOffered
    );
    assert_eq!(
        refused(&[wire("market.bars", BARS_CALL.1)], &offered, 2),
        CallsRefused::NotOffered
    );
    assert_eq!(
        refused(
            &[WireCall {
                name: None,
                arguments: Some("{}".to_owned())
            }],
            &offered,
            2
        ),
        CallsRefused::NotOffered
    );
    assert_eq!(
        refused(
            &[WireCall {
                name: Some("note".to_owned()),
                arguments: None
            }],
            &offered,
            2
        ),
        CallsRefused::BadArguments
    );
    for bad in [
        "",
        "{",
        r#"{"text":1}"#,
        r#"{"text":"a","more":1}"#,
        "{}",
        r#"[{"text":"a"}]"#,
    ] {
        assert_eq!(
            refused(&[wire("note", bad)], &offered, 2),
            CallsRefused::BadArguments,
            "{bad:?}"
        );
    }
    assert_eq!(
        refused(
            &[wire("note", NOTE_CALL.1), wire("shell__exec", "{}")],
            &offered,
            2
        ),
        CallsRefused::NotOffered,
        "one bad call refuses the reply"
    );
}

#[test]
fn the_wire_schema_describes_each_form_of_the_schema_language() {
    let schema = common::json(
        r#"{"type":"object","properties":{"b":{"type":"boolean"},"d":{"type":"decimal"},"e":{"type":"string","enum":["x","y"]},"i":{"type":"integer","minimum":0,"maximum":9},"l":{"type":"array","max_items":2,"items":{"type":"string","max_length":3}}}}"#,
    );
    assert_eq!(
        wire_schema(&schema),
        serde_json::json!({
            "type": "object",
            "properties": {
                "b": {"type": "boolean"},
                "d": {"type": "string", "description": "a decimal number written as text, such as \"0.6\""},
                "e": {"type": "string", "enum": ["x", "y"]},
                "i": {"type": "integer", "minimum": 0, "maximum": 9},
                "l": {"type": "array", "items": {"type": "string", "maxLength": 3}, "maxItems": 2},
            },
            "required": ["b", "d", "e", "i", "l"],
            "additionalProperties": false,
        })
    );
    assert_eq!(
        wire_schema(&common::json(r#"{"type":"float"}"#)),
        serde_json::Value::Null
    );
}

#[test]
fn top_p_is_a_decimal_in_millionths() {
    for (millionths, text) in [
        (1, "0.000001"),
        (900_000, "0.9"),
        (950_000, "0.95"),
        (123_450, "0.12345"),
        (1_000_000, "1"),
    ] {
        let top_p = TopP::from_millionths(millionths).unwrap();
        assert_eq!(top_p.text(), text);
        assert_eq!(top_p.millionths(), millionths);
    }
    assert_eq!(TopP::from_millionths(0), None);
    assert_eq!(TopP::from_millionths(1_000_001), None);
}

#[test]
fn the_budgets_are_settings_and_a_missing_one_refuses() {
    fn lookup<'a>(
        deployment: Option<&'a str>,
        workspace: Option<&'a str>,
    ) -> impl Fn(&str) -> Option<String> + 'a {
        move |var: &str| match var {
            DEPLOYMENT_MONTHLY_VAR => deployment.map(str::to_owned),
            WORKSPACE_MONTHLY_VAR => workspace.map(str::to_owned),
            _ => None,
        }
    }
    assert_eq!(
        Budgets::from_lookup(lookup(Some("50"), Some(" 12.5 "))),
        Ok(Budgets {
            deployment_monthly: Usd6::parse("50").unwrap(),
            workspace_monthly: Usd6::parse("12.5").unwrap(),
        })
    );
    assert_eq!(
        Budgets::from_lookup(lookup(None, Some("1"))),
        Err(BudgetError::Missing(DEPLOYMENT_MONTHLY_VAR))
    );
    assert_eq!(
        Budgets::from_lookup(lookup(Some("1"), None)),
        Err(BudgetError::Missing(WORKSPACE_MONTHLY_VAR))
    );
    for bad in ["", "-1", "fifty", "1e3", "0.0000001"] {
        assert_eq!(
            Budgets::from_lookup(lookup(Some(bad), Some("1"))),
            Err(BudgetError::Invalid(DEPLOYMENT_MONTHLY_VAR)),
            "{bad:?}"
        );
        assert_eq!(
            Budgets::from_lookup(lookup(Some("1"), Some(bad))),
            Err(BudgetError::Invalid(WORKSPACE_MONTHLY_VAR)),
            "{bad:?}"
        );
    }
    let error = Budgets::from_lookup(lookup(Some("not-a-secret-but-never-printed"), Some("1")))
        .unwrap_err()
        .to_string();
    assert!(
        !error.contains("never-printed"),
        "the value is never in the error"
    );
    assert!(error.contains(DEPLOYMENT_MONTHLY_VAR));
}
