//! Goal completion ([mandate spec §3.1](../../../docs/specs/mandate.md#31-goals-and-stop-conditions-dec-46-dec-59)).
//!
//! A `profit_stop` is **not** decided here: §3.1 confirms it by breach time inside the risk state
//! (§5.6), so [`status`] returns [`GoalStatus::ConfirmedInRiskState`] rather than `Running`. Returning
//! `Running` would invite a caller to decide the same condition twice, in two places, with two answers.

use mandate_num::{NumError, Price, Qty, Usd};
use mandate_time::{UtcNanos, new_york_midnight};

use crate::document::{Goal, OnComplete, Pointer};
use crate::risk::{GoalReason, StopReason, ThenAction};
use crate::validate::ValidatedMandate;
use crate::{SchemaDec, SpecError};

/// What the agent holds and what the market offers, which is all §3.1 needs to say whether a goal is
/// done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalInputs {
    pub now: UtcNanos,
    pub position_qty: Qty,
    /// The sum of the agent's buy fills in the goal instrument, fees included. Sales never reduce it
    /// (§3.1).
    pub goal_spent_usd: Usd,
    pub min_order_usd: Usd,
    /// The instrument's quantity increment, as the reference cases carry it: a decimal (`1` for whole
    /// shares, `0.0001` for BTC), not the whole-or-fractional grid `mandate_num::ShareIncrement`
    /// describes, which cannot express `0.0001` and would make MC-L02's dust remainder look tradable.
    /// Zero is [`SpecError::InvalidInput`].
    pub qty_increment: Qty,
    pub ask: Price,
}

/// Where a goal stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalStatus {
    Running,
    /// A `profit_stop`: the risk state confirms it (§3.1, §5.6).
    ConfirmedInRiskState,
    Done {
        reason: GoalReason,
        then: ThenAction,
        stop_reason: StopReason,
    },
}

/// Whether the goal is done, why, and what follows (§3.1).
///
/// An `end_date` ends the goal at 00:00 America/New_York **after** that date, so the last risk day of
/// the goal is the date itself.
pub fn status(mandate: &ValidatedMandate, inputs: &GoalInputs) -> Result<GoalStatus, SpecError> {
    let goal = &mandate.mandate().goal;
    let ended = end_date_passed(goal, inputs.now)?;
    match goal {
        Goal::ProfitStop { .. } if ended => Ok(GoalStatus::Done {
            reason: GoalReason::EndDate,
            then: ThenAction::DiscretionaryExitAllThenRetire,
            stop_reason: StopReason::EndDate,
        }),
        Goal::ProfitStop { .. } => Ok(GoalStatus::ConfirmedInRiskState),
        Goal::Continuous { on_complete, .. } => {
            Ok(done_if(ended.then_some(GoalReason::EndDate), *on_complete))
        }
        Goal::Accumulate {
            target_qty,
            max_spend_usd,
            on_complete,
            ..
        } => {
            let reason = if quantity_exhausted(target_qty, inputs)? {
                Some(GoalReason::TargetQty)
            } else if spend_exhausted(max_spend_usd, inputs)? {
                Some(GoalReason::MaxSpend)
            } else {
                ended.then_some(GoalReason::EndDate)
            };
            Ok(done_if(reason, *on_complete))
        }
    }
}

/// `Done` with the owner's `on_complete` when there is a reason, `Running` when there is none.
fn done_if(reason: Option<GoalReason>, on_complete: OnComplete) -> GoalStatus {
    match reason {
        Some(reason) => GoalStatus::Done {
            reason,
            then: ThenAction::Applied(on_complete),
            stop_reason: StopReason::GoalComplete,
        },
        None => GoalStatus::Running,
    }
}

/// Whether `now` is at or after 00:00 America/New_York on the day after `end_date`: the date is the
/// goal's last risk day, and the midnight that closes it is the first instant the goal is over.
fn end_date_passed(goal: &Goal, now: UtcNanos) -> Result<bool, SpecError> {
    match goal.end_date() {
        Some(end_date) => Ok(now >= new_york_midnight(end_date.next()?)?),
        None => Ok(false),
    }
}

/// Whether what remains of `target_qty` is below one increment, or worth less than the minimum order
/// at the ask. A position at or past the target leaves nothing, which is below any increment.
fn quantity_exhausted(target_qty: &SchemaDec, inputs: &GoalInputs) -> Result<bool, SpecError> {
    if inputs.qty_increment.is_zero() {
        return Err(SpecError::InvalidInput {
            what: "qty_increment, which must be above zero",
        });
    }
    let target = Qty::parse(target_qty.as_str()).map_err(out_of_range("/goal/target_qty"))?;
    let remaining = match target.checked_sub(inputs.position_qty) {
        Ok(remaining) => remaining,
        Err(NumError::Negative) => return Ok(true),
        Err(other) => return Err(other.into()),
    };
    Ok(remaining < inputs.qty_increment || remaining.notional(inputs.ask)? < inputs.min_order_usd)
}

/// Whether what remains of `max_spend_usd` is below the minimum order. Goal spend counts fees and is
/// never reduced by a sale, which is the caller's sum; spend past the cap leaves a negative remainder,
/// which is below any minimum.
fn spend_exhausted(max_spend_usd: &SchemaDec, inputs: &GoalInputs) -> Result<bool, SpecError> {
    let cap = max_spend_usd
        .to_usd()
        .map_err(out_of_range("/goal/max_spend_usd"))?;
    Ok(cap.checked_sub(inputs.goal_spent_usd)? < inputs.min_order_usd)
}

fn out_of_range(path: &'static str) -> impl Fn(NumError) -> SpecError {
    move |cause| SpecError::OutOfRange {
        path: Pointer::new(path),
        cause,
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use mandate_num::{Price, Qty, Usd};
    use mandate_time::UtcNanos;

    use super::{GoalInputs, quantity_exhausted};
    use crate::{DecGrammar, SchemaDec};

    type Checked = Result<(), Box<dyn Error>>;

    fn inputs(position: &str, ask: &str) -> Result<GoalInputs, Box<dyn Error>> {
        Ok(GoalInputs {
            now: UtcNanos::parse("2026-09-21T15:00:00.000000000Z")?,
            position_qty: Qty::parse(position)?,
            goal_spent_usd: Usd::ZERO,
            min_order_usd: Usd::parse("1")?,
            qty_increment: Qty::parse("0.0001")?,
            ask: Price::parse(ask)?,
        })
    }

    /// A remainder worth exactly the minimum order can still be bought; a cent less cannot (§3.1).
    ///
    /// 0.01 of a 0.15 target remains, so at an ask of 100 it is worth 1.00, the minimum itself.
    #[test]
    fn a_remainder_worth_exactly_the_minimum_order_is_not_done() -> Checked {
        let target = SchemaDec::parse("0.15", DecGrammar::PositiveDecimal)?;
        assert!(!quantity_exhausted(&target, &inputs("0.14", "100")?)?);
        assert!(quantity_exhausted(&target, &inputs("0.14", "99.99")?)?);
        Ok(())
    }

    /// A position past the target leaves nothing to buy, whatever the ask (§3.1).
    #[test]
    fn a_position_past_the_target_is_done() -> Checked {
        let target = SchemaDec::parse("0.15", DecGrammar::PositiveDecimal)?;
        assert!(quantity_exhausted(&target, &inputs("0.2", "1000000")?)?);
        Ok(())
    }
}
