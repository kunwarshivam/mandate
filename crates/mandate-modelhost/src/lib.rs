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
use mandate_canon::Digest;
use mandate_num::Price;
use mandate_runtime::ModelOutput;
use mandate_spec::document::{ModelId, SignalModel};
use mandate_spec::validate::RegisteredModel;
use mandate_time::{Date, ExchangeCalendar, UtcNanos};

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
    let _ = (model_id, model_version, ma_crossover::SOURCES);
    Err(Refusal::Unimplemented { story: "E15-13" })
}

/// The pinned model's output on `closes` at `now`, or why there is none.
pub fn evaluate(
    pin: &Pin,
    registry: &BTreeMap<ModelId, RegisteredModel>,
    calendar: &ExchangeCalendar,
    closes: &DailyCloses,
    now: UtcNanos,
) -> Result<Evaluation, Refusal> {
    let _ = (pin, registry, calendar, closes, now);
    Err(Refusal::Unimplemented { story: "E15-13" })
}
