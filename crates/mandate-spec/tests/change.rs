//! Change classification (`kind: change`, MC-C01 to MC-C48; mandate spec §9, DEC-43, DEC-121,
//! DEC-128 item 11, DEC-172).
//!
//! The cases pin one change each. These pin the rows of §9.2's table one by one, both directions of
//! each, and four properties with oracles of their own: step-up is required exactly when some changed
//! path increases risk, the verdict is the join over the changed paths, an allocation-only change is
//! classified by its direction alone, and a reducing or neutral autonomy change never makes any
//! decision less strict (MI-11).
//!
//! Every pending test asserts verdicts of at least two classes through `classify`, so no classifier
//! that returns one class whatever it is given passes any of them.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{ASSET_A, arr, b, base, edit, i, obj, s, with, with_all};
use mandate_canon::Value;
use mandate_domain::Environment;
use mandate_num::Usd;
use mandate_spec::Mandate;
use mandate_spec::change::{
    ChangeClass, Classification, PIN_SWITCH_PATHS, changed_paths, classify, classify_autonomy,
    pinning_switch,
};
use mandate_spec::document::{Autonomy, Pointer, ProvenanceMap};
use mandate_spec::validate::{ValidationContext, Violation, validate};
use mandate_time::Date;
use proptest::prelude::*;
use proptest::test_runner::TestRunner;

use ChangeClass::{Invalid, Neutral, RiskIncreasing, RiskReducing};

const ASSET_C: &str = "7b4a1c2e-4444-4a2b-9c3d-000000000004";

fn parse(document: &Value) -> Mandate {
    Mandate::parse(document).expect("the test document parses")
}

fn classified(old: &Value, new: &Value) -> Classification {
    classify(&parse(old), &parse(new)).expect("two parsed mandates classify")
}

/// The V-codes `validate` reports for a document, against an otherwise permissive paper context.
fn violations(document: &Value) -> BTreeSet<Violation> {
    let context = ValidationContext {
        account_equity_usd: Usd::parse("25000").expect("a dollar amount"),
        other_allocations_usd: Usd::ZERO,
        validation_date: Date::parse("2026-09-24").expect("a date"),
        registry: None,
        provenance: ProvenanceMap::default(),
        workspace_users: 1,
        approver_users: 1,
        independent_approval_required: false,
        disclosures_accepted: BTreeSet::new(),
        instrument_groups: BTreeMap::new(),
        claimed_by_other_agents: BTreeSet::new(),
        connection_environment: Some(Environment::Paper),
        connection_loss_carry_usd: Usd::ZERO,
        eligibility_failures: BTreeSet::new(),
        previous_version: None,
        current_mandate_version: None,
    };
    validate(&parse(document), &context)
        .expect("the document is evaluable")
        .violations
}

/// The class, the changed paths as text, and whether step-up is required.
fn verdict(old: &Value, new: &Value) -> (ChangeClass, Vec<String>, bool) {
    let c = classified(old, new);
    let paths = c
        .changed_paths
        .iter()
        .map(|p| p.as_str().to_owned())
        .collect();
    (c.class, paths, c.step_up_required)
}

/// Asserts a one-path change from `old`: the class, that exactly `path` changed, and that step-up
/// follows the class.
fn one_path(old: &Value, changes: &[(&str, Option<Value>)], path: &str, class: ChangeClass) {
    let new = edit(old, changes);
    let (got, paths, step_up) = verdict(old, &new);
    assert_eq!(got, class, "{path}: {changes:?}");
    assert_eq!(paths, vec![path.to_owned()], "{path}: {changes:?}");
    assert_eq!(
        step_up,
        class == RiskIncreasing,
        "{path}: step-up follows the class"
    );
}

/// `base()` as an accumulate goal on its first pinned instrument (V-003's shape).
fn accumulate() -> Value {
    with_all(&[
        ("/goal/type", Some(s("accumulate"))),
        ("/goal/instrument", Some(s(ASSET_A))),
        ("/goal/target_qty", Some(s("10"))),
        ("/goal/max_avg_price", Some(s("120"))),
        ("/goal/max_spend_usd", Some(s("1200"))),
        ("/goal/end_date", Some(s("2026-12-31"))),
        ("/goal/on_complete", Some(s("hold_protected"))),
    ])
}

fn profit_stop() -> Value {
    with(
        "/goal",
        Some(obj(vec![
            ("type", s("profit_stop")),
            ("profit_level", s("0.2")),
            ("end_date", Value::Null),
        ])),
    )
}

/// `research_equity`'s shape on the base: unpinned, empty pinned list, an admitting model, and a
/// research envelope.
fn research() -> Value {
    with_all(&[
        ("/universe/pinned", Some(b(false))),
        ("/universe/pinned_instruments", Some(arr(vec![]))),
        ("/universe/max_instruments", Some(i(5))),
        (
            "/behavior/signal_models/0/admits_instruments",
            Some(b(true)),
        ),
        (
            "/behavior/research",
            Some(obj(vec![
                ("interval_s", i(3600)),
                ("cost_cap_usd_per_day", s("5")),
                ("max_revisions_per_lineage", i(3)),
            ])),
        ),
    ])
}

fn instrument(asset_id: &str, symbol: &str) -> Value {
    obj(vec![
        ("asset_id", s(asset_id)),
        ("symbol", s(symbol)),
        ("asset_class", s("us_equity")),
    ])
}

/// The five edits of DEC-121's pinning switch, applied to `old`: pinned, one instrument, a
/// `max_instruments` of 1, research cleared, and the admitting flag cleared.
fn pinned(old: &Value) -> Value {
    edit(
        old,
        &[
            ("/universe/pinned", Some(b(true))),
            (
                "/universe/pinned_instruments",
                Some(arr(vec![instrument(ASSET_A, "AAA")])),
            ),
            ("/universe/max_instruments", Some(i(1))),
            ("/behavior/research", Some(Value::Null)),
            (
                "/behavior/signal_models/0/admits_instruments",
                Some(b(false)),
            ),
        ],
    )
}

fn rule(id: &str, field: &str, op: &str, value: Value, then: &str) -> Value {
    obj(vec![
        ("id", s(id)),
        (
            "when",
            obj(vec![("field", s(field)), ("op", s(op)), ("value", value)]),
        ),
        ("then", s(then)),
    ])
}

fn large_orders(threshold: &str, then: &str) -> Value {
    rule("large_orders", "order_usd", "gt", s(threshold), then)
}

fn low_score(threshold: &str, then: &str) -> Value {
    rule("low_score", "combined_score", "lt", s(threshold), then)
}

fn routine(purposes: &[&str], then: &str) -> Value {
    rule(
        "routine",
        "purpose",
        "in",
        arr(purposes.iter().map(|p| s(p)).collect()),
        then,
    )
}

/// `btc_accumulator`'s three rules on the base: two `ask` thresholds, then the routine `auto`.
fn three_rules() -> Value {
    with(
        "/autonomy/rules",
        Some(arr(vec![
            large_orders("900", "ask"),
            low_score("0.65", "ask"),
            routine(&["increase", "open"], "auto"),
        ])),
    )
}

fn autonomy_of(document: &Value) -> Autonomy {
    parse(document).autonomy
}

fn autonomy_class(old: &Value, new: &Value) -> ChangeClass {
    classify_autonomy(&autonomy_of(old), &autonomy_of(new)).expect("two rule sets classify")
}

/// MC-C01's shape: the last rung moves with `max_drawdown`, and the ladder is reported as one array,
/// not as `/risk/drawdown_ladder/2/at` (planted bug 18). Objects are walked to the changed leaf, a
/// `null` replaced by an object is reported where the `null` was, and the list is in pointer order
/// whatever order the edits were made in.
#[test]
fn arrays_are_compared_whole_and_objects_are_walked_to_the_leaf() {
    let old = parse(&base());
    let paths = |new: &Value| -> Vec<String> {
        changed_paths(&old, &parse(new))
            .expect("two parsed mandates diff")
            .iter()
            .map(|p| p.as_str().to_owned())
            .collect()
    };
    assert_eq!(
        paths(&with_all(&[
            ("/risk/max_drawdown", Some(s("0.09"))),
            ("/risk/drawdown_ladder/2/at", Some(s("0.09"))),
        ])),
        vec!["/risk/drawdown_ladder", "/risk/max_drawdown"]
    );
    assert_eq!(
        paths(&with_all(&[
            ("/risk/max_order_usd", Some(s("900"))),
            ("/capital/allocation_usd", Some(s("9000"))),
            ("/behavior/sizing/rebalance_band", Some(s("0.1"))),
        ])),
        vec![
            "/behavior/sizing/rebalance_band",
            "/capital/allocation_usd",
            "/risk/max_order_usd"
        ],
        "pointer order, not edit order"
    );
    let hours = obj(vec![
        ("start", s("23:00")),
        ("end", s("07:00")),
        ("timezone", s("America/New_York")),
    ]);
    assert_eq!(
        paths(&with("/notifications/quiet_hours", Some(hours.clone()))),
        vec!["/notifications/quiet_hours"]
    );
    let old = parse(&with("/notifications/quiet_hours", Some(hours)));
    let later = with_all(&[
        (
            "/notifications/quiet_hours",
            Some(obj(vec![
                ("start", s("22:00")),
                ("end", s("07:00")),
                ("timezone", s("America/New_York")),
            ])),
        ),
        ("/behavior/signal_models/0/params/0/value", Some(s("30"))),
    ]);
    let walked: Vec<String> = changed_paths(&old, &parse(&later))
        .expect("two parsed mandates diff")
        .iter()
        .map(|p| p.as_str().to_owned())
        .collect();
    assert_eq!(
        walked,
        vec![
            "/behavior/signal_models",
            "/notifications/quiet_hours/start"
        ]
    );
    assert_eq!(
        classified(
            &base(),
            &with_all(&[
                ("/risk/max_drawdown", Some(s("0.09"))),
                ("/risk/drawdown_ladder/2/at", Some(s("0.09"))),
            ])
        )
        .class,
        RiskIncreasing,
        "MC-C01"
    );
    assert_eq!(
        classified(&base(), &with("/notifications/quiet_hours/start", None)).class,
        Neutral,
        "no path changed when the edit names nothing"
    );
}

/// `btc_accumulator`'s canonical JSON, the fixture's version vector (§9.1).
const BTC_ACCUMULATOR: &str = concat!(
    "{\"autonomy\":{\"admission\":\"ask\",\"approval\":{\"approvers\":[\"role:approver\"],",
    "\"on_timeout\":\"skip\",\"timeout_s\":600,\"two_approver_above_usd\":null},",
    "\"default\":\"ask\",\"rules\":[{\"id\":\"large_orders\",\"then\":\"ask\",",
    "\"when\":{\"field\":\"order_usd\",\"op\":\"gt\",\"value\":\"900\"}},",
    "{\"id\":\"low_score\",\"then\":\"ask\",\"when\":{\"field\":\"combined_score\",",
    "\"op\":\"lt\",\"value\":\"0.65\"}},{\"id\":\"routine\",\"then\":\"auto\",",
    "\"when\":{\"field\":\"purpose\",\"op\":\"in\",\"value\":[\"increase\",\"open\"]}}]},",
    "\"behavior\":{\"cadence\":{\"event_sources\":[\"news\",\"price\"],\"interval_s\":900},",
    "\"description\":\"Buy dips; avoid trading 30 minutes around major macro releases.\",",
    "\"research\":null,\"signal_models\":[{\"admits_instruments\":false,",
    "\"content_hash\":\"sha256:1111111111111111111111111111111111111111111111111111111111111111\",",
    "\"id\":\"quant.mean_reversion\",\"max_output_age_s\":1800,",
    "\"params\":[{\"key\":\"lookback_bars\",\"value\":\"20\"},{\"key\":\"z_entry\",",
    "\"value\":\"1.5\"}],\"version\":\"1.0.0\",\"weight\":\"1\"}],",
    "\"sizing\":{\"entry_threshold\":\"0.3\",\"exit_threshold\":\"0.3\",",
    "\"method\":\"conviction_linear\",\"rebalance_band\":\"0.05\"}},",
    "\"capital\":{\"allocation_usd\":\"10000\",\"max_loss_from_allocation\":\"0.1\"},",
    "\"connection_id\":\"conn_alpaca_paper_01\",\"environment\":\"paper\",",
    "\"goal\":{\"end_date\":\"2026-12-31\",",
    "\"instrument\":\"7b4a1c2e-1111-4a2b-9c3d-000000000001\",\"max_avg_price\":\"58000\",",
    "\"max_spend_usd\":\"9000\",\"on_complete\":\"hold_protected\",\"target_qty\":\"0.15\",",
    "\"type\":\"accumulate\"},\"mandate_schema_version\":1,\"name\":\"btc-accumulator\",",
    "\"notifications\":{\"channels\":[\"email\",\"web_push\"],",
    "\"quiet_hours\":{\"end\":\"07:00\",\"start\":\"23:00\",",
    "\"timezone\":\"America/New_York\"}},",
    "\"protection\":{\"crypto_stop_limit_offset\":\"0.005\",\"enabled\":true,",
    "\"stop_distance\":\"0.08\",\"take_profit_distance\":null},",
    "\"risk\":{\"breach_confirm_s\":60,\"daily_breach_min_s\":3600,",
    "\"daily_loss_action\":\"exits_only\",\"drawdown_ladder\":[{\"action\":\"scale_sizes\",",
    "\"at\":\"0.03\",\"factor\":\"0.5\"},{\"action\":\"exits_only\",\"at\":\"0.06\",",
    "\"factor\":null},{\"action\":\"flatten_and_pause\",\"at\":\"0.08\",\"factor\":null}],",
    "\"hysteresis\":\"0.01\",\"max_daily_loss\":\"0.02\",\"max_drawdown\":\"0.08\",",
    "\"max_gross_exposure_usd\":\"10000\",\"max_order_usd\":\"1000\",",
    "\"max_orders_per_day\":50,\"max_position_fraction\":\"1\",",
    "\"max_position_usd\":\"10000\",\"reentry_cooldown_s\":3600,",
    "\"scale_action\":\"limit_buys\",\"scale_lift_after_s\":600},",
    "\"source_text_ref\":\"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",",
    "\"universe\":{\"asset_classes\":[\"crypto\"],\"leveraged_etp_disclosure_version\":null,",
    "\"leveraged_etps_enabled\":false,\"max_instruments\":1,\"pinned\":true,",
    "\"pinned_instruments\":[{\"asset_class\":\"crypto\",",
    "\"asset_id\":\"7b4a1c2e-1111-4a2b-9c3d-000000000001\",\"symbol\":\"BTC/USD\"}]}}",
);

/// The version is `sha256:` plus the SHA-256 of the canonical bytes, pinned by the literal digest the
/// fixture's version vector states rather than by comparing two runs of the same writer, which a
/// non-canonical writer would pass (the backlog's E10-1 review row).
#[test]
fn the_version_vector_is_pinned_by_its_literal_digest() {
    let value = mandate_canon::parse(BTC_ACCUMULATOR.as_bytes()).expect("canonical JSON");
    let mandate = parse(&value);
    assert_eq!(
        mandate.canonical_bytes().expect("canonical bytes"),
        BTC_ACCUMULATOR.as_bytes()
    );
    assert_eq!(
        format!("sha256:{}", mandate.version().expect("a version").digest()),
        "sha256:9fb03f7ebea3976370394bfab907d99869f50196f7623c3a63d69ef0e8bf3cda"
    );
}

/// A document classified against itself changed nothing: neutral, no paths, no step-up. The same
/// base with one maximum raised is the contrast that makes the first half mean something.
#[test]
fn an_unchanged_document_is_neutral_with_no_paths() {
    assert_eq!(verdict(&base(), &base()), (Neutral, vec![], false));
    assert_eq!(
        verdict(&base(), &with("/risk/max_order_usd", Some(s("1100")))),
        (RiskIncreasing, vec!["/risk/max_order_usd".to_owned()], true)
    );
}

/// §9.2's maximums: larger is increasing, smaller is reducing, each on its own path. The goal's and
/// the research envelope's maximums need a document that has them.
#[test]
fn every_maximum_is_increasing_when_raised_and_reducing_when_lowered() {
    let on_base: [(&str, Value, Value); 13] = [
        ("/capital/allocation_usd", s("12000"), s("8000")),
        ("/capital/max_loss_from_allocation", s("0.2"), s("0.05")),
        ("/risk/max_position_usd", s("2000"), s("1000")),
        ("/risk/max_position_fraction", s("0.6"), s("0.4")),
        ("/risk/max_gross_exposure_usd", s("4000"), s("2000")),
        ("/risk/max_order_usd", s("1100"), s("900")),
        ("/risk/max_orders_per_day", i(60), i(40)),
        ("/risk/max_daily_loss", s("0.03"), s("0.01")),
        ("/risk/max_drawdown", s("0.09"), s("0.07")),
        ("/risk/breach_confirm_s", i(120), i(30)),
        ("/protection/stop_distance", s("0.06"), s("0.04")),
        ("/behavior/sizing/exit_threshold", s("0.4"), s("0.2")),
        ("/universe/max_instruments", i(3), i(1)),
    ];
    for (path, higher, lower) in on_base {
        one_path(&base(), &[(path, Some(higher))], path, RiskIncreasing);
        one_path(&base(), &[(path, Some(lower))], path, RiskReducing);
    }
    let on_goal: [(&str, Value, Value); 3] = [
        ("/goal/target_qty", s("12"), s("8")),
        ("/goal/max_spend_usd", s("1500"), s("1000")),
        ("/goal/max_avg_price", s("130"), s("110")),
    ];
    for (path, higher, lower) in on_goal {
        one_path(&accumulate(), &[(path, Some(higher))], path, RiskIncreasing);
        one_path(&accumulate(), &[(path, Some(lower))], path, RiskReducing);
    }
    let level = "/goal/profit_level";
    one_path(
        &profit_stop(),
        &[(level, Some(s("0.3")))],
        level,
        RiskIncreasing,
    );
    one_path(
        &profit_stop(),
        &[(level, Some(s("0.1")))],
        level,
        RiskReducing,
    );
    let on_research: [(&str, Value, Value); 2] = [
        ("/behavior/research/cost_cap_usd_per_day", s("8"), s("2")),
        ("/behavior/research/max_revisions_per_lineage", i(5), i(1)),
    ];
    for (path, higher, lower) in on_research {
        one_path(&research(), &[(path, Some(higher))], path, RiskIncreasing);
        one_path(&research(), &[(path, Some(lower))], path, RiskReducing);
    }
}

/// `null` is unbounded (§9.2, "null means unbounded"), so a maximum removed is the largest rise there
/// is, and one set from `null` is a fall. The decimal order is by value, not by text: `9` is below
/// `10` although it sorts after it.
#[test]
fn a_null_maximum_is_unbounded_and_values_compare_by_number_not_text() {
    let price = "/goal/max_avg_price";
    one_path(
        &accumulate(),
        &[(price, Some(Value::Null))],
        price,
        RiskIncreasing,
    );
    let open = edit(&accumulate(), &[(price, Some(Value::Null))]);
    one_path(&open, &[(price, Some(s("120")))], price, RiskReducing);
    let per_day = "/risk/max_orders_per_day";
    one_path(&base(), &[(per_day, Some(i(9)))], per_day, RiskReducing);
    one_path(&base(), &[(per_day, Some(i(100)))], per_day, RiskIncreasing);
    let order = "/risk/max_order_usd";
    one_path(&base(), &[(order, Some(s("999.99")))], order, RiskReducing);
    one_path(
        &base(),
        &[(order, Some(s("1000.01")))],
        order,
        RiskIncreasing,
    );
}

/// §9.2's minimums: smaller is increasing, larger is reducing.
#[test]
fn every_minimum_is_increasing_when_lowered_and_reducing_when_raised() {
    let on_base: [(&str, Value, Value); 6] = [
        ("/behavior/sizing/entry_threshold", s("0.2"), s("0.4")),
        ("/behavior/sizing/rebalance_band", s("0.01"), s("0.1")),
        ("/risk/hysteresis", s("0.005"), s("0.02")),
        ("/risk/reentry_cooldown_s", i(600), i(7200)),
        ("/risk/daily_breach_min_s", i(1800), i(7200)),
        ("/risk/scale_lift_after_s", i(300), i(1200)),
    ];
    for (path, lower, higher) in on_base {
        one_path(&base(), &[(path, Some(lower))], path, RiskIncreasing);
        one_path(&base(), &[(path, Some(higher))], path, RiskReducing);
    }
    let interval = "/behavior/research/interval_s";
    one_path(
        &research(),
        &[(interval, Some(i(1800)))],
        interval,
        RiskIncreasing,
    );
    one_path(
        &research(),
        &[(interval, Some(i(7200)))],
        interval,
        RiskReducing,
    );
}

/// The ladder row: new actions, or any `at` or `factor` larger, is increasing even when another rung
/// tightened; otherwise reducing.
#[test]
fn the_ladder_is_increasing_when_its_actions_change_or_any_at_or_factor_rises() {
    let ladder = "/risk/drawdown_ladder";
    one_path(
        &base(),
        &[("/risk/drawdown_ladder/0/at", Some(s("0.04")))],
        ladder,
        RiskIncreasing,
    );
    one_path(
        &base(),
        &[("/risk/drawdown_ladder/0/at", Some(s("0.02")))],
        ladder,
        RiskReducing,
    );
    one_path(
        &base(),
        &[("/risk/drawdown_ladder/0/factor", Some(s("0.6")))],
        ladder,
        RiskIncreasing,
    );
    one_path(
        &base(),
        &[("/risk/drawdown_ladder/0/factor", Some(s("0.4")))],
        ladder,
        RiskReducing,
    );
    one_path(
        &base(),
        &[
            ("/risk/drawdown_ladder/0/at", Some(s("0.02"))),
            ("/risk/drawdown_ladder/1/at", Some(s("0.07"))),
        ],
        ladder,
        RiskIncreasing,
    );
    one_path(
        &base(),
        &[(
            "/risk/drawdown_ladder/1/action",
            Some(s("flatten_and_pause")),
        )],
        ladder,
        RiskIncreasing,
    );
    one_path(
        &base(),
        &[("/risk/drawdown_ladder/0", None)],
        ladder,
        RiskIncreasing,
    );
}

/// `limit_buys` to `trim_to_target` is reducing; the reverse is increasing.
#[test]
fn scale_action_to_trim_is_reducing_and_back_is_increasing() {
    let path = "/risk/scale_action";
    one_path(
        &base(),
        &[(path, Some(s("trim_to_target")))],
        path,
        RiskReducing,
    );
    let trim = with(path, Some(s("trim_to_target")));
    one_path(
        &trim,
        &[(path, Some(s("limit_buys")))],
        path,
        RiskIncreasing,
    );
}

/// Pinned instruments (MC-C02, MC-C03): an added instrument is increasing, a removed one reducing, and
/// the same instruments in another order change nothing that matters. An instrument whose id stays
/// while its symbol changes is a new entry and is increasing, the fail-safe reading (DEC-172 item 1).
#[test]
fn a_pinned_instrument_added_is_increasing_and_one_removed_is_reducing() {
    let path = "/universe/pinned_instruments";
    let three = arr(vec![
        instrument(ASSET_A, "AAA"),
        instrument("7b4a1c2e-3333-4a2b-9c3d-000000000003", "BBB"),
        instrument(ASSET_C, "CCC"),
    ]);
    one_path(&base(), &[(path, Some(three))], path, RiskIncreasing);
    one_path(
        &base(),
        &[("/universe/pinned_instruments/1", None)],
        path,
        RiskReducing,
    );
    let swapped = arr(vec![
        instrument("7b4a1c2e-3333-4a2b-9c3d-000000000003", "BBB"),
        instrument(ASSET_A, "AAA"),
    ]);
    one_path(&base(), &[(path, Some(swapped))], path, Neutral);
    one_path(
        &base(),
        &[("/universe/pinned_instruments/0/symbol", Some(s("AAB")))],
        path,
        RiskIncreasing,
    );
}

/// An asset class added is increasing; one removed is reducing (MC-C40, MC-C41).
#[test]
fn an_asset_class_added_is_increasing_and_one_removed_is_reducing() {
    let path = "/universe/asset_classes";
    let both = arr(vec![s("crypto"), s("us_equity")]);
    one_path(&base(), &[(path, Some(both.clone()))], path, RiskIncreasing);
    let wide = with(path, Some(both));
    one_path(
        &wide,
        &[(path, Some(arr(vec![s("us_equity")])))],
        path,
        RiskReducing,
    );
}

/// `behavior.research` set from `null` is increasing, because the research agent may now admit;
/// cleared to `null` it is reducing.
#[test]
fn research_set_from_null_is_increasing_and_cleared_is_reducing() {
    let path = "/behavior/research";
    let envelope = obj(vec![
        ("interval_s", i(3600)),
        ("cost_cap_usd_per_day", s("5")),
        ("max_revisions_per_lineage", i(3)),
    ]);
    one_path(&base(), &[(path, Some(envelope))], path, RiskIncreasing);
    one_path(
        &research(),
        &[(path, Some(Value::Null))],
        path,
        RiskReducing,
    );
}

/// The crypto stop-limit offset: newly set or larger is increasing; cleared or smaller is reducing.
/// `null` here means "no crypto", not "unbounded", so the maximum row's reading does not apply.
#[test]
fn a_crypto_stop_limit_offset_set_or_widened_is_increasing() {
    let path = "/protection/crypto_stop_limit_offset";
    one_path(&base(), &[(path, Some(s("0.005")))], path, RiskIncreasing);
    let set = with(path, Some(s("0.005")));
    one_path(&set, &[(path, Some(s("0.01")))], path, RiskIncreasing);
    one_path(&set, &[(path, Some(s("0.003")))], path, RiskReducing);
    one_path(&set, &[(path, Some(Value::Null))], path, RiskReducing);
}

/// `end_date`: later or removed is increasing (MC-C35); earlier, or set where there was none, is
/// reducing.
#[test]
fn an_end_date_later_or_removed_is_increasing_and_earlier_is_reducing() {
    let path = "/goal/end_date";
    one_path(
        &base(),
        &[(path, Some(s("2026-12-31")))],
        path,
        RiskReducing,
    );
    let dated = with(path, Some(s("2026-12-31")));
    one_path(
        &dated,
        &[(path, Some(s("2027-01-01")))],
        path,
        RiskIncreasing,
    );
    one_path(&dated, &[(path, Some(s("2026-06-30")))], path, RiskReducing);
    one_path(&dated, &[(path, Some(Value::Null))], path, RiskIncreasing);
}

/// Leveraged ETPs on and protection off are increasing; the reverse of each is reducing.
#[test]
fn leveraged_etps_on_and_protection_off_are_increasing() {
    let etps = "/universe/leveraged_etps_enabled";
    one_path(&base(), &[(etps, Some(b(true)))], etps, RiskIncreasing);
    one_path(
        &with(etps, Some(b(true))),
        &[(etps, Some(b(false)))],
        etps,
        RiskReducing,
    );
    let enabled = "/protection/enabled";
    one_path(
        &base(),
        &[(enabled, Some(b(false)))],
        enabled,
        RiskIncreasing,
    );
    one_path(
        &with(enabled, Some(b(false))),
        &[(enabled, Some(b(true)))],
        enabled,
        RiskReducing,
    );
}

/// Notifications and the name: removing a channel is increasing (MC-C08), adding one is neutral
/// (MC-C07), and quiet hours (MC-C09) and the name (MC-C23) are neutral.
#[test]
fn a_channel_removed_is_increasing_and_the_neutral_rows_are_neutral() {
    let channels = "/notifications/channels";
    let two = arr(vec![s("email"), s("sms")]);
    one_path(&base(), &[(channels, Some(two.clone()))], channels, Neutral);
    one_path(
        &with(channels, Some(two)),
        &[(channels, Some(arr(vec![s("sms")])))],
        channels,
        RiskIncreasing,
    );
    one_path(
        &base(),
        &[("/name", Some(s("two-stock-hold")))],
        "/name",
        Neutral,
    );
    let hours = "/notifications/quiet_hours";
    let quiet = obj(vec![
        ("start", s("23:00")),
        ("end", s("07:00")),
        ("timezone", s("America/New_York")),
    ]);
    one_path(&base(), &[(hours, Some(quiet))], hours, Neutral);
}

/// §9.2's last row: signal models, sizing, the description, cadence, `daily_loss_action`,
/// `take_profit_distance`, the goal type, `on_complete`, and every path the table does not name are
/// increasing. The allocation lowered beside each is the reducing contrast.
#[test]
fn every_unlisted_path_is_increasing() {
    let unlisted: [(&str, Value, &str); 11] = [
        (
            "/behavior/description",
            s("Buy dips."),
            "/behavior/description",
        ),
        (
            "/behavior/cadence/interval_s",
            i(1800),
            "/behavior/cadence/interval_s",
        ),
        (
            "/behavior/cadence/event_sources",
            arr(vec![s("news"), s("price")]),
            "/behavior/cadence/event_sources",
        ),
        (
            "/behavior/signal_models/0/weight",
            s("0.5"),
            "/behavior/signal_models",
        ),
        (
            "/behavior/signal_models/0/max_output_age_s",
            i(900),
            "/behavior/signal_models",
        ),
        (
            "/risk/daily_loss_action",
            s("flatten_and_pause"),
            "/risk/daily_loss_action",
        ),
        (
            "/protection/take_profit_distance",
            s("0.1"),
            "/protection/take_profit_distance",
        ),
        ("/goal/on_complete", s("release"), "/goal/on_complete"),
        (
            "/source_text_ref",
            s("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            "/source_text_ref",
        ),
        (
            "/universe/leveraged_etp_disclosure_version",
            s("sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
            "/universe/leveraged_etp_disclosure_version",
        ),
        (
            "/behavior/signal_models/0/admits_instruments",
            b(true),
            "/behavior/signal_models",
        ),
    ];
    let lower = ("/capital/allocation_usd", Some(s("9000")));
    for (path, value, reported) in unlisted {
        one_path(
            &base(),
            &[(path, Some(value.clone()))],
            reported,
            RiskIncreasing,
        );
        let (class, paths, step_up) =
            verdict(&base(), &with_all(&[lower.clone(), (path, Some(value))]));
        assert_eq!(
            (class, step_up),
            (RiskIncreasing, true),
            "{path} outweighs a reduction"
        );
        assert_eq!(paths.len(), 2, "{path}");
    }
    one_path(&base(), &[lower], "/capital/allocation_usd", RiskReducing);
    let (class, paths, _) = verdict(&base(), &profit_stop());
    assert_eq!(class, RiskIncreasing, "a new goal type is increasing");
    assert!(paths.contains(&"/goal/type".to_owned()), "{paths:?}");
}

/// A changed environment or connection is invalid, not classified (MC-C34, V-031), whatever else
/// changed with it: a reduction, an increase, or DEC-121's pinning switch. Invalid needs no step-up,
/// because it is refused. The same reduction without the connection change is the contrast.
#[test]
fn a_changed_environment_or_connection_is_invalid_whatever_else_changed() {
    let reduce = ("/risk/max_daily_loss", Some(s("0.01")));
    assert_eq!(
        verdict(&base(), &with_all(std::slice::from_ref(&reduce))),
        (RiskReducing, vec!["/risk/max_daily_loss".to_owned()], false)
    );
    assert_eq!(
        verdict(&base(), &with("/risk/max_order_usd", Some(s("5000")))),
        (RiskIncreasing, vec!["/risk/max_order_usd".to_owned()], true)
    );
    let live = ("/environment", Some(s("live")));
    assert_eq!(
        verdict(&base(), &with_all(std::slice::from_ref(&live))),
        (Invalid, vec!["/environment".to_owned()], false)
    );
    let other = ("/connection_id", Some(s("conn_alpaca_paper_02")));
    for extra in [reduce, ("/risk/max_order_usd", Some(s("5000"))), live] {
        let (class, paths, step_up) = verdict(&base(), &with_all(&[other.clone(), extra]));
        assert_eq!((class, step_up), (Invalid, false), "{paths:?}");
    }
    let switched = edit(&pinned(&research()), &[other]);
    assert_eq!(classified(&research(), &switched).class, Invalid);
}

/// DEC-121 (MC-C36): pinning a mandate whose research agent could admit, with research and every
/// admitting flag cleared and `max_instruments` not raised, is one reducing change, although three
/// of its five paths are increasing on their own rows.
#[test]
fn pinning_a_research_mandate_is_one_reducing_change() {
    let (old, new) = (research(), pinned(&research()));
    let (class, paths, step_up) = verdict(&old, &new);
    assert_eq!((class, step_up), (RiskReducing, false));
    assert_eq!(
        paths,
        vec![
            "/behavior/research",
            "/behavior/signal_models",
            "/universe/max_instruments",
            "/universe/pinned",
            "/universe/pinned_instruments"
        ]
    );
    let (old, new) = (parse(&old), parse(&new));
    let changed = changed_paths(&old, &new).expect("two parsed mandates diff");
    assert!(pinning_switch(&old, &new, &changed).expect("decidable"));
    assert!(
        changed
            .iter()
            .all(|p| PIN_SWITCH_PATHS.contains(&p.as_str()))
    );
    let (class, _, step_up) = verdict(&pinned(&research()), &research());
    assert_eq!(
        (class, step_up),
        (RiskIncreasing, true),
        "unpinning is increasing and needs step-up (MC-C37)"
    );
}

/// Pinning is the switch only as a whole. Each variant below breaks one condition and is then
/// classified path by path (DEC-121: "any other combination of those fields follows the per-path
/// rules"). Four of them are increasing that way: pinning a mandate that had no admitting model
/// (MC-C48), raising `max_instruments`, keeping the research envelope, and changing a sixth path. The
/// fifth, keeping a model admitting, is a document V-037 rejects (a pinned universe with a model
/// that admits), so no valid version reaches it; its reducing verdict is the per-path mechanics on
/// an invalid document, not a change an owner could make. It leaves `signal_models` unchanged, so
/// no increasing path is left: its pinned row is reducing because the old version had an admitting
/// model, and the rest are removals. It is still not the switch, which is what the first assertion
/// pins, and the last assertions pin that it is invalid while the switch's own pinned version is
/// not. Unpinning is increasing on its own row whatever the old version's models were.
#[test]
fn pinning_is_the_switch_only_as_a_whole() {
    let no_agent = edit(
        &research(),
        &[
            ("/behavior/research", Some(Value::Null)),
            (
                "/behavior/signal_models/0/admits_instruments",
                Some(b(false)),
            ),
        ],
    );
    let variants: [(&str, Value, Value, ChangeClass); 5] = [
        (
            "no admitting model before",
            no_agent.clone(),
            pinned(&no_agent),
            RiskIncreasing,
        ),
        (
            "max_instruments raised",
            research(),
            edit(
                &pinned(&research()),
                &[("/universe/max_instruments", Some(i(6)))],
            ),
            RiskIncreasing,
        ),
        (
            "research kept",
            research(),
            edit(
                &pinned(&research()),
                &[(
                    "/behavior/research",
                    Some(obj(vec![
                        ("interval_s", i(3600)),
                        ("cost_cap_usd_per_day", s("5")),
                        ("max_revisions_per_lineage", i(3)),
                    ])),
                )],
            ),
            RiskIncreasing,
        ),
        (
            "a model still admitting",
            research(),
            edit(
                &pinned(&research()),
                &[(
                    "/behavior/signal_models/0/admits_instruments",
                    Some(b(true)),
                )],
            ),
            RiskReducing,
        ),
        (
            "a sixth path",
            research(),
            edit(
                &pinned(&research()),
                &[("/risk/max_daily_loss", Some(s("0.01")))],
            ),
            RiskIncreasing,
        ),
    ];
    for (what, old, new, class) in variants {
        let (old, new) = (parse(&old), parse(&new));
        let changed = changed_paths(&old, &new).expect("two parsed mandates diff");
        assert!(
            !pinning_switch(&old, &new, &changed).expect("decidable"),
            "{what}: not the switch"
        );
        let c = classify(&old, &new).expect("two parsed mandates classify");
        assert_eq!(
            (c.class, c.step_up_required),
            (class, class == RiskIncreasing),
            "{what}"
        );
    }
    let (old, new) = (parse(&research()), parse(&pinned(&research())));
    let changed = changed_paths(&old, &new).expect("two parsed mandates diff");
    assert!(
        pinning_switch(&old, &new, &changed).expect("decidable"),
        "the whole switch is one"
    );
    let kept_admitting = edit(
        &pinned(&research()),
        &[(
            "/behavior/signal_models/0/admits_instruments",
            Some(b(true)),
        )],
    );
    let unpinned = edit(&kept_admitting, &[("/universe/pinned", Some(b(false)))]);
    assert_eq!(
        verdict(&kept_admitting, &unpinned),
        (RiskIncreasing, vec!["/universe/pinned".to_owned()], true),
        "unpinning is increasing even from a version whose model could admit"
    );
    assert!(
        violations(&kept_admitting).contains(&Violation::V037),
        "a pinned universe with a model still admitting is invalid (V-037)"
    );
    assert!(
        !violations(&pinned(&research())).contains(&Violation::V037),
        "the switch's own pinned version clears every admitting flag, so V-037 holds"
    );
}

/// The switch reads the change's own paths. A list that leaves a changed path out, or names one that
/// did not change, is not the switch, so a caller cannot make a change reducing by passing the wrong
/// list (DEC-172 item 2). The same two documents with their own paths are.
#[test]
fn the_switch_needs_the_changes_own_paths() {
    let (old, new) = (parse(&research()), parse(&pinned(&research())));
    let changed = changed_paths(&old, &new).expect("two parsed mandates diff");
    assert!(pinning_switch(&old, &new, &changed).expect("decidable"));
    let fewer: Vec<Pointer> = changed.iter().skip(1).cloned().collect();
    assert!(!pinning_switch(&old, &new, &fewer).expect("decidable"));
    assert!(!pinning_switch(&old, &new, &[]).expect("decidable"));
    let mut more = changed.clone();
    more.push(Pointer::new("/risk/max_daily_loss"));
    assert!(!pinning_switch(&old, &new, &more).expect("decidable"));
    let sixth = parse(&edit(
        &pinned(&research()),
        &[("/risk/max_daily_loss", Some(s("0.01")))],
    ));
    assert!(
        !pinning_switch(&old, &sixth, &changed).expect("decidable"),
        "the five paths, when a sixth also changed"
    );
    assert!(
        !pinning_switch(&old, &old, &[]).expect("decidable"),
        "no change is no switch"
    );
    assert_eq!(
        classify(&old, &new).expect("classifies").class,
        RiskReducing
    );
    assert_eq!(
        classify(&old, &sixth).expect("classifies").class,
        RiskIncreasing
    );
}

/// Turning the research agent off without pinning is increasing (MC-C42): the signal-model row stays
/// fail safe, and only the pinning switch covers the whole mode change.
#[test]
fn turning_research_off_without_pinning_is_increasing() {
    let off = edit(
        &research(),
        &[
            ("/behavior/research", Some(Value::Null)),
            (
                "/behavior/signal_models/0/admits_instruments",
                Some(b(false)),
            ),
        ],
    );
    assert_eq!(
        verdict(&research(), &off),
        (
            RiskIncreasing,
            vec![
                "/behavior/research".to_owned(),
                "/behavior/signal_models".to_owned()
            ],
            true
        )
    );
    assert_eq!(
        verdict(
            &research(),
            &edit(&research(), &[("/behavior/research", Some(Value::Null))])
        )
        .0,
        RiskReducing,
        "clearing research alone is its own row's reduction"
    );
}

/// §9.2's six reducing autonomy shapes, one at a time: a `then` or the default or the admission
/// ceiling made stricter (MC-C11, MC-C46); a rule added that is at least as strict as everything after
/// it (MC-C13); a rule removed when everything after it is at least as strict (MC-C16); a
/// single-comparison `auto` rule narrowed; an `ask` rule widened with nothing stricter after it
/// (MC-C05); and `two_approver_above_usd` set or lowered (MC-C28).
#[test]
fn each_reducing_autonomy_shape_is_reducing() {
    let old = three_rules();
    let without_auto = edit(&old, &[("/autonomy/rules/2", None)]);
    let two = edit(
        &old,
        &[("/autonomy/approval/two_approver_above_usd", Some(s("700")))],
    );
    let reducing: [(&str, &Value, Value); 11] = [
        (
            "a then stricter",
            &old,
            edit(
                &old,
                &[(
                    "/autonomy/rules/2",
                    Some(routine(&["increase", "open"], "ask")),
                )],
            ),
        ),
        (
            "the default stricter",
            &old,
            edit(&old, &[("/autonomy/default", Some(s("deny")))]),
        ),
        (
            "the admission ceiling stricter",
            &old,
            edit(&old, &[("/autonomy/admission", Some(s("deny")))]),
        ),
        (
            "a deny rule added first",
            &old,
            edit(
                &old,
                &[(
                    "/autonomy/rules",
                    Some(arr(vec![
                        rule("no_crypto", "session", "eq", s("crypto"), "deny"),
                        large_orders("900", "ask"),
                        low_score("0.65", "ask"),
                        routine(&["increase", "open"], "auto"),
                    ])),
                )],
            ),
        ),
        ("the auto rule removed", &old, without_auto.clone()),
        (
            "an ask removed before an ask and the default",
            &without_auto,
            edit(&without_auto, &[("/autonomy/rules/0", None)]),
        ),
        (
            "the auto rule narrowed",
            &old,
            edit(
                &old,
                &[("/autonomy/rules/2", Some(routine(&["open"], "auto")))],
            ),
        ),
        (
            "the low-score ask widened",
            &old,
            edit(
                &old,
                &[("/autonomy/rules/1", Some(low_score("0.7", "ask")))],
            ),
        ),
        (
            "the large-order ask widened",
            &old,
            edit(
                &old,
                &[("/autonomy/rules/0", Some(large_orders("800", "ask")))],
            ),
        ),
        ("two approvers set", &old, two.clone()),
        (
            "two approvers lowered",
            &two,
            edit(
                &two,
                &[("/autonomy/approval/two_approver_above_usd", Some(s("500")))],
            ),
        ),
    ];
    for (what, before, new) in reducing {
        assert_eq!(autonomy_class(before, &new), RiskReducing, "{what}");
        let c = classified(before, &new);
        assert_eq!(
            (c.class, c.step_up_required),
            (RiskReducing, false),
            "{what}"
        );
    }
    let looser = edit(&old, &[("/autonomy/default", Some(s("auto")))]);
    assert_eq!(
        autonomy_class(&old, &looser),
        RiskIncreasing,
        "the contrast: the default loosened"
    );
    assert_eq!(classified(&old, &looser).class, RiskIncreasing);
}

/// Everything else in autonomy is increasing (MC-C04, MC-C12, MC-C14, MC-C15, MC-C17, MC-C18,
/// MC-C47): a `then`, the default, or the ceiling loosened; an `ask` rule narrowed or an `auto` rule
/// widened; an `ask` rule widened with a stricter rule after it; a rule added looser than what follows
/// it or removed with something looser after it; a reorder; a new field, operator, or compound
/// condition; the approvers or the timeout changed; a rule renamed; and the two-approver threshold
/// raised or cleared. (`on_timeout` is the schema's `const: skip`, so no document can change it.)
#[test]
fn every_other_autonomy_change_is_increasing() {
    let old = three_rules();
    let deny_last = edit(
        &old,
        &[(
            "/autonomy/rules/2",
            Some(rule("no_crypto", "session", "eq", s("crypto"), "deny")),
        )],
    );
    let two = edit(
        &old,
        &[("/autonomy/approval/two_approver_above_usd", Some(s("700")))],
    );
    let increasing: [(&str, &Value, Value); 18] = [
        (
            "an ask made auto",
            &old,
            edit(
                &old,
                &[("/autonomy/rules/0", Some(large_orders("900", "auto")))],
            ),
        ),
        (
            "the default loosened",
            &old,
            edit(&old, &[("/autonomy/default", Some(s("auto")))]),
        ),
        (
            "the ceiling loosened",
            &old,
            edit(&old, &[("/autonomy/admission", Some(s("auto")))]),
        ),
        (
            "an ask narrowed",
            &old,
            edit(
                &old,
                &[("/autonomy/rules/0", Some(large_orders("950", "ask")))],
            ),
        ),
        (
            "an auto widened",
            &old,
            edit(
                &old,
                &[(
                    "/autonomy/rules/2",
                    Some(routine(&["increase", "open", "risk_exit"], "auto")),
                )],
            ),
        ),
        (
            "an ask widened before a deny",
            &deny_last,
            edit(
                &deny_last,
                &[("/autonomy/rules/1", Some(low_score("0.7", "ask")))],
            ),
        ),
        (
            "an ask added before a deny",
            &deny_last,
            edit(
                &deny_last,
                &[(
                    "/autonomy/rules",
                    Some(arr(vec![
                        rule("ask_all", "order_usd", "gt", s("0"), "ask"),
                        large_orders("900", "ask"),
                        low_score("0.65", "ask"),
                        rule("no_crypto", "session", "eq", s("crypto"), "deny"),
                    ])),
                )],
            ),
        ),
        (
            "an auto added first",
            &old,
            edit(
                &old,
                &[(
                    "/autonomy/rules",
                    Some(arr(vec![
                        rule("all_auto", "order_usd", "gt", s("0"), "auto"),
                        large_orders("900", "ask"),
                        low_score("0.65", "ask"),
                        routine(&["increase", "open"], "auto"),
                    ])),
                )],
            ),
        ),
        (
            "an ask removed before an auto",
            &old,
            edit(&old, &[("/autonomy/rules/1", None)]),
        ),
        (
            "reordered",
            &old,
            edit(
                &old,
                &[(
                    "/autonomy/rules",
                    Some(arr(vec![
                        low_score("0.65", "ask"),
                        large_orders("900", "ask"),
                        routine(&["increase", "open"], "auto"),
                    ])),
                )],
            ),
        ),
        (
            "a field changed",
            &old,
            edit(
                &old,
                &[(
                    "/autonomy/rules/0",
                    Some(rule(
                        "large_orders",
                        "position_usd_after",
                        "gt",
                        s("900"),
                        "ask",
                    )),
                )],
            ),
        ),
        (
            "an operator changed",
            &old,
            edit(
                &old,
                &[(
                    "/autonomy/rules/0",
                    Some(rule("large_orders", "order_usd", "gte", s("900"), "ask")),
                )],
            ),
        ),
        (
            "a compound condition",
            &old,
            edit(
                &old,
                &[(
                    "/autonomy/rules/0/when",
                    Some(obj(vec![(
                        "all",
                        arr(vec![
                            obj(vec![
                                ("field", s("order_usd")),
                                ("op", s("gt")),
                                ("value", s("900")),
                            ]),
                            obj(vec![
                                ("field", s("session")),
                                ("op", s("eq")),
                                ("value", s("crypto")),
                            ]),
                        ]),
                    )])),
                )],
            ),
        ),
        (
            "the approvers changed",
            &old,
            edit(
                &old,
                &[(
                    "/autonomy/approval/approvers",
                    Some(arr(vec![s("role:owner")])),
                )],
            ),
        ),
        (
            "the timeout changed",
            &old,
            edit(&old, &[("/autonomy/approval/timeout_s", Some(i(1200)))]),
        ),
        (
            "two approvers raised",
            &two,
            edit(
                &two,
                &[("/autonomy/approval/two_approver_above_usd", Some(s("900")))],
            ),
        ),
        (
            "two approvers cleared",
            &two,
            edit(
                &two,
                &[(
                    "/autonomy/approval/two_approver_above_usd",
                    Some(Value::Null),
                )],
            ),
        ),
        (
            "a rule renamed",
            &old,
            edit(&old, &[("/autonomy/rules/0/id", Some(s("big_orders")))]),
        ),
    ];
    for (what, before, new) in increasing {
        assert_eq!(autonomy_class(before, &new), RiskIncreasing, "{what}");
        let c = classified(before, &new);
        assert_eq!(
            (c.class, c.step_up_required),
            (RiskIncreasing, true),
            "{what}"
        );
    }
    let stricter = edit(&old, &[("/autonomy/default", Some(s("deny")))]);
    assert_eq!(
        autonomy_class(&old, &stricter),
        RiskReducing,
        "the contrast: the default made stricter"
    );
    assert_eq!(classified(&old, &stricter).class, RiskReducing);
}

/// Two identical rule sets are neutral (DEC-172 item 3), and a rule set holding the same id twice is
/// increasing whatever else changed, because §9.2's shapes identify a rule by its id and no shape can
/// be read off a duplicated one (DEC-172 item 6). The last case is the one a by-id reading gets wrong:
/// both sets repeat the id, so the kept ids agree in order, and every new rule equals the first old rule
/// of its id although a `deny` became an `ask`.
#[test]
fn identical_rules_are_neutral_and_a_repeated_rule_id_is_increasing() {
    let old = autonomy_of(&three_rules());
    assert_eq!(classify_autonomy(&old, &old).expect("classifies"), Neutral);
    let mut stricter = old.clone();
    stricter.default = mandate_domain::AutonomyDecision::Deny;
    assert_eq!(
        classify_autonomy(&old, &stricter).expect("classifies"),
        RiskReducing
    );
    let mut repeated = stricter.clone();
    let first = repeated.rules.first().cloned().expect("three rules");
    repeated.rules.push(first);
    assert_eq!(
        classify_autonomy(&old, &repeated).expect("classifies"),
        RiskIncreasing,
        "the new set repeats an id"
    );
    assert_eq!(
        classify_autonomy(&repeated, &stricter).expect("classifies"),
        RiskIncreasing,
        "the old set repeated an id"
    );
    let mut twice = old.clone();
    twice.default = mandate_domain::AutonomyDecision::Auto;
    twice.rules.truncate(2);
    let first = twice.rules.first().cloned().expect("a first rule");
    if let Some(second) = twice.rules.get_mut(1) {
        second.id = first.id.clone();
        second.then = mandate_domain::AutonomyDecision::Deny;
    }
    let mut hidden = twice.clone();
    if let Some(second) = hidden.rules.get_mut(1) {
        *second = first;
    }
    assert_eq!(
        classify_autonomy(&twice, &hidden).expect("classifies"),
        RiskIncreasing,
        "both sets repeat an id, and the second rule's deny became the first rule's ask: read by id, \
         each new rule matches an old one exactly, while every low-score action falls from deny to auto"
    );
    let rules = three_rules();
    let deny = edit(&rules, &[("/autonomy/default", Some(s("deny")))]);
    assert_eq!(classified(&rules, &deny).class, RiskReducing);
    assert_eq!(classified(&deny, &rules).class, RiskIncreasing);
}

/// One single-path edit the join property may apply, with the class and the reported path §9.2's
/// table gives it, written out by hand rather than computed.
struct Edit {
    path: &'static str,
    value: Value,
    class: ChangeClass,
    reported: &'static str,
}

fn e(path: &'static str, value: Value, class: ChangeClass) -> Edit {
    Edit {
        path,
        value,
        class,
        reported: path,
    }
}

/// Groups of edits, one group per field, so no two edits in a draw touch the same path. Every row of
/// §9.2 that the base can express appears, in each direction it has.
fn slots() -> Vec<Vec<Edit>> {
    vec![
        vec![
            e("/capital/allocation_usd", s("12000"), RiskIncreasing),
            e("/capital/allocation_usd", s("8000"), RiskReducing),
        ],
        vec![
            e("/risk/max_order_usd", s("1200"), RiskIncreasing),
            e("/risk/max_order_usd", s("800"), RiskReducing),
        ],
        vec![
            e("/risk/max_orders_per_day", i(60), RiskIncreasing),
            e("/risk/max_orders_per_day", i(40), RiskReducing),
        ],
        vec![
            e("/risk/hysteresis", s("0.005"), RiskIncreasing),
            e("/risk/hysteresis", s("0.02"), RiskReducing),
        ],
        vec![
            e("/behavior/sizing/entry_threshold", s("0.2"), RiskIncreasing),
            e("/behavior/sizing/entry_threshold", s("0.4"), RiskReducing),
        ],
        vec![e("/name", s("renamed"), Neutral)],
        vec![
            e(
                "/notifications/channels",
                arr(vec![s("email"), s("sms")]),
                Neutral,
            ),
            e(
                "/notifications/channels",
                arr(vec![s("sms")]),
                RiskIncreasing,
            ),
        ],
        vec![e(
            "/notifications/quiet_hours",
            obj(vec![
                ("start", s("23:00")),
                ("end", s("07:00")),
                ("timezone", s("America/New_York")),
            ]),
            Neutral,
        )],
        vec![e("/risk/scale_action", s("trim_to_target"), RiskReducing)],
        vec![e(
            "/behavior/description",
            s("Buy strength."),
            RiskIncreasing,
        )],
        vec![e("/goal/end_date", s("2026-12-31"), RiskReducing)],
        vec![
            Edit {
                path: "/risk/drawdown_ladder/0/at",
                value: s("0.04"),
                class: RiskIncreasing,
                reported: "/risk/drawdown_ladder",
            },
            Edit {
                path: "/risk/drawdown_ladder/0/at",
                value: s("0.02"),
                class: RiskReducing,
                reported: "/risk/drawdown_ladder",
            },
        ],
        vec![
            e("/autonomy/default", s("deny"), RiskReducing),
            e("/autonomy/default", s("auto"), RiskIncreasing),
        ],
        vec![
            e("/protection/stop_distance", s("0.06"), RiskIncreasing),
            e("/protection/stop_distance", s("0.04"), RiskReducing),
        ],
        vec![
            e(
                "/universe/pinned_instruments",
                arr(vec![instrument(ASSET_A, "AAA")]),
                RiskReducing,
            ),
            e(
                "/universe/pinned_instruments",
                arr(vec![
                    instrument(ASSET_A, "AAA"),
                    instrument("7b4a1c2e-3333-4a2b-9c3d-000000000003", "BBB"),
                    instrument(ASSET_C, "CCC"),
                ]),
                RiskIncreasing,
            ),
        ],
        vec![e(
            "/universe/leveraged_etps_enabled",
            b(true),
            RiskIncreasing,
        )],
        vec![e(
            "/autonomy/approval/two_approver_above_usd",
            s("700"),
            RiskReducing,
        )],
        vec![e(
            "/universe/asset_classes",
            arr(vec![s("crypto"), s("us_equity")]),
            RiskIncreasing,
        )],
    ]
}

/// For each slot, no edit (`None`) or the index of one of its edits.
fn draws() -> impl Strategy<Value = Vec<Option<usize>>> {
    let sizes: Vec<usize> = slots().iter().map(Vec::len).collect();
    sizes
        .into_iter()
        .map(|n| prop::option::weighted(0.3, 0..n).boxed())
        .collect::<Vec<_>>()
}

/// The picked edits of one draw.
fn picked(draw: &[Option<usize>]) -> Vec<Edit> {
    slots()
        .into_iter()
        .zip(draw)
        .filter_map(|(mut slot, pick)| pick.map(|k| slot.swap_remove(k)))
        .collect()
}

fn applied(edits: &[Edit]) -> Value {
    let changes: Vec<(&str, Option<Value>)> = edits
        .iter()
        .map(|ed| (ed.path, Some(ed.value.clone())))
        .collect();
    with_all(&changes)
}

/// The oracle's join: a rank per class, the largest wins, nothing is neutral.
fn joined(classes: impl IntoIterator<Item = ChangeClass>) -> ChangeClass {
    let rank = |c: ChangeClass| match c {
        RiskIncreasing => 2,
        RiskReducing => 1,
        _ => 0,
    };
    classes
        .into_iter()
        .max_by_key(|c| rank(*c))
        .filter(|c| rank(*c) > 0)
        .unwrap_or(Neutral)
}

/// 256 cases, the default, with shrinking capped at 256 steps and nothing persisted under `tests/`.
/// A classifier that breaks a property is found within a few cases; the cap keeps its shrinking inside
/// the mutation gate's timeout, which counts a timed-out mutant as a failed run rather than a caught
/// one.
fn runner() -> TestRunner {
    TestRunner::new(ProptestConfig {
        cases: 256,
        max_shrink_iters: 256,
        failure_persistence: None,
        ..ProptestConfig::default()
    })
}

fn run(
    name: &str,
    strategy: impl Strategy<Value = Vec<Option<usize>>>,
    check: impl Fn(Vec<Option<usize>>) -> Result<(), TestCaseError>,
) {
    let mut runner = runner();
    if let Err(failure) = runner.run(&strategy, check) {
        panic!("{name}: {failure}");
    }
}

/// Step-up is required exactly when some changed path increases risk (§9.2's last paragraph): a
/// change with an increasing edit is never classified as not needing step-up, and one without is
/// never asked for it. The oracle is the hand-written class of each edit, never the classifier.
#[test]
fn step_up_is_required_exactly_when_some_changed_path_increases_risk() {
    run("step-up", draws(), |draw| {
        let edits = picked(&draw);
        let increases = edits.iter().any(|ed| ed.class == RiskIncreasing);
        let c = classify(&parse(&base()), &parse(&applied(&edits)))
            .map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
        prop_assert_eq!(c.step_up_required, increases, "{:?}", c);
        prop_assert_eq!(c.class == RiskIncreasing, increases, "{:?}", c);
        Ok(())
    });
}

/// The verdict is the join over the changed paths (DEC-128 item 11): increasing if any path is, else
/// reducing if any is, else neutral; the paths are exactly the edited ones, in pointer order; and the
/// whole change classifies as the join of classifying each edit alone.
#[test]
fn the_classification_is_the_join_over_changed_paths() {
    let old = parse(&base());
    run("join", draws(), |draw| {
        let edits = picked(&draw);
        let c = classify(&old, &parse(&applied(&edits)))
            .map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
        prop_assert_eq!(c.class, joined(edits.iter().map(|ed| ed.class)));
        let reported: BTreeSet<&str> = edits.iter().map(|ed| ed.reported).collect();
        let paths: Vec<&str> = c.changed_paths.iter().map(Pointer::as_str).collect();
        prop_assert_eq!(paths, reported.into_iter().collect::<Vec<_>>());
        let mut alone = Vec::new();
        for ed in &edits {
            let single = classify(&old, &parse(&with(ed.path, Some(ed.value.clone()))))
                .map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
            prop_assert_eq!(single.class, ed.class, "{} alone", ed.path);
            alone.push(single.class);
        }
        prop_assert_eq!(c.class, joined(alone));
        Ok(())
    });
}

/// An allocation as canonical decimal text from a count of cents.
fn dollars(cents: u64) -> String {
    let (whole, part) = (cents / 100, cents % 100);
    match part {
        0 => whole.to_string(),
        p if p % 10 == 0 => format!("{whole}.{}", p / 10),
        p => format!("{whole}.{p:02}"),
    }
}

/// An allocation-only change is classified by its direction alone: raised is increasing and needs
/// step-up, lowered is reducing and does not, and nothing else in the document moves the verdict. The
/// oracle compares the two amounts as integers of cents.
///
/// MI-2 — an applied allocation change never triggers or lifts a limit — is the risk state's, not
/// this function's: `tests/risk.rs` pins it under E6-4 (§5.1), and classification reads no state, so
/// it can trigger or lift nothing (DEC-172 item 7).
#[test]
fn an_allocation_only_change_is_classified_by_its_direction_alone() {
    let mut runner = runner();
    let amounts = (1u64..100_000_000_000, 1u64..100_000_000_000, draws());
    let outcome = runner.run(&amounts, |(was, now, draw)| {
        prop_assume!(was != now);
        let context = picked(&draw);
        let old_doc = edit(
            &applied(&context),
            &[("/capital/allocation_usd", Some(s(&dollars(was))))],
        );
        let new_doc = edit(
            &old_doc,
            &[("/capital/allocation_usd", Some(s(&dollars(now))))],
        );
        let c = classify(&parse(&old_doc), &parse(&new_doc))
            .map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
        let class = if now > was {
            RiskIncreasing
        } else {
            RiskReducing
        };
        prop_assert_eq!(c.class, class, "{} to {}", dollars(was), dollars(now));
        prop_assert_eq!(c.step_up_required, now > was);
        let paths: Vec<&str> = c.changed_paths.iter().map(Pointer::as_str).collect();
        prop_assert_eq!(paths, vec!["/capital/allocation_usd"]);
        Ok(())
    });
    if let Err(failure) = outcome {
        panic!("allocation: {failure}");
    }
}

/// A rule in the MI-11 property's own terms, evaluated by [`decide`] and never by the crate: one
/// comparison with each of §6.3's operator kinds, `gt`, `lt`, `in`, `not_in`, `eq`, and `ne`.
#[derive(Debug, Clone, PartialEq)]
enum When {
    OrderAbove(u32),
    ScoreBelow(u32),
    PurposeIn(Vec<&'static str>),
    PurposeNotIn(Vec<&'static str>),
    SessionIs(&'static str),
    SessionIsNot(&'static str),
}

#[derive(Debug, Clone, PartialEq)]
struct R {
    id: u8,
    when: When,
    then: u8,
}

#[derive(Debug, Clone, PartialEq)]
struct Rules {
    rules: Vec<R>,
    default: u8,
}

const PURPOSES: [&str; 3] = ["increase", "open", "risk_exit"];
const SESSIONS: [&str; 2] = ["crypto", "regular"];
const THENS: [&str; 3] = ["auto", "ask", "deny"];

/// Hundredths as canonical decimal text: 65 is `0.65`, 70 is `0.7`.
fn hundredths(n: u32) -> String {
    match n {
        0 => "0".to_owned(),
        100.. => "1".to_owned(),
        n => format!("0.{n:02}").trim_end_matches('0').to_owned(),
    }
}

fn to_document(rules: &Rules) -> Value {
    let list = rules
        .rules
        .iter()
        .map(|r| {
            let id = format!("r{}", r.id);
            let then = THENS[usize::from(r.then)];
            match &r.when {
                When::OrderAbove(n) => rule(&id, "order_usd", "gt", s(&n.to_string()), then),
                When::ScoreBelow(n) => rule(&id, "combined_score", "lt", s(&hundredths(*n)), then),
                When::PurposeIn(p) => rule(
                    &id,
                    "purpose",
                    "in",
                    arr(p.iter().map(|x| s(x)).collect()),
                    then,
                ),
                When::PurposeNotIn(p) => rule(
                    &id,
                    "purpose",
                    "not_in",
                    arr(p.iter().map(|x| s(x)).collect()),
                    then,
                ),
                When::SessionIs(x) => rule(&id, "session", "eq", s(x), then),
                When::SessionIsNot(x) => rule(&id, "session", "ne", s(x), then),
            }
        })
        .collect();
    with_all(&[
        ("/autonomy/rules", Some(arr(list))),
        (
            "/autonomy/default",
            Some(s(THENS[usize::from(rules.default)])),
        ),
    ])
}

/// One action's facts.
#[derive(Debug, Clone, Copy)]
struct Action {
    order: u32,
    score: u32,
    purpose: &'static str,
    session: &'static str,
}

/// §6.2's first-match evaluation, written from the spec: the first rule whose condition holds decides,
/// and the default decides when none does.
fn decide(rules: &Rules, a: Action) -> u8 {
    rules
        .rules
        .iter()
        .find(|r| match &r.when {
            When::OrderAbove(n) => a.order > *n,
            When::ScoreBelow(n) => a.score < *n,
            When::PurposeIn(p) => p.contains(&a.purpose),
            When::PurposeNotIn(p) => !p.contains(&a.purpose),
            When::SessionIs(x) => a.session == *x,
            When::SessionIsNot(x) => a.session != *x,
        })
        .map_or(rules.default, |r| r.then)
}

/// Every action at, just below, and just above every threshold in either rule set, in every purpose and
/// session: the whole space both rule sets can tell apart.
fn actions(a: &Rules, b: &Rules) -> Vec<Action> {
    let mut orders = BTreeSet::from([0u32]);
    let mut scores = BTreeSet::from([0u32, 100]);
    for r in a.rules.iter().chain(&b.rules) {
        match r.when {
            When::OrderAbove(n) => orders.extend([n.saturating_sub(1), n, n + 1]),
            When::ScoreBelow(n) => scores.extend([n.saturating_sub(1), n, n + 1]),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for &order in &orders {
        for &score in &scores {
            for purpose in PURPOSES {
                for session in SESSIONS {
                    out.push(Action {
                        order,
                        score,
                        purpose,
                        session,
                    });
                }
            }
        }
    }
    out
}

fn when_strategy() -> impl Strategy<Value = When> {
    prop_oneof![
        (1u32..20).prop_map(|n| When::OrderAbove(n * 100)),
        (1u32..99).prop_map(When::ScoreBelow),
        prop::sample::subsequence(PURPOSES.to_vec(), 1..=3).prop_map(When::PurposeIn),
        prop::sample::subsequence(PURPOSES.to_vec(), 1..=3).prop_map(When::PurposeNotIn),
        prop::sample::select(SESSIONS.to_vec()).prop_map(When::SessionIs),
        prop::sample::select(SESSIONS.to_vec()).prop_map(When::SessionIsNot),
    ]
}

fn rules_strategy() -> impl Strategy<Value = Rules> {
    (
        prop::collection::vec((when_strategy(), 0u8..3), 0..5),
        0u8..3,
    )
        .prop_map(|(list, default)| Rules {
            rules: list
                .into_iter()
                .enumerate()
                .map(|(k, (when, then))| R {
                    id: u8::try_from(k).unwrap_or(0),
                    when,
                    then,
                })
                .collect(),
            default,
        })
}

/// A change to a rule set, and whether §9.2 lists it as a reducing shape. The shapes are decided here
/// from the spec's wording and the rule set's own values, never by asking the crate.
///
/// Op 4 moves one value of a kept rule's single comparison: an `auto` rule's so it matches less
/// often (a higher `gt`, a lower `lt`, a shorter `in` list, a longer `not_in` list), and an `ask` or
/// `deny` rule's so it matches more often (the reverse), which is reducing when no later rule or the
/// default is stricter. An `eq` or `ne` value has no direction, so moving it is never a listed shape.
fn changed(old: &Rules, op: u8, k: usize, pick: u32) -> (Rules, bool) {
    let mut new = old.clone();
    let n = old.rules.len();
    let later_max = |rules: &Rules, from: usize| {
        rules
            .rules
            .iter()
            .skip(from)
            .map(|r| r.then)
            .chain([rules.default])
            .max()
            .unwrap_or(0)
    };
    let later_min = |rules: &Rules, from: usize| {
        rules
            .rules
            .iter()
            .skip(from)
            .map(|r| r.then)
            .chain([rules.default])
            .min()
            .unwrap_or(0)
    };
    match (op, n) {
        (0, 1..) => {
            let r = &mut new.rules[k % n];
            if r.then < 2 {
                r.then += 1;
                return (new, true);
            }
            r.then = 0;
            (new, false)
        }
        (1, _) => {
            if new.default < 2 {
                new.default += 1;
                return (new, true);
            }
            new.default = 0;
            (new, false)
        }
        (2, _) => {
            let at = k % (n + 1);
            let then = later_max(old, at);
            new.rules.insert(
                at,
                R {
                    id: 9,
                    when: When::OrderAbove(pick % 20 * 100 + 50),
                    then,
                },
            );
            (new, true)
        }
        (3, 1..) => {
            let at = k % n;
            let removed = new.rules.remove(at);
            (new, later_min(old, at + 1) >= removed.then)
        }
        (4, 1..) => {
            let at = k % n;
            let stricter_after = later_max(old, at + 1) > old.rules[at].then;
            let r = &mut new.rules[at];
            let auto = r.then == 0;
            let widen = !auto;
            match &mut r.when {
                When::OrderAbove(v) => *v = if widen { v.saturating_sub(50) } else { *v + 50 },
                When::ScoreBelow(v) => {
                    *v = if widen {
                        (*v + 1).min(100)
                    } else {
                        v.saturating_sub(1)
                    }
                }
                When::PurposeIn(p) => {
                    if widen {
                        if let Some(extra) = PURPOSES.iter().find(|x| !p.contains(x)) {
                            p.push(extra);
                            p.sort_unstable();
                        }
                    } else if p.len() > 1 {
                        p.pop();
                    }
                }
                When::PurposeNotIn(p) => {
                    if !widen {
                        if let Some(extra) = PURPOSES.iter().find(|x| !p.contains(x)) {
                            p.push(extra);
                            p.sort_unstable();
                        }
                    } else if p.len() > 1 {
                        p.pop();
                    }
                }
                When::SessionIs(x) | When::SessionIsNot(x) => {
                    *x = if *x == SESSIONS[0] {
                        SESSIONS[1]
                    } else {
                        SESSIONS[0]
                    };
                    return (new, false);
                }
            }
            let reducing = new != *old && (auto || !stricter_after);
            (new, reducing)
        }
        (5, 1..) => {
            let r = &mut new.rules[k % n];
            r.then = r.then.saturating_sub(1);
            (new, false)
        }
        (6, 2..) => {
            new.rules.swap(k % n, (k + 1) % n);
            (new, false)
        }
        (7, 1..) => {
            let at = k % n;
            new.rules[at].when = match old.rules[at].when {
                When::SessionIs(_) => When::OrderAbove(pick % 20 * 100),
                _ => When::SessionIs(SESSIONS[usize::try_from(pick % 2).unwrap_or(0)]),
            };
            (new, false)
        }
        (8, 1..) => {
            let at = k % n;
            let r = &mut new.rules[at];
            if r.then == 0 {
                if let When::OrderAbove(v) = &mut r.when {
                    *v = v.saturating_sub(50);
                }
            } else if let When::OrderAbove(v) = &mut r.when {
                *v += 50;
            }
            (new, false)
        }
        _ => {
            new.rules.insert(
                0,
                R {
                    id: 8,
                    when: When::OrderAbove(pick % 20 * 100),
                    then: 0,
                },
            );
            (new, false)
        }
    }
}

/// MI-11: a version classified reducing or neutral never makes any autonomy decision less strict,
/// checked by evaluating every distinguishable action under both rule sets with the property's own
/// evaluator. And each of §9.2's reducing shapes, applied once, is classified reducing — without that
/// half a classifier that called everything increasing would pass.
///
/// 4096 cases rather than [`runner`]'s 256: a single-comparison move (op 4) on one given operator
/// kind is about one draw in two hundred, and a `not_in` rule read like an `in` one must be caught
/// under every seed, not most.
#[test]
fn a_reducing_or_neutral_autonomy_change_never_loosens_a_decision() {
    let mut runner = TestRunner::new(ProptestConfig {
        cases: 4096,
        max_shrink_iters: 256,
        failure_persistence: None,
        ..ProptestConfig::default()
    });
    let draw = (
        rules_strategy(),
        0u8..10,
        0usize..8,
        0u32..1000,
        any::<bool>(),
    );
    let outcome = runner.run(&draw, |(old, op, k, pick, twice)| {
        let (mut new, mut shape) = changed(&old, op, k, pick);
        if twice {
            new = changed(&new, (op + 3) % 10, k + 1, pick + 7).0;
            shape = false;
        }
        let (before, after) = (parse(&to_document(&old)), parse(&to_document(&new)));
        let class = classify_autonomy(&before.autonomy, &after.autonomy)
            .map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
        if shape && new != old {
            prop_assert_eq!(
                class,
                RiskReducing,
                "a listed shape: {:?} to {:?}",
                old,
                new
            );
        }
        if matches!(class, RiskReducing | Neutral) {
            for a in actions(&old, &new) {
                prop_assert!(
                    decide(&new, a) >= decide(&old, a),
                    "{:?} loosened {:?} from {} to {} ({:?} to {:?})",
                    class,
                    a,
                    decide(&old, a),
                    decide(&new, a),
                    old,
                    new
                );
            }
        }
        prop_assert_eq!(
            classify(&before, &after)
                .map_err(|e| TestCaseError::fail(format!("{e:?}")))?
                .class,
            class,
            "the autonomy row is the whole mandate's verdict when only autonomy changed"
        );
        Ok(())
    });
    if let Err(failure) = outcome {
        panic!("MI-11: {failure}");
    }
}

/// [`decide`] is the oracle the MI-11 property trusts, so it is checked on its own: the first match
/// decides, and the default decides when nothing matches.
#[test]
fn the_mi11_oracle_is_first_match_then_default() {
    let rules = Rules {
        rules: vec![
            R {
                id: 0,
                when: When::OrderAbove(900),
                then: 1,
            },
            R {
                id: 1,
                when: When::SessionIs("crypto"),
                then: 2,
            },
            R {
                id: 2,
                when: When::PurposeIn(vec!["open"]),
                then: 0,
            },
        ],
        default: 1,
    };
    let a = |order, session, purpose| Action {
        order,
        score: 50,
        purpose,
        session,
    };
    assert_eq!(
        decide(&rules, a(1000, "crypto", "open")),
        1,
        "the first match"
    );
    assert_eq!(decide(&rules, a(100, "crypto", "open")), 2);
    assert_eq!(decide(&rules, a(100, "regular", "open")), 0);
    assert_eq!(
        decide(&rules, a(100, "regular", "increase")),
        1,
        "the default"
    );
    let negated = Rules {
        rules: vec![
            R {
                id: 0,
                when: When::SessionIsNot("crypto"),
                then: 2,
            },
            R {
                id: 1,
                when: When::PurposeNotIn(vec!["increase", "open"]),
                then: 0,
            },
        ],
        default: 1,
    };
    assert_eq!(decide(&negated, a(100, "regular", "open")), 2, "ne");
    assert_eq!(decide(&negated, a(100, "crypto", "risk_exit")), 0, "not_in");
    assert_eq!(
        decide(&negated, a(100, "crypto", "open")),
        1,
        "neither holds"
    );
    assert_eq!(hundredths(65), "0.65");
    assert_eq!(hundredths(70), "0.7");
    assert_eq!(hundredths(5), "0.05");
    assert_eq!(hundredths(0), "0");
    assert_eq!(hundredths(100), "1");
    assert_eq!(dollars(1_000_000), "10000");
    assert_eq!(dollars(1_050), "10.5");
    assert_eq!(dollars(1_005), "10.05");
}
