//! The reviewed rule set and fee schedule, each judged on the bytes the executor's config
//! references hash (DEC-470 item 2; E19-11, DEC-505), moved here from the paper shell unchanged
//! (DEC-2380 item 5).

use std::time::Duration;

use mandate_canon::{Digest, Key, Value};
use mandate_domain::Environment;
use mandate_executor::ExecutorConfig;
use mandate_gate_context::gate_rule_set;
use mandate_risk::GateConfig;
use mandate_time::{Date, TradingCalendar};

use crate::error::{InputError, missing};

/// The largest quote-age bound the rule-set artifact may name: a minute-old quote is already older
/// than any marketable decision should rest on.
const MAX_QUOTE_AGE_S: u64 = 60;

/// One artifact's bytes and their parse: the refs hash `bytes`, and every check reads `value`.
pub struct Artifact {
    pub bytes: Vec<u8>,
    pub value: Value,
}

fn executor_configuration(rule_set: &Value) -> Result<ExecutorConfig, InputError> {
    let value = required_exact_object(
        rule_set,
        "executor",
        &[
            "bracket_partial_fill_timeout_s",
            "exit_step_s",
            "gtc_expiry_days",
            "max_intent_age_s",
            "max_unprotected_s",
            "protective_replace_buffer_trading_days",
            "restriction_403_threshold",
            "stop_watchdog_s",
            "unknown_absent_lookups",
            "unknown_absent_window_s",
        ],
    )?;
    Ok(ExecutorConfig {
        max_intent_age_s: required_positive_i64(value, "max_intent_age_s")?,
        unknown_absent_lookups: required_positive_u32(value, "unknown_absent_lookups")?,
        unknown_absent_window_s: required_positive_i64(value, "unknown_absent_window_s")?,
        protective_replace_buffer_trading_days: required_positive_u32(
            value,
            "protective_replace_buffer_trading_days",
        )?,
        restriction_403_threshold: required_positive_u32(value, "restriction_403_threshold")?,
        bracket_partial_fill_timeout_s: required_positive_i64(
            value,
            "bracket_partial_fill_timeout_s",
        )?,
        max_unprotected_s: required_positive_i64(value, "max_unprotected_s")?,
        stop_watchdog_s: required_positive_i64(value, "stop_watchdog_s")?,
        exit_step_s: required_positive_i64(value, "exit_step_s")?,
        gtc_expiry_days: required_positive_u32(value, "gtc_expiry_days")?,
    })
}

fn required_exact_object<'a>(
    value: &'a Value,
    name: &'static str,
    members: &[&str],
) -> Result<&'a Value, InputError> {
    let nested = value.get(name).ok_or_else(|| missing(name))?;
    let exact = nested
        .as_object()
        .is_some_and(|object| object.keys().map(Key::as_str).eq(members.iter().copied()));
    if exact {
        Ok(nested)
    } else {
        Err(missing(name))
    }
}

pub fn required_positive_u64(value: &Value, name: &'static str) -> Result<u64, InputError> {
    value
        .get(name)
        .and_then(Value::as_int)
        .filter(|number| *number > 0)
        .ok_or_else(|| missing(name))
}

fn required_positive_u32(value: &Value, name: &'static str) -> Result<u32, InputError> {
    u32::try_from(required_positive_u64(value, name)?).map_err(|_| missing(name))
}

/// A positive duration read as `i64`: every value [`required_positive_u64`] admits is at most
/// 2^53 − 1, the canonical integer bound (journal spec §4.4), so the conversion refuses nothing
/// it reads; it stays checked rather than cast.
fn required_positive_i64(value: &Value, name: &'static str) -> Result<i64, InputError> {
    i64::try_from(required_positive_u64(value, name)?).map_err(|_| missing(name))
}

/// The paper-only overestimate's schedule name, whose artifact has [`FEE_MEMBERS`].
const PAPER_SCHEDULE: &str = "conservative_v1";
const FEE_MEMBERS: &[&str] = &["effective_from", "environment", "schedule"];
/// The live Robinhood schedule's artifact also names the broker it serves (DEC-958).
const LIVE_FEE_MEMBERS: &[&str] = &["broker", "effective_from", "environment", "schedule"];
pub const CALENDAR_MEMBERS: &[&str] = &["first", "holidays", "last", "special_sessions"];
pub const RULE_MEMBERS: &[&str] = &[
    "executor",
    "gate",
    "gate_config",
    "iex_quote_max_age_s",
    "ruleset_version",
    "story",
];

/// The registered fee schedule's artifact, with exactly the members its `schedule` names: the
/// live Robinhood schedule's [`LIVE_FEE_MEMBERS`], and every other schedule the paper artifact's
/// [`FEE_MEMBERS`], so a paper artifact reads exactly as before.
pub fn fee_artifact(bytes: Vec<u8>) -> Result<Artifact, InputError> {
    let value = mandate_canon::parse(&bytes).map_err(|_| missing("a canonical config artifact"))?;
    let members = match value.get("schedule").and_then(Value::as_str) {
        Some(mandate_executor::ROBINHOOD_LIVE_SCHEDULE) => LIVE_FEE_MEMBERS,
        Some(_) | None => FEE_MEMBERS,
    };
    artifact_from(bytes, members)
}

/// The reviewed fee schedule over the reviewed calendar: the paper-only overestimate, which
/// refuses any environment but `paper`, or the live Robinhood schedule, which refuses any
/// environment but `live` and any broker but `robinhood` (DEC-958).
pub fn fees(fee: &Artifact, calendar: &Artifact) -> Result<mandate_accounting::Config, InputError> {
    let schedule = required_text(&fee.value, "schedule")?;
    if schedule != PAPER_SCHEDULE && schedule != mandate_executor::ROBINHOOD_LIVE_SCHEDULE {
        return Err(missing("schedule"));
    }
    let first = Date::parse(required_text(&calendar.value, "first")?)
        .map_err(|_| missing("the trading calendar's first date"))?;
    let last = Date::parse(required_text(&calendar.value, "last")?)
        .map_err(|_| missing("the trading calendar's last date"))?;
    require_empty_array(&calendar.value, "holidays")?;
    require_empty_array(&calendar.value, "special_sessions")?;
    let calendar = TradingCalendar::new(first, last, [], [])
        .map_err(|_| missing("the effective trading calendar"))?;
    let environment = required_text(&fee.value, "environment")?;
    let first_day = required_text(&fee.value, "effective_from")?;
    if schedule == PAPER_SCHEDULE {
        mandate_executor::paper_only_fee_config(environment, calendar, first_day)
    } else {
        mandate_executor::robinhood_live_fee_config(
            environment,
            required_text(&fee.value, "broker")?,
            calendar,
            first_day,
        )
    }
    .map_err(InputError::Executor)
}

/// The reviewed rule set's quote age bound and its gate and executor configurations; the gate's is
/// `mandate-gate-context`'s one reading (DEC-2388 item 2).
pub fn rule_set(rules: &Artifact) -> Result<(Duration, GateConfig, ExecutorConfig), InputError> {
    let gate_config = gate_rule_set(&rules.value)?;
    let quote_max_age_s = rules
        .value
        .get("iex_quote_max_age_s")
        .and_then(Value::as_int)
        .filter(|seconds| (1..=MAX_QUOTE_AGE_S).contains(seconds))
        .ok_or_else(|| missing("a reviewed IEX quote age of 1 to 60 seconds"))?;
    Ok((
        Duration::from_secs(quote_max_age_s),
        gate_config,
        executor_configuration(&rules.value)?,
    ))
}

pub fn artifact_from(bytes: Vec<u8>, members: &[&str]) -> Result<Artifact, InputError> {
    let value = mandate_canon::parse(&bytes).map_err(|_| missing("a canonical config artifact"))?;
    let exact = value
        .as_object()
        .is_some_and(|object| object.keys().map(Key::as_str).eq(members.iter().copied()));
    if !exact {
        return Err(missing(
            "a config artifact with exactly its reviewed members",
        ));
    }
    Ok(Artifact { bytes, value })
}

pub fn reference(bytes: &[u8]) -> String {
    format!("sha256:{}", Digest::of(bytes).to_hex())
}

pub fn required_text<'a>(value: &'a Value, name: &'static str) -> Result<&'a str, InputError> {
    value
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| missing(name))
}

/// An environment as the fee schedule's `environment` member names it.
pub fn environment_text(environment: Environment) -> &'static str {
    match environment {
        Environment::Paper => "paper",
        Environment::Live => "live",
    }
}

fn require_empty_array(value: &Value, name: &'static str) -> Result<(), InputError> {
    match value.get(name).and_then(Value::as_array) {
        Some([]) => Ok(()),
        Some(_) | None => Err(missing(name)),
    }
}
