//! The binding gate's call site.
//!
//! `mandate-risk` (layer 4) is a **direct, crate-private dependency**: not injected, not behind a
//! trait, and not replaceable by configuration, because a binding gate a caller can substitute is
//! not independent of agent logic (`AGENTS.md` rule 1, task brief interpretation 4). Stream G's
//! evaluation is not yet implemented, so until it is this function runs the checks whose inputs
//! the account stream itself carries — the account state, the agent's mode, an `Unknown` order in
//! the instrument, the working universe, and the quantity a sell may take — and nothing it runs
//! can be replaced from outside. It can only be narrowed further when stream G's evaluation is
//! added in front of the submission.

use mandate_accounting::{InstrumentId, Side};
use mandate_canon::Value;
use mandate_num::{Qty, SignedQty};

use crate::error::ExecutorError;
use crate::payload::{object, text};
use crate::ports::Ports;
use crate::state::ExecutorState;
use crate::types::{AccountState, AgentId, GateCheck, GateVerdict, Mode, OrderState, Purpose};

/// What one gate run concluded, journaled as `GateDecided` with the verdict, the first failing
/// check's reason code, and the whole `checks` list (journal spec §9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateDecision {
    pub verdict: GateVerdict,
    pub checks: Vec<GateCheck>,
    /// Whether the first failing check is one of the holds `AGENTS.md` rule 13 names — agent mode
    /// `paused` or `stopped`, or an `Unknown` order in the instrument — which keep an exit
    /// waiting and never deny it.
    held: bool,
}

impl GateDecision {
    /// The verdict's journal name: `allow`, `deny`, or `hold`.
    pub(crate) fn verdict_name(&self) -> &'static str {
        match &self.verdict {
            GateVerdict::Allow => "allow",
            GateVerdict::Deny { .. } if self.held => "hold",
            GateVerdict::Deny { .. } => "deny",
        }
    }

    pub(crate) fn reason_code(&self) -> &str {
        match &self.verdict {
            GateVerdict::Allow => "",
            GateVerdict::Deny { reason_code } => reason_code,
        }
    }

    pub(crate) fn allows(&self) -> bool {
        self.verdict == GateVerdict::Allow
    }

    /// The `checks` list as the journal carries it: each check's id and whether it passed.
    pub(crate) fn checks_value(&self) -> Result<Value, ExecutorError> {
        let checks = self
            .checks
            .iter()
            .map(|check| {
                object(vec![
                    ("id", text(check.id)),
                    ("passed", Value::Bool(check.passed)),
                ])
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Value::Array(checks))
    }
}

/// One order a gate run is asked about.
pub(crate) struct Proposal<'s> {
    pub(crate) agent: &'s AgentId,
    pub(crate) instrument: &'s InstrumentId,
    pub(crate) side: Side,
    pub(crate) qty: Qty,
    pub(crate) purpose: Purpose,
}

/// Runs the checks in trading-domain spec §9.1's evaluation order against fresh folded state.
/// Called once when an intent arrives and again immediately before every submission, which is
/// what makes a gate re-check able to abandon an intent the first pass allowed (§5.7,
/// interpretation 11).
///
/// Risk reduction is never denied here by conduct controls, eligibility, day-trade budgets,
/// buying power, or opening-session rules (`AGENTS.md` rule 13): none of those is among these
/// checks, and a risk-reducing order a mode or an `Unknown` order stops is **held**, never
/// denied.
pub(crate) fn decide(
    state: &ExecutorState,
    proposal: &Proposal<'_>,
    ports: &Ports<'_>,
) -> Result<GateDecision, ExecutorError> {
    let adds = proposal.purpose.adds_risk();
    let mut checks = Vec::new();
    let mut first: Option<(&'static str, bool)> = None;
    let mut record = |id: &'static str, failure: Option<(&'static str, bool)>| {
        first = first.or(failure);
        checks.push(GateCheck {
            id,
            passed: failure.is_none(),
            inputs: Value::Null,
            computed: Value::Null,
        });
    };

    record("account_state", account_failure(state.account_state, adds));
    record("agent_mode", mode_failure(state, proposal, adds));
    let unknown = state.orders.values().any(|order| {
        order.state == OrderState::Unknown && &order.instrument == proposal.instrument
    });
    record(
        "unknown_order_in_flight",
        unknown.then_some(("unknown_order_in_flight", !adds)),
    );
    let outside = adds && !ports.mandates.covers(proposal.agent, proposal.instrument);
    record(
        "universe",
        outside.then_some(("instrument_not_in_universe", false)),
    );
    let exceeds =
        proposal.side == Side::Sell && available(state, proposal.instrument)? < proposal.qty;
    record(
        "sell_exceeds_available",
        exceeds.then_some(("sell_exceeds_available", false)),
    );
    let (verdict, held) = match first {
        None => (GateVerdict::Allow, false),
        Some((reason, held)) => (
            GateVerdict::Deny {
                reason_code: reason.to_owned(),
            },
            held,
        ),
    };
    Ok(GateDecision {
        verdict,
        checks,
        held,
    })
}

/// §9.1 check 1: account state is evaluated before agent mode (§7.3).
fn account_failure(state: AccountState, adds: bool) -> Option<(&'static str, bool)> {
    match state {
        AccountState::Blocked => Some(("account_trading_blocked", false)),
        AccountState::ClosingOnly if adds => Some(("account_restricted", false)),
        AccountState::Active | AccountState::ClosingOnly => None,
    }
}

/// The agent's mode (§7.4). A risk-increasing order is denied by any mode stricter than `normal`;
/// a risk-reducing one is only ever held, and only by `paused` or `stopped`. A flatten is the kill
/// switch's own sell and is exempt from the mode it applied (§5.5).
fn mode_failure(
    state: &ExecutorState,
    proposal: &Proposal<'_>,
    adds: bool,
) -> Option<(&'static str, bool)> {
    let mode = state.effective_mode(proposal.agent);
    let reason = match mode {
        Mode::Normal => return None,
        Mode::ExitsOnly => "agent_exits_only",
        Mode::Paused => "agent_paused",
        Mode::Stopped => "agent_stopped",
    };
    if adds {
        Some((reason, false))
    } else if mode >= Mode::Paused && proposal.purpose != Purpose::Flatten {
        Some((reason, true))
    } else {
        None
    }
}

/// §5.3 rules 3 and 4: a sell may take at most the position less the open non-protective sells,
/// so no order crosses zero. Protective legs are left out because the executor cancels them
/// before a risk-reducing sell (§5.3's note on the first gate decision, §5.4).
fn available(state: &ExecutorState, instrument: &InstrumentId) -> Result<Qty, ExecutorError> {
    let held = state
        .positions
        .get(instrument)
        .copied()
        .unwrap_or(SignedQty::ZERO);
    let long = if held.is_negative() {
        Qty::ZERO
    } else {
        held.abs()
    };
    let selling = state
        .orders
        .values()
        .filter(|order| {
            &order.instrument == instrument
                && order.side == Side::Sell
                && order.purpose != Purpose::Protective
                && !order.state.is_terminal()
                && order.state != OrderState::Intent
        })
        .try_fold(Qty::ZERO, |total, order| total.checked_add(order.qty))?;
    Ok(long.checked_sub(selling).unwrap_or(Qty::ZERO))
}
