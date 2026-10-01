//! The executor's own checks at the binding gate's call site: **partial**, and named so.
//!
//! Stream G's §9.1 evaluation (`mandate-risk`) is not wired here yet. It arrives through a gate
//! port whose production adapter assembles G's inputs and fails closed (the coordinator's ruling
//! on DEC-129's partial gate). Until then this module runs only the checks whose inputs the
//! account stream itself carries — the account state, the agent's mode, an `Unknown` order in the
//! instrument, the working universe, and the quantity a sell may take — and every verdict it
//! returns is a [`PartialGateDecision`], journaled with `evaluation: account_stream_only`. An
//! allow from it is **not** the §9.1 evaluation, and no paper run may treat it as one. Nothing it
//! runs can be replaced from outside; the gate port can only narrow it further.

use mandate_accounting::{InstrumentId, Side};
use mandate_canon::Value;
use mandate_num::{Qty, SignedQty};

use crate::error::ExecutorError;
use crate::payload::{object, text};
use crate::ports::Ports;
use crate::protection::rests;
use crate::state::ExecutorState;
use crate::types::{AccountState, AgentId, GateCheck, GateVerdict, Mode, OrderState, Purpose};

/// What the executor's own account-stream checks concluded: a **partial** gate verdict, journaled
/// as `GateDecided` with `evaluation: account_stream_only`, the verdict, the first failing check's
/// reason code, and the whole `checks` list (journal spec §9). It is never the §9.1 evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartialGateDecision {
    pub verdict: GateVerdict,
    pub checks: Vec<GateCheck>,
    /// Whether the first failing check holds rather than denies: one of the holds `AGENTS.md`
    /// rule 13 names for an exit (agent mode `paused` or `stopped`, an `Unknown` order in the
    /// instrument, or `broker` on a blocked account), or `startup_reconciliation_pending` on an
    /// opening until an account has been journaled, at any time, and a reconciliation has run
    /// since this process started (#206 review; #230 review, minor 1).
    held: bool,
}

impl PartialGateDecision {
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

    /// DEC-160 (12): an allowed discretionary exit with nothing to price from is **held**
    /// `exit_unpriced`, protection resting, and re-evaluated at every tick. A discretionary exit
    /// may be paced, never denied (rule 13); no other exit is held for a price.
    pub(crate) fn unpriced(mut self) -> Self {
        self.checks.push(GateCheck {
            id: "exit_priceable",
            passed: false,
            inputs: Value::Null,
            computed: Value::Null,
        });
        self.verdict = GateVerdict::Deny {
            reason_code: UNPRICED.to_owned(),
        };
        self.held = true;
        self
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

/// The hold reason of an exit with nothing to price from (DEC-160 (12)).
pub(crate) const UNPRICED: &str = "exit_unpriced";

/// One order a gate run is asked about.
pub(crate) struct Proposal<'s> {
    pub(crate) agent: &'s AgentId,
    pub(crate) instrument: &'s InstrumentId,
    pub(crate) side: Side,
    pub(crate) qty: Qty,
    pub(crate) purpose: Purpose,
    /// Whether the order carries protection prices: a bracket, never a plain add (§5.4).
    pub(crate) bracketed: bool,
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
pub(crate) fn account_stream_checks(
    state: &ExecutorState,
    proposal: &Proposal<'_>,
    ports: &Ports<'_>,
) -> Result<PartialGateDecision, ExecutorError> {
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
    let unattributed = unattributed_opening(state, proposal.instrument, adds);
    record(
        "protection_attributed",
        unattributed.then_some(("protection_unattributed", true)),
    );
    let blocked = adds && !proposal.bracketed && rests(state, proposal.instrument);
    record(
        "protective_order",
        blocked.then_some(("add_blocked_by_protective_order", false)),
    );
    let unreconciled = unreconciled_opening(state, adds);
    record(
        "startup_reconciliation",
        unreconciled.then_some(("startup_reconciliation_pending", true)),
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
    Ok(PartialGateDecision {
        verdict,
        checks,
        held,
    })
}

/// DEC-160's leg-agent rule fails closed: a broker-created protective leg no agent can be named for
/// leaves its instrument **held** for openings, never denied, until the leg is attributed or gone;
/// a reconciliation establishes its presence, not its owner (trading-domain spec §5.4, §9.1 check
/// 4, before `add_blocked_by_protective_order`). Exits pass (`AGENTS.md` rule 13).
fn unattributed_opening(state: &ExecutorState, instrument: &InstrumentId, adds: bool) -> bool {
    adds && state.orders.values().any(|order| {
        order.agent.is_none()
            && &order.instrument == instrument
            && !order.state.is_terminal()
            && order.filled_qty < order.qty
    })
}

/// The startup reconciliation, last so a check that denies is reported first: an opening is
/// **held**, never denied, until the broker's account has been journaled and a reconciliation has
/// run since this process started, in either order (§11, the coordinator's ruling on #174). The
/// executor enforces it, never the shell's habit of reporting the account first. Exits pass
/// (`AGENTS.md` rule 13).
fn unreconciled_opening(state: &ExecutorState, adds: bool) -> bool {
    adds && !(state.observed.is_some() && state.reconciled_since_start())
}

/// §9.1 check 1: account state is evaluated before agent mode (§7.3). A blocked account denies a
/// risk-increasing order; a risk-reducing one is only **held**, for the broker, which is one of the
/// four holds `AGENTS.md` rule 13 names — the executor itself never denies an exit on it.
fn account_failure(state: AccountState, adds: bool) -> Option<(&'static str, bool)> {
    match state {
        AccountState::Blocked if adds => Some(("account_trading_blocked", false)),
        AccountState::Blocked => Some(("broker", true)),
        AccountState::ClosingOnly if adds => Some(("account_restricted", false)),
        AccountState::Active | AccountState::ClosingOnly => None,
    }
}

/// The agent's mode (§7.4). A risk-increasing order is denied by any mode stricter than `normal`;
/// a risk-reducing one is only ever held, and only by `paused` or `stopped`.
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
    } else if mode >= Mode::Paused {
        Some((reason, true))
    } else {
        None
    }
}

/// §5.3 rules 3 and 4: a sell may take at most the position less the open non-protective sells,
/// so no order crosses zero. Protective legs are left out because the executor cancels them
/// before a risk-reducing sell (§5.3's note on the first gate decision, §5.4). An open sell counts
/// what it may still sell, its unfilled quantity: its fills have already left the position
/// (DEC-160 (20), DEC-260), so counting them again would deny an exit that crosses nothing.
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
        .try_fold(Qty::ZERO, |total, order| {
            total.checked_add(order.qty.checked_sub(order.filled_qty).unwrap_or(Qty::ZERO))
        })?;
    Ok(long.checked_sub(selling).unwrap_or(Qty::ZERO))
}

#[cfg(test)]
mod attribution_tests {
    use mandate_accounting::{InstrumentId, Side};
    use mandate_num::{Qty, SignedQty};

    use super::{Proposal, account_stream_checks, unattributed_opening};
    use crate::error::ExecutorError;
    use crate::ids::ClientOrderId;
    use crate::ports::Ports;
    use crate::reconcile::tests::{Everything, Ids, executor_config, fees};
    use crate::state::ExecutorState;
    use crate::types::{
        AccountRef, AccountScope, AgentId, GateVerdict, Order, OrderState, Purpose, WorkspaceId,
    };

    fn state() -> ExecutorState {
        ExecutorState::new(AccountScope {
            account: AccountRef("acct-1".to_owned()),
            workspace: WorkspaceId("ws1".to_owned()),
        })
    }

    /// A broker-created leg of 10 AAPL resting for `agent`, or for no one.
    fn leg(agent: Option<&str>, state: OrderState, filled: &str) -> Result<Order, ExecutorError> {
        Ok(Order {
            client_order_id: ClientOrderId::parse("md-oco-1")?,
            intent_id: None,
            agent: agent.map(|agent| AgentId(agent.to_owned())),
            instrument: InstrumentId::new("AAPL")?,
            side: Side::Sell,
            qty: Qty::parse("10")?,
            filled_qty: Qty::parse(filled)?,
            state,
            attempt: 1,
            purpose: Purpose::Protective,
            absent_lookups: 0,
            first_absence_at: None,
            cancel_unconfirmed: false,
            replaced_by: None,
            created_on: None,
        })
    }

    /// DEC-160 3(c): an opening is held while an ownerless leg still works in its instrument, and
    /// only then — never an exit (`AGENTS.md` rule 13), never another instrument, and not once the
    /// leg is attributed, done, or wholly filled.
    #[test]
    fn only_an_opening_where_an_ownerless_leg_works_is_held() -> Result<(), ExecutorError> {
        let aapl = InstrumentId::new("AAPL")?;
        let msft = InstrumentId::new("MSFT")?;
        assert!(
            !unattributed_opening(&state(), &aapl, true),
            "nothing ownerless holds nothing"
        );
        for (agent, at, filled, held) in [
            (None, OrderState::Accepted, "0", true),
            (None, OrderState::PartiallyFilled, "5", true),
            (None, OrderState::PendingCancel, "0", true),
            (None, OrderState::PartiallyFilled, "10", false),
            (None, OrderState::Canceled, "0", false),
            (None, OrderState::Expired, "0", false),
            (None, OrderState::Filled, "10", false),
            (Some("agent-a"), OrderState::Accepted, "0", false),
        ] {
            let mut state = state();
            let order = leg(agent, at, filled)?;
            state.orders.insert(order.client_order_id.clone(), order);
            let case = format!("{agent:?} {at:?} filled {filled}");
            assert_eq!(unattributed_opening(&state, &aapl, true), held, "{case}");
            assert!(
                !unattributed_opening(&state, &aapl, false),
                "{case}: an exit is never held for attribution (rule 13)"
            );
            assert!(
                !unattributed_opening(&state, &msft, true),
                "{case}: another instrument is untouched"
            );
        }
        Ok(())
    }

    /// #258 round 1, major 1 and minor 3: the gate runs the attribution check, in its §9.1 place
    /// (check 4, after the quantity a sell may take and before the startup reconciliation), and an
    /// ownerless leg **holds** an opening with `protection_unattributed` — never denies it — while
    /// an exit in the same instrument passes it.
    #[test]
    fn the_gate_holds_an_opening_on_an_ownerless_leg_in_its_place() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let aapl = InstrumentId::new("AAPL")?;
        let agent = AgentId("agent-a".to_owned());
        let mut state = state();
        state
            .positions
            .insert(aapl.clone(), SignedQty::parse("10")?);
        let order = leg(None, OrderState::Accepted, "0")?;
        state.orders.insert(order.client_order_id.clone(), order);
        let ask = |side: Side, purpose: Purpose| {
            account_stream_checks(
                &state,
                &Proposal {
                    agent: &agent,
                    instrument: &aapl,
                    side,
                    qty: Qty::parse("5")?,
                    purpose,
                    bracketed: false,
                },
                &ports,
            )
        };
        let opening = ask(Side::Buy, Purpose::Increase)?;
        assert_eq!(
            opening
                .checks
                .iter()
                .map(|check| (check.id, check.passed))
                .collect::<Vec<_>>(),
            vec![
                ("account_state", true),
                ("agent_mode", true),
                ("unknown_order_in_flight", true),
                ("universe", true),
                ("sell_exceeds_available", true),
                ("protection_attributed", false),
                ("protective_order", true),
                ("startup_reconciliation", false),
            ]
        );
        assert_eq!(
            (opening.verdict.clone(), opening.verdict_name()),
            (
                GateVerdict::Deny {
                    reason_code: "protection_unattributed".to_owned()
                },
                "hold"
            ),
            "the attribution hold is the first failure, and a hold"
        );
        let exit = ask(Side::Sell, Purpose::RiskExit)?;
        assert_eq!(
            (exit.verdict.clone(), exit.verdict_name()),
            (GateVerdict::Allow, "allow"),
            "the exit passes every check"
        );
        Ok(())
    }
}
