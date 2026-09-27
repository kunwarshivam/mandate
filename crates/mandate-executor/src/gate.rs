//! The binding gate's call site.
//!
//! `mandate-risk` (layer 4) is a **direct, crate-private dependency** in the implementation PR:
//! not injected, not behind a trait, and not replaceable by configuration, because a binding gate
//! a caller can substitute is not independent of agent logic (`AGENTS.md` rule 1, task brief
//! interpretation 4). Stream G's crate does not exist yet, so the call site is this function and
//! the dependency is added when it does — a workspace crate, so `docs/dependencies.md` is
//! unaffected.

use crate::error::ExecutorError;
use crate::ports::Ports;
use crate::state::ExecutorState;
use crate::types::{GateCheck, GateVerdict, IntentBody, Purpose, RiskClock};

/// What one gate run concluded, journaled as `GateDecided` with the verdict, the first failing
/// check's reason code, and the whole `checks` list (journal spec §9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateDecision {
    pub verdict: GateVerdict,
    pub checks: Vec<GateCheck>,
}

/// Runs trading-domain spec §9.1's eight checks in its evaluation order against fresh folded
/// state. Called once when an intent arrives and again immediately before every submission, which
/// is what makes a gate re-check able to abandon an intent the first pass allowed
/// (§5.7, interpretation 11).
///
/// Risk reduction is never denied here by conduct controls, eligibility, day-trade budgets,
/// buying power, or opening-session rules (`AGENTS.md` rule 13): those checks do not apply to a
/// [`Purpose`] the rule exempts.
#[allow(
    dead_code,
    reason = "the tests PR ships the call site and its contract; `handle` and `reconcile` call it in the implementation PR (DEC-77, DEC-83)"
)]
pub(crate) fn decide(
    state: &ExecutorState,
    body: &IntentBody,
    purpose: Purpose,
    at: RiskClock,
    ports: &Ports<'_>,
) -> Result<GateDecision, ExecutorError> {
    let _ = (state, body, purpose, at, ports);
    Err(ExecutorError::Unimplemented { story: "E7-2" })
}
