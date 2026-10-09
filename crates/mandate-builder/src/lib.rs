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
//! The agent's decision layer: model outputs and the owner's fixed weights in, a proposal already
//! clipped to the limits and an AUTO / ASK / DENY classification out
//! ([mandate spec §6](../../../docs/specs/mandate.md#6-autonomy-dec-42-dec-48-dec-58) and
//! [§8.3](../../../docs/specs/mandate.md#83-order-builder-conviction_linear-dec-47-dec-60);
//! [task brief](../../../docs/project/tasks/E6-2-autonomy-and-order-builder.md), DEC-130).
//!
//! **It proposes; the gate decides, never the reverse** (DEC-130 item 2). This crate does not
//! depend on `mandate-risk`: the §6.2 step 2 dry run arrives as a [`GateVerdict`] **value**, so no
//! defect here can turn a verdict into an allow, and the builder can never be the reason a limit
//! goes unenforced (AGENTS.md rule 1). The [`Purpose`](mandate_domain::Purpose) on a proposal is a
//! label; the gate assigns purpose from side and position (§6.1) and re-derives it.
//!
//! **The `trim_to_target` risk exit is not here** (DEC-130 item 3). §6.2 step 1 has the risk engine
//! propose it first, and the runtime calls this crate only when there is no trim to place, so the
//! builder can never suppress a risk exit — a stronger property than sequencing the two inside it.
//!
//! **The vocabulary and the §6.3 condition tree are stream F's** (DEC-128 items 18, 21, and 23).
//! [`Purpose`](mandate_domain::Purpose), [`AutonomyDecision`](mandate_domain::AutonomyDecision),
//! [`MarketSession`](mandate_domain::MarketSession), [`AssetClass`](mandate_domain::AssetClass) and
//! [`AssetId`](mandate_domain::AssetId) come from `mandate-domain`; the
//! [`Autonomy`](mandate_spec::document::Autonomy) block, its [`Rule`](mandate_spec::document::Rule)s
//! and the [`Condition`](mandate_spec::condition::Condition) tree from `mandate-spec`. This crate
//! imports them rather than carrying the copies the merged brief proposed, because F landed first.
//! What is this crate's is §6.2's order, the built-in AUTO purposes, the admission ceiling, the
//! client ceiling, the approver count, every [`Facts`](mandate_spec::condition::Facts)
//! implementation, and the whole of §8.3.
//!
//! `MarketSession` carries five variants and §6.3's `session` field names four. The mapping is the
//! identity on `pre_market`, `regular`, `after_hours` and `crypto`;
//! [`MarketSession::Overnight`](mandate_domain::MarketSession::Overnight) has no §6.3 name because
//! no order may trade there (DEC-30), so a market in that session is
//! [`BuilderError::UntradableSession`] rather than a rule evaluated against a value the language
//! cannot express.
//!
//! Everything is pure: no clock, no randomness, no I/O, no state between calls, `BTreeSet`
//! throughout (ES-21). `now` is an argument. Arithmetic is `mandate-num`'s and exact (ES-04,
//! DEC-89): §8.3 states three roundings and this crate takes no others, so an input too wide for an
//! exact chain refuses the proposal instead of approximating it (DEC-130 item 8).

mod autonomy;
mod builder;
mod opening;

pub use autonomy::{
    ActionContext, ApprovalRequest, Classification, DecidedBy, GateVerdict, Outcome, RequestedBy,
    classify, decide,
};
pub use builder::{
    AccountSnapshot, AccumulateGoal, Action, BuilderMandate, Clip, Combined, Direction, GoalKind,
    HoldReason, Limits, Market, ModelOutput, ModelVersion, OrderShape, Proposal, RiskContext,
    SignalModel, Sizes, Sizing, autonomy_policy, buy_action, combine, propose,
};
pub use opening::{Venue, deployable, opening_form, propose_on};

use mandate_domain::DomainError;
use mandate_num::NumError;
use mandate_spec::SpecError;
use mandate_time::TimeError;

/// Why the builder proposed nothing, or could not classify an action. One cause each, with a stable
/// `code()` (ES-09).
///
/// [`BuilderError::ConditionTooDeep`], [`BuilderError::ConditionTypeMismatch`] and
/// [`BuilderError::ReservedField`] are named here although DEC-128 item 18 puts those three errors
/// in `mandate-spec`: they are the **order path's** re-check of V-017, V-018 and V-023, which
/// [`classify`] does because it is handed a §6 block and cannot assume validation ran, and
/// `mandate-spec` does not carry the three variants yet. The codes are the ones item 18 names, so
/// when `mandate-spec::validate` lands them the implementation PR maps its `SpecError` onto these
/// and no caller sees a different code. Recorded in this PR's Decisions needed.
///
/// Every variant is a **refusal**, and a refusal adds no risk: nothing is proposed, nothing is
/// submitted, and nothing is approved. That is what AGENTS.md rule 3 and DEC-06 require of an
/// ambiguous input, whereas an approximated size would be an order nobody specified (DEC-130
/// item 8).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BuilderError {
    /// A stub of a DEC-77 tests PR returns this, so every pending test fails on it (DEC-77, DEC-83,
    /// DEC-110). Nothing returns it since E6-12's implementation; it stays because
    /// `tests/vocabulary.rs` pins every stable code, and removing it is a tests change of its own.
    #[error("the order builder is not implemented yet")]
    Unimplemented,
    /// v1 has one sizing method and no calibration (DEC-47), so any other is refused rather than
    /// sized by `conviction_linear` under another name.
    #[error("this sizing method is not `conviction_linear`")]
    UnsupportedSizingMethod,
    /// §6.3 conditions nest to four levels (V-017). Validation rejects a deeper one; the order path
    /// refuses it again rather than evaluating a tree it was never meant to hold.
    #[error("a rule's condition nests deeper than the four levels V-017 allows")]
    ConditionTooDeep,
    /// A condition value that is not of its field's type (V-023): a list against a decimal, an
    /// ordering operator against an enum, text that is not a member of the field's enum.
    #[error("a rule compares a field against a value of another type")]
    ConditionTypeMismatch,
    /// `unusual_input` is reserved until the input-drift detector ships (V-018, E17-5). A rule
    /// naming it is refused, so the field cannot be reached at all.
    #[error("a rule reads `unusual_input`, which V-018 reserves")]
    ReservedField,
    /// Two rules with one id: the first-match walk would report a `by` that names either.
    #[error("two autonomy rules share an id")]
    DuplicateRuleId,
    /// §8.3's W is the sum over all configured models, so an empty list has no denominator and no
    /// meaning; V-006 requires at least one.
    #[error("the mandate configures no signal model")]
    NoSignalModels,
    /// The weights sum to zero, so §8.3 step 1 would divide by zero.
    #[error("the configured weights sum to zero")]
    WeightSumZero,
    /// `bid > ask`. §8.3 prices a buy at the ask and an exit at the bid; sizing off a crossed quote
    /// prices an order against a market that does not exist. This is the one refusal
    /// `reference/mandate/ref.py` does not raise, and no committed case reaches it (DEC-130
    /// item 17).
    #[error("the quote is crossed: the bid is above the ask")]
    CrossedQuote,
    /// An `accumulate` goal names one instrument and the universe is pinned to it (V-003), so a
    /// market for another instrument is a caller's mistake, not a goal that clips nothing.
    #[error("the accumulate goal names another instrument than the market")]
    AccumulateInstrumentMismatch,
    /// Not the schema's `major.minor.patch` of at most six digits a part.
    #[error("not the schema's semantic-version form")]
    MalformedModelVersion,
    /// [`decide`] was handed a proposal that holds. A hold proposes nothing, so there is nothing
    /// for the gate to have judged and nothing to classify; answering "skipped" would make a hold
    /// and a denied order one outcome on the journal.
    #[error("the proposal holds, so there is nothing to decide")]
    NothingProposed,
    /// [`decide`] was handed a buy labelled with a purpose other than `open` or `increase`, or with
    /// two labels that disagree. A buy adds risk whatever it is called, so its label must not be
    /// the reason it reaches §6.2 step 3's built-in AUTO instead of the owner's rules; the builder
    /// never proposes one, and refusing it adds no risk (`AGENTS.md` rule 3).
    #[error("a buy is labelled with a purpose other than open or increase")]
    MislabelledBuy,
    /// No order may trade in the overnight session (DEC-30), and §6.3's `session` field has no name
    /// for it, so an **opening or increasing** order in that session is refused rather than
    /// classified against a value the condition language cannot express.
    ///
    /// It is refused on the buy path only. A discretionary exit is paced, never denied
    /// (`AGENTS.md` rule 13), and needs no rule and so no session name: refusing one here would
    /// make an untradable session a reason an exit did not go out, which is the one thing rule 13
    /// forbids. The session rule that holds an equity exit is the gate's `defer` (trading spec
    /// §9.6, DEC-130 item 15).
    #[error("no opening order may trade in the overnight session")]
    UntradableSession,
    /// A model output naming another instrument than the market being sized.
    ///
    /// §8.2 gives every output an `instrument_id` and §8.3 sizes **one** instrument at a time, so an
    /// output for another one is a caller's mistake, not a signal. Counting it would let one
    /// instrument's conviction open or enlarge a position in another; ignoring it silently would
    /// make that mistake indistinguishable from a model that did not answer. Refusing adds no risk
    /// and says which input was not interpreted (DEC-85).
    #[error("a model output names another instrument than the market")]
    OutputInstrumentMismatch,
    /// The platform's opening policy and the broker's capability profile leave no form for an
    /// opening: no `limit` cell in a quantity (never notional) that lists the opening's time in
    /// force, or a US equity outside the regular session (trading spec §5.2, LT-3, DEC-854). A
    /// refusal before the intent, never a fallback to another order type. Reached on the buy path
    /// and at deployment only, so it never stops an exit (`AGENTS.md` rule 13).
    #[error("the opening policy and the broker's profile leave no order form for an opening")]
    NoOpeningForm,
    #[error(transparent)]
    Num(#[from] NumError),
    #[error(transparent)]
    Time(#[from] TimeError),
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error(transparent)]
    Spec(#[from] SpecError),
}

impl BuilderError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unimplemented => "unimplemented",
            Self::UnsupportedSizingMethod => "unsupported_sizing_method",
            Self::ConditionTooDeep => "condition_too_deep",
            Self::ConditionTypeMismatch => "condition_type_mismatch",
            Self::ReservedField => "reserved_field",
            Self::DuplicateRuleId => "duplicate_rule_id",
            Self::NoSignalModels => "no_signal_models",
            Self::WeightSumZero => "weight_sum_zero",
            Self::CrossedQuote => "crossed_quote",
            Self::AccumulateInstrumentMismatch => "accumulate_instrument_mismatch",
            Self::MalformedModelVersion => "malformed_model_version",
            Self::NothingProposed => "nothing_proposed",
            Self::MislabelledBuy => "mislabelled_buy",
            Self::UntradableSession => "untradable_session",
            Self::OutputInstrumentMismatch => "output_instrument_mismatch",
            Self::NoOpeningForm => "no_opening_form",
            Self::Num(e) => e.code(),
            Self::Time(e) => e.code(),
            Self::Domain(e) => e.code(),
            Self::Spec(e) => e.code(),
        }
    }
}
