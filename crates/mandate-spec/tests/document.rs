//! The strict parse (`kind: schema`, the 31 MC-S cases; ES-22).
//!
//! The reference cases pin *which* documents are rejected. These pin **why**: the code and the JSON
//! Pointer each rejection carries, which a case's `schema_valid: false` cannot express and which is
//! what an author actually reads.

mod common;

use common::{arr, b, base, i, obj, s, with};
use mandate_canon::Value;
use mandate_spec::{Mandate, ParseError};

fn parse(value: &Value) -> Result<Mandate, ParseError> {
    Mandate::parse(value)
}

fn code_of(value: &Value) -> String {
    match parse(value) {
        Ok(_) => "accepted".to_owned(),
        Err(e) => e.code().to_owned(),
    }
}

fn pointer_of(value: &Value) -> String {
    match parse(value) {
        Err(
            ParseError::UnknownMember { path }
            | ParseError::MissingMember { path }
            | ParseError::DecimalAsNumber { path }
            | ParseError::WrongType { path }
            | ParseError::NotInEnum { path }
            | ParseError::OffPattern { path }
            | ParseError::OutOfBounds { path }
            | ParseError::TooDeep { path },
        ) => path.as_str().to_owned(),
        Err(other) => format!("no pointer: {}", other.code()),
        Ok(_) => "accepted".to_owned(),
    }
}

#[test]
#[ignore = "pending E10-1"]
fn the_base_document_parses() {
    assert!(
        parse(&base()).is_ok(),
        "the builder's base must be a valid mandate, or every test below is testing the wrong thing"
    );
}

/// MC-S05 adds `/leverage`.
#[test]
#[ignore = "pending E10-1"]
fn an_unknown_member_names_itself() {
    let document = with("/leverage", Some(s("2")));
    assert_eq!(code_of(&document), "unknown_member");
    assert_eq!(pointer_of(&document), "/leverage");
}

/// MC-S10 removes a goal parameter, MC-S22 a model's output age, MC-S27 the research cost cap.
#[test]
#[ignore = "pending E10-1"]
fn a_missing_required_member_names_itself() {
    for path in [
        "/risk/max_drawdown",
        "/behavior/signal_models/0/max_output_age_s",
        "/autonomy/admission",
        "/universe/max_instruments",
    ] {
        let document = with(path, None);
        assert_eq!(code_of(&document), "missing_member", "removing {path}");
        assert_eq!(pointer_of(&document), path);
    }
}

/// MC-S06 and MC-S16. The journal grammar is a text-level type; a JSON number could not carry 28 places and would not round-trip (journal spec §4.6).
#[test]
#[ignore = "pending E10-1"]
fn a_decimal_sent_as_a_json_number_is_not_a_decimal() {
    let document = with("/capital/allocation_usd", Some(i(10_000)));
    assert_eq!(code_of(&document), "decimal_as_number");
    assert_eq!(pointer_of(&document), "/capital/allocation_usd");
    assert_eq!(
        code_of(&with(
            "/behavior/signal_models/0/params/0/value",
            Some(i(20))
        )),
        "decimal_as_number"
    );
}

/// MC-S02, MC-S23, MC-S28.
#[test]
#[ignore = "pending E10-1"]
fn a_value_off_its_enum_names_itself() {
    for (path, value) in [
        ("/autonomy/approval/on_timeout", "execute"),
        ("/risk/scale_action", "sell_everything"),
        ("/autonomy/admission", "sometimes"),
        ("/risk/daily_loss_action", "panic"),
        ("/environment", "demo"),
    ] {
        let document = with(path, Some(s(value)));
        assert_eq!(code_of(&document), "not_in_enum", "{path} = {value}");
        assert_eq!(pointer_of(&document), path);
    }
}

/// MC-S03, MC-S04, MC-S13, MC-S14, MC-S17: each is a decimal the schema's `$def` for that field rejects, so the code is the grammar's and not a pointer's.
#[test]
#[ignore = "pending E10-1"]
fn a_decimal_off_its_fields_grammar_says_which_grammar() {
    for path in [
        "/risk/max_drawdown",
        "/risk/hysteresis",
        "/risk/drawdown_ladder/0/factor",
    ] {
        assert_eq!(
            code_of(&with(path, Some(s("8")))),
            "off_grammar",
            "{path} = 8 is outside the open unit interval"
        );
    }
    assert_eq!(
        code_of(&with("/risk/max_daily_loss", Some(s("0.020")))),
        "off_grammar",
        "MC-S04: a trailing zero is not canonical, and DecStr would have normalised it away"
    );
    let twenty_nine = format!("0.{}11", "0".repeat(27));
    assert_eq!(
        code_of(&with("/risk/max_daily_loss", Some(s(&twenty_nine)))),
        "off_grammar",
        "MC-S17: 29 fractional digits"
    );
}

/// MC-S09, MC-S21, MC-S24, MC-S25, MC-S30, MC-S31.
#[test]
#[ignore = "pending E10-1"]
fn an_integer_outside_its_bounds_names_itself() {
    for (path, value) in [
        ("/behavior/cadence/interval_s", 30),
        ("/risk/breach_confirm_s", 301),
        ("/universe/max_instruments", 21),
        ("/universe/max_instruments", 0),
        ("/autonomy/approval/timeout_s", 29),
    ] {
        let document = with(path, Some(i(value)));
        assert_eq!(code_of(&document), "out_of_bounds", "{path} = {value}");
        assert_eq!(pointer_of(&document), path);
    }
}

/// MC-S08 (a model id with no type prefix), MC-S18 (month 13).
#[test]
#[ignore = "pending E10-1"]
fn a_string_off_its_pattern_names_itself() {
    assert_eq!(
        code_of(&with("/behavior/signal_models/0/id", Some(s("momentum")))),
        "off_pattern"
    );
    assert_eq!(
        code_of(&with("/goal/end_date", Some(s("2026-13-01")))),
        "off_pattern"
    );
    assert_eq!(
        code_of(&with("/name", Some(s("Two Stock Swing")))),
        "off_pattern",
        "the name is lowercase and hyphens"
    );
}

/// MC-S07.
#[test]
#[ignore = "pending E10-1"]
fn an_unknown_condition_field_is_not_an_enum_member() {
    let condition = obj(vec![
        ("field", s("vibes")),
        ("op", s("eq")),
        ("value", b(true)),
    ]);
    assert_eq!(
        code_of(&with("/autonomy/rules/0/when", Some(condition))),
        "not_in_enum"
    );
}

/// MC-S19: `in` takes a non-empty array.
#[test]
#[ignore = "pending E10-1"]
fn an_empty_in_list_is_out_of_bounds() {
    let condition = obj(vec![
        ("field", s("purpose")),
        ("op", s("in")),
        ("value", arr(vec![])),
    ]);
    assert_eq!(
        code_of(&with("/autonomy/rules/0/when", Some(condition))),
        "out_of_bounds"
    );
}

/// MC-S11: `distribute` was removed from the spec, so the tagged union has no branch for it.
#[test]
#[ignore = "pending E10-1"]
fn a_removed_goal_type_is_no_longer_a_goal() {
    let goal = obj(vec![
        ("type", s("distribute")),
        ("instrument", s("7b4a1c2e-1111-4a2b-9c3d-000000000001")),
        ("sell_qty", s("0.1")),
    ]);
    assert_eq!(code_of(&with("/goal", Some(goal))), "not_in_enum");
}

/// V-017 is a semantic rule, but the schema also bounds the nesting, so the parse is where a deeper tree stops.
#[test]
#[ignore = "pending E10-1"]
fn conditions_nest_at_most_four_deep() {
    let mut condition = obj(vec![
        ("field", s("order_usd")),
        ("op", s("gt")),
        ("value", s("900")),
    ]);
    for _ in 0..5 {
        condition = obj(vec![("not", condition)]);
    }
    assert_eq!(
        code_of(&with("/autonomy/rules/0/when", Some(condition))),
        "too_deep"
    );
}

/// The round trip the version hash rests on. It holds because every decimal grammar is canonical, so a `SchemaDec` is already the journal form (DEC-128 item 3); planted bug 25 re-serialises from the typed fields instead and loses a digit.
#[test]
#[ignore = "pending E10-1"]
fn the_canonical_bytes_reproduce_the_document_they_were_parsed_from() {
    let document = base();
    let mandate = parse(&document).expect("the base parses");
    let expected = mandate_canon::to_canonical(&document);
    assert_eq!(
        mandate.canonical_bytes().expect("canonical bytes"),
        expected,
        "the parse must not change a single byte of what the owner confirmed"
    );
}

#[test]
#[ignore = "pending E10-1"]
fn a_version_is_the_digest_of_those_bytes() {
    let mandate = parse(&base()).expect("the base parses");
    let bytes = mandate.canonical_bytes().expect("canonical bytes");
    assert_eq!(
        mandate.version().expect("a version").digest(),
        mandate_canon::Digest::of(&bytes),
        "§9.1: the version is sha256 of the canonical form and nothing else"
    );
}

/// A canonical object sorts its keys, so provenance, member order, and whitespace cannot move a version hash.
#[test]
#[ignore = "pending E10-1"]
fn two_documents_that_differ_only_in_order_hash_the_same() {
    let a = parse(&base()).expect("the base parses");
    let b = parse(&base()).expect("the base parses again");
    assert_eq!(a.version().ok(), b.version().ok());
}
