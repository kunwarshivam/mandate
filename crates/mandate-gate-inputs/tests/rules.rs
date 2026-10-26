//! The reviewed rule set and fee schedule, read from the tracer's reviewed artifacts and refused
//! member by member (DEC-2380 item 5).

use std::time::Duration;

use mandate_domain::Environment;
use mandate_executor::ExecutorConfig;
use mandate_gate_inputs::{
    Artifact, CALENDAR_MEMBERS, InputError, RULE_MEMBERS, artifact_from, environment_text,
    fee_artifact, fees, reference, required_positive_u64, required_text, rule_set,
};
use mandate_num::{Fraction, Usd};
use mandate_risk::GateConfig;

const FEE: &str = include_str!("../../mandate-shell/tests/fixtures/tracer/config/fee-config.json");
const CALENDAR: &str =
    include_str!("../../mandate-shell/tests/fixtures/tracer/config/trading-calendar.json");
const RULES: &str = include_str!("../../mandate-shell/tests/fixtures/tracer/config/rule-set.json");
const LIVE_FEE: &str = r#"{"broker":"robinhood","effective_from":"2026-01-01","environment":"live","schedule":"robinhood_live_2026_10_16"}"#;

fn text<E: std::fmt::Display>(error: E) -> String {
    error.to_string()
}

fn rules(text_: &str) -> Result<Artifact, InputError> {
    artifact_from(text_.trim().as_bytes().to_vec(), RULE_MEMBERS)
}

fn calendar(text_: &str) -> Result<Artifact, InputError> {
    artifact_from(text_.trim().as_bytes().to_vec(), CALENDAR_MEMBERS)
}

fn read(text_: &str) -> Result<(Duration, GateConfig, ExecutorConfig), InputError> {
    rule_set(&rules(text_)?)
}

fn missing(result: Result<impl std::fmt::Debug, InputError>) -> Option<&'static str> {
    match result {
        Err(InputError::Missing { what }) => Some(what),
        Err(_) | Ok(_) => None,
    }
}

fn usd(text_: &str) -> Result<Usd, String> {
    Usd::parse(text_).map_err(text)
}

fn fraction(text_: &str) -> Result<Fraction, String> {
    Fraction::parse(text_).map_err(text)
}

#[test]
fn the_reviewed_rule_set_reads_whole() -> Result<(), String> {
    let (quote_max_age, gate, executor) = read(RULES).map_err(text)?;
    assert_eq!(quote_max_age, Duration::from_secs(10));
    assert_eq!(
        gate,
        GateConfig {
            price_floor: usd("5")?,
            liquidity_floor_usd: usd("1000000")?,
            crypto_liquidity_floor_usd: usd("1000000")?,
            collar_liquid_threshold_usd: usd("50000000")?,
            collar_liquid_x: fraction("0.01")?,
            collar_other_x: fraction("0.02")?,
            collar_crypto_x: fraction("0.02")?,
            collar_passive_band: fraction("0.2")?,
            opposite_fill_interval_s: 60,
            min_resting_time_s: 2,
            order_to_fill_max: 10,
            order_to_fill_min_orders: 20,
            order_size_participation: fraction("0.05")?,
            daily_participation: fraction("0.05")?,
            close_window_minutes: 10,
            legacy_pdt_equity_threshold: usd("25000")?,
            etp_classification_max_age_s: 604_800,
        }
    );
    assert_eq!(
        executor,
        ExecutorConfig {
            max_intent_age_s: 120,
            unknown_absent_lookups: 3,
            unknown_absent_window_s: 15,
            protective_replace_buffer_trading_days: 5,
            restriction_403_threshold: 3,
            bracket_partial_fill_timeout_s: 60,
            max_unprotected_s: 60,
            stop_watchdog_s: 60,
            exit_step_s: 5,
            gtc_expiry_days: 90,
        }
    );
    Ok(())
}

#[test]
fn every_count_and_duration_must_be_positive_and_fit() {
    let counts = [
        ("bracket_partial_fill_timeout_s", "60"),
        ("exit_step_s", "5"),
        ("gtc_expiry_days", "90"),
        ("max_intent_age_s", "120"),
        ("max_unprotected_s", "60"),
        ("protective_replace_buffer_trading_days", "5"),
        ("restriction_403_threshold", "3"),
        ("stop_watchdog_s", "60"),
        ("unknown_absent_lookups", "3"),
        ("unknown_absent_window_s", "15"),
        ("close_window_minutes", "10"),
        ("etp_classification_max_age_s", "604800"),
        ("min_resting_time_s", "2"),
        ("opposite_fill_interval_s", "60"),
        ("order_to_fill_max", "10"),
        ("order_to_fill_min_orders", "20"),
    ];
    for (name, value) in counts {
        let reviewed = format!("\"{name}\":{value}");
        assert!(RULES.contains(&reviewed), "{name}");
        let changed = RULES.replace(&reviewed, &format!("\"{name}\":0"));
        assert_eq!(missing(read(&changed)), Some(name), "{name} = 0");
    }
    for name in [
        "gtc_expiry_days",
        "protective_replace_buffer_trading_days",
        "restriction_403_threshold",
        "unknown_absent_lookups",
        "close_window_minutes",
        "min_resting_time_s",
    ] {
        let reviewed = counts
            .iter()
            .find(|(count, _)| *count == name)
            .map(|(count, value)| format!("\"{count}\":{value}"))
            .unwrap_or_default();
        let changed = RULES.replace(&reviewed, &format!("\"{name}\":4294967296"));
        assert_eq!(missing(read(&changed)), Some(name), "{name} beyond u32");
    }
    let max_intent = RULES.replace(
        "\"max_intent_age_s\":120",
        "\"max_intent_age_s\":4294967296",
    );
    assert_eq!(
        read(&max_intent)
            .map(|(_, _, executor)| executor.max_intent_age_s)
            .ok(),
        Some(4_294_967_296),
        "an i64 duration holds more than a u32"
    );
}

#[test]
fn the_quote_age_bound_is_one_to_sixty_seconds() -> Result<(), String> {
    for (age, admitted) in [("0", false), ("1", true), ("60", true), ("61", false)] {
        let changed = RULES.replace(
            "\"iex_quote_max_age_s\":10",
            &format!("\"iex_quote_max_age_s\":{age}"),
        );
        let read = read(&changed);
        assert_eq!(read.is_ok(), admitted, "{age}");
        if let Ok((bound, _, _)) = read {
            assert_eq!(bound.as_secs().to_string(), age);
        }
    }
    Ok(())
}

#[test]
fn the_rule_sets_identity_and_shape_are_held() {
    for (from, to, what) in [
        (
            "\"gate\":\"trading-domain-9.1\"",
            "\"gate\":\"trading-domain-9.2\"",
            "gate",
        ),
        (
            "\"ruleset_version\":\"v1\"",
            "\"ruleset_version\":\"v2\"",
            "ruleset_version",
        ),
        ("\"story\":\"E7-7\"", "\"story\":\"E7-8\"", "story"),
        (
            "\"exit_step_s\":5,",
            "\"exit_step_s\":5,\"extra_s\":1,",
            "executor",
        ),
        (
            "\"price_floor\":\"5\"",
            "\"price_floor\":\"5\",\"q\":\"1\"",
            "gate_config",
        ),
        ("\"price_floor\":\"5\"", "\"price_floor\":\"x\"", ""),
    ] {
        let changed = RULES.replace(from, to);
        assert_ne!(changed, RULES, "{from}");
        let result = read(&changed);
        assert!(result.is_err(), "{from}");
        if !what.is_empty() {
            assert_eq!(missing(result), Some(what), "{from}");
        }
    }
    let extra = RULES.replacen('{', "{\"a\":1,", 1);
    assert!(rules(&extra).is_err(), "a member the run does not use");
    let short = RULES.replace(",\"story\":\"E7-7\"", "");
    assert!(rules(&short).is_err(), "a member the run needs");
    assert!(rules("not json").is_err());
}

#[test]
fn the_paper_schedule_holds_the_paper_environment() -> Result<(), String> {
    let calendar = calendar(CALENDAR).map_err(text)?;
    let fee = fee_artifact(FEE.trim().as_bytes().to_vec()).map_err(text)?;
    assert!(fees(&fee, &calendar).is_ok());
    let live =
        fee_artifact(FEE.trim().replace("\"paper\"", "\"live\"").into_bytes()).map_err(text)?;
    assert!(
        fees(&live, &calendar).is_err(),
        "the paper overestimate is paper-only"
    );
    let other = fee_artifact(
        FEE.trim()
            .replace("conservative_v1", "other_v1")
            .into_bytes(),
    )
    .map_err(text)?;
    assert_eq!(missing(fees(&other, &calendar)), Some("schedule"));
    Ok(())
}

#[test]
fn the_live_schedule_names_its_broker_and_environment() -> Result<(), String> {
    let calendar = calendar(CALENDAR).map_err(text)?;
    let live = fee_artifact(LIVE_FEE.as_bytes().to_vec()).map_err(text)?;
    assert!(fees(&live, &calendar).is_ok());
    let paper =
        fee_artifact(LIVE_FEE.replace("\"live\"", "\"paper\"").into_bytes()).map_err(text)?;
    assert!(fees(&paper, &calendar).is_err());
    let other =
        fee_artifact(LIVE_FEE.replace("robinhood\"", "alpaca\"").into_bytes()).map_err(text)?;
    assert!(fees(&other, &calendar).is_err());
    assert!(
        fee_artifact(
            FEE.trim()
                .replace("conservative_v1", "robinhood_live_2026_10_16")
                .into_bytes()
        )
        .is_err(),
        "the live schedule's artifact names its broker"
    );
    assert!(
        fee_artifact(
            LIVE_FEE
                .replace("robinhood_live_2026_10_16", "conservative_v1")
                .into_bytes()
        )
        .is_err(),
        "the paper artifact names no broker"
    );
    Ok(())
}

#[test]
fn the_calendar_holds_its_dates_and_no_exceptions() -> Result<(), String> {
    let fee = fee_artifact(FEE.trim().as_bytes().to_vec()).map_err(text)?;
    for (from, to) in [
        ("\"holidays\":[]", "\"holidays\":[\"2026-07-03\"]"),
        (
            "\"special_sessions\":[]",
            "\"special_sessions\":[\"2026-11-27\"]",
        ),
        ("\"holidays\":[]", "\"holidays\":\"none\""),
        ("\"first\":\"2026-01-01\"", "\"first\":\"2026-13-01\""),
        ("\"last\":\"2026-12-31\"", "\"last\":\"2026-12-32\""),
        ("\"first\":\"2026-01-01\"", "\"first\":\"2027-01-01\""),
    ] {
        let changed = CALENDAR.trim().replace(from, to);
        assert_ne!(changed, CALENDAR.trim(), "{from}");
        let calendar = calendar(&changed).map_err(text)?;
        assert!(fees(&fee, &calendar).is_err(), "{to}");
    }
    Ok(())
}

#[test]
fn references_are_the_sha256_of_the_bytes() {
    assert_eq!(
        reference(b""),
        "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        reference(b"abc"),
        "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn environments_are_named_as_the_fee_schedule_names_them() {
    assert_eq!(environment_text(Environment::Paper), "paper");
    assert_eq!(environment_text(Environment::Live), "live");
}

#[test]
fn members_are_read_by_type() -> Result<(), String> {
    let value = mandate_canon::parse(br#"{"n":1,"s":"x","z":0}"#).map_err(text)?;
    assert_eq!(required_positive_u64(&value, "n").map_err(text)?, 1);
    assert_eq!(missing(required_positive_u64(&value, "z")), Some("z"));
    assert_eq!(missing(required_positive_u64(&value, "s")), Some("s"));
    assert_eq!(required_text(&value, "s").map_err(text)?, "x");
    assert_eq!(missing(required_text(&value, "n")), Some("n"));
    assert_eq!(missing(required_text(&value, "absent")), Some("absent"));
    Ok(())
}
