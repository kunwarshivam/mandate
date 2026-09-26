//! Goal completion ([mandate spec §3.1](../../../docs/specs/mandate.md#31-goals-and-stop-conditions-dec-46-dec-59)).
//!
//! A `profit_stop` is **not** decided here: §3.1 confirms it by breach time inside the risk state
//! (§5.6), so [`status`] returns [`GoalStatus::ConfirmedInRiskState`] rather than `Running`. Returning
//! `Running` would invite a caller to decide the same condition twice, in two places, with two answers.

use mandate_num::{Price, Qty, ShareIncrement, Usd};
use mandate_time::UtcNanos;

use crate::SpecError;
use crate::risk::{GoalReason, StopReason, ThenAction};
use crate::validate::ValidatedMandate;

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
    pub qty_increment: ShareIncrement,
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
    let _ = (mandate, inputs);
    Err(SpecError::Unimplemented)
}
