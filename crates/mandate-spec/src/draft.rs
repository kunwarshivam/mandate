//! What goal-first drafting proposes for the loss answer ([mandate spec §7](../../../docs/specs/mandate.md#7-compiler-and-platform-proposals-dec-97);
//! DEC-182, DEC-695 items 6 and 7): the three loss fields the answer maps to, and the drawdown
//! ladder proposed beneath them. Every value is an exact decimal, never a float.
//!
//! E10-7 S1b's tests PR (DEC-77): both functions return [`SpecError::Unimplemented`], so every
//! test pending on them fails at the stubs until the implementation PR replaces them.

use mandate_num::{Ratio, Usd};

use crate::document::{LadderRung, Source};
use crate::{SchemaDec, SpecError};

/// The owner's answer to "how much can you stand to lose": a share of the allocation, or dollars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LossAnswer {
    Fraction(Ratio),
    Usd(Usd),
}

/// A drafted value and its source (§2.1): `user_stated` for the floor only, `platform_proposed` for
/// every other value, which is inactive until the owner confirms it (V-020, MI-12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drafted<T> {
    pub value: T,
    pub source: Source,
}

/// The three fields the loss answer maps to, and no others (DEC-182). F is the answer rounded down
/// to whole basis points; `max_loss_from_allocation` is F (`user_stated`), `max_drawdown` is 0.8 × F
/// and `max_daily_loss` 0.2 × F (`platform_proposed`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LossFields {
    pub max_loss_from_allocation: Drafted<SchemaDec>,
    pub max_drawdown: Drafted<SchemaDec>,
    pub max_daily_loss: Drafted<SchemaDec>,
}

/// The ladder and hysteresis proposed beneath a drawdown D, both `platform_proposed` (DEC-695 item
/// 7): `scale_sizes` (factor 0.5) at 0.375 × D, `exits_only` at 0.75 × D, `flatten_and_pause` at
/// exactly D (V-011), and hysteresis 0.125 × D, all but the flatten rung rounded down to whole bp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposedLadder {
    pub drawdown_ladder: Drafted<Vec<LadderRung>>,
    pub hysteresis: Drafted<SchemaDec>,
}

/// Why the owner is asked the loss question again rather than shown a draft.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskAgain {
    /// F is 0: no loss at all, or less than one basis point of the allocation.
    NoLoss,
    /// F is 1 or more: the whole allocation or beyond.
    WholeAllocation,
    /// Rounding leaves a rung or the hysteresis at 0, or breaks V-010 or V-012. No rung is
    /// ever dropped instead: that would de-risk later than the template's ladder.
    LadderCollapses,
}

/// A draft, or the reason the question is asked again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Draft<T> {
    Proposed(T),
    AskAgain(AskAgain),
}

/// DEC-695 item 6: the loss answer as its three fields, a dollar answer divided by `allocation_usd`
/// first. An F of 0, of 1 or more, or with no ladder beneath its drawdown is asked again.
pub fn loss_answer_fields(
    answer: &LossAnswer,
    allocation_usd: Usd,
) -> Result<Draft<LossFields>, SpecError> {
    let _ = (answer, allocation_usd);
    Err(SpecError::Unimplemented)
}

/// DEC-695 item 7: the ladder and hysteresis proposed beneath the drawdown `max_drawdown`, or
/// [`AskAgain::LadderCollapses`] when rounding collapses them.
pub fn proposed_ladder(max_drawdown: &SchemaDec) -> Result<Draft<ProposedLadder>, SpecError> {
    let _ = max_drawdown;
    Err(SpecError::Unimplemented)
}
