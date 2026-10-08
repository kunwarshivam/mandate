//! `quant.ma_crossover`'s own code in the host: how its parameters are read and how its signal
//! becomes an output. With the crossover itself (`mandate-backtest`'s `strategy/ma_crossover.rs`),
//! it is one of the source files the model's content hash lists
//! ([DEC-504](../../../docs/project/decisions/DEC-504.md) item 1), so an edit here is an edit to the
//! model and needs a new model version.

use mandate_backtest::{Signal, StrategyConfig};
use mandate_canon::{Int, Key, Value};
use mandate_num::{Bps, Conviction, Unit, Usd};
use mandate_runtime::ModelDirection;
use mandate_spec::document::{ModelParam, ParamValue};

use crate::Refusal;

/// The id and version this host has compiled content for: its catalogue, not a choice of model,
/// which only a confirmed mandate's pin makes (DEC-503 item 3).
pub(crate) const MODEL_ID: &str = "quant.ma_crossover";
pub(crate) const MODEL_VERSION: &str = "1.0.0";

/// The source files whose bytes the content object lists, by path from the repository root,
/// sorted, as this build embedded them (DEC-504 item 2).
pub(crate) const SOURCES: [(&str, &[u8]); 2] = [
    (
        "crates/mandate-backtest/src/strategy/ma_crossover.rs",
        include_bytes!("../../mandate-backtest/src/strategy/ma_crossover.rs"),
    ),
    (
        "crates/mandate-modelhost/src/ma_crossover.rs",
        include_bytes!("ma_crossover.rs"),
    ),
];

/// Each parameter's name and inclusive integer bounds, sorted by name, with no default (DEC-52).
pub(crate) const PARAMS: [(&str, u32, u32); 2] =
    [("fast_periods", 1, 1000), ("slow_periods", 2, 1000)];

const METHODOLOGY: &str = "Long when the mean of the last fast_periods daily closes is strictly \
above the mean of the last slow_periods daily closes, compared by cross-multiplying the window \
sums; otherwise no output.";

fn text(s: &str) -> Value {
    Value::Str(s.to_owned())
}

fn int(n: u32) -> Result<Value, Refusal> {
    Int::new(u64::from(n))
        .map(Value::Int)
        .ok_or(Refusal::ContentObject)
}

fn object(members: Vec<(&str, Value)>) -> Result<Value, Refusal> {
    members
        .into_iter()
        .map(|(k, v)| Key::new(k).map(|k| (k, v)))
        .collect::<Result<_, _>>()
        .map(Value::Object)
        .map_err(|_| Refusal::ContentObject)
}

/// DEC-518 item 3's object, with each listed file's SHA-256 from the embedded bytes.
pub(crate) fn content_object() -> Result<Value, Refusal> {
    let code = SOURCES
        .iter()
        .map(|(path, bytes)| {
            let sha = mandate_canon::Digest::of(bytes).to_hex();
            object(vec![("path", text(path)), ("sha256", text(&sha))])
        })
        .collect::<Result<Vec<_>, _>>()?;
    let params = PARAMS
        .iter()
        .map(|(name, min, max)| {
            let typed = [
                ("max", int(*max)?),
                ("min", int(*min)?),
                ("name", text(name)),
            ];
            object(
                typed
                    .into_iter()
                    .chain([("type", text("integer"))])
                    .collect(),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let long = object(vec![
        ("confidence", text("1")),
        ("conviction", text("1")),
        ("direction", text("long")),
    ])?;
    object(vec![
        ("authorship", text("platform")),
        ("code", Value::Array(code)),
        ("content_version", int(1)?),
        (
            "inputs",
            object(vec![
                ("bars", text("daily_close")),
                ("instrument", text("pinned")),
                ("minimum_count", text("slow_periods")),
                ("trading_day", text("us_equity_regular_session")),
            ])?,
        ),
        ("kind", text("quant_model_content")),
        ("methodology", text(METHODOLOGY)),
        ("model_id", text(MODEL_ID)),
        ("model_version", text(MODEL_VERSION)),
        (
            "output",
            object(vec![
                ("flat", text("none")),
                ("long", long),
                ("undecided", text("none")),
            ])?,
        ),
        (
            "params_schema",
            object(vec![
                (
                    "constraints",
                    Value::Array(vec![text("fast_periods < slow_periods")]),
                ),
                ("params", Value::Array(params)),
            ])?,
        ),
    ])
}

/// The two windows the pin's parameters state: exactly the schema's keys, each an integer within
/// its bounds, the fast window below the slow one.
pub(crate) fn config(params: &[ModelParam]) -> Result<StrategyConfig, Refusal> {
    let mut keys: Vec<&str> = params.iter().map(|p| p.key.as_str()).collect();
    keys.sort_unstable();
    if keys != PARAMS.map(|(name, _, _)| name) {
        return Err(Refusal::ParamKeys);
    }
    let window = |(name, min, max): (&'static str, u32, u32)| {
        let refused = Refusal::ParamValue { key: name };
        let value = params.iter().find(|p| p.key == name).map(|p| &p.value);
        let Some(ParamValue::Decimal(dec)) = value else {
            return Err(refused);
        };
        match dec.as_str().parse::<u32>() {
            Ok(n) if (min..=max).contains(&n) => Ok(n),
            _ => Err(refused),
        }
    };
    let [fast, slow] = PARAMS;
    let (fast_periods, slow_periods) = (window(fast)?, window(slow)?);
    if fast_periods >= slow_periods {
        return Err(Refusal::WindowsCrossed);
    }
    Ok(StrategyConfig {
        fast_periods,
        slow_periods,
        collar: Bps::ZERO,
        target_notional: Usd::ZERO,
    })
}

/// The model's opinion on a signal (DEC-157 item 4): `Long` is direction `long`, conviction 1
/// and confidence 1; `Flat` and `Undecided` are no output.
pub(crate) fn opinion(
    signal: Signal,
) -> Result<Option<(ModelDirection, Conviction, Unit)>, Refusal> {
    match signal {
        Signal::Long => {
            let conviction = Conviction::parse("1").map_err(|_| Refusal::OutputUnrepresentable)?;
            Ok(Some((ModelDirection::Long, conviction, Unit::ONE)))
        }
        Signal::Flat | Signal::Undecided => Ok(None),
    }
}

/// The holding horizon an output states: the pinned `max_output_age_s`, since the model has no
/// horizon of its own and the output stops being fresh at the same instant (DEC-518 item 5).
pub(crate) fn horizon_s(max_output_age_s: u32) -> u64 {
    u64::from(max_output_age_s)
}
