//! Each rule of the slice on the mandate that breaks it and nothing else, each boundary on both sides,
//! the readings DEC-161 records, and three properties whose oracles are computed apart from the rules.
//!
//! `tests/validate.rs` and the 67 MC-V cases cover the spec's examples. These pin what they do not: every
//! branch of every rule, so a rule weakened in any one branch fails by name, which is what the diff's
//! mutation gate needs to see.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::{Digest, Value};
use mandate_domain::{AssetId, Environment};
use mandate_num::{NumError, Usd};
use mandate_time::Date;
use proptest::prelude::*;

use super::{
    GroupId, PreviousVersion, RegisteredModel, ValidationContext, ValidationReport, Violation,
    Warning, covers, pointer, recheck_at_application, validate,
};
use crate::document::{ConnectionId, ModelId, Pointer, Provenance, ProvenanceMap, Source};
use crate::{Mandate, ParseError, SpecError};

type Checked = Result<(), String>;
/// `(pointer, JSON text)` pairs, applied in order.
type Patch<'a> = &'a [(&'a str, &'a str)];
type Owned<'a> = Vec<(&'a str, &'a str)>;
type Entries<'a> = &'a [(&'a str, Source, bool)];

const ASSET_A: &str = "7b4a1c2e-2222-4a2b-9c3d-000000000002";
const ASSET_B: &str = "7b4a1c2e-3333-4a2b-9c3d-000000000003";
const HASH: &str = "2222222222222222222222222222222222222222222222222222222222222222";

/// A mandate that breaks no rule: two pinned equities, protection on with no crypto, one registered
/// model, a three-rung ladder, one rule, quiet hours, and an end date after the validation date.
const BASE: &str = r#"{
  "mandate_schema_version": 1, "name": "two-stock-swing", "source_text_ref": null,
  "environment": "paper", "connection_id": "conn_alpaca_paper_01",
  "capital": {"allocation_usd": "10000", "max_loss_from_allocation": "0.1"},
  "goal": {"type": "continuous", "end_date": "2026-12-31", "on_complete": "hold_protected"},
  "universe": {"pinned": true, "pinned_instruments": [
      {"asset_id": "7b4a1c2e-2222-4a2b-9c3d-000000000002", "symbol": "XYZ", "asset_class": "us_equity"},
      {"asset_id": "7b4a1c2e-3333-4a2b-9c3d-000000000003", "symbol": "QRS", "asset_class": "us_equity"}],
    "max_instruments": 2, "asset_classes": ["us_equity"], "leveraged_etps_enabled": false,
    "leveraged_etp_disclosure_version": null},
  "behavior": {"description": "Swing two stocks.",
    "signal_models": [{"id": "quant.momentum", "version": "1.0.0",
      "content_hash": "sha256:2222222222222222222222222222222222222222222222222222222222222222",
      "params": [{"key": "lookback_bars", "value": "20"}, {"key": "z_entry", "value": "2"}],
      "weight": "1", "max_output_age_s": 1800, "admits_instruments": false}],
    "research": null,
    "cadence": {"interval_s": 900, "event_sources": ["fills", "price"]},
    "sizing": {"method": "conviction_linear", "entry_threshold": "0.3", "exit_threshold": "0.3",
      "rebalance_band": "0.05"}},
  "protection": {"enabled": true, "stop_distance": "0.05", "take_profit_distance": "0.1",
    "crypto_stop_limit_offset": null},
  "risk": {"max_position_usd": "1500", "max_position_fraction": "0.5", "max_gross_exposure_usd": "3000",
    "max_order_usd": "1000", "max_orders_per_day": 20, "max_daily_loss": "0.02",
    "daily_loss_action": "exits_only", "max_drawdown": "0.08",
    "drawdown_ladder": [{"at": "0.02", "action": "scale_sizes", "factor": "0.5"},
      {"at": "0.05", "action": "exits_only", "factor": null},
      {"at": "0.08", "action": "flatten_and_pause", "factor": null}],
    "hysteresis": "0.01", "scale_action": "limit_buys", "breach_confirm_s": 60,
    "daily_breach_min_s": 3600, "scale_lift_after_s": 600, "reentry_cooldown_s": 3600},
  "autonomy": {"rules": [
      {"id": "large_orders", "when": {"field": "order_usd", "op": "gt", "value": "900"}, "then": "ask"}],
    "default": "ask", "admission": "ask",
    "approval": {"timeout_s": 600, "on_timeout": "skip", "approvers": ["role:approver", "role:owner"],
      "two_approver_above_usd": null}},
  "notifications": {"channels": ["email", "phone"],
    "quiet_hours": {"start": "22:00", "end": "07:00", "timezone": "America/New_York"}}
}"#;

fn json(text: &str) -> Result<Value, String> {
    mandate_canon::parse(text.as_bytes()).map_err(|e| format!("{text}: {e:?}"))
}

/// The base with each `(pointer, JSON text)` written in turn: a member or an index is replaced, and
/// `-` appends to an array.
fn document(patches: &[(&str, &str)]) -> Result<Value, String> {
    let mut document = json(BASE)?;
    for (path, text) in patches {
        let value = json(text)?;
        let tokens: Vec<&str> = path.split('/').skip(1).collect();
        let (last, parents) = tokens.split_last().ok_or("an empty pointer")?;
        let mut node = &mut document;
        for token in parents {
            node = match node {
                Value::Object(members) => members
                    .iter_mut()
                    .find(|(key, _)| key.as_str() == *token)
                    .map(|(_, child)| child),
                Value::Array(items) => token.parse::<usize>().ok().and_then(|n| items.get_mut(n)),
                _ => None,
            }
            .ok_or_else(|| format!("`{path}` does not resolve"))?;
        }
        match node {
            Value::Object(members) => {
                let slot = members
                    .iter_mut()
                    .find(|(key, _)| key.as_str() == *last)
                    .map(|(_, child)| child)
                    .ok_or_else(|| format!("`{path}` names no member"))?;
                *slot = value;
            }
            Value::Array(items) if *last == "-" => items.push(value),
            Value::Array(items) => {
                let slot = last
                    .parse::<usize>()
                    .ok()
                    .and_then(|n| items.get_mut(n))
                    .ok_or_else(|| format!("`{path}` names no item"))?;
                *slot = value;
            }
            _ => return Err(format!("`{path}` is not inside a container")),
        }
    }
    Ok(document)
}

pub(crate) fn mandate(patches: &[(&str, &str)]) -> Result<Mandate, String> {
    Mandate::parse(&document(patches)?).map_err(|e| format!("{patches:?}: {e}"))
}

fn digest(hex: &str) -> Result<Digest, String> {
    Digest::from_hex(hex).ok_or_else(|| format!("`{hex}` is not a digest"))
}

fn asset(text: &str) -> Result<AssetId, String> {
    AssetId::parse(text).map_err(|e| e.to_string())
}

fn usd(text: &str) -> Result<Usd, String> {
    Usd::parse(text).map_err(|e| e.to_string())
}

/// The context the base breaks nothing in: the base's one model registered as it is written.
pub(crate) fn context() -> Result<ValidationContext, String> {
    let model = RegisteredModel {
        version: "1.0.0".to_owned(),
        content_hash: digest(HASH)?,
        params: BTreeSet::from(["lookback_bars".to_owned(), "z_entry".to_owned()]),
        admits_instruments: false,
    };
    let id = ModelId::parse("quant.momentum").map_err(|e| e.to_string())?;
    Ok(ValidationContext {
        account_equity_usd: usd("25000")?,
        other_allocations_usd: Usd::ZERO,
        validation_date: Date::parse("2026-09-24").map_err(|e| e.to_string())?,
        registry: Some(BTreeMap::from([(id, model)])),
        provenance: ProvenanceMap::default(),
        workspace_users: 1,
        approver_users: 1,
        independent_approval_required: false,
        disclosures_accepted: BTreeSet::new(),
        instrument_groups: BTreeMap::new(),
        claimed_by_other_agents: BTreeSet::new(),
        connection_environment: None,
        connection_loss_carry_usd: Usd::ZERO,
        eligibility_failures: BTreeSet::new(),
        previous_version: None,
        current_mandate_version: None,
    })
}

fn provenance(entries: &[(&str, Source, bool)]) -> ProvenanceMap {
    ProvenanceMap::new(
        entries
            .iter()
            .map(|(path, source, confirmed)| {
                (
                    Pointer::new(path),
                    Provenance {
                        source: *source,
                        confirmed: *confirmed,
                    },
                )
            })
            .collect(),
    )
}

fn report(patches: &[(&str, &str)], ctx: &ValidationContext) -> Result<ValidationReport, String> {
    validate(&mandate(patches)?, ctx).map_err(|e| format!("{patches:?}: {e}"))
}

fn codes(patches: &[(&str, &str)], ctx: &ValidationContext) -> Result<BTreeSet<Violation>, String> {
    report(patches, ctx).map(|r| r.violations)
}

/// Asserts each row reports exactly the codes it states, and names the row that does not.
fn expect_rows(rows: &[(&str, Patch<'_>, &[Violation])]) -> Checked {
    let ctx = context()?;
    for (name, patches, expected) in rows {
        let got = codes(patches, &ctx)?;
        let wanted: BTreeSet<Violation> = expected.iter().copied().collect();
        if got != wanted {
            return Err(format!("{name}: expected {wanted:?}, got {got:?}"));
        }
    }
    Ok(())
}

#[test]
fn the_base_breaks_nothing_and_warns_of_nothing() -> Checked {
    let report = report(&[], &context()?)?;
    if !report.violations.is_empty() || !report.warnings.is_empty() {
        return Err(format!("the base is not clean: {report:?}"));
    }
    Ok(())
}

/// Every branch of every document rule, each on a mandate that breaks that branch and nothing else.
#[test]
fn each_document_rule_fires_on_each_of_its_branches_and_nothing_else() -> Checked {
    let unsorted_pins = r#"[
      {"asset_id": "7b4a1c2e-3333-4a2b-9c3d-000000000003", "symbol": "QRS", "asset_class": "us_equity"},
      {"asset_id": "7b4a1c2e-2222-4a2b-9c3d-000000000002", "symbol": "XYZ", "asset_class": "us_equity"}]"#;
    let repeated_pin = r#"[
      {"asset_id": "7b4a1c2e-2222-4a2b-9c3d-000000000002", "symbol": "XYZ", "asset_class": "us_equity"},
      {"asset_id": "7b4a1c2e-2222-4a2b-9c3d-000000000002", "symbol": "XYZW", "asset_class": "us_equity"}]"#;
    let second_model = r#"{"id": "quant.carry", "version": "1.0.0",
      "content_hash": "sha256:2222222222222222222222222222222222222222222222222222222222222222",
      "params": [], "weight": "1", "max_output_age_s": 1800, "admits_instruments": false}"#;
    let rule = |id: &str, when: &str, then: &str| {
        format!(r#"{{"id": "{id}", "when": {when}, "then": "{then}"}}"#)
    };
    let two_scale_rungs = |lower: &str, upper: &str| {
        format!(
            r#"[{{"at": "0.02", "action": "scale_sizes", "factor": "{lower}"}},
                {{"at": "0.03", "action": "scale_sizes", "factor": "{upper}"}},
                {{"at": "0.05", "action": "exits_only", "factor": null}},
                {{"at": "0.08", "action": "flatten_and_pause", "factor": null}}]"#
        )
    };
    let v040_past = two_scale_rungs("0.123456", "0.1234567");
    let v040_fits = two_scale_rungs("0.123456", "0.123456");
    let v040_cancelling = two_scale_rungs("0.0000000008192", "0.1220703125");
    let second_rule = rule(
        "large_orders",
        r#"{"field": "order_usd", "op": "gt", "value": "5"}"#,
        "ask",
    );
    let deep = rule(
        "deep",
        r#"{"all": [{"any": [{"not": {"all": [{"field": "order_usd", "op": "gt", "value": "9"}]}}]}]}"#,
        "ask",
    );
    let at_four = rule(
        "at_four",
        r#"{"all": [{"any": [{"not": {"field": "order_usd", "op": "gt", "value": "9"}}]}]}"#,
        "ask",
    );
    let unusual = rule(
        "unusual",
        r#"{"field": "unusual_input", "op": "eq", "value": true}"#,
        "ask",
    );
    let crypto = r#"["crypto", "us_equity"]"#;
    let v = |c| vec![c];
    let rows: Vec<(&str, Owned<'_>, Vec<Violation>)> = vec![
        (
            "V-008 crypto with no offset",
            vec![("/universe/asset_classes", crypto)],
            v(Violation::V008),
        ),
        (
            "V-008 crypto with an offset",
            vec![
                ("/universe/asset_classes", crypto),
                ("/protection/crypto_stop_limit_offset", r#""0.005""#),
            ],
            vec![],
        ),
        (
            "V-008 disabled with a stop",
            vec![
                ("/protection/enabled", "false"),
                ("/protection/take_profit_distance", "null"),
            ],
            v(Violation::V008),
        ),
        (
            "V-008 disabled with a take-profit",
            vec![
                ("/protection/enabled", "false"),
                ("/protection/stop_distance", "null"),
            ],
            v(Violation::V008),
        ),
        (
            "V-008 disabled with an offset",
            vec![
                ("/protection/enabled", "false"),
                ("/protection/stop_distance", "null"),
                ("/protection/take_profit_distance", "null"),
                ("/protection/crypto_stop_limit_offset", r#""0.005""#),
            ],
            v(Violation::V008),
        ),
        (
            "V-008 disabled and clear",
            vec![
                ("/protection/enabled", "false"),
                ("/protection/stop_distance", "null"),
                ("/protection/take_profit_distance", "null"),
            ],
            vec![],
        ),
        (
            "V-009 a pin repeated under another symbol",
            vec![("/universe/pinned_instruments", repeated_pin)],
            v(Violation::V009),
        ),
        (
            "V-009 pins",
            vec![("/universe/pinned_instruments", unsorted_pins)],
            v(Violation::V009),
        ),
        (
            "V-009 models, the second unregistered",
            vec![("/behavior/signal_models/-", second_model)],
            vec![Violation::V007, Violation::V009],
        ),
        (
            "V-009 params",
            vec![(
                "/behavior/signal_models/0/params",
                r#"[{"key": "z_entry", "value": "2"}, {"key": "lookback_bars", "value": "20"}]"#,
            )],
            vec![Violation::V007, Violation::V009],
        ),
        (
            "V-009 approvers",
            vec![(
                "/autonomy/approval/approvers",
                r#"["role:owner", "role:approver"]"#,
            )],
            v(Violation::V009),
        ),
        (
            "V-009 sources",
            vec![("/behavior/cadence/event_sources", r#"["price", "fills"]"#)],
            v(Violation::V009),
        ),
        (
            "V-009 channels",
            vec![("/notifications/channels", r#"["phone", "email"]"#)],
            v(Violation::V009),
        ),
        (
            "V-009 classes",
            vec![
                ("/universe/asset_classes", r#"["us_equity", "crypto"]"#),
                ("/protection/crypto_stop_limit_offset", r#""0.005""#),
            ],
            v(Violation::V009),
        ),
        (
            "V-009 rule ids",
            vec![("/autonomy/rules/-", &second_rule)],
            v(Violation::V009),
        ),
        (
            "V-010 equal rungs",
            vec![("/risk/drawdown_ladder/1/at", r#""0.02""#)],
            v(Violation::V010),
        ),
        (
            "V-010 severity",
            vec![
                (
                    "/risk/drawdown_ladder/0",
                    r#"{"at": "0.02", "action": "exits_only", "factor": null}"#,
                ),
                (
                    "/risk/drawdown_ladder/1",
                    r#"{"at": "0.05", "action": "scale_sizes", "factor": "0.5"}"#,
                ),
            ],
            v(Violation::V010),
        ),
        (
            "V-010 scale with no factor",
            vec![("/risk/drawdown_ladder/0/factor", "null")],
            v(Violation::V010),
        ),
        (
            "V-010 factor on exits_only",
            vec![("/risk/drawdown_ladder/1/factor", r#""0.5""#)],
            v(Violation::V010),
        ),
        (
            "V-040 one factor of 13 places",
            vec![("/risk/drawdown_ladder/0/factor", r#""0.1234567890123""#)],
            v(Violation::V040),
        ),
        (
            "V-040 one factor of 12 places",
            vec![("/risk/drawdown_ladder/0/factor", r#""0.123456789012""#)],
            vec![],
        ),
        (
            "V-040 two factors of 13 places together",
            vec![("/risk/drawdown_ladder", &v040_past)],
            v(Violation::V040),
        ),
        (
            "V-040 two factors of 12 places together",
            vec![("/risk/drawdown_ladder", &v040_fits)],
            vec![],
        ),
        (
            "V-040 is a sum, not the whole product's places",
            vec![("/risk/drawdown_ladder", &v040_cancelling)],
            v(Violation::V040),
        ),
        (
            "V-040 reads only scale rungs",
            vec![(
                "/risk/drawdown_ladder/1/factor",
                r#""0.1234567890123456789012345""#,
            )],
            v(Violation::V010),
        ),
        (
            "V-011 two flattens",
            vec![(
                "/risk/drawdown_ladder/1",
                r#"{"at": "0.05", "action": "flatten_and_pause", "factor": null}"#,
            )],
            v(Violation::V011),
        ),
        (
            "V-011 last is not a flatten",
            vec![
                (
                    "/risk/drawdown_ladder/1",
                    r#"{"at": "0.05", "action": "flatten_and_pause", "factor": null}"#,
                ),
                (
                    "/risk/drawdown_ladder/2",
                    r#"{"at": "0.08", "action": "exits_only", "factor": null}"#,
                ),
            ],
            vec![Violation::V010, Violation::V011],
        ),
        (
            "V-011 at off max_drawdown",
            vec![("/risk/max_drawdown", r#""0.09""#)],
            v(Violation::V011),
        ),
        (
            "V-012 at the first rung",
            vec![("/risk/hysteresis", r#""0.02""#)],
            v(Violation::V012),
        ),
        (
            "V-012 just below",
            vec![("/risk/hysteresis", r#""0.0199""#)],
            vec![],
        ),
        (
            "V-013 order above position",
            vec![("/risk/max_order_usd", r#""1500.01""#)],
            v(Violation::V013),
        ),
        (
            "V-013 order at position",
            vec![("/risk/max_order_usd", r#""1500""#)],
            vec![],
        ),
        (
            "V-013 position above gross",
            vec![("/risk/max_position_usd", r#""3000.5""#)],
            v(Violation::V013),
        ),
        (
            "V-013 gross above allocation",
            vec![("/risk/max_gross_exposure_usd", r#""10001""#)],
            v(Violation::V013),
        ),
        (
            "V-013 gross at allocation",
            vec![("/risk/max_gross_exposure_usd", r#""10000""#)],
            vec![],
        ),
        (
            "V-014 floor below drawdown",
            vec![("/capital/max_loss_from_allocation", r#""0.0799""#)],
            v(Violation::V014),
        ),
        (
            "V-014 floor at drawdown",
            vec![("/capital/max_loss_from_allocation", r#""0.08""#)],
            vec![],
        ),
        (
            "V-015 no such day",
            vec![("/goal/end_date", r#""2027-02-29""#)],
            v(Violation::V015),
        ),
        (
            "V-015 a leap day",
            vec![("/goal/end_date", r#""2028-02-29""#)],
            vec![],
        ),
        ("V-015 no end", vec![("/goal/end_date", "null")], vec![]),
        (
            "V-016 start is end",
            vec![("/notifications/quiet_hours/end", r#""22:00""#)],
            v(Violation::V016),
        ),
        (
            "V-016 no quiet hours",
            vec![("/notifications/quiet_hours", "null")],
            vec![],
        ),
        (
            "V-017 depth five",
            vec![("/autonomy/rules/-", &deep)],
            v(Violation::V017),
        ),
        (
            "V-017 depth four",
            vec![("/autonomy/rules/-", &at_four)],
            vec![],
        ),
        (
            "V-018 unusual_input",
            vec![("/autonomy/rules/-", &unusual)],
            v(Violation::V018),
        ),
    ];
    let rows: Vec<(&str, Patch<'_>, &[Violation])> = rows
        .iter()
        .map(|(name, patches, expected)| (*name, patches.as_slice(), expected.as_slice()))
        .collect();
    expect_rows(&rows)
}

/// V-023 over every field kind and operator group §6.3 gives, and the two readings DEC-161 adds: a
/// `thesis_confidence` outside [0, 1], and a decimal wider than the order path can compare.
#[test]
fn v023_types_every_comparison_by_its_field() -> Checked {
    let ok: &[&str] = &[
        r#"{"field": "purpose", "op": "in", "value": ["increase", "open"]}"#,
        r#"{"field": "purpose", "op": "eq", "value": "open"}"#,
        r#"{"field": "session", "op": "not_in", "value": ["after_hours", "crypto", "pre_market"]}"#,
        r#"{"field": "session", "op": "ne", "value": "regular"}"#,
        r#"{"field": "asset_class", "op": "eq", "value": "crypto"}"#,
        r#"{"field": "asset_class", "op": "in", "value": ["us_equity"]}"#,
        r#"{"field": "instrument", "op": "eq", "value": "anything at all"}"#,
        r#"{"field": "instrument", "op": "in", "value": ["a", "b"]}"#,
        r#"{"field": "new_instrument", "op": "eq", "value": true}"#,
        r#"{"field": "first_trade_in_instrument", "op": "ne", "value": false}"#,
        r#"{"field": "order_usd", "op": "lte", "value": "-5"}"#,
        r#"{"field": "order_usd", "op": "gt", "value": "2000"}"#,
        r#"{"field": "combined_score", "op": "gte", "value": "1"}"#,
        r#"{"field": "combined_score", "op": "lt", "value": "0"}"#,
        r#"{"field": "drawdown", "op": "gt", "value": "0.5"}"#,
        r#"{"field": "thesis_confidence", "op": "lt", "value": "0.6"}"#,
        r#"{"field": "daily_pnl_fraction", "op": "lt", "value": "-2"}"#,
        r#"{"field": "bought_today_usd", "op": "ne", "value": "0.000000000000000000000001"}"#,
    ];
    let broken: &[&str] = &[
        r#"{"field": "purpose", "op": "in", "value": ["open", "risk_exit"]}"#,
        r#"{"field": "purpose", "op": "ne", "value": "risk_exit"}"#,
        r#"{"field": "purpose", "op": "eq", "value": ["open"]}"#,
        r#"{"field": "purpose", "op": "in", "value": "open"}"#,
        r#"{"field": "purpose", "op": "gt", "value": "open"}"#,
        r#"{"field": "purpose", "op": "eq", "value": true}"#,
        r#"{"field": "session", "op": "eq", "value": "overnight"}"#,
        r#"{"field": "asset_class", "op": "in", "value": ["us_equity", "option"]}"#,
        r#"{"field": "instrument", "op": "lt", "value": "a"}"#,
        r#"{"field": "new_instrument", "op": "gt", "value": true}"#,
        r#"{"field": "new_instrument", "op": "in", "value": ["true"]}"#,
        r#"{"field": "new_instrument", "op": "eq", "value": "true"}"#,
        r#"{"field": "order_usd", "op": "in", "value": ["900"]}"#,
        r#"{"field": "order_usd", "op": "in", "value": "2000"}"#,
        r#"{"field": "order_usd", "op": "not_in", "value": "2000"}"#,
        r#"{"field": "order_usd", "op": "gt", "value": "900.0"}"#,
        r#"{"field": "order_usd", "op": "gt", "value": "lots"}"#,
        r#"{"field": "order_usd", "op": "eq", "value": false}"#,
        r#"{"field": "combined_score", "op": "gt", "value": "1.01"}"#,
        r#"{"field": "combined_score", "op": "gt", "value": "-0.1"}"#,
        r#"{"field": "drawdown", "op": "gt", "value": "2"}"#,
        r#"{"field": "thesis_confidence", "op": "lt", "value": "1.5"}"#,
        r#"{"field": "bought_today_usd", "op": "ne", "value": "0.0000000000000000000000001"}"#,
    ];
    let ctx = context()?;
    for (comparisons, expected) in [
        (ok, BTreeSet::new()),
        (broken, BTreeSet::from([Violation::V023])),
    ] {
        for when in comparisons {
            let rule = format!(r#"{{"id": "typed", "when": {when}, "then": "ask"}}"#);
            let got = codes(&[("/autonomy/rules/-", &rule)], &ctx)?;
            if got != expected {
                return Err(format!("{when}: expected {expected:?}, got {got:?}"));
            }
        }
    }
    Ok(())
}

/// The rules that read the context, each on both sides of its boundary.
#[test]
fn each_context_rule_fires_on_its_side_of_the_boundary() -> Checked {
    let base = context()?;
    let with =
        |change: &dyn Fn(&mut ValidationContext) -> Checked| -> Result<ValidationContext, String> {
            let mut ctx = base.clone();
            change(&mut ctx)?;
            Ok(ctx)
        };
    let leveraged = [
        ("/universe/leveraged_etps_enabled", "true"),
        (
            "/universe/leveraged_etp_disclosure_version",
            r#""sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb""#,
        ),
    ];
    let accepted = |hex: &'static str| {
        with(&move |c| {
            c.disclosures_accepted = BTreeSet::from([digest(hex)?]);
            Ok(())
        })
    };
    let bs = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let cs = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    let two_approvers = [("/autonomy/approval/two_approver_above_usd", r#""700""#)];
    let rows: Vec<(&str, Owned<'_>, ValidationContext, Vec<Violation>)> = vec![
        (
            "V-001 another environment",
            vec![],
            with(&|c| {
                c.connection_environment = Some(Environment::Live);
                Ok(())
            })?,
            vec![Violation::V001],
        ),
        (
            "V-001 the same environment",
            vec![],
            with(&|c| {
                c.connection_environment = Some(Environment::Paper);
                Ok(())
            })?,
            vec![],
        ),
        (
            "V-002 one cent over",
            vec![],
            with(&|c| {
                c.other_allocations_usd = usd("15000.01")?;
                Ok(())
            })?,
            vec![Violation::V002],
        ),
        (
            "V-002 exactly equal",
            vec![],
            with(&|c| {
                c.other_allocations_usd = usd("15000")?;
                Ok(())
            })?,
            vec![],
        ),
        (
            "V-005 no disclosure accepted",
            leveraged.to_vec(),
            base.clone(),
            vec![Violation::V005],
        ),
        (
            "V-005 another version accepted",
            leveraged.to_vec(),
            accepted(cs)?,
            vec![Violation::V005],
        ),
        (
            "V-005 that version accepted",
            leveraged.to_vec(),
            accepted(bs)?,
            vec![],
        ),
        (
            "V-005 enabled with no version",
            vec![("/universe/leveraged_etps_enabled", "true")],
            accepted(bs)?,
            vec![Violation::V005],
        ),
        (
            "V-006 the instrument itself",
            vec![],
            with(&|c| {
                c.claimed_by_other_agents = BTreeSet::from([asset(ASSET_B)?]);
                Ok(())
            })?,
            vec![Violation::V006],
        ),
        (
            "V-006 another instrument of its group",
            vec![],
            with(&|c| {
                let other = asset("7b4a1c2e-4444-4a2b-9c3d-000000000004")?;
                c.instrument_groups = BTreeMap::from([
                    (asset(ASSET_A)?, GroupId::new("grp")),
                    (other.clone(), GroupId::new("grp")),
                ]);
                c.claimed_by_other_agents = BTreeSet::from([other]);
                Ok(())
            })?,
            vec![Violation::V006],
        ),
        (
            "V-006 an ungrouped instrument elsewhere",
            vec![],
            with(&|c| {
                c.claimed_by_other_agents =
                    BTreeSet::from([asset("7b4a1c2e-4444-4a2b-9c3d-000000000004")?]);
                Ok(())
            })?,
            vec![],
        ),
        (
            "V-006 a group whose id spells a pinned asset id (DEC-161)",
            vec![],
            with(&|c| {
                let other = asset("7b4a1c2e-4444-4a2b-9c3d-000000000004")?;
                c.instrument_groups = BTreeMap::from([(other.clone(), GroupId::new(ASSET_A))]);
                c.claimed_by_other_agents = BTreeSet::from([other]);
                Ok(())
            })?,
            vec![],
        ),
        (
            "V-007 an unregistered id",
            vec![],
            with(&|c| {
                c.registry = Some(BTreeMap::new());
                Ok(())
            })?,
            vec![Violation::V007],
        ),
        (
            "V-007 another version",
            vec![("/behavior/signal_models/0/version", r#""1.0.1""#)],
            base.clone(),
            vec![Violation::V007],
        ),
        (
            "V-007 another hash",
            vec![(
                "/behavior/signal_models/0/content_hash",
                r#""sha256:9999999999999999999999999999999999999999999999999999999999999999""#,
            )],
            base.clone(),
            vec![Violation::V007],
        ),
        (
            "V-007 a parameter short",
            vec![(
                "/behavior/signal_models/0/params",
                r#"[{"key": "lookback_bars", "value": "20"}]"#,
            )],
            base.clone(),
            vec![Violation::V007],
        ),
        (
            "V-007 no registry to compare with",
            vec![("/behavior/signal_models/0/version", r#""1.0.1""#)],
            with(&|c| {
                c.registry = None;
                Ok(())
            })?,
            vec![],
        ),
        (
            "V-024 no approver",
            vec![],
            with(&|c| {
                c.approver_users = 0;
                Ok(())
            })?,
            vec![Violation::V024],
        ),
        (
            "V-024 two needed, one there",
            two_approvers.to_vec(),
            base.clone(),
            vec![Violation::V024],
        ),
        (
            "V-024 two needed, two there",
            two_approvers.to_vec(),
            with(&|c| {
                c.approver_users = 2;
                Ok(())
            })?,
            vec![],
        ),
        (
            "V-030 a day before",
            vec![("/goal/end_date", r#""2026-09-23""#)],
            base.clone(),
            vec![Violation::V030],
        ),
        (
            "V-030 the day itself",
            vec![("/goal/end_date", r#""2026-09-24""#)],
            base.clone(),
            vec![],
        ),
        (
            "V-031 another environment",
            vec![],
            with(&|c| {
                c.previous_version = Some(PreviousVersion {
                    environment: Environment::Live,
                    connection_id: ConnectionId::parse("conn_alpaca_paper_01")
                        .map_err(|e| e.to_string())?,
                    mandate: None,
                    mandate_version: None,
                });
                Ok(())
            })?,
            vec![Violation::V031],
        ),
        (
            "V-031 another connection",
            vec![],
            with(&|c| {
                c.previous_version = Some(PreviousVersion {
                    environment: Environment::Paper,
                    connection_id: ConnectionId::parse("conn_other").map_err(|e| e.to_string())?,
                    mandate: None,
                    mandate_version: None,
                });
                Ok(())
            })?,
            vec![Violation::V031],
        ),
        (
            "V-031 the same pair",
            vec![],
            with(&|c| {
                c.previous_version = Some(PreviousVersion {
                    environment: Environment::Paper,
                    connection_id: ConnectionId::parse("conn_alpaca_paper_01")
                        .map_err(|e| e.to_string())?,
                    mandate: None,
                    mandate_version: None,
                });
                Ok(())
            })?,
            vec![],
        ),
        (
            "V-032 carry at the floor budget",
            vec![],
            with(&|c| {
                c.connection_loss_carry_usd = usd("1000")?;
                Ok(())
            })?,
            vec![Violation::V032],
        ),
        (
            "V-032 carry just below it",
            vec![],
            with(&|c| {
                c.connection_loss_carry_usd = usd("999.99")?;
                Ok(())
            })?,
            vec![],
        ),
    ];
    for (name, patches, ctx, expected) in rows {
        let got = codes(&patches, &ctx)?;
        let wanted: BTreeSet<Violation> = expected.into_iter().collect();
        if got != wanted {
            return Err(format!("{name}: expected {wanted:?}, got {got:?}"));
        }
    }
    Ok(())
}

/// V-033 needs an accumulate goal, whose own universe the base does not have, so it is its own mandate.
#[test]
fn v033_trim_to_target_is_refused_only_with_accumulate() -> Checked {
    let goal = r#"{"type": "accumulate", "instrument": "7b4a1c2e-2222-4a2b-9c3d-000000000002",
      "target_qty": "10", "max_avg_price": null, "max_spend_usd": "5000", "end_date": null,
      "on_complete": "hold_protected"}"#;
    let pins = r#"[{"asset_id": "7b4a1c2e-2222-4a2b-9c3d-000000000002", "symbol": "XYZ", "asset_class": "us_equity"}]"#;
    let trim = ("/risk/scale_action", r#""trim_to_target""#);
    expect_rows(&[
        ("continuous may trim", &[trim], &[]),
        (
            "accumulate may not",
            &[
                ("/goal", goal),
                ("/universe/pinned_instruments", pins),
                trim,
            ],
            &[Violation::V033],
        ),
        (
            "accumulate limiting buys",
            &[("/goal", goal), ("/universe/pinned_instruments", pins)],
            &[],
        ),
    ])
}

/// V-020 over each source and each listed default's value, and the approvers' single-user rule.
#[test]
fn v020_reads_the_source_the_confirmation_and_the_listed_value() -> Checked {
    let ctx = context()?;
    let rows: &[(&str, Patch<'_>, Entries<'_>, u32, bool)] = &[
        (
            "stated and confirmed",
            &[],
            &[("/risk/max_drawdown", Source::UserStated, true)],
            1,
            false,
        ),
        (
            "stated, unconfirmed",
            &[],
            &[("/risk/max_drawdown", Source::UserStated, false)],
            1,
            true,
        ),
        (
            "entered, unconfirmed",
            &[],
            &[("/risk/max_drawdown", Source::UserEntered, false)],
            1,
            true,
        ),
        (
            "proposed and confirmed",
            &[],
            &[("/risk/max_drawdown", Source::PlatformProposed, true)],
            1,
            false,
        ),
        (
            "proposed, unconfirmed",
            &[],
            &[("/risk/max_drawdown", Source::PlatformProposed, false)],
            1,
            true,
        ),
        (
            "template",
            &[],
            &[("/risk/max_drawdown", Source::TemplateStructure, true)],
            1,
            true,
        ),
        (
            "system field",
            &[],
            &[("/source_text_ref", Source::TemplateStructure, false)],
            1,
            false,
        ),
        (
            "under a system field",
            &[],
            &[("/mandate_schema_version/x", Source::PlatformDefault, false)],
            1,
            false,
        ),
        (
            "default name",
            &[],
            &[("/name", Source::PlatformDefault, false)],
            1,
            false,
        ),
        (
            "default under notifications",
            &[],
            &[(
                "/notifications/quiet_hours/start",
                Source::PlatformDefault,
                true,
            )],
            1,
            false,
        ),
        (
            "default on a sibling spelled alike",
            &[],
            &[("/notifications_x", Source::PlatformDefault, true)],
            1,
            true,
        ),
        (
            "default above a listed path",
            &[],
            &[("/autonomy", Source::PlatformDefault, true)],
            1,
            true,
        ),
        (
            "default ask",
            &[],
            &[("/autonomy/default", Source::PlatformDefault, true)],
            1,
            false,
        ),
        (
            "default under a field whose value §7 fixes",
            &[],
            &[("/autonomy/default/x", Source::PlatformDefault, true)],
            1,
            true,
        ),
        (
            "default deny",
            &[("/autonomy/default", r#""deny""#)],
            &[("/autonomy/default", Source::PlatformDefault, true)],
            1,
            true,
        ),
        (
            "default admission deny",
            &[("/autonomy/admission", r#""deny""#)],
            &[("/autonomy/admission", Source::PlatformDefault, true)],
            1,
            true,
        ),
        (
            "default skip",
            &[],
            &[(
                "/autonomy/approval/on_timeout",
                Source::PlatformDefault,
                true,
            )],
            1,
            false,
        ),
        (
            "default leveraged off",
            &[],
            &[(
                "/universe/leveraged_etps_enabled",
                Source::PlatformDefault,
                true,
            )],
            1,
            false,
        ),
        (
            "default leveraged on",
            &[("/universe/leveraged_etps_enabled", "true")],
            &[(
                "/universe/leveraged_etps_enabled",
                Source::PlatformDefault,
                true,
            )],
            1,
            true,
        ),
        (
            "default null disclosure",
            &[],
            &[(
                "/universe/leveraged_etp_disclosure_version",
                Source::PlatformDefault,
                true,
            )],
            1,
            false,
        ),
        (
            "default set disclosure",
            &[(
                "/universe/leveraged_etp_disclosure_version",
                r#""sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb""#,
            )],
            &[(
                "/universe/leveraged_etp_disclosure_version",
                Source::PlatformDefault,
                true,
            )],
            1,
            true,
        ),
        (
            "default paper",
            &[],
            &[("/environment", Source::PlatformDefault, true)],
            1,
            false,
        ),
        (
            "default approvers, one user",
            &[],
            &[(
                "/autonomy/approval/approvers",
                Source::PlatformDefault,
                true,
            )],
            1,
            false,
        ),
        (
            "default approvers, two users",
            &[],
            &[(
                "/autonomy/approval/approvers",
                Source::PlatformDefault,
                true,
            )],
            2,
            true,
        ),
        (
            "default limit",
            &[],
            &[("/risk/max_order_usd", Source::PlatformDefault, true)],
            1,
            true,
        ),
    ];
    for (name, patches, entries, users, broken) in rows {
        let mut ctx = ctx.clone();
        ctx.provenance = provenance(entries);
        ctx.workspace_users = *users;
        let got = codes(patches, &ctx)?.contains(&Violation::V020);
        if got != *broken {
            return Err(format!("{name}: V-020 expected {broken}, got {got}"));
        }
    }
    Ok(())
}

/// V-022 reads an entry on the `auto`'s path, under it, or above it (DEC-161), and only an `auto`.
#[test]
fn v022_reads_every_entry_that_speaks_for_an_auto() -> Checked {
    let auto_rule = r#"{"id": "routine", "when": {"field": "purpose", "op": "eq", "value": "open"}, "then": "auto"}"#;
    let ask_rule = r#"{"id": "routine", "when": {"field": "purpose", "op": "eq", "value": "open"}, "then": "ask"}"#;
    let default_auto = [("/autonomy/default", r#""auto""#)];
    let admission_auto = [("/autonomy/admission", r#""auto""#)];
    let rule_auto = [("/autonomy/rules/-", auto_rule)];
    let rule_ask = [("/autonomy/rules/-", ask_rule)];
    let rows: Vec<(&str, Patch<'_>, Entries<'_>, bool)> = vec![
        (
            "an unmentioned auto is the owner's",
            &default_auto,
            &[],
            false,
        ),
        (
            "entered and confirmed",
            &default_auto,
            &[("/autonomy/default", Source::UserEntered, true)],
            false,
        ),
        (
            "entered, unconfirmed",
            &default_auto,
            &[("/autonomy/default", Source::UserEntered, false)],
            true,
        ),
        (
            "stated",
            &default_auto,
            &[("/autonomy/default", Source::UserStated, true)],
            true,
        ),
        (
            "proposed",
            &admission_auto,
            &[("/autonomy/admission", Source::PlatformProposed, true)],
            true,
        ),
        (
            "proposed from above",
            &admission_auto,
            &[("/autonomy", Source::PlatformProposed, true)],
            true,
        ),
        (
            "a rule stated",
            &rule_auto,
            &[("/autonomy/rules/1", Source::UserStated, true)],
            true,
        ),
        (
            "a rule's then stated",
            &rule_auto,
            &[("/autonomy/rules/1/then", Source::UserStated, true)],
            true,
        ),
        (
            "the rules stated",
            &rule_auto,
            &[("/autonomy/rules", Source::UserStated, true)],
            true,
        ),
        (
            "another rule stated",
            &rule_auto,
            &[("/autonomy/rules/0", Source::UserStated, true)],
            false,
        ),
        (
            "its condition stated",
            &rule_auto,
            &[("/autonomy/rules/1/when", Source::UserStated, true)],
            false,
        ),
        (
            "a stated ask",
            &rule_ask,
            &[("/autonomy/rules/1", Source::UserStated, true)],
            false,
        ),
        (
            "a stated ask default",
            &[],
            &[("/autonomy/default", Source::UserStated, true)],
            false,
        ),
    ];
    let base = context()?;
    for (name, patches, entries, broken) in rows {
        let mut ctx = base.clone();
        ctx.provenance = provenance(entries);
        let got = codes(patches, &ctx)?.contains(&Violation::V022);
        if got != broken {
            return Err(format!("{name}: V-022 expected {broken}, got {got}"));
        }
    }
    Ok(())
}

/// V-038 on the three never-proposed paths, under and above them (DEC-161), and nowhere else.
#[test]
fn v038_reads_every_entry_that_speaks_for_a_never_proposed_path() -> Checked {
    let rows: &[(&str, Source, bool)] = &[
        (
            "/universe/pinned_instruments",
            Source::PlatformProposed,
            true,
        ),
        (
            "/universe/pinned_instruments/0/asset_id",
            Source::PlatformProposed,
            true,
        ),
        ("/universe", Source::PlatformProposed, true),
        ("", Source::PlatformProposed, true),
        ("/environment", Source::PlatformProposed, true),
        ("/connection_id", Source::PlatformProposed, true),
        ("/universe/max_instruments", Source::PlatformProposed, false),
        ("/universe/pinned", Source::PlatformProposed, false),
        ("/connection_idx", Source::PlatformProposed, false),
        ("/universe/pinned_instruments", Source::UserStated, false),
        ("/environment", Source::PlatformDefault, false),
    ];
    let base = context()?;
    for (path, source, broken) in rows {
        let mut ctx = base.clone();
        ctx.provenance = provenance(&[(path, *source, true)]);
        let got = codes(&[], &ctx)?.contains(&Violation::V038);
        if got != *broken {
            return Err(format!(
                "{source:?} at `{path}`: V-038 expected {broken}, got {got}"
            ));
        }
    }
    Ok(())
}

/// W-001, W-002 on both sides of its boundary, W-003, and W-005 on a catch-all that is not last.
#[test]
fn each_warning_fires_on_its_condition_alone() -> Checked {
    let catch_all = r#"{"id": "routine", "when": {"field": "purpose", "op": "in", "value": ["increase", "open"]}, "then": "ask"}"#;
    let later = r#"{"id": "later", "when": {"field": "order_usd", "op": "gt", "value": "5000"}, "then": "deny"}"#;
    let mut failing = context()?;
    failing.eligibility_failures = BTreeSet::from([asset(ASSET_A)?]);
    let base = context()?;
    let rows: Vec<(&str, Owned<'_>, &ValidationContext, Vec<Warning>)> = vec![
        ("W-001", vec![], &failing, vec![Warning::W001]),
        (
            "W-002 a position at its stop loses exactly the budget",
            vec![
                ("/risk/max_position_usd", r#""2000""#),
                ("/protection/stop_distance", r#""0.1""#),
            ],
            &base,
            vec![],
        ),
        (
            "W-002 one cent past it",
            vec![
                ("/risk/max_position_usd", r#""2000""#),
                ("/protection/stop_distance", r#""0.100005""#),
            ],
            &base,
            vec![Warning::W002],
        ),
        (
            "W-003",
            vec![
                ("/protection/enabled", "false"),
                ("/protection/stop_distance", "null"),
                ("/protection/take_profit_distance", "null"),
            ],
            &base,
            vec![Warning::W003],
        ),
        (
            "W-005 a rule after a catch-all",
            vec![
                ("/autonomy/rules/0", catch_all),
                ("/autonomy/rules/-", later),
            ],
            &base,
            vec![Warning::W005],
        ),
        (
            "W-005 a catch-all last",
            vec![("/autonomy/rules/-", catch_all)],
            &base,
            vec![],
        ),
    ];
    for (name, patches, ctx, expected) in rows {
        let got = report(&patches, ctx)?.warnings;
        let wanted: BTreeSet<Warning> = expected.into_iter().collect();
        if got != wanted {
            return Err(format!("{name}: expected {wanted:?}, got {got:?}"));
        }
    }
    Ok(())
}

/// The four figures on the crypto branch, where the offset is added, and the base's own; and no
/// position-at-stop figure when protection is off, even with a stop still written (V-008).
#[test]
fn the_worst_case_adds_the_crypto_offset_only_with_crypto() -> Checked {
    let ctx = context()?;
    let crypto = [
        ("/universe/asset_classes", r#"["crypto", "us_equity"]"#),
        ("/protection/crypto_stop_limit_offset", r#""0.005""#),
    ];
    let with_offset = report(&crypto, &ctx)?.worst_case;
    let offset_only = report(
        &[("/protection/crypto_stop_limit_offset", r#""0.005""#)],
        &ctx,
    )?
    .worst_case;
    let disabled_with_a_stop = report(
        &[
            ("/protection/enabled", "false"),
            ("/protection/take_profit_distance", "null"),
        ],
        &ctx,
    )?
    .worst_case;
    let expected = [
        (with_offset.one_position_at_stop_usd, Some(usd("82.5")?)),
        (offset_only.one_position_at_stop_usd, Some(usd("75")?)),
        (disabled_with_a_stop.one_position_at_stop_usd, None),
        (Some(with_offset.daily_loss_budget_usd), Some(usd("200")?)),
        (
            Some(with_offset.flatten_trigger_loss_usd),
            Some(usd("800")?),
        ),
        (
            Some(with_offset.lifetime_floor_loss_usd),
            Some(usd("1000")?),
        ),
    ];
    for (index, (got, wanted)) in expected.into_iter().enumerate() {
        if got != wanted {
            return Err(format!("figure {index}: expected {wanted:?}, got {got:?}"));
        }
    }
    Ok(())
}

/// A fraction `Fraction` cannot hold makes the document unevaluable, naming the field; it is never
/// rounded into a figure and never reported as a V-code (DEC-128 item 4, DEC-161).
#[test]
fn a_figure_the_arithmetic_cannot_hold_names_its_field() -> Checked {
    let ctx = context()?;
    for (path, value) in [
        ("/risk/max_position_fraction", r#""0.1234567891""#),
        ("/risk/max_daily_loss", r#""0.0000000001""#),
        ("/protection/stop_distance", r#""0.0000000001""#),
    ] {
        let got = validate(&mandate(&[(path, value)])?, &ctx);
        match got {
            Err(SpecError::OutOfRange {
                path: at,
                cause: NumError::TooPrecise,
            }) if at.as_str() == path => {}
            other => {
                return Err(format!(
                    "{path}: expected out_of_range there, got {other:?}"
                ));
            }
        }
    }
    Ok(())
}

/// A mandate whose fields were changed after parsing is not validated: the report would describe a
/// document nobody hashes.
#[test]
fn a_diverged_mandate_is_refused() -> Checked {
    let mut changed = mandate(&[])?;
    changed.universe.max_instruments = 1;
    match validate(&changed, &context()?) {
        Err(SpecError::Parse(ParseError::Diverged)) => Ok(()),
        other => Err(format!("expected diverged, got {other:?}")),
    }
}

/// Canonical keys are `[a-z][a-z0-9_]*`, so a pointer into a mandate needs no `~` escapes; what it
/// does need is RFC 6901's array index, digits with no leading zero.
#[test]
fn a_pointer_reads_members_and_canonical_indices() -> Checked {
    let document = json(r#"{"a": {"b": [10, 20], "c": true, "d": null}}"#)?;
    let rows: [(&str, Option<&str>); 11] = [
        ("", Some("whole")),
        ("/a/b/1", Some("20")),
        ("/a/b/0", Some("10")),
        ("/a/c", Some("true")),
        ("/a/d", Some("null")),
        ("/a/b/01", None),
        ("/a/b/+1", None),
        ("/a/b/2", None),
        ("/a/e", None),
        ("a", None),
        ("/a/b/0/x", None),
    ];
    for (path, expected) in rows {
        let got = pointer(&document, path).map(|value| match value {
            Value::Int(n) => n.get().to_string(),
            Value::Bool(flag) => flag.to_string(),
            Value::Null => "null".to_owned(),
            _ => "whole".to_owned(),
        });
        if got.as_deref() != expected {
            return Err(format!("`{path}`: expected {expected:?}, got {got:?}"));
        }
    }
    Ok(())
}

/// `Mandate::at` answers from the document as parsed, and refuses once the fields have moved away from
/// it.
#[test]
fn a_mandate_answers_a_pointer_from_its_own_document() -> Checked {
    let parsed = mandate(&[])?;
    let at = |path: &str| parsed.at(&Pointer::new(path)).map_err(|e| e.to_string());
    if at("/risk/max_drawdown")? != Some(Value::Str("0.08".to_owned())) {
        return Err("`/risk/max_drawdown` should read the document's 0.08".to_owned());
    }
    if at("/notifications/channels/1")? != Some(Value::Str("phone".to_owned())) {
        return Err("an index should read the item as written".to_owned());
    }
    if at("/risk/no_such_limit")?.is_some() {
        return Err("a pointer that names nothing is `None`".to_owned());
    }
    let mut changed = parsed.clone();
    changed.risk.max_orders_per_day = 1;
    match changed.at(&Pointer::new("/risk/max_drawdown")) {
        Err(ParseError::Diverged) => Ok(()),
        other => Err(format!("a diverged mandate must not answer, got {other:?}")),
    }
}

#[test]
fn a_pointer_covers_itself_and_what_lies_under_it() -> Checked {
    let rows = [
        ("/a", "/a", true),
        ("/a", "/a/b", true),
        ("/a", "/ab", false),
        ("/a/b", "/a", false),
        ("", "/a", true),
        ("/a", "", false),
    ];
    for (prefix, path, expected) in rows {
        if covers(prefix, path) != expected {
            return Err(format!("covers({prefix:?}, {path:?}) should be {expected}"));
        }
    }
    Ok(())
}

#[test]
fn a_connection_id_is_the_schemas_id() -> Checked {
    for good in ["c", "conn_alpaca-paper_01", &"x".repeat(64)] {
        let parsed = ConnectionId::parse(good).map_err(|e| format!("{good}: {e}"))?;
        if parsed.as_str() != good {
            return Err(format!("`{good}` read as `{}`", parsed.as_str()));
        }
    }
    for bad in ["", "conn.1", "conn 1", &"x".repeat(65)] {
        if ConnectionId::parse(bad).map_err(|e| e.code()) != Err("off_pattern") {
            return Err(format!("`{bad}` must be off_pattern"));
        }
    }
    Ok(())
}

/// A research agent's model: `llm.`, admitting, and the research envelope beside it. The registry is
/// left out of the context these rows run in, so renaming the base's model reads as V-036 alone and
/// never as V-007.
const RESEARCH: &str =
    r#"{"interval_s": 3600, "cost_cap_usd_per_day": "5", "max_revisions_per_lineage": 3}"#;

fn unregistered() -> Result<ValidationContext, String> {
    let mut ctx = context()?;
    ctx.registry = None;
    Ok(ctx)
}

/// The rows of the field split run with no registry, so a model's id is free to change.
fn expect_unregistered_rows(rows: &[(&str, Owned<'_>, &[Violation])]) -> Checked {
    let ctx = unregistered()?;
    for (name, patches, expected) in rows {
        let got = codes(patches, &ctx)?;
        let wanted: BTreeSet<Violation> = expected.iter().copied().collect();
        if got != wanted {
            return Err(format!("{name}: expected {wanted:?}, got {got:?}"));
        }
    }
    Ok(())
}

/// E17-1's field split (§2.3): V-034, V-035, V-036, V-037, and V-039, each on both sides.
#[test]
fn each_universe_rule_fires_on_each_of_its_branches_and_nothing_else() -> Checked {
    let unpinned = [
        ("/universe/pinned", "false"),
        ("/universe/pinned_instruments", "[]"),
    ];
    let llm = [
        ("/behavior/signal_models/0/id", r#""llm.research""#),
        ("/behavior/signal_models/0/admits_instruments", "true"),
    ];
    let second_llm = r#"{"id": "llm.second", "version": "1.0.0",
      "content_hash": "sha256:2222222222222222222222222222222222222222222222222222222222222222",
      "params": [], "weight": "1", "max_output_age_s": 1800, "admits_instruments": true}"#;
    let research = [("/behavior/research", RESEARCH)];
    let join = |parts: &[&[(&'static str, &'static str)]]| -> Owned<'static> {
        parts.iter().flat_map(|part| part.iter().copied()).collect()
    };
    let rows: Vec<(&str, Owned<'_>, &[Violation])> = vec![
        ("the base, pinned to two", vec![], &[]),
        ("unpinned and empty", join(&[&unpinned]), &[]),
        (
            "V-034 unpinned with instruments",
            vec![("/universe/pinned", "false")],
            &[Violation::V034],
        ),
        (
            "V-034 pinned with none",
            vec![("/universe/pinned_instruments", "[]")],
            &[Violation::V034],
        ),
        (
            "V-035 a ceiling below the pinned count",
            vec![("/universe/max_instruments", "1")],
            &[Violation::V035],
        ),
        (
            "V-036 a research agent with its envelope",
            join(&[&unpinned, &llm, &research]),
            &[],
        ),
        (
            "V-036 a research agent without its envelope",
            join(&[&unpinned, &llm]),
            &[Violation::V036],
        ),
        (
            "V-036 an envelope without a research agent",
            join(&[&unpinned, &research]),
            &[Violation::V036],
        ),
        (
            "V-036 an admitting model that is not llm",
            join(&[
                &unpinned,
                &[("/behavior/signal_models/0/admits_instruments", "true")],
                &research,
            ]),
            &[Violation::V036],
        ),
        (
            "V-036 two admitting models",
            join(&[&unpinned, &llm, &research])
                .into_iter()
                .chain([("/behavior/signal_models/-", second_llm)])
                .collect(),
            &[Violation::V036],
        ),
        (
            "V-037 a pinned universe with a research agent",
            join(&[&llm, &research]),
            &[Violation::V037],
        ),
        (
            "V-039 the last pinned class not allowed",
            vec![("/universe/pinned_instruments/1/asset_class", r#""crypto""#)],
            &[Violation::V039],
        ),
        (
            "V-039 the first pinned class not allowed",
            vec![("/universe/pinned_instruments/0/asset_class", r#""crypto""#)],
            &[Violation::V039],
        ),
        (
            "V-039 the same class once allowed",
            vec![
                ("/universe/pinned_instruments/1/asset_class", r#""crypto""#),
                ("/universe/asset_classes", r#"["crypto", "us_equity"]"#),
                ("/protection/crypto_stop_limit_offset", r#""0.005""#),
            ],
            &[],
        ),
    ];
    expect_unregistered_rows(&rows)
}

/// V-003: an `accumulate` goal is pinned to exactly its instrument and has no research envelope.
#[test]
fn v003_pins_an_accumulator_to_its_one_instrument() -> Checked {
    let goal = |instrument: &str| {
        format!(
            r#"{{"type": "accumulate", "instrument": "{instrument}", "target_qty": "10",
              "max_avg_price": null, "max_spend_usd": "1000", "end_date": "2026-12-31",
              "on_complete": "hold_protected"}}"#
        )
    };
    let on_a = goal(ASSET_A);
    let on_b = goal(ASSET_B);
    let only_a = r#"[{"asset_id": "7b4a1c2e-2222-4a2b-9c3d-000000000002", "symbol": "XYZ", "asset_class": "us_equity"}]"#;
    let only_b = r#"[{"asset_id": "7b4a1c2e-3333-4a2b-9c3d-000000000003", "symbol": "QRS", "asset_class": "us_equity"}]"#;
    let rows: Vec<(&str, Owned<'_>, &[Violation])> = vec![
        (
            "pinned to its instrument",
            vec![("/goal", &on_a), ("/universe/pinned_instruments", only_a)],
            &[],
        ),
        (
            "pinned to its instrument and another",
            vec![("/goal", &on_a)],
            &[Violation::V003],
        ),
        (
            "pinned to its instrument, listed last of two",
            vec![("/goal", &on_b)],
            &[Violation::V003],
        ),
        (
            "pinned to another instrument",
            vec![("/goal", &on_a), ("/universe/pinned_instruments", only_b)],
            &[Violation::V003],
        ),
        (
            "not pinned",
            vec![
                ("/goal", &on_a),
                ("/universe/pinned", "false"),
                ("/universe/pinned_instruments", "[]"),
            ],
            &[Violation::V003],
        ),
        (
            "pinned to its instrument with a research envelope",
            vec![
                ("/goal", &on_a),
                ("/universe/pinned_instruments", only_a),
                ("/behavior/research", RESEARCH),
            ],
            &[Violation::V003, Violation::V036],
        ),
    ];
    expect_unregistered_rows(&rows)
}

/// W-006: `admission: auto` warns only with a research agent configured.
#[test]
fn w006_warns_of_auto_admission_only_with_a_research_agent() -> Checked {
    let ctx = unregistered()?;
    let agent = [
        ("/universe/pinned", "false"),
        ("/universe/pinned_instruments", "[]"),
        ("/behavior/signal_models/0/id", r#""llm.research""#),
        ("/behavior/signal_models/0/admits_instruments", "true"),
        ("/behavior/research", RESEARCH),
    ];
    let auto = ("/autonomy/admission", r#""auto""#);
    let with_auto: Owned<'_> = agent.iter().copied().chain([auto]).collect();
    let envelope_only = [
        ("/universe/pinned", "false"),
        ("/universe/pinned_instruments", "[]"),
        ("/behavior/research", RESEARCH),
        auto,
    ];
    let rows: Vec<(&str, Owned<'_>, Vec<Warning>, Vec<Violation>)> = vec![
        (
            "auto with a research agent",
            with_auto,
            vec![Warning::W006],
            vec![],
        ),
        ("ask with a research agent", agent.to_vec(), vec![], vec![]),
        ("auto with none", vec![auto], vec![], vec![]),
        (
            "auto with a research envelope but no admitting model (DEC-161 item 10)",
            envelope_only.to_vec(),
            vec![],
            vec![Violation::V036],
        ),
    ];
    for (name, patches, warnings, violations) in rows {
        let report = report(&patches, &ctx)?;
        let wanted: BTreeSet<Warning> = warnings.into_iter().collect();
        let refused: BTreeSet<Violation> = violations.into_iter().collect();
        if report.warnings != wanted || report.violations != refused {
            return Err(format!(
                "{name}: expected {wanted:?} and {refused:?}, got {report:?}"
            ));
        }
    }
    Ok(())
}

/// The stop a worst case uses: the stop, the offset added only with crypto, none without protection,
/// and a distance a `Ratio` cannot hold named by its field.
#[test]
fn the_worst_case_stop_distance_adds_the_offset_only_with_crypto() -> Checked {
    let crypto = ("/universe/asset_classes", r#"["crypto", "us_equity"]"#);
    let offset = ("/protection/crypto_stop_limit_offset", r#""0.005""#);
    let disabled = [
        ("/protection/enabled", "false"),
        ("/protection/stop_distance", "null"),
        ("/protection/take_profit_distance", "null"),
    ];
    let rows: Vec<(&str, Owned<'_>, Option<&str>)> = vec![
        ("the base's stop", vec![], Some("0.05")),
        ("an offset without crypto", vec![offset], Some("0.05")),
        (
            "the offset with crypto",
            vec![crypto, offset],
            Some("0.055"),
        ),
        (
            "a sum past one",
            vec![
                crypto,
                ("/protection/stop_distance", r#""0.9""#),
                ("/protection/crypto_stop_limit_offset", r#""0.25""#),
            ],
            Some("1.15"),
        ),
        ("protection disabled", disabled.to_vec(), None),
        (
            "protection disabled with its stop still written (V-008)",
            vec![
                ("/protection/enabled", "false"),
                ("/protection/take_profit_distance", "null"),
            ],
            None,
        ),
    ];
    for (name, patches, expected) in rows {
        let got = super::worst_case_stop_distance(&mandate(&patches)?)
            .map_err(|e| format!("{name}: {e}"))?;
        if got.as_ref().map(crate::SchemaDec::as_str) != expected {
            return Err(format!("{name}: expected {expected:?}, got {got:?}"));
        }
    }
    let too_precise = format!(r#""0.{}1""#, "0".repeat(24));
    for (path, patches) in [
        (
            "/protection/stop_distance",
            vec![
                crypto,
                offset,
                ("/protection/stop_distance", too_precise.as_str()),
            ],
        ),
        (
            "/protection/crypto_stop_limit_offset",
            vec![
                crypto,
                ("/protection/crypto_stop_limit_offset", too_precise.as_str()),
            ],
        ),
    ] {
        match super::worst_case_stop_distance(&mandate(&patches)?) {
            Err(SpecError::OutOfRange { path: at, .. }) if at.as_str() == path => {}
            other => {
                return Err(format!(
                    "{path}: expected out_of_range there, got {other:?}"
                ));
            }
        }
    }
    Ok(())
}

/// Adds one provenance entry, keeping the others, so two breakers that each state one compose.
fn state(ctx: &mut ValidationContext, path: &str, source: Source) {
    let mut entries = ctx.provenance.entries().clone();
    entries.insert(
        Pointer::new(path),
        Provenance {
            source,
            confirmed: true,
        },
    );
    ctx.provenance = ProvenanceMap::new(entries);
}

type Breaker = (
    Violation,
    &'static [(&'static str, &'static str)],
    fn(&mut ValidationContext),
);

/// The breakers the property below combines. Each changes what no other one touches and breaks the one
/// rule it names, which this table states by hand: the oracle is the table, not the rules.
///
/// V-015 and V-030 both need `end_date`, so V-030 moves the validation date instead; an invalid date has
/// no calendar position, so with V-015 chosen V-030 cannot fire and the property drops it.
const BREAKERS: [Breaker; 26] = [
    (Violation::V001, &[], |c| {
        c.connection_environment = Some(Environment::Live)
    }),
    (Violation::V002, &[], |c| c.account_equity_usd = Usd::ZERO),
    (
        Violation::V005,
        &[("/universe/leveraged_etps_enabled", "true")],
        |_| {},
    ),
    (Violation::V006, &[], |c| {
        c.claimed_by_other_agents = AssetId::parse(ASSET_B).into_iter().collect();
    }),
    (
        Violation::V007,
        &[("/behavior/signal_models/0/version", r#""9.9.9""#)],
        |_| {},
    ),
    (
        Violation::V008,
        &[
            ("/protection/enabled", "false"),
            ("/protection/stop_distance", "null"),
        ],
        |_| {},
    ),
    (
        Violation::V009,
        &[("/notifications/channels", r#"["phone", "email"]"#)],
        |_| {},
    ),
    (
        Violation::V010,
        &[("/risk/drawdown_ladder/1/factor", r#""0.5""#)],
        |_| {},
    ),
    (
        Violation::V012,
        &[("/risk/hysteresis", r#""0.03""#)],
        |_| {},
    ),
    (
        Violation::V013,
        &[("/risk/max_order_usd", r#""2000""#)],
        |_| {},
    ),
    (
        Violation::V014,
        &[("/capital/max_loss_from_allocation", r#""0.05""#)],
        |_| {},
    ),
    (
        Violation::V015,
        &[("/goal/end_date", r#""2027-02-30""#)],
        |_| {},
    ),
    (
        Violation::V016,
        &[("/notifications/quiet_hours/end", r#""22:00""#)],
        |_| {},
    ),
    (
        Violation::V017,
        &[(
            "/autonomy/rules/0/when",
            r#"{"all": [{"any": [{"not": {"all": [{"field": "order_usd", "op": "gt", "value": "9"}]}}]}]}"#,
        )],
        |_| {},
    ),
    (
        Violation::V018,
        &[(
            "/autonomy/rules/-",
            r#"{"id": "drift", "when": {"field": "unusual_input", "op": "eq", "value": true}, "then": "ask"}"#,
        )],
        |_| {},
    ),
    (Violation::V020, &[], |c| {
        state(c, "/risk/max_daily_loss", Source::PlatformDefault)
    }),
    (
        Violation::V022,
        &[("/autonomy/default", r#""auto""#)],
        |c| {
            state(c, "/autonomy/default", Source::UserStated);
        },
    ),
    (
        Violation::V023,
        &[(
            "/autonomy/rules/-",
            r#"{"id": "typed", "when": {"field": "session", "op": "eq", "value": "night"}, "then": "ask"}"#,
        )],
        |_| {},
    ),
    (Violation::V024, &[], |c| c.approver_users = 0),
    (Violation::V034, &[("/universe/pinned", "false")], |_| {}),
    (
        Violation::V035,
        &[("/universe/max_instruments", "1")],
        |_| {},
    ),
    (
        Violation::V039,
        &[("/universe/pinned_instruments/1/asset_class", r#""crypto""#)],
        |_| {},
    ),
    (Violation::V038, &[], |c| {
        state(c, "/connection_id", Source::PlatformProposed);
    }),
    (Violation::V030, &[], |c| {
        c.validation_date = Date::parse("2027-01-01").unwrap_or(c.validation_date);
    }),
    (Violation::V031, &[], |c| {
        c.previous_version =
            ConnectionId::parse("conn_other")
                .ok()
                .map(|connection_id| PreviousVersion {
                    environment: Environment::Paper,
                    connection_id,
                    mandate: None,
                    mandate_version: None,
                });
    }),
    (Violation::V032, &[], |c| {
        c.connection_loss_carry_usd = Usd::parse("1000").unwrap_or(Usd::ZERO);
    }),
];

/// The paths §7 lists, restated as a match so the property's oracle shares no table with the rule.
fn oracle_default_allowed(path: &str, default_is_ask: bool, users: u32) -> bool {
    match path {
        "/name"
        | "/notifications"
        | "/notifications/channels"
        | "/autonomy/approval/on_timeout"
        | "/environment"
        | "/universe/leveraged_etps_enabled"
        | "/universe/leveraged_etp_disclosure_version" => true,
        "/autonomy/default" => default_is_ask,
        "/autonomy/approval/approvers" => users <= 1,
        _ => false,
    }
}

const PROVENANCE_PATHS: [&str; 15] = [
    "/name",
    "/notifications",
    "/notifications/channels",
    "/autonomy",
    "/autonomy/default",
    "/autonomy/approval/approvers",
    "/autonomy/approval/on_timeout",
    "/environment",
    "/universe/leveraged_etps_enabled",
    "/universe/leveraged_etp_disclosure_version",
    "/universe/max_instruments",
    "/risk/max_drawdown",
    "/capital/allocation_usd",
    "/source_text_ref",
    "/mandate_schema_version",
];

const SOURCES: [Source; 5] = [
    Source::UserStated,
    Source::UserEntered,
    Source::TemplateStructure,
    Source::PlatformProposed,
    Source::PlatformDefault,
];

/// `digits × 10^-scale` in canonical decimal text: no trailing fractional zero, `0` for zero.
fn decimal(digits: u128, scale: usize) -> String {
    let text = format!("{digits:0>width$}", width = scale.saturating_add(1));
    let (whole, fraction) = text.split_at(text.len().saturating_sub(scale));
    let fraction = fraction.trim_end_matches('0');
    if fraction.is_empty() {
        whole.to_owned()
    } else {
        format!("{whole}.{fraction}")
    }
}

/// §4.2's figures in integers: amounts in cents, fractions in millionths.
fn oracle_figures(
    allocation_cents: u128,
    position_cents: u128,
    fraction: u128,
    stop: u128,
    daily: u128,
) -> Option<[String; 3]> {
    let cap = position_cents
        .checked_mul(1_000_000)?
        .min(allocation_cents.checked_mul(fraction)?);
    Some([
        decimal(cap.checked_mul(stop)?, 14),
        decimal(allocation_cents.checked_mul(daily)?, 8),
        decimal(cap, 8),
    ])
}

proptest! {
    /// §4.1 and §4: whatever combination of rules a mandate breaks, the report carries exactly those
    /// codes, none of them twice and nothing else.
    #[test]
    fn every_code_a_mandate_breaks_is_reported_and_no_other(
        chosen in prop::collection::vec(any::<bool>(), BREAKERS.len())
    ) {
        let mut patches: Vec<(&str, &str)> = Vec::new();
        let mut ctx = context().map_err(TestCaseError::fail)?;
        let mut expected = BTreeSet::new();
        for ((violation, own, change), on) in BREAKERS.iter().zip(&chosen) {
            if *on {
                patches.extend_from_slice(own);
                change(&mut ctx);
                expected.insert(*violation);
            }
        }
        if expected.contains(&Violation::V015) {
            expected.remove(&Violation::V030);
        }
        let got = codes(&patches, &ctx).map_err(TestCaseError::fail)?;
        prop_assert_eq!(got, expected);
    }

    /// MI-12 through V-020: a field that is unconfirmed, from a template, or a platform default off
    /// §7's list or its value, always reports V-020, and nothing else does.
    #[test]
    fn a_proposed_or_unconfirmed_envelope_field_always_reports_v020(
        entries in prop::collection::btree_map(
            prop::sample::select(PROVENANCE_PATHS.to_vec()),
            (prop::sample::select(SOURCES.to_vec()), any::<bool>()),
            0..6,
        ),
        default_is_ask in any::<bool>(),
        users in 1u32..4,
    ) {
        let mut ctx = context().map_err(TestCaseError::fail)?;
        ctx.workspace_users = users;
        ctx.provenance = ProvenanceMap::new(
            entries
                .iter()
                .map(|(path, (source, confirmed))| {
                    (Pointer::new(path), Provenance { source: *source, confirmed: *confirmed })
                })
                .collect(),
        );
        let expected = entries.iter().any(|(path, (source, confirmed))| {
            let system = matches!(*path, "/source_text_ref" | "/mandate_schema_version");
            let allowed = match source {
                Source::PlatformDefault => oracle_default_allowed(path, default_is_ask, users),
                Source::TemplateStructure => false,
                Source::UserStated | Source::UserEntered | Source::PlatformProposed => *confirmed,
            };
            !system && !allowed
        });
        let default = if default_is_ask { r#""ask""# } else { r#""deny""# };
        let got = codes(&[("/autonomy/default", default)], &ctx).map_err(TestCaseError::fail)?;
        prop_assert_eq!(got.contains(&Violation::V020), expected, "{:?}", entries);
    }

    /// §4.2's figures equal the products an integer oracle computes, and none falls when the
    /// allocation grows.
    #[test]
    fn the_worst_case_is_the_spec_products_and_monotone_in_the_allocation(
        allocation in 1u128..1_000_000_000,
        growth in 0u128..1_000_000_000,
        position in 1u128..1_000_000_000,
        fraction in 1u128..=1_000_000,
        stop in 1u128..1_000_000,
        daily in 1u128..1_000_000,
    ) {
        let ctx = context().map_err(TestCaseError::fail)?;
        let mut previous: Option<super::WorstCase> = None;
        for cents in [allocation, allocation.saturating_add(growth)] {
            let texts = [
                decimal(cents, 2),
                decimal(position, 2),
                decimal(fraction, 6),
                decimal(stop, 6),
                decimal(daily, 6),
            ];
            let quoted: Vec<String> = texts.iter().map(|t| format!("\"{t}\"")).collect();
            let fields = [
                "/capital/allocation_usd",
                "/risk/max_position_usd",
                "/risk/max_position_fraction",
                "/protection/stop_distance",
                "/risk/max_daily_loss",
            ];
            let patches: Vec<(&str, &str)> =
                fields.iter().copied().zip(quoted.iter().map(String::as_str)).collect();
            let figures = report(&patches, &ctx).map_err(TestCaseError::fail)?.worst_case;
            let [at_stop, budget, _cap] = oracle_figures(cents, position, fraction, stop, daily)
                .ok_or_else(|| TestCaseError::fail("the oracle overflowed"))?;
            prop_assert_eq!(figures.one_position_at_stop_usd, Usd::parse(&at_stop).ok());
            prop_assert_eq!(Some(figures.daily_loss_budget_usd), Usd::parse(&budget).ok());
            if let Some(before) = previous {
                prop_assert!(before.one_position_at_stop_usd <= figures.one_position_at_stop_usd);
                prop_assert!(before.daily_loss_budget_usd <= figures.daily_loss_budget_usd);
                prop_assert!(before.flatten_trigger_loss_usd <= figures.flatten_trigger_loss_usd);
                prop_assert!(before.lifetime_floor_loss_usd <= figures.lifetime_floor_loss_usd);
            }
            previous = Some(figures);
        }
    }
}

/// V-047 at each count around its boundary, at validation and at application: refused exactly when
/// the policy is on with none or one user, as its own code beside the others the context breaks, and
/// the recheck reports V-002 and V-047 and nothing else.
#[test]
fn v047_refuses_the_policy_below_two_users_at_validation_and_at_application() -> Checked {
    let base = mandate(&[])?;
    for (users, required, refused) in [
        (0, true, true),
        (1, true, true),
        (2, true, false),
        (u32::MAX, true, false),
        (0, false, false),
        (1, false, false),
    ] {
        let mut ctx = context()?;
        ctx.workspace_users = users;
        ctx.independent_approval_required = required;
        let expected = if refused {
            BTreeSet::from([Violation::V047])
        } else {
            BTreeSet::new()
        };
        let row = format!("{users} users, policy {required}");
        let found = validate(&base, &ctx).map_err(|e| format!("{row}: {e}"))?;
        if found.violations != expected {
            return Err(format!("{row}: validation gave {:?}", found.violations));
        }
        let rechecked = recheck_at_application(&base, &ctx).map_err(|e| format!("{row}: {e}"))?;
        if rechecked != expected {
            return Err(format!("{row}: the recheck gave {rechecked:?}"));
        }
        ctx.approver_users = 0;
        ctx.other_allocations_usd = usd("15000.01")?;
        let mut both = expected.clone();
        both.insert(Violation::V002);
        let rechecked = recheck_at_application(&base, &ctx).map_err(|e| format!("{row}: {e}"))?;
        if rechecked != both {
            return Err(format!(
                "{row}, short of equity: the recheck gave {rechecked:?}"
            ));
        }
        ctx.other_allocations_usd = usd("15000")?;
        let rechecked = recheck_at_application(&base, &ctx).map_err(|e| format!("{row}: {e}"))?;
        if rechecked != expected {
            return Err(format!(
                "{row}, equity exactly met: the recheck gave {rechecked:?}"
            ));
        }
    }
    Ok(())
}
