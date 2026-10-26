//! The reviewed rule set's gate configuration, the one reading both gates take (DEC-2388 item 2).

use mandate_gate_context::{RuleSetError, gate_rule_set};
use mandate_num::{Fraction, Usd};
use mandate_risk::GateConfig;

const RULES: &str = include_str!("../../mandate-shell/tests/fixtures/tracer/config/rule-set.json");

fn text<E: std::fmt::Display>(error: E) -> String {
    error.to_string()
}

fn read(json: &str) -> Result<GateConfig, String> {
    let value = mandate_canon::parse(json.trim().as_bytes()).map_err(text)?;
    gate_rule_set(&value).map_err(|error| match error {
        RuleSetError::Missing { what } => format!("missing {what}"),
        RuleSetError::Num(error) => format!("num {error}"),
    })
}

fn usd(text_: &str) -> Result<Usd, String> {
    Usd::parse(text_).map_err(text)
}

fn fraction(text_: &str) -> Result<Fraction, String> {
    Fraction::parse(text_).map_err(text)
}

#[test]
fn the_reviewed_gate_configuration_reads_whole() -> Result<(), String> {
    assert_eq!(
        read(RULES)?,
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
    Ok(())
}

#[test]
fn another_gate_version_or_story_is_refused() {
    for (from, to, what) in [
        (
            "\"gate\":\"trading-domain-9.1\"",
            "\"gate\":\"trading-domain-9.2\"",
            "gate",
        ),
        ("\"gate\":\"trading-domain-9.1\"", "\"gate\":1", "gate"),
        (
            "\"ruleset_version\":\"v1\"",
            "\"ruleset_version\":\"v2\"",
            "ruleset_version",
        ),
        ("\"story\":\"E7-7\"", "\"story\":\"E7-8\"", "story"),
        ("\"story\":\"E7-7\"", "\"tale\":\"E7-7\"", "story"),
    ] {
        let changed = RULES.replace(from, to);
        assert_ne!(changed, RULES, "{from}");
        assert_eq!(read(&changed), Err(format!("missing {what}")), "{to}");
    }
}

#[test]
fn the_gate_configuration_holds_exactly_its_reviewed_members() {
    for (from, to) in [
        ("\"price_floor\":\"5\"", "\"price_floor\":\"5\",\"q\":\"1\""),
        ("\"price_floor\":\"5\"", "\"q\":\"5\""),
        ("\"gate_config\":{", "\"gate_config\":{\"a\":1,"),
        ("\"gate_config\":", "\"gate_settings\":"),
    ] {
        let changed = RULES.replace(from, to);
        assert_ne!(changed, RULES, "{from}");
        assert_eq!(
            read(&changed),
            Err("missing gate_config".to_owned()),
            "{to}"
        );
    }
}

#[test]
fn every_count_must_be_positive_and_fit_a_u32() {
    for (name, value) in [
        ("close_window_minutes", "10"),
        ("etp_classification_max_age_s", "604800"),
        ("min_resting_time_s", "2"),
        ("opposite_fill_interval_s", "60"),
        ("order_to_fill_max", "10"),
        ("order_to_fill_min_orders", "20"),
    ] {
        let reviewed = format!("\"{name}\":{value}");
        assert!(RULES.contains(&reviewed), "{name}");
        for changed in ["0", "4294967296", "\"10\""] {
            let rules = RULES.replace(&reviewed, &format!("\"{name}\":{changed}"));
            assert_eq!(
                read(&rules),
                Err(format!("missing {name}")),
                "{name} = {changed}"
            );
        }
        let largest = RULES.replace(&reviewed, &format!("\"{name}\":4294967295"));
        assert!(read(&largest).is_ok(), "{name} at u32::MAX");
        let one = RULES.replace(&reviewed, &format!("\"{name}\":1"));
        assert!(read(&one).is_ok(), "{name} = 1");
    }
}

#[test]
fn every_amount_is_read_exactly_or_refused() {
    for (name, value) in [
        ("price_floor", "5"),
        ("liquidity_floor_usd", "1000000"),
        ("crypto_liquidity_floor_usd", "1000000"),
        ("collar_liquid_threshold_usd", "50000000"),
        ("legacy_pdt_equity_threshold", "25000"),
        ("collar_liquid_x", "0.01"),
        ("collar_other_x", "0.02"),
        ("collar_crypto_x", "0.02"),
        ("collar_passive_band", "0.2"),
        ("order_size_participation", "0.05"),
        ("daily_participation", "0.05"),
    ] {
        let reviewed = format!("\"{name}\":\"{value}\"");
        assert!(RULES.contains(&reviewed), "{name}");
        let unreadable = RULES.replace(&reviewed, &format!("\"{name}\":\"x\""));
        assert!(
            read(&unreadable).is_err_and(|error| error.starts_with("num ")),
            "{name} unreadable"
        );
        let untyped = RULES.replace(&reviewed, &format!("\"{name}\":5"));
        assert_eq!(
            read(&untyped),
            Err(format!("missing {name}")),
            "{name} untyped"
        );
    }
}
