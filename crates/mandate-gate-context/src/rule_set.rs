//! The reviewed rule set's gate configuration (E7-7, DEC-505), read once for every process whose
//! gate judges an order: the executor's binding gate through `mandate-gate-inputs`, and the cell's
//! runtime's advisory pass, which may not reach the executor (DEC-2387 item 2). One reading, so the
//! two gates can never judge one rule set two ways.

use mandate_canon::{Key, Value};
use mandate_num::{Fraction, NumError, Usd};
use mandate_risk::GateConfig;

/// A rule set the gate cannot use: each refuses the reading rather than guessing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuleSetError {
    /// A member is missing, has the wrong type, or holds a value the reviewed rules refuse.
    #[error("missing or unusable: {what}")]
    Missing { what: &'static str },
    #[error(transparent)]
    Num(#[from] NumError),
}

fn missing(what: &'static str) -> RuleSetError {
    RuleSetError::Missing { what }
}

const GATE_CONFIG_MEMBERS: &[&str] = &[
    "close_window_minutes",
    "collar_crypto_x",
    "collar_liquid_threshold_usd",
    "collar_liquid_x",
    "collar_other_x",
    "collar_passive_band",
    "crypto_liquidity_floor_usd",
    "daily_participation",
    "etp_classification_max_age_s",
    "legacy_pdt_equity_threshold",
    "liquidity_floor_usd",
    "min_resting_time_s",
    "opposite_fill_interval_s",
    "order_size_participation",
    "order_to_fill_max",
    "order_to_fill_min_orders",
    "price_floor",
];

/// The gate configuration of the reviewed rule set `rule_set`: a `trading-domain-9.1` gate at
/// rule-set version `v1` of story E7-7, whose `gate_config` holds exactly its reviewed members.
///
/// # Errors
/// [`RuleSetError`] for another gate, version or story, and for a member it cannot read.
pub fn gate_rule_set(rule_set: &Value) -> Result<GateConfig, RuleSetError> {
    require_text(rule_set, "gate", "trading-domain-9.1")?;
    require_text(rule_set, "ruleset_version", "v1")?;
    require_text(rule_set, "story", "E7-7")?;
    let value = rule_set
        .get("gate_config")
        .filter(|nested| {
            nested.as_object().is_some_and(|object| {
                object
                    .keys()
                    .map(Key::as_str)
                    .eq(GATE_CONFIG_MEMBERS.iter().copied())
            })
        })
        .ok_or_else(|| missing("gate_config"))?;
    Ok(GateConfig {
        price_floor: usd(value, "price_floor")?,
        liquidity_floor_usd: usd(value, "liquidity_floor_usd")?,
        crypto_liquidity_floor_usd: usd(value, "crypto_liquidity_floor_usd")?,
        collar_liquid_threshold_usd: usd(value, "collar_liquid_threshold_usd")?,
        collar_liquid_x: fraction(value, "collar_liquid_x")?,
        collar_other_x: fraction(value, "collar_other_x")?,
        collar_crypto_x: fraction(value, "collar_crypto_x")?,
        collar_passive_band: fraction(value, "collar_passive_band")?,
        opposite_fill_interval_s: positive_u32(value, "opposite_fill_interval_s")?,
        min_resting_time_s: positive_u32(value, "min_resting_time_s")?,
        order_to_fill_max: positive_u32(value, "order_to_fill_max")?,
        order_to_fill_min_orders: positive_u32(value, "order_to_fill_min_orders")?,
        order_size_participation: fraction(value, "order_size_participation")?,
        daily_participation: fraction(value, "daily_participation")?,
        close_window_minutes: positive_u32(value, "close_window_minutes")?,
        legacy_pdt_equity_threshold: usd(value, "legacy_pdt_equity_threshold")?,
        etp_classification_max_age_s: positive_u32(value, "etp_classification_max_age_s")?,
    })
}

fn text<'a>(value: &'a Value, name: &'static str) -> Result<&'a str, RuleSetError> {
    value
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| missing(name))
}

fn require_text(value: &Value, name: &'static str, expected: &str) -> Result<(), RuleSetError> {
    if text(value, name)? == expected {
        Ok(())
    } else {
        Err(missing(name))
    }
}

fn usd(value: &Value, name: &'static str) -> Result<Usd, RuleSetError> {
    Ok(Usd::parse(text(value, name)?)?)
}

fn fraction(value: &Value, name: &'static str) -> Result<Fraction, RuleSetError> {
    Ok(Fraction::parse(text(value, name)?)?)
}

fn positive_u32(value: &Value, name: &'static str) -> Result<u32, RuleSetError> {
    value
        .get(name)
        .and_then(Value::as_int)
        .filter(|number| *number > 0)
        .and_then(|number| u32::try_from(number).ok())
        .ok_or_else(|| missing(name))
}
