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
//! Autonomy classification and the order builder (backlog E6-2, [mandate spec §6] and [§8.3];
//! [task brief](../../../docs/project/tasks/E6-2-autonomy-and-order-builder.md), DEC-130).
//!
//! This is the agent's decision layer: signal-model outputs and the owner's fixed weights in, a
//! proposal already clipped to the mandate's limits out, and every proposed action classified AUTO,
//! ASK, or DENY by the owner's rules. Every entry point is pure — no clock, no I/O, no randomness,
//! ordered collections only — so the same inputs give the same proposal (ADR-0001 ES-21).
//!
//! **The builder proposes; the gate decides; never the reverse.** This crate does not depend on
//! `mandate-risk`: the §6.2 step 2 dry run reaches [`decide`] as a [`GateVerdict`] **value**, so no
//! defect here can turn a verdict into an allow, and the builder is never the reason a limit goes
//! unenforced (`AGENTS.md` rule 1). The [`Purpose`] on a proposal is a label the gate re-derives
//! from side and position; it is never binding (spec §6.1, trading spec §9.1). For the same reason
//! the §5.5 `trim_to_target` risk exit is not here: the runtime asks the risk engine first and calls
//! [`propose`] only when there is no trim to place, so this crate cannot suppress a risk exit
//! (DEC-130 items 2 and 3).
//!
//! Spec §5.2 says comparisons are exact and only reported ratios round, and §8.3 states exactly
//! three roundings — the `round₁₂` of its step 1. So every other value in the chain is exact, the
//! wide intermediates live in [`mandate_num::UsdExact`], and an input too precise for the types to
//! hold refuses the order rather than approximating a size nobody asked for (DEC-130 items 7 and 8):
//! refusing to propose adds no risk, which is what `AGENTS.md` rule 3 asks of an ambiguous input.
//!
//! [mandate spec §6]: ../../../docs/specs/mandate.md#6-autonomy-dec-42-dec-48-dec-58
//! [§8.3]: ../../../docs/specs/mandate.md#83-order-builder-conviction_linear-dec-47-dec-60

mod combine;
mod order;
mod policy;

pub use combine::{
    Combined, ContentHash, Direction, ModelId, ModelOutput, ModelVersion, SignalModel, combine,
};
pub use order::{
    AccountSnapshot, AccumulateGoal, Action, BuilderMandate, Clip, GateVerdict, GoalKind,
    HoldReason, Limits, Market, OrderShape, Outcome, Proposal, RiskContext, Sizes, Sizing,
    SizingMethod, decide, propose,
};
pub use policy::{
    ActionContext, Approval, Autonomy, AutonomyPolicy, Condition, DecidedBy, Decision, Field, Kind,
    OnTimeout, Op, Purpose, Rule, RuleId, Session, Value, classify,
};

use mandate_num::NumError;
use mandate_time::TimeError;

/// The maximum nesting depth a condition may reach (mandate spec V-017).
pub const MAX_CONDITION_DEPTH: u32 = 4;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BuilderError {
    /// The stubs of this story's tests PR return this, so every pending test fails on them
    /// (DEC-77, DEC-83); the implementation PR replaces the stubs and removes the variant.
    #[error("the order builder is not implemented yet")]
    Unimplemented,
    #[error("a condition nests deeper than the four levels V-017 allows")]
    ConditionTooDeep,
    #[error("condition on `{0}` compares a value its field's type does not take (V-023)")]
    ConditionTypeMismatch(&'static str),
    /// `unusual_input` is reserved until the input-drift detector ships (spec §6.3, V-018), so a
    /// rule naming it is refused when the policy is built and the field is never reachable.
    #[error("field `unusual_input` is reserved until the input-drift detector ships (V-018)")]
    ReservedField,
    #[error("two autonomy rules share the id `{0}`")]
    DuplicateRuleId(String),
    #[error("a mandate configures at least one signal model")]
    NoSignalModels,
    #[error("the configured weights sum to zero, so no conviction is defined")]
    WeightSumZero,
    /// Sizing off a crossed quote would price an order against a market that does not exist. No
    /// committed reference case reaches this, and `reference/mandate/ref.py` does not raise it
    /// (DEC-130 item 17).
    #[error("the quote is crossed: its bid is above its ask")]
    CrossedQuote,
    #[error("an accumulate goal sizes only its own instrument")]
    AccumulateInstrumentMismatch,
    #[error(transparent)]
    Num(#[from] NumError),
    #[error(transparent)]
    Time(#[from] TimeError),
}

impl BuilderError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unimplemented => "unimplemented",
            Self::ConditionTooDeep => "condition_too_deep",
            Self::ConditionTypeMismatch(_) => "condition_type_mismatch",
            Self::ReservedField => "reserved_field",
            Self::DuplicateRuleId(_) => "duplicate_rule_id",
            Self::NoSignalModels => "no_signal_models",
            Self::WeightSumZero => "weight_sum_zero",
            Self::CrossedQuote => "crossed_quote",
            Self::AccumulateInstrumentMismatch => "accumulate_instrument_mismatch",
            Self::Num(e) => e.code(),
            Self::Time(e) => e.code(),
        }
    }
}
