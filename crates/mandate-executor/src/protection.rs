//! The protective sequences and the exit price ladder (E7-4, trading-domain spec §5.4 to §5.6).

use mandate_accounting::InstrumentId;
use mandate_num::Price;

use crate::error::ExecutorError;
use crate::ports::Ports;
use crate::state::ExecutorState;
use crate::types::{ExitTier, MarketObservation, Purpose, RiskClock};

/// Where a ladder price came from, so a journaled exit says which of §5.6's three fallbacks was
/// used rather than only what it priced at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LadderReference {
    /// The best bid of a fresh, sane quote.
    FreshQuote,
    /// The last sane bid within the last five minutes.
    LastSaneBid,
    /// The last trade.
    LastTrade,
}

/// One rung of the exit price ladder: the limit to submit, which reference it came from, which
/// step this is, and whether the offset has reached `max_exit_offset` — the floor at which the
/// order rests and the owner is alerted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LadderPrice {
    pub limit: Price,
    pub reference: LadderReference,
    pub step: u32,
    pub at_floor: bool,
}

/// Prices one rung of trading-domain spec §5.6's ladder for a sell (a buy is symmetric).
///
/// The reference bid is the best bid of a fresh, sane quote; failing that the last sane bid
/// within five minutes; failing that the last trade. The limit is reference × (1 − offset),
/// rounded per §2.1. A step happens only after `exit_step_s` has elapsed, with the offset raised
/// by `exit_offset_step` and repriced from the **current** reference, and the offset never
/// exceeds `max_exit_offset`.
///
/// An owner exit outside the regular session prices from the bid the owner confirmed and never
/// below the floor that `OwnerExitRequested` carries (§5.5, mandate spec §6.1).
#[allow(
    dead_code,
    reason = "the tests PR ships the call site and its contract; `handle` and `reconcile` call it in the implementation PR (DEC-77, DEC-83)"
)]
pub(crate) fn ladder_price(
    tier: ExitTier,
    observations: &[MarketObservation],
    step: u32,
    now: RiskClock,
    floor: Option<Price>,
) -> Result<LadderPrice, ExecutorError> {
    let _ = (tier, observations, step, now, floor);
    Err(ExecutorError::Unimplemented { story: "E7-4" })
}

/// Whether one instrument's position is fully covered by resting protective orders right now.
///
/// Only the whole-share part of a fractional position can be protected, and the fraction is
/// disclosed rather than hidden (§5.4).
#[allow(
    dead_code,
    reason = "the tests PR ships the call site and its contract; `handle` and `reconcile` call it in the implementation PR (DEC-77, DEC-83)"
)]
pub(crate) fn is_protected(
    state: &ExecutorState,
    instrument: &InstrumentId,
    ports: &Ports<'_>,
) -> Result<bool, ExecutorError> {
    let _ = (state, instrument, ports);
    Err(ExecutorError::Unimplemented { story: "E7-4" })
}

/// Whether an exit of this purpose may be held at all. `AGENTS.md` rule 13 names exactly four
/// holds — agent mode `paused`, agent mode `stopped`, an `Unknown` order in the same instrument,
/// and the broker — and no pacing control is among them.
#[allow(
    dead_code,
    reason = "the tests PR ships the call site and its contract; `handle` and `reconcile` call it in the implementation PR (DEC-77, DEC-83)"
)]
pub(crate) fn exit_hold(
    state: &ExecutorState,
    instrument: &InstrumentId,
    purpose: Purpose,
) -> Result<Option<&'static str>, ExecutorError> {
    let _ = (state, instrument, purpose);
    Err(ExecutorError::Unimplemented { story: "E7-4" })
}
