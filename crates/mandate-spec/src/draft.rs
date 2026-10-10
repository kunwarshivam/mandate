//! What goal-first drafting proposes for the loss answer ([mandate spec §7](../../../docs/specs/mandate.md#7-compiler-and-platform-proposals-dec-97);
//! DEC-182, DEC-695 items 6 and 7): the three loss fields the answer maps to, and the drawdown
//! ladder proposed beneath them. Every value is an exact decimal, never a float.
//!
//! An input the schema excludes (an allocation of 0 or less, a drawdown outside (0, 1)) is a typed
//! [`SpecError::InvalidInput`], never a draft or a panic (DEC-901). A negative answer is less than
//! no loss, so it is asked again as [`AskAgain::NoLoss`].

use mandate_num::{Fraction, Ratio, Rounding, Usd};

use crate::document::{LadderAction, LadderRung, Source};
use crate::{DecGrammar, SchemaDec, SpecError};

/// Whole basis points: every drafted value but the flatten rung is rounded down to 4 places.
const BP_PLACES: u32 = 4;

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
    /// F is 0 or less: no loss at all, less than one basis point of the allocation, or a negative
    /// answer.
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
/// first. An F of 0 or less, of 1 or more, or with no ladder beneath its drawdown is asked again.
/// An allocation of 0 or less is [`SpecError::InvalidInput`] (DEC-901).
pub fn loss_answer_fields(
    answer: &LossAnswer,
    allocation_usd: Usd,
) -> Result<Draft<LossFields>, SpecError> {
    if allocation_usd <= Usd::ZERO {
        return Err(SpecError::InvalidInput {
            what: "allocation_usd",
        });
    }
    let floor = match *answer {
        LossAnswer::Fraction(share) => round_down_to_bp(ratio_as_usd(share)?)?,
        LossAnswer::Usd(amount) => ratio_as_usd(
            amount
                .negated()
                .ratio_to(allocation_usd, BP_PLACES, Rounding::Ceiling)?
                .negated(),
        )?,
    };
    if floor <= Usd::ZERO {
        return Ok(Draft::AskAgain(AskAgain::NoLoss));
    }
    if floor >= Usd::parse("1")? {
        return Ok(Draft::AskAgain(AskAgain::WholeAllocation));
    }
    let drawdown = open_fraction(floor.times_fraction(Fraction::parse("0.8")?)?)?;
    if let Draft::AskAgain(why) = proposed_ladder(&drawdown)? {
        return Ok(Draft::AskAgain(why));
    }
    Ok(Draft::Proposed(LossFields {
        max_loss_from_allocation: Drafted {
            value: open_fraction(floor)?,
            source: Source::UserStated,
        },
        max_drawdown: Drafted {
            value: drawdown,
            source: Source::PlatformProposed,
        },
        max_daily_loss: Drafted {
            value: open_fraction(floor.times_fraction(Fraction::parse("0.2")?)?)?,
            source: Source::PlatformProposed,
        },
    }))
}

/// DEC-695 item 7: the ladder and hysteresis proposed beneath the drawdown `max_drawdown`, or
/// [`AskAgain::LadderCollapses`] when rounding collapses them. A D outside (0, 1) is not an
/// `open_fraction`, so it is [`SpecError::InvalidInput`] (DEC-901).
///
/// A hysteresis of 0 is the only collapse that can happen. Once it is at least 1 bp, D is at least
/// 8 bp, so 0.125 D, 0.375 D, 0.75 D and D lie at least 2 bp apart; rounding the first three down
/// moves each by less than 1 bp, which keeps hysteresis < halving < exits only < D (V-010, V-012).
pub fn proposed_ladder(max_drawdown: &SchemaDec) -> Result<Draft<ProposedLadder>, SpecError> {
    let flatten =
        SchemaDec::parse(max_drawdown.as_str(), DecGrammar::OpenFraction).map_err(|_| {
            SpecError::InvalidInput {
                what: "max_drawdown",
            }
        })?;
    let drawdown = flatten.to_usd()?;
    let share_of_drawdown = |share: &str| -> Result<Usd, SpecError> {
        round_down_to_bp(drawdown.times_fraction(Fraction::parse(share)?)?)
    };
    let hysteresis = share_of_drawdown("0.125")?;
    if hysteresis <= Usd::ZERO {
        return Ok(Draft::AskAgain(AskAgain::LadderCollapses));
    }
    let rung = |at: SchemaDec, action, factor| LadderRung { at, action, factor };
    let rungs = vec![
        rung(
            open_fraction(share_of_drawdown("0.375")?)?,
            LadderAction::ScaleSizes,
            Some(open_fraction(Usd::parse("0.5")?)?),
        ),
        rung(
            open_fraction(share_of_drawdown("0.75")?)?,
            LadderAction::ExitsOnly,
            None,
        ),
        rung(flatten, LadderAction::FlattenAndPause, None),
    ];
    Ok(Draft::Proposed(ProposedLadder {
        drawdown_ladder: Drafted {
            value: rungs,
            source: Source::PlatformProposed,
        },
        hysteresis: Drafted {
            value: open_fraction(hysteresis)?,
            source: Source::PlatformProposed,
        },
    }))
}

/// `value` rounded down (towards negative infinity) to whole basis points: the ceiling of its
/// negation, negated, since [`Rounding`] has no floor mode.
fn round_down_to_bp(value: Usd) -> Result<Usd, SpecError> {
    Ok(value
        .negated()
        .round(BP_PLACES, Rounding::Ceiling)?
        .negated())
}

/// The same decimal as dollars, the carrier the rounding and the ratios above take.
fn ratio_as_usd(value: Ratio) -> Result<Usd, SpecError> {
    Ok(Usd::parse(&value.to_string())?)
}

/// A drafted value as the `open_fraction` its field declares. Every caller has already shown it
/// lies strictly between 0 and 1, so the mismatch is a guard rather than an outcome.
fn open_fraction(value: Usd) -> Result<SchemaDec, SpecError> {
    SchemaDec::parse(&value.to_string(), DecGrammar::OpenFraction).map_err(|_| {
        SpecError::InvalidInput {
            what: "drafted fraction",
        }
    })
}
