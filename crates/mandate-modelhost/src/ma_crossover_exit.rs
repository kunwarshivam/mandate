//! `quant.ma_crossover` 1.1.0 (DEC-2390): 1.0.0's crossover, parameters, inputs and `Long`, whose
//! `Flat` is an output too: direction `long` with conviction −1, which favors exiting (mandate spec
//! §8.2), so a crossover down reaches the builder's §8.3 discretionary exit. Its `Flat` is the
//! backtest's (the fast mean at or below the slow one), so the model sells when the backtest of the
//! same strategy sells. It is one of the source files 1.1.0's content hash lists, with 1.0.0's two,
//! which stay byte for byte as they are so 1.0.0's pinned hash never moves (DEC-504 item 1).

use mandate_backtest::Signal;
use mandate_canon::{Digest, Key, Object, Value};
use mandate_num::{Conviction, Unit};
use mandate_runtime::ModelDirection;

use crate::{Refusal, ma_crossover};

/// The version this file is the content of.
pub(crate) const MODEL_VERSION: &str = "1.1.0";

/// The source files whose bytes 1.1.0's content object lists, by path from the repository root,
/// sorted, as this build embedded them (DEC-504 item 2).
pub(crate) const SOURCES: [(&str, &[u8]); 3] = [
    (
        "crates/mandate-backtest/src/strategy/ma_crossover.rs",
        include_bytes!("../../mandate-backtest/src/strategy/ma_crossover.rs"),
    ),
    (
        "crates/mandate-modelhost/src/ma_crossover.rs",
        include_bytes!("ma_crossover.rs"),
    ),
    (
        "crates/mandate-modelhost/src/ma_crossover_exit.rs",
        include_bytes!("ma_crossover_exit.rs"),
    ),
];

const METHODOLOGY: &str = "Long with conviction 1 when the mean of the last fast_periods daily \
closes is strictly above the mean of the last slow_periods daily closes, compared by \
cross-multiplying the window sums; otherwise long with conviction -1, which favors exiting.";

fn text(s: &str) -> Value {
    Value::Str(s.to_owned())
}

fn object(members: Vec<(&str, Value)>) -> Result<Value, Refusal> {
    members
        .into_iter()
        .map(|(k, v)| Key::new(k).map(|k| (k, v)))
        .collect::<Result<Object, _>>()
        .map(Value::Object)
        .map_err(|_| Refusal::ContentObject)
}

/// DEC-518 item 3's object for 1.1.0: 1.0.0's, with its own version, methodology, listed sources,
/// and a `flat` that is the exit output.
pub(crate) fn content_object() -> Result<Value, Refusal> {
    let Value::Object(mut members) = ma_crossover::content_object()? else {
        return Err(Refusal::ContentObject);
    };
    let code = SOURCES
        .iter()
        .map(|(path, bytes)| {
            let sha = Digest::of(bytes).to_hex();
            object(vec![("path", text(path)), ("sha256", text(&sha))])
        })
        .collect::<Result<Vec<_>, _>>()?;
    let opinion = |conviction: &str| {
        object(vec![
            ("confidence", text("1")),
            ("conviction", text(conviction)),
            ("direction", text("long")),
        ])
    };
    let output = object(vec![
        ("flat", opinion("-1")?),
        ("long", opinion("1")?),
        ("undecided", text("none")),
    ])?;
    for (name, value) in [
        ("code", Value::Array(code)),
        ("methodology", text(METHODOLOGY)),
        ("model_version", text(MODEL_VERSION)),
        ("output", output),
    ] {
        members.insert(Key::new(name).map_err(|_| Refusal::ContentObject)?, value);
    }
    Ok(Value::Object(members))
}

/// The model's opinion on a signal: `Long` as 1.0.0 answers it; `Flat` is direction `long`,
/// conviction −1 and confidence 1; `Undecided` is no output.
pub(crate) fn opinion(
    signal: Signal,
) -> Result<Option<(ModelDirection, Conviction, Unit)>, Refusal> {
    match signal {
        Signal::Long => ma_crossover::opinion(signal),
        Signal::Flat => {
            let conviction = Conviction::parse("-1").map_err(|_| Refusal::OutputUnrepresentable)?;
            Ok(Some((ModelDirection::Long, conviction, Unit::ONE)))
        }
        Signal::Undecided => Ok(None),
    }
}
