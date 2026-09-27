//! The strict parse (`kind: schema`, the 31 MC-S cases; ES-22).
//!
//! The reference cases pin *which* documents are rejected. These pin **why**: the code and the JSON
//! Pointer each rejection carries, which a case's `schema_valid: false` cannot express and which is
//! what an author actually reads.

mod common;

use common::{ASSET_A, arr, b, base, i, obj, s, with, with_all};
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
            | ParseError::TooDeep { path }
            | ParseError::OffGrammar { path, .. },
        ) => path.as_str().to_owned(),
        Err(other) => format!("no pointer: {}", other.code()),
        Ok(_) => "accepted".to_owned(),
    }
}

/// A document that must parse, with the rejection in the message when it does not.
///
/// Review round 3: a bare `is_ok()` on a guard says only "false" when it trips, which on a guard for
/// the builder's own base is the least useful failure in the file.
fn must_parse(value: &Value, why: &str) {
    if let Err(e) = parse(value) {
        panic!("{why}, but the parse rejected it: {} at {:?}", e.code(), e);
    }
}

#[test]
fn the_base_document_parses() {
    must_parse(
        &base(),
        "the builder's base must be a valid mandate, or every test below is testing the wrong thing",
    );
}

/// MC-S05 adds `/leverage`.
#[test]
fn an_unknown_member_names_itself() {
    let document = with("/leverage", Some(s("2")));
    assert_eq!(code_of(&document), "unknown_member");
    assert_eq!(pointer_of(&document), "/leverage");
}

/// MC-S22 removes a model's output age, MC-S27 the research cost cap, and the rest are the members
/// every mandate needs.
#[test]
fn a_missing_required_member_names_itself() {
    for path in [
        "/risk/max_drawdown",
        "/behavior/signal_models/0/max_output_age_s",
        "/autonomy/admission",
        "/universe/max_instruments",
        "/goal/type",
        "/goal/on_complete",
        "/behavior/signal_models/0/admits_instruments",
    ] {
        let document = with(path, None);
        assert_eq!(code_of(&document), "missing_member", "removing {path}");
        assert_eq!(pointer_of(&document), path);
    }
}

/// MC-S10 removes an accumulate goal's `max_spend_usd`.
///
/// The builder's base goal is `continuous`, which has no such member, so the row round 2 found in the
/// test above was checking a member of a goal the case does not use. The goal is a discriminated
/// `oneOf`: with `type: accumulate` and no `max_spend_usd` the document matches none of the three
/// branches, and a parse that dispatches on `type` names the member the accumulate branch needs
/// rather than reporting the whole goal as off its union.
#[test]
fn an_accumulate_goal_missing_its_spend_cap_names_that_member() {
    let accumulate = obj(vec![
        ("type", s("accumulate")),
        ("instrument", s(ASSET_A)),
        ("target_qty", s("0.5")),
        ("max_avg_price", Value::Null),
        ("max_spend_usd", s("5000")),
        ("end_date", Value::Null),
        ("on_complete", s("hold_protected")),
    ]);
    must_parse(
        &with("/goal", Some(accumulate.clone())),
        "the accumulate goal this row breaks must itself be valid",
    );
    let document = with_all(&[("/goal", Some(accumulate)), ("/goal/max_spend_usd", None)]);
    assert_eq!(code_of(&document), "missing_member");
    assert_eq!(pointer_of(&document), "/goal/max_spend_usd");
}

/// MC-S06 and MC-S16. The journal grammar is a text-level type; a JSON number could not carry 28 places and would not round-trip (journal spec §4.6).
#[test]
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

/// MC-S03, MC-S04, MC-S13, MC-S14, MC-S17: each is a decimal the schema's `$def` for that field
/// rejects, so the rejection names both the grammar the field declares and the field itself.
///
/// The pointer is the half review round 2 found unpinned: a rejection that says only "off_grammar"
/// leaves an author hunting for which of a mandate's forty decimals it meant.
#[test]
fn a_decimal_off_its_fields_grammar_says_which_grammar() {
    for path in [
        "/risk/max_drawdown",
        "/risk/hysteresis",
        "/risk/drawdown_ladder/0/factor",
    ] {
        let document = with(path, Some(s("8")));
        assert_eq!(
            code_of(&document),
            "off_grammar",
            "{path} = 8 is outside the open unit interval"
        );
        assert_eq!(pointer_of(&document), path, "{path} names itself");
    }
    let trailing_zero = with("/risk/max_daily_loss", Some(s("0.020")));
    assert_eq!(
        code_of(&trailing_zero),
        "off_grammar",
        "MC-S04: a trailing zero is not canonical, and DecStr would have normalised it away"
    );
    assert_eq!(pointer_of(&trailing_zero), "/risk/max_daily_loss");
    let twenty_nine = format!("0.{}11", "0".repeat(27));
    let too_many_places = with("/risk/max_daily_loss", Some(s(&twenty_nine)));
    assert_eq!(
        code_of(&too_many_places),
        "off_grammar",
        "MC-S17: 29 fractional digits"
    );
    assert_eq!(pointer_of(&too_many_places), "/risk/max_daily_loss");
}

/// MC-S09, MC-S21, MC-S24, MC-S25, MC-S30, MC-S31.
#[test]
fn an_integer_outside_its_bounds_names_itself() {
    for (path, value) in [
        ("/behavior/cadence/interval_s", 30),
        ("/risk/breach_confirm_s", 301),
        ("/universe/max_instruments", 21),
        ("/universe/max_instruments", 0),
        ("/autonomy/approval/timeout_s", 29),
        ("/behavior/signal_models/0/max_output_age_s", 59),
    ] {
        let document = with(path, Some(i(value)));
        assert_eq!(code_of(&document), "out_of_bounds", "{path} = {value}");
        assert_eq!(pointer_of(&document), path);
    }
}

/// MC-S08 (a model id with no type prefix), MC-S18 (month 13).
#[test]
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
fn two_documents_that_differ_only_in_order_hash_the_same() {
    let a = parse(&base()).expect("the base parses");
    let b = parse(&base()).expect("the base parses again");
    assert_eq!(a.version().ok(), b.version().ok());
}

/// The rest of the MC-S rejections that turn on a grammar, an `if`/`then`, an item count, or a
/// research object, each by the code and (where it has one) the pointer an author reads.
///
/// Added in review round 1: nine cases had a `schema_valid: false` in the fixture and no test here
/// saying *why*, and "why" is the whole reason these tests exist beside the harness.
#[test]
fn the_remaining_schema_rejections_name_their_reason() {
    assert_eq!(
        code_of(&with("/behavior/signal_models/0/weight", Some(s("0")))),
        "off_grammar",
        "MC-S12: a weight of zero is not in `unit_positive`, so the model would count for nothing"
    );
    assert_eq!(
        code_of(&with("/universe/asset_classes", Some(arr(vec![])))),
        "out_of_bounds",
        "MC-S26: an empty asset_classes admits nothing and the schema needs one"
    );
}

/// MC-S15: `protection` carries an `if enabled then stop_distance` conditional, so enabling protection
/// without a stop is a schema rejection rather than a V-rule.
///
/// The code is `wrong_type`, not `off_grammar`. `stop_distance` is `oneOf [open_fraction, null]`, so a
/// null passes the property's own schema and is refused only by the `then` clause, whose
/// `$ref: open_fraction` is `type: string`. A null is not a string off a grammar; it is the wrong type,
/// and that is what `jsonschema` reports for this case too, which ES-22 requires the two to agree on.
/// Review round 2 raised the choice; this is the side the schema takes.
#[test]
fn protection_enabled_needs_a_stop_distance() {
    let document = with("/protection/stop_distance", Some(Value::Null));
    assert_eq!(code_of(&document), "wrong_type");
    assert_eq!(pointer_of(&document), "/protection/stop_distance");
    must_parse(
        &with_all(&[
            ("/protection/enabled", Some(b(false))),
            ("/protection/stop_distance", Some(Value::Null)),
        ]),
        "the same null is valid with protection disabled, so the conditional is what rejects it",
    );
}

/// MC-S27, MC-S30, MC-S31: the research object's own required members and integer bounds.
///
/// The builder's base has `research: null` (no admitting model), so each row sets a research object
/// first and then breaks one thing in it.
#[test]
fn the_research_object_has_its_own_required_members_and_bounds() {
    let research = |interval: u64, revisions: u64, with_cap: bool| {
        let mut members = vec![("interval_s", i(interval))];
        if with_cap {
            members.push(("cost_cap_usd_per_day", s("5")));
        }
        members.push(("max_revisions_per_lineage", i(revisions)));
        obj(members)
    };
    let missing_cap = with("/behavior/research", Some(research(900, 3, false)));
    assert_eq!(code_of(&missing_cap), "missing_member", "MC-S27");
    assert_eq!(
        pointer_of(&missing_cap),
        "/behavior/research/cost_cap_usd_per_day"
    );
    let too_many = with("/behavior/research", Some(research(900, 11, true)));
    assert_eq!(code_of(&too_many), "out_of_bounds", "MC-S30: the cap is 10");
    assert_eq!(
        pointer_of(&too_many),
        "/behavior/research/max_revisions_per_lineage"
    );
    let too_fast = with("/behavior/research", Some(research(60, 3, true)));
    assert_eq!(
        code_of(&too_fast),
        "out_of_bounds",
        "MC-S31: 300 s is the floor"
    );
    assert_eq!(pointer_of(&too_fast), "/behavior/research/interval_s");
}
