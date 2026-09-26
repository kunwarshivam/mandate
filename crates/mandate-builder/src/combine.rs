//! Signal-model outputs and how §8.3 step 1 combines them ([mandate spec §8.1], §8.2, §8.3).
//!
//! [mandate spec §8.1]: ../../../docs/specs/mandate.md#81-signal-model-contract-dec-52-dec-97

use std::collections::BTreeSet;

use mandate_accounting::InstrumentId;
use mandate_num::{Conviction, SizeFraction, Unit};
use mandate_time::UtcNanos;

use crate::BuilderError;

/// A registered signal model's id, `quant.`, `fast.`, or `llm.` prefixed (spec §8.1). The grammar is
/// `mandate-spec`'s to validate; this crate compares ids and orders them.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ModelId(String);

/// A model's semantic version, compared for equality against the pinned value.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ModelVersion(String);

/// The content hash of a model's code, prompt, parameter schema, and underlying model identity
/// (spec §8.1), compared for equality against the pinned value.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ContentHash(String);

macro_rules! text_id {
    ($name:ident) => {
        impl $name {
            pub fn new(text: impl Into<String>) -> Self {
                Self(text.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

text_id!(ModelId);
text_id!(ModelVersion);
text_id!(ContentHash);

/// A signal model as the mandate pins it (spec §8.1, V-007): the triple an output must match, the
/// owner's fixed weight, and how old an output may be. There is no calibration in v1 (DEC-47), so a
/// weight is data, never derived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalModel {
    pub id: ModelId,
    pub version: ModelVersion,
    pub content_hash: ContentHash,
    pub weight: SizeFraction,
    pub max_output_age_s: u32,
}

/// `long` only in v1 (spec §8.2, DEC-32). One variant, so a direction v1 does not support is
/// unrepresentable rather than rejected, which is the trust ladder's first rung (DEC-130 item 17).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Long,
}

/// One signal-model output (spec §8.2). An output whose `expires_at` is at or before its `as_of` is
/// simply never fresh, which is the freshness test's own answer and needs no error of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelOutput {
    pub model_id: ModelId,
    pub model_version: ModelVersion,
    pub content_hash: ContentHash,
    pub instrument: InstrumentId,
    pub as_of: UtcNanos,
    pub expires_at: UtcNanos,
    pub direction: Direction,
    pub conviction: Conviction,
    pub confidence: Unit,
}

/// What §8.3 step 1 produces, each figure one `round₁₂` of one exact quotient.
///
/// The two convictions differ only in what a **missing** model counts as: zero for
/// `exit_conviction`, so an outage never forces a sell, and fully bearish for `buy_conviction`, so an
/// outage never enlarges a buy (MI-10). All three are reported even when the builder holds, so an
/// outage is visible as an outage rather than as a blank (`MC-B20`, DEC-130 item 9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Combined {
    /// The models whose latest fresh output counted, sorted, so two runs cannot differ by iteration
    /// order (ES-21).
    pub outputs_used: BTreeSet<ModelId>,
    pub exit_conviction: Conviction,
    pub buy_conviction: Conviction,
    pub score: Unit,
}

/// Combines the fresh outputs with the owner's fixed weights (spec §8.3 step 1).
///
/// An output counts when its id, version, **and** content hash all equal the pinned triple — a
/// mismatch in any of the three is ignored and counts as missing, so a substituted model lowers a
/// buy conviction and never raises one (spec §8.1, DEC-67) — and when it is **fresh**:
/// `as_of ≤ now < expires_at` and `now − as_of ≤ max_output_age_s` for that model. Only the latest
/// fresh output per model counts: latest `as_of`, ties by position in `outputs`, which the caller
/// supplies in journal `seq` order (DEC-130 item 10).
///
/// W is the sum of **every** configured model's weight, fresh or not, which is what makes a missing
/// model count for something.
///
/// Errors: `no_signal_models`; `weight_sum_zero`; and the arithmetic and time errors it wraps.
pub fn combine(
    models: &[SignalModel],
    outputs: &[ModelOutput],
    now: UtcNanos,
) -> Result<Combined, BuilderError> {
    let _ = (models, outputs, now);
    Err(BuilderError::Unimplemented)
}
