//! The reader's own boundaries, one row per bound or pattern edge the MC-S cases do not reach.
//!
//! `tests/document.rs` pins the code and pointer of each MC-S rejection. These pin the rest of the
//! schema: each `minLength`, `maxLength`, `minItems`, `maxItems`, `uniqueItems`, and pattern edge, on
//! both sides, and the typed value each accepted branch reads into. The oracle is the schema text,
//! transcribed per row, never the reader's own predicates.

use mandate_canon::{Key, Value};
use mandate_domain::{AutonomyDecision, Environment};
use mandate_time::Date;

use crate::condition::{Condition, ConditionValue};
use crate::document::{
    Channel, EventSource, Goal, HourMinute, LadderAction, LimitAction, OnComplete, ParamValue,
    ScaleAction,
};
use crate::{Mandate, ParseError};

/// `btc_accumulator` with every optional branch filled: research, quiet hours, a two-approver
/// threshold, a take-profit, and rules that use `any`, `not`, a list, a boolean, and text.
const BASE: &str = r#"{
  "mandate_schema_version": 1, "name": "btc-accumulator",
  "source_text_ref": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "environment": "live", "connection_id": "conn_alpaca_paper_01",
  "capital": {"allocation_usd": "10000", "max_loss_from_allocation": "0.1"},
  "goal": {"type": "accumulate", "instrument": "7b4a1c2e-1111-4a2b-9c3d-000000000001",
    "target_qty": "0.15", "max_avg_price": "58000", "max_spend_usd": "9000",
    "end_date": "2026-12-31", "on_complete": "release"},
  "universe": {"pinned": true, "pinned_instruments": [{"asset_id": "7b4a1c2e-1111-4a2b-9c3d-000000000001",
    "symbol": "BTC/USD", "asset_class": "crypto"}], "max_instruments": 1,
    "asset_classes": ["crypto", "us_equity"], "leveraged_etps_enabled": false,
    "leveraged_etp_disclosure_version": null},
  "behavior": {"description": "Buy dips.",
    "signal_models": [{"id": "quant.mean_reversion", "version": "1.0.0",
      "content_hash": "sha256:1111111111111111111111111111111111111111111111111111111111111111",
      "params": [{"key": "lookback_bars", "value": "20"}, {"key": "flag", "value": true},
        {"key": "mode", "value": "fast"}],
      "weight": "1", "max_output_age_s": 1800, "admits_instruments": false}],
    "research": {"interval_s": 3600, "cost_cap_usd_per_day": "5", "max_revisions_per_lineage": 3},
    "cadence": {"interval_s": 900, "event_sources": ["filings", "news", "schedule"]},
    "sizing": {"method": "conviction_linear", "entry_threshold": "0.3", "exit_threshold": "0.3",
      "rebalance_band": "0.05"}},
  "protection": {"enabled": true, "stop_distance": "0.08", "take_profit_distance": "0.1",
    "crypto_stop_limit_offset": "0.005"},
  "risk": {"max_position_usd": "10000", "max_position_fraction": "1", "max_gross_exposure_usd": "10000",
    "max_order_usd": "1000", "max_orders_per_day": 50, "max_daily_loss": "0.02",
    "daily_loss_action": "flatten_and_pause", "max_drawdown": "0.08",
    "drawdown_ladder": [{"at": "0.03", "action": "scale_sizes", "factor": "0.5"},
      {"at": "0.06", "action": "exits_only", "factor": null},
      {"at": "0.08", "action": "flatten_and_pause", "factor": null}],
    "hysteresis": "0.01", "scale_action": "trim_to_target", "breach_confirm_s": 60,
    "daily_breach_min_s": 3600, "scale_lift_after_s": 600, "reentry_cooldown_s": 3600},
  "autonomy": {"rules": [
      {"id": "large_orders", "when": {"any": [{"field": "order_usd", "op": "gt", "value": "900"}]}, "then": "ask"},
      {"id": "not_new", "when": {"not": {"field": "new_instrument", "op": "eq", "value": true}}, "then": "deny"},
      {"id": "routine", "when": {"all": [{"field": "purpose", "op": "in", "value": ["increase", "open"]},
        {"field": "session", "op": "eq", "value": "regular"}]}, "then": "auto"}],
    "default": "deny", "admission": "auto",
    "approval": {"timeout_s": 600, "on_timeout": "skip", "approvers": ["role:approver", "user:bob"],
      "two_approver_above_usd": "5000"}},
  "notifications": {"channels": ["phone", "slack", "sms", "telegram"],
    "quiet_hours": {"start": "23:05", "end": "07:30", "timezone": "America/New_York"}}
}"#;

fn json(text: &str) -> Result<Value, String> {
    mandate_canon::parse(text.as_bytes()).map_err(|e| format!("{text}: {e:?}"))
}

/// The base with the value at `path` replaced (or, with `None`, removed).
fn with(path: &str, value: Option<Value>) -> Result<Value, String> {
    let mut document = json(BASE)?;
    let tokens: Vec<&str> = path.split('/').skip(1).collect();
    let (last, parents) = tokens.split_last().ok_or("an empty pointer")?;
    let mut node = &mut document;
    for token in parents {
        node = match node {
            Value::Object(members) => members.get_mut(*token),
            Value::Array(items) => token.parse::<usize>().ok().and_then(|n| items.get_mut(n)),
            _ => None,
        }
        .ok_or_else(|| format!("{path} names nothing in the base"))?;
    }
    match (node, value) {
        (Value::Object(members), Some(v)) => {
            members.insert(Key::new(last).map_err(|e| e.to_string())?, v);
        }
        (Value::Object(members), None) => {
            members.remove(*last);
        }
        (Value::Array(items), Some(v)) => {
            let slot = last
                .parse::<usize>()
                .ok()
                .and_then(|n| items.get_mut(n))
                .ok_or_else(|| format!("{path} names nothing in the base"))?;
            *slot = v;
        }
        _ => return Err(format!("{path} cannot be set")),
    }
    Ok(document)
}

fn text(s: &str) -> Value {
    Value::Str(s.to_owned())
}

fn repeat(unit: &str, n: usize) -> Value {
    text(&unit.repeat(n))
}

fn items(item: &Value, n: usize) -> Value {
    Value::Array(vec![item.clone(); n])
}

/// The code and pointer of a parse, `ok` when it accepts.
fn outcome(document: &Value) -> (String, String) {
    let Err(error) = Mandate::parse(document) else {
        return ("ok".to_owned(), String::new());
    };
    let pointer = match &error {
        ParseError::UnknownMember { path }
        | ParseError::MissingMember { path }
        | ParseError::DecimalAsNumber { path }
        | ParseError::WrongType { path }
        | ParseError::NotInEnum { path }
        | ParseError::OffPattern { path }
        | ParseError::OutOfBounds { path }
        | ParseError::NotUnique { path }
        | ParseError::TooDeep { path }
        | ParseError::OffGrammar { path, .. } => path.as_str().to_owned(),
        _ => String::new(),
    };
    (error.code().to_owned(), pointer)
}

/// Each row: set `path` to a value, and the code the schema's rule for that path gives. A rejection
/// names `path` itself unless the row gives the pointer of an item inside it.
fn check(rows: &[(&str, Value, &str, Option<&str>)]) -> Result<(), String> {
    for (path, value, want, at) in rows {
        let (code, pointer) = outcome(&with(path, Some(value.clone()))?);
        assert_eq!(code, *want, "{path} = {value:?}");
        if *want != "ok" {
            assert_eq!(pointer, at.unwrap_or(path), "{path} = {value:?}");
        }
    }
    Ok(())
}

#[test]
fn the_filled_base_parses_into_the_branches_it_states() -> Result<(), String> {
    let mandate = Mandate::parse(&json(BASE)?).map_err(|e| e.code().to_owned())?;
    assert_eq!(mandate.environment, Environment::Live);
    assert!(mandate.source_text_ref.is_some());
    assert_eq!(
        mandate.goal.end_date(),
        Date::parse("2026-12-31").ok().as_ref()
    );
    assert_eq!(mandate.goal.on_complete(), Some(OnComplete::Release));
    assert!(matches!(
        mandate.goal,
        Goal::Accumulate {
            max_avg_price: Some(_),
            ..
        }
    ));
    assert_eq!(mandate.universe.asset_classes.len(), 2);
    assert_eq!(mandate.universe.pinned_instruments.len(), 1);
    assert_eq!(
        mandate
            .behavior
            .cadence
            .event_sources
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        vec![
            EventSource::Filings,
            EventSource::News,
            EventSource::Schedule
        ]
    );
    let params: Vec<&ParamValue> = mandate
        .behavior
        .signal_models
        .iter()
        .flat_map(|m| m.params.iter().map(|p| &p.value))
        .collect();
    assert!(matches!(params.as_slice(), [
        ParamValue::Decimal(_),
        ParamValue::Bool(true),
        ParamValue::Text(mode),
    ] if mode == "fast"));
    assert!(mandate.behavior.research.is_some());
    assert!(mandate.protection.take_profit_distance.is_some());
    assert!(mandate.protection.crypto_stop_limit_offset.is_some());
    assert_eq!(mandate.risk.daily_loss_action, LimitAction::FlattenAndPause);
    assert_eq!(mandate.risk.scale_action, ScaleAction::TrimToTarget);
    assert_eq!(
        mandate
            .risk
            .drawdown_ladder
            .iter()
            .map(|r| r.action)
            .collect::<Vec<_>>(),
        vec![
            LadderAction::ScaleSizes,
            LadderAction::ExitsOnly,
            LadderAction::FlattenAndPause
        ]
    );
    assert_eq!(mandate.autonomy.default, AutonomyDecision::Deny);
    assert_eq!(mandate.autonomy.admission, AutonomyDecision::Auto);
    assert_eq!(
        mandate
            .autonomy
            .rules
            .iter()
            .map(|r| r.then)
            .collect::<Vec<_>>(),
        vec![
            AutonomyDecision::Ask,
            AutonomyDecision::Deny,
            AutonomyDecision::Auto
        ]
    );
    let whens: Vec<&Condition> = mandate.autonomy.rules.iter().map(|r| &r.when).collect();
    assert!(matches!(whens.as_slice(), [
        Condition::Any(any),
        Condition::Not(not),
        Condition::All(all),
    ] if any.len() == 1
        && matches!(**not, Condition::Compare { value: ConditionValue::Bool(true), .. })
        && matches!(all.as_slice(), [
            Condition::Compare { value: ConditionValue::List(list), .. },
            Condition::Compare { value: ConditionValue::Text(session), .. },
        ] if list.len() == 2 && session == "regular")));
    assert!(matches!(
        mandate.autonomy.rules.first().map(|r| &r.when),
        Some(Condition::Any(any)) if matches!(any.as_slice(),
            [Condition::Compare { value: ConditionValue::Decimal(d), .. }] if d.as_str() == "900")
    ));
    assert_eq!(mandate.autonomy.approval.approvers.len(), 2);
    assert!(mandate.autonomy.approval.two_approver_above_usd.is_some());
    assert_eq!(
        mandate
            .notifications
            .channels
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        vec![
            Channel::Phone,
            Channel::Slack,
            Channel::Sms,
            Channel::Telegram
        ]
    );
    let quiet = mandate.notifications.quiet_hours.ok_or("quiet hours")?;
    assert_eq!(
        quiet.start,
        HourMinute {
            hour: 23,
            minute: 5
        }
    );
    assert_eq!(
        quiet.end,
        HourMinute {
            hour: 7,
            minute: 30
        }
    );
    Ok(())
}

#[test]
fn every_string_pattern_on_both_sides_of_its_edge() -> Result<(), String> {
    let digest = |unit: &str, n: usize| text(&format!("sha256:{}", unit.repeat(n)));
    check(&[
        ("/name", text("a"), "ok", None),
        ("/name", text("0-a-9"), "ok", None),
        ("/name", repeat("a", 63), "ok", None),
        ("/name", repeat("a", 64), "off_pattern", None),
        ("/name", text("-a"), "off_pattern", None),
        ("/name", text("a_b"), "off_pattern", None),
        ("/name", text(""), "off_pattern", None),
        ("/name", Value::Null, "wrong_type", None),
        ("/connection_id", text("A_b-9"), "ok", None),
        ("/connection_id", repeat("a", 64), "ok", None),
        ("/connection_id", repeat("a", 65), "off_pattern", None),
        ("/connection_id", text(""), "off_pattern", None),
        ("/connection_id", text("a.b"), "off_pattern", None),
        ("/source_text_ref", Value::Null, "ok", None),
        ("/source_text_ref", digest("A", 64), "off_pattern", None),
        ("/source_text_ref", digest("a", 63), "off_pattern", None),
        ("/source_text_ref", repeat("a", 71), "off_pattern", None),
        (
            "/goal/instrument",
            text("7B4A1C2E-1111-4a2b-9c3d-000000000001"),
            "off_pattern",
            None,
        ),
        ("/goal/end_date", text("2026-01-01"), "ok", None),
        ("/goal/end_date", text("2026-10-31"), "ok", None),
        ("/goal/end_date", text("2026-00-10"), "off_pattern", None),
        ("/goal/end_date", text("2026-01-00"), "off_pattern", None),
        ("/goal/end_date", text("2026-01-32"), "off_pattern", None),
        ("/goal/end_date", text("2026-1-010"), "off_pattern", None),
        ("/goal/end_date", text("2026/01/01"), "off_pattern", None),
        ("/goal/end_date", text("2026-01/01"), "off_pattern", None),
        ("/goal/end_date", text("20a6-01-01"), "off_pattern", None),
        ("/goal/end_date", text("2026-0a-01"), "off_pattern", None),
        ("/goal/end_date", text("2026-01-01 "), "off_pattern", None),
        (
            "/notifications/quiet_hours/start",
            text("00:00"),
            "ok",
            None,
        ),
        (
            "/notifications/quiet_hours/start",
            text("23:59"),
            "ok",
            None,
        ),
        (
            "/notifications/quiet_hours/start",
            text("24:00"),
            "off_pattern",
            None,
        ),
        (
            "/notifications/quiet_hours/start",
            text("12:60"),
            "off_pattern",
            None,
        ),
        (
            "/notifications/quiet_hours/start",
            text("1:000"),
            "off_pattern",
            None,
        ),
        (
            "/notifications/quiet_hours/start",
            text("12-00"),
            "off_pattern",
            None,
        ),
        (
            "/notifications/quiet_hours/start",
            text("12:000"),
            "off_pattern",
            None,
        ),
        (
            "/notifications/quiet_hours/timezone",
            text("UTC"),
            "not_in_enum",
            None,
        ),
        (
            "/behavior/signal_models/0/version",
            text("0.0.0"),
            "ok",
            None,
        ),
        (
            "/behavior/signal_models/0/version",
            text("999999.10.0"),
            "ok",
            None,
        ),
        (
            "/behavior/signal_models/0/version",
            text("1000000.0.0"),
            "off_pattern",
            None,
        ),
        (
            "/behavior/signal_models/0/version",
            text("01.0.0"),
            "off_pattern",
            None,
        ),
        (
            "/behavior/signal_models/0/version",
            text("1.0"),
            "off_pattern",
            None,
        ),
        (
            "/behavior/signal_models/0/version",
            text("1.0.0.0"),
            "off_pattern",
            None,
        ),
        (
            "/behavior/signal_models/0/version",
            text("1..0"),
            "off_pattern",
            None,
        ),
        (
            "/behavior/signal_models/0/version",
            text("1.a.0"),
            "off_pattern",
            None,
        ),
        (
            "/behavior/signal_models/0/params/0/key",
            text("Lookback"),
            "off_pattern",
            None,
        ),
        (
            "/behavior/signal_models/0/params/0/key",
            repeat("a", 64),
            "ok",
            None,
        ),
        (
            "/behavior/signal_models/0/params/0/key",
            repeat("a", 65),
            "off_pattern",
            None,
        ),
        ("/autonomy/rules/0/id", text("Large"), "off_pattern", None),
        (
            "/autonomy/approval/approvers/1",
            text("nobody"),
            "off_pattern",
            None,
        ),
        (
            "/autonomy/approval/approvers/1",
            Value::Bool(true),
            "wrong_type",
            None,
        ),
    ])
}

#[test]
fn every_length_and_count_on_both_sides_of_its_bound() -> Result<(), String> {
    let pinned = json(
        r#"{"asset_id": "7b4a1c2e-1111-4a2b-9c3d-000000000001", "symbol": "B", "asset_class": "crypto"}"#,
    )?;
    let pinned_many = Value::Array(
        (0..21)
            .map(|n| {
                json(&format!(
                    r#"{{"asset_id": "7b4a1c2e-1111-4a2b-9c3d-0000000000{n:02}", "symbol": "B", "asset_class": "crypto"}}"#
                ))
            })
            .collect::<Result<_, _>>()?,
    );
    let rung = json(r#"{"at": "0.03", "action": "scale_sizes", "factor": "0.5"}"#)?;
    let param = json(r#"{"key": "k", "value": "1"}"#)?;
    let compare = json(r#"{"field": "order_usd", "op": "gt", "value": "1"}"#)?;
    let rule = json(
        r#"{"id": "r", "when": {"field": "order_usd", "op": "gt", "value": "1"}, "then": "ask"}"#,
    )?;
    let models = "/behavior/signal_models";
    let model = json(BASE)?
        .get("behavior")
        .and_then(|b| b.get("signal_models"))
        .and_then(|m| m.as_array())
        .and_then(|m| m.first())
        .cloned()
        .ok_or("the base's model")?;
    check(&[
        (
            "/universe/pinned_instruments/0/symbol",
            repeat("é", 32),
            "ok",
            None,
        ),
        (
            "/universe/pinned_instruments/0/symbol",
            repeat("a", 33),
            "out_of_bounds",
            None,
        ),
        (
            "/universe/pinned_instruments/0/symbol",
            text(""),
            "out_of_bounds",
            None,
        ),
        ("/behavior/description", repeat("é", 4000), "ok", None),
        (
            "/behavior/description",
            repeat("a", 4001),
            "out_of_bounds",
            None,
        ),
        (
            "/behavior/signal_models/0/params/2/value",
            repeat("é", 200),
            "ok",
            None,
        ),
        (
            "/behavior/signal_models/0/params/2/value",
            repeat("a", 201),
            "out_of_bounds",
            None,
        ),
        (
            "/autonomy/rules/2/when/all/1/value",
            repeat("é", 64),
            "ok",
            None,
        ),
        (
            "/autonomy/rules/2/when/all/1/value",
            repeat("a", 65),
            "out_of_bounds",
            None,
        ),
        (
            "/autonomy/rules/2/when/all/0/value",
            Value::Array(vec![repeat("a", 65)]),
            "out_of_bounds",
            Some("/autonomy/rules/2/when/all/0/value/0"),
        ),
        (
            "/autonomy/rules/2/when/all/0/value",
            items(&text("open"), 50),
            "ok",
            None,
        ),
        (
            "/autonomy/rules/2/when/all/0/value",
            items(&text("open"), 51),
            "out_of_bounds",
            None,
        ),
        (
            "/autonomy/rules/2/when/all",
            items(&compare, 10),
            "ok",
            None,
        ),
        (
            "/autonomy/rules/2/when/all",
            items(&compare, 11),
            "out_of_bounds",
            None,
        ),
        (
            "/autonomy/rules/0/when/any",
            Value::Array(vec![]),
            "out_of_bounds",
            None,
        ),
        (
            "/universe/pinned_instruments",
            pinned_many,
            "out_of_bounds",
            None,
        ),
        (
            "/universe/pinned_instruments",
            items(&pinned, 2),
            "not_unique",
            Some("/universe/pinned_instruments/1"),
        ),
        (
            "/universe/asset_classes",
            Value::Array(vec![text("crypto"), text("crypto")]),
            "not_unique",
            Some("/universe/asset_classes/1"),
        ),
        (
            "/behavior/cadence/event_sources",
            Value::Array(vec![]),
            "ok",
            None,
        ),
        (
            "/behavior/cadence/event_sources",
            Value::Array(vec![text("rumours")]),
            "not_in_enum",
            Some("/behavior/cadence/event_sources/0"),
        ),
        (
            "/notifications/channels",
            Value::Array(vec![]),
            "out_of_bounds",
            None,
        ),
        (
            "/notifications/channels",
            Value::Array(vec![text("email"), text("email")]),
            "not_unique",
            Some("/notifications/channels/1"),
        ),
        (
            "/autonomy/approval/approvers",
            Value::Array(vec![]),
            "out_of_bounds",
            None,
        ),
        (
            "/autonomy/approval/approvers",
            items(&text("role:owner"), 2),
            "not_unique",
            Some("/autonomy/approval/approvers/1"),
        ),
        ("/risk/drawdown_ladder", items(&rung, 5), "ok", None),
        (
            "/risk/drawdown_ladder",
            items(&rung, 6),
            "out_of_bounds",
            None,
        ),
        (
            "/risk/drawdown_ladder",
            Value::Array(vec![]),
            "out_of_bounds",
            None,
        ),
        (
            "/behavior/signal_models/0/params",
            items(&param, 32),
            "ok",
            None,
        ),
        (
            "/behavior/signal_models/0/params",
            items(&param, 33),
            "out_of_bounds",
            None,
        ),
        (models, items(&model, 10), "ok", None),
        (models, items(&model, 11), "out_of_bounds", None),
        (models, Value::Array(vec![]), "out_of_bounds", None),
        ("/autonomy/rules", items(&rule, 50), "ok", None),
        ("/autonomy/rules", items(&rule, 51), "out_of_bounds", None),
        ("/autonomy/rules", Value::Array(vec![]), "ok", None),
        ("/autonomy/rules", text("none"), "wrong_type", None),
    ])
}

#[test]
fn every_value_type_the_schema_refuses_names_its_pointer() -> Result<(), String> {
    let int = |n: u64| json(&n.to_string());
    check(&[
        ("/mandate_schema_version", int(2)?, "not_in_enum", None),
        ("/mandate_schema_version", text("1"), "not_in_enum", None),
        ("/capital", text("x"), "wrong_type", None),
        (
            "/capital/allocation_usd",
            Value::Bool(true),
            "wrong_type",
            None,
        ),
        (
            "/behavior/cadence/interval_s",
            text("900"),
            "wrong_type",
            None,
        ),
        (
            "/behavior/cadence/interval_s",
            int(4_294_967_296)?,
            "out_of_bounds",
            None,
        ),
        ("/universe/pinned", text("true"), "wrong_type", None),
        (
            "/behavior/signal_models/0/params/1/value",
            Value::Null,
            "wrong_type",
            None,
        ),
        (
            "/behavior/signal_models/0/params/1/value",
            text("-0"),
            "ok",
            None,
        ),
        ("/autonomy/rules/0/when", text("always"), "wrong_type", None),
        (
            "/autonomy/rules/0/when/any/0/value",
            int(900)?,
            "wrong_type",
            None,
        ),
        (
            "/autonomy/rules/0/when/any/0/value",
            Value::Array(vec![Value::Bool(true)]),
            "wrong_type",
            Some("/autonomy/rules/0/when/any/0/value/0"),
        ),
        (
            "/autonomy/rules/0/when/any/0/value",
            text("lots"),
            "ok",
            None,
        ),
        (
            "/autonomy/rules/0/when/any/0/op",
            text("like"),
            "not_in_enum",
            None,
        ),
        (
            "/autonomy/rules/0/when/extra",
            Value::Bool(true),
            "unknown_member",
            None,
        ),
        (
            "/autonomy/rules/1/when/extra",
            Value::Bool(true),
            "unknown_member",
            None,
        ),
        ("/autonomy/rules/1/then", text("maybe"), "not_in_enum", None),
        ("/autonomy/default", text("maybe"), "not_in_enum", None),
        ("/goal", text("accumulate"), "wrong_type", None),
        (
            "/goal",
            json(r#"{}"#)?,
            "missing_member",
            Some("/goal/type"),
        ),
        ("/goal/extra", Value::Null, "unknown_member", None),
        (
            "/goal",
            json(r#"{"type": "profit_stop", "profit_level": "500", "end_date": null}"#)?,
            "ok",
            None,
        ),
        (
            "/goal",
            json(
                r#"{"type": "profit_stop", "profit_level": "500", "end_date": null, "on_complete": "release"}"#,
            )?,
            "unknown_member",
            Some("/goal/on_complete"),
        ),
        (
            "/goal",
            json(r#"{"type": "continuous", "end_date": null, "on_complete": "disarm_ladder"}"#)?,
            "ok",
            None,
        ),
        ("/protection/enabled", Value::Bool(false), "ok", None),
        ("/notifications/quiet_hours", Value::Null, "ok", None),
        ("/behavior/research", Value::Null, "ok", None),
    ])
}

#[test]
fn a_disabled_protection_keeps_the_stop_it_was_given() -> Result<(), String> {
    let off = |stop: Value| -> Result<Option<String>, String> {
        let document = with("/protection/enabled", Some(Value::Bool(false)))?;
        let document = match document {
            Value::Object(mut members) => {
                if let Some(Value::Object(protection)) = members.get_mut("protection") {
                    protection.insert(Key::new("stop_distance").map_err(|e| e.to_string())?, stop);
                }
                Value::Object(members)
            }
            other => other,
        };
        let mandate = Mandate::parse(&document).map_err(|e| e.code().to_owned())?;
        Ok(mandate
            .protection
            .stop_distance
            .map(|d| d.as_str().to_owned()))
    };
    assert_eq!(off(text("0.05"))?, Some("0.05".to_owned()));
    assert_eq!(off(Value::Null)?, None);
    Ok(())
}

/// The schema's `date` is a pattern, so a day no calendar has is a valid document (MC-V22) that V-015
/// rejects: the typed goal has no date for it, and the text stays in the canonical form.
#[test]
fn a_pattern_valid_date_no_calendar_has_parses_with_no_typed_date() -> Result<(), String> {
    for day in ["2026-02-30", "0000-01-01"] {
        let document = with("/goal/end_date", Some(text(day)))?;
        let mandate = Mandate::parse(&document).map_err(|e| e.code().to_owned())?;
        assert_eq!(mandate.goal.end_date(), None, "{day}");
        assert_eq!(
            mandate.canonical_bytes().map_err(|e| e.code().to_owned())?,
            mandate_canon::to_canonical(&document),
            "{day} is kept as the owner wrote it"
        );
    }
    Ok(())
}

/// A condition is read to one level past V-017's limit, so V-017 reports depth 5 and the parse
/// stops at 6 without recursing further (DEC-151).
#[test]
fn conditions_are_read_to_depth_five_and_refused_at_six() -> Result<(), String> {
    let nest = |levels: usize| -> Result<Value, String> {
        let mut condition = json(r#"{"field": "order_usd", "op": "gt", "value": "1"}"#)?;
        for _ in 1..levels {
            condition = json(&format!(
                r#"{{"any": [{}]}}"#,
                String::from_utf8(mandate_canon::to_canonical(&condition))
                    .map_err(|e| e.to_string())?
            ))?;
        }
        Ok(condition)
    };
    check(&[
        ("/autonomy/rules/0/when", nest(5)?, "ok", None),
        (
            "/autonomy/rules/0/when",
            nest(6)?,
            "too_deep",
            Some("/autonomy/rules/0/when/any/0/any/0/any/0/any/0/any/0"),
        ),
    ])
}

/// The fields are public, so a version must refuse to name a mandate whose fields no longer match
/// the document it was parsed from.
#[test]
fn a_mandate_changed_after_parsing_has_no_version() -> Result<(), String> {
    let mandate = Mandate::parse(&json(BASE)?).map_err(|e| e.code().to_owned())?;
    assert!(mandate.version().is_ok());
    let mut changed = mandate.clone();
    changed.risk.max_orders_per_day = 51;
    assert_eq!(changed.canonical().err(), Some(ParseError::Diverged));
    assert_eq!(changed.canonical_bytes().err(), Some(ParseError::Diverged));
    assert_eq!(changed.version().err(), Some(ParseError::Diverged));
    assert_eq!(ParseError::Diverged.code(), "diverged");
    assert_eq!(
        ParseError::NotUnique {
            path: crate::Pointer::new("/a")
        }
        .code(),
        "not_unique"
    );
    Ok(())
}
