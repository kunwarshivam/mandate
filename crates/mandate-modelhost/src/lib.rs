#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]
//! The model host (E15-13, [DEC-503](../../../docs/project/decisions/DEC-503.md),
//! [DEC-504](../../../docs/project/decisions/DEC-504.md)): it runs a pinned quant model outside the
//! production cycle and the shell, and hands back only its typed [`ModelOutput`].
//!
//! [`evaluate`] is pure. It reads no file, no clock and no global calendar: `now` and the
//! calendar are arguments. It refuses unless the confirmed mandate's pin, the matching
//! `model_registry` entry and the content the host computes for that id and version agree, and
//! unless the closes are the pinned instrument's, complete, at least `slow_periods` long, and end
//! on the calendar's last completed regular session at `now`. Every refusal is no output, which
//! can only shrink a buy (mandate spec §8.3, MI-10). Layering keeps it from sizing, the gate, the
//! journal, the executor and the connector (FT-5).

mod ma_crossover;

use std::collections::BTreeMap;

use mandate_accounting::InstrumentId;
use mandate_canon::{Digest, to_canonical};
use mandate_num::{Conviction, Price, Unit};
use mandate_runtime::{ModelDirection, ModelOutput, RiskClock};
use mandate_spec::document::{ModelId, SignalModel};
use mandate_spec::validate::RegisteredModel;
use mandate_time::{Date, ExchangeCalendar, Session, UtcNanos};

use mandate_backtest::BacktestError;
pub use mandate_backtest::Signal;

/// The confirmed mandate's pin of one signal model, and the instrument it is evaluated for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    pub model: SignalModel,
    pub instrument_id: InstrumentId,
}

/// Daily closes of one instrument, oldest first, one per regular session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DailyCloses {
    pub instrument_id: InstrumentId,
    pub closes: Vec<(Date, Price)>,
}

/// What an evaluation that passed every check says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Evaluation {
    /// A `Long` signal, as the one output the model emits (boxed: the output is far larger than a
    /// signal).
    Long(Box<ModelOutput>),
    /// `Flat` or `Undecided`: no output, so no buy (DEC-157 item 4).
    NoOutput(Signal),
}

/// A model's content object ([DEC-504](../../../docs/project/decisions/DEC-504.md) item 1): its
/// canonical JSON bytes and their SHA-256, the hash a mandate pins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Content {
    pub canonical: Vec<u8>,
    pub hash: Digest,
}

/// Every way the host refuses. Each is no output.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// The body of every stub in the tests PR (DEC-77).
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
    #[error("the host has no compiled content for this model id and version")]
    UnknownModel,
    #[error("the pin's content hash is not the host's")]
    PinHashMismatch,
    #[error("the model registry has no entry for the pinned model")]
    NotRegistered,
    #[error("the model registry's entry disagrees with the pin or the host")]
    RegistryMismatch,
    #[error("the pin's parameter keys are not the model's parameter schema")]
    ParamKeys,
    #[error("parameter {key} is not an integer within the model's bounds")]
    ParamValue { key: &'static str },
    #[error("the fast window is not below the slow window")]
    WindowsCrossed,
    #[error("the closes are another instrument's")]
    WrongInstrument,
    #[error("the closes skip a regular session, repeat one, or name a day that did not trade")]
    ClosesIncomplete,
    #[error("the closes do not end on the last completed regular session")]
    ClosesEnd,
    #[error("fewer closes than the slow window")]
    TooFewCloses,
    #[error("the calendar cannot name the last completed regular session at this clock")]
    CalendarCannotName,
    #[error("the output's expiry is beyond the risk clock's range")]
    ExpiryOverflow,
    #[error("the crossover's exact arithmetic over the closes failed")]
    SignalArithmetic,
    #[error("the output's conviction or confidence cannot be represented")]
    OutputUnrepresentable,
    #[error("the host cannot build the model's content object")]
    ContentObject,
}

impl Refusal {
    /// Stable reason code (ADR-0001 ES-09), one per refusal.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unimplemented { .. } => "unimplemented",
            Self::UnknownModel => "unknown_model",
            Self::PinHashMismatch => "pin_hash_mismatch",
            Self::NotRegistered => "not_registered",
            Self::RegistryMismatch => "registry_mismatch",
            Self::ParamKeys => "param_keys",
            Self::ParamValue { .. } => "param_value",
            Self::WindowsCrossed => "windows_crossed",
            Self::WrongInstrument => "wrong_instrument",
            Self::ClosesIncomplete => "closes_incomplete",
            Self::ClosesEnd => "closes_end",
            Self::TooFewCloses => "too_few_closes",
            Self::CalendarCannotName => "calendar_cannot_name",
            Self::ExpiryOverflow => "expiry_overflow",
            Self::SignalArithmetic => "signal_arithmetic",
            Self::OutputUnrepresentable => "output_unrepresentable",
            Self::ContentObject => "content_object",
        }
    }
}

/// The content object the host computes for `model_id` at `model_version`, from the source bytes
/// it was built with, or [`Refusal::UnknownModel`] for a model it has no compiled content for.
pub fn content(model_id: &str, model_version: &str) -> Result<Content, Refusal> {
    if (model_id, model_version) != (ma_crossover::MODEL_ID, ma_crossover::MODEL_VERSION) {
        return Err(Refusal::UnknownModel);
    }
    let canonical = to_canonical(&ma_crossover::content_object()?);
    let hash = Digest::of(&canonical);
    Ok(Content { canonical, hash })
}

/// The pinned model's output on `closes` at `now`, or why there is none.
pub fn evaluate(
    pin: &Pin,
    registry: &BTreeMap<ModelId, RegisteredModel>,
    calendar: &ExchangeCalendar,
    closes: &DailyCloses,
    now: UtcNanos,
) -> Result<Evaluation, Refusal> {
    let model = &pin.model;
    let host = content(model.id.as_str(), &model.version)?;
    if model.content_hash != host.hash {
        return Err(Refusal::PinHashMismatch);
    }
    let entry = registry.get(&model.id).ok_or(Refusal::NotRegistered)?;
    let schema = ma_crossover::PARAMS.map(|(name, _, _)| name.to_owned());
    if entry.version != model.version
        || entry.content_hash != host.hash
        || entry.params != schema.into()
        || entry.admits_instruments
        || model.admits_instruments
    {
        return Err(Refusal::RegistryMismatch);
    }
    let config = ma_crossover::config(&model.params)?;
    if closes.instrument_id != pin.instrument_id {
        return Err(Refusal::WrongInstrument);
    }
    complete(calendar, &closes.closes)?;
    let last = calendar
        .last_completed_regular_session(now)
        .ok()
        .flatten()
        .ok_or(Refusal::CalendarCannotName)?;
    let Some((end, _)) = closes.closes.last() else {
        return Err(Refusal::TooFewCloses);
    };
    if *end != last {
        return Err(Refusal::ClosesEnd);
    }
    let slow = usize::try_from(config.slow_periods).map_err(|_| Refusal::ParamValue {
        key: "slow_periods",
    })?;
    if closes.closes.len() < slow {
        return Err(Refusal::TooFewCloses);
    }
    let prices: Vec<Price> = closes.closes.iter().map(|(_, p)| *p).collect();
    let signal = config
        .signal(&prices)
        .map_err(|_: BacktestError| Refusal::SignalArithmetic)?;
    if !ma_crossover::is_output(signal) {
        return Ok(Evaluation::NoOutput(signal));
    }
    let close = calendar
        .sessions(last)
        .map_err(|_| Refusal::CalendarCannotName)?
        .into_iter()
        .find(|span| span.session() == Session::Regular)
        .ok_or(Refusal::CalendarCannotName)?
        .end();
    let as_of = close.secs();
    let expires = as_of
        .checked_add(i64::from(model.max_output_age_s))
        .ok_or(Refusal::ExpiryOverflow)?;
    Ok(Evaluation::Long(Box::new(ModelOutput {
        model_id: model.id.as_str().to_owned(),
        model_version: model.version.clone(),
        content_hash: host.hash,
        instrument_id: pin.instrument_id.clone(),
        as_of: RiskClock::from_secs(as_of),
        expires_at: RiskClock::from_secs(expires),
        direction: ModelDirection::Long,
        conviction: Conviction::parse("1").map_err(|_| Refusal::OutputUnrepresentable)?,
        confidence: Unit::ONE,
        horizon_s: u64::from(model.max_output_age_s),
        thesis_ref: None,
        evidence: Vec::new(),
        invalidation: None,
        thesis_id: None,
        lineage_id: None,
        ignored: None,
    })))
}

/// Every close on a regular-session day, each the session after the one before.
fn complete(calendar: &ExchangeCalendar, closes: &[(Date, Price)]) -> Result<(), Refusal> {
    let trades = |day: Date| {
        calendar
            .is_trading_day(day)
            .map_err(|_| Refusal::CalendarCannotName)
    };
    for pair in closes.windows(2) {
        let [(before, _), (after, _)] = pair else {
            return Err(Refusal::ClosesIncomplete);
        };
        let mut day = before.next().map_err(|_| Refusal::ClosesIncomplete)?;
        while day < *after {
            if trades(day)? {
                return Err(Refusal::ClosesIncomplete);
            }
            day = day.next().map_err(|_| Refusal::ClosesIncomplete)?;
        }
        if day != *after {
            return Err(Refusal::ClosesIncomplete);
        }
    }
    for (day, _) in closes {
        if !trades(*day)? {
            return Err(Refusal::ClosesIncomplete);
        }
    }
    Ok(())
}
