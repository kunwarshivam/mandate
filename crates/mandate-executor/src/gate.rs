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
use mandate_num::{Price, Qty, SignedQty};
use mandate_risk::{
    Check as BindingCheck, CheckOutcome, GateInput, GatePass, Origin, ProposedKind, ProposedOrder,
    TimeInForce as GateTimeInForce, Verdict,
};

use crate::error::ExecutorError;
use crate::payload::{object, text};
use crate::ports::{BindingGateInput, BindingGateSource, Ports};
use crate::protection::{between_rungs, rests};
use crate::state::ExecutorState;
use crate::types::{
    AccountState, AgentId, GateCheck, GateVerdict, Mode, OrderState, ProtectionPrices, Purpose,
    TimeInForce,
};

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
    /// An allowed discretionary exit sized to what is left beside the rungs exit ladders will
    /// still send (DEC-410 item 3): journaled as `sized_qty`, and folded into the intent's
    /// quantity.
    sized: Option<Qty>,
    /// An allowed discretionary exit with nothing left beside those rungs (DEC-410 item 3): denied
    /// `sell_exceeds_available` only if nothing else holds it now ([`Self::crowded_out`]), so a
    /// hold for the session or a price comes first and it is decided again at release.
    crowded: bool,
    deferred: bool,
    binding: Option<BindingEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BindingEvidence {
    checks: Vec<CheckOutcome>,
    data_profile: String,
    feed: String,
    asset: mandate_risk::AssetId,
    market: mandate_risk::MarketSnapshot,
    side: Side,
    paced_limit: Option<Price>,
}

impl PartialGateDecision {
    /// The verdict's journal name: `allow`, `deny`, or `hold`.
    pub(crate) fn verdict_name(&self) -> &'static str {
        match &self.verdict {
            GateVerdict::Allow => "allow",
            GateVerdict::Deny { .. } if self.deferred => "defer",
            GateVerdict::Deny { .. } if self.held => "hold",
            GateVerdict::Deny { .. } => "deny",
        }
    }

    /// The quantity an allowed sell was sized to, if the gate sized it (DEC-410).
    pub(crate) fn sized(&self) -> Option<Qty> {
        self.sized
    }

    pub(crate) fn paced_limit(&self) -> Option<Price> {
        self.binding
            .as_ref()
            .and_then(|evidence| evidence.paced_limit)
    }

    pub(crate) fn is_binding(&self) -> bool {
        self.binding.is_some()
    }

    /// Whether this is [`Self::crowded_out`]'s denial, which is journaled even on a held
    /// intent's re-check, so it is terminal rather than retried at every tick (DEC-410 item 3).
    pub(crate) fn crowded_denial(&self) -> bool {
        self.crowded && !self.held && self.verdict != GateVerdict::Allow
    }

    /// DEC-410 item 3, applied after the session and price holds: an allowed discretionary exit
    /// with nothing left beside the rungs ladders will still send is denied
    /// `sell_exceeds_available`. The denial is terminal; a later proposal is judged afresh.
    pub(crate) fn crowded_out(mut self) -> Self {
        if !self.crowded || self.verdict != GateVerdict::Allow {
            return self;
        }
        for check in &mut self.checks {
            if check.id == "sell_exceeds_available" {
                check.passed = false;
            }
        }
        self.verdict = GateVerdict::Deny {
            reason_code: "sell_exceeds_available".to_owned(),
        };
        self.sized = None;
        self
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
    pub(crate) fn unpriced(self) -> Self {
        self.held_for("exit_priceable", UNPRICED)
    }

    /// The coordinator's ruling D1 on [#174] (5926142854): an equity exit while no v1 session is
    /// open is **held** `session_closed` for the calendar's next pre-market open, rule 13's broker
    /// hold, protection resting; one the calendar can give no next open for is held
    /// `session_unknown` and alerted, never sent.
    pub(crate) fn closed(self, reason: &'static str) -> Self {
        self.held_for("session_open", reason)
    }

    fn held_for(mut self, check: &'static str, reason: &str) -> Self {
        self.checks.push(GateCheck {
            id: check,
            passed: false,
            inputs: Value::Null,
            computed: Value::Null,
        });
        self.verdict = GateVerdict::Deny {
            reason_code: reason.to_owned(),
        };
        self.held = true;
        self.sized = None;
        self.crowded = false;
        self
    }

    /// The `checks` list as the journal carries it: each check's id and whether it passed.
    pub(crate) fn checks_value(&self) -> Result<Value, ExecutorError> {
        if let Some(evidence) = &self.binding {
            return binding_checks_value(&evidence.checks);
        }
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

    pub(crate) fn binding_journal_fields(
        &self,
    ) -> Result<Vec<(&'static str, Value)>, ExecutorError> {
        let Some(evidence) = &self.binding else {
            return Ok(vec![("evaluation", text("account_stream_only"))]);
        };
        let quotes = evidence.market.quote.map_or_else(Vec::new, |quote| {
            vec![object(vec![
                ("instrument_id", text(evidence.asset.as_str())),
                ("bid", text(quote.bid.to_string())),
                ("ask", text(quote.ask.to_string())),
                ("as_of", text(quote.at.to_string())),
                ("feed", text(&evidence.feed)),
            ])]
        });
        let marks = evidence.market.quote.map_or_else(Vec::new, |quote| {
            let mark = match evidence.side {
                Side::Buy => quote.ask,
                Side::Sell => quote.bid,
            };
            vec![object(vec![
                ("instrument_id", text(evidence.asset.as_str())),
                ("price", text(mark.to_string())),
                ("source", text("quote")),
                ("kind", text("risk")),
            ])]
        });
        Ok(vec![
            ("data_profile", text(&evidence.data_profile)),
            ("quotes_used", Value::Array(quotes.into_iter().collect::<Result<_, _>>()?)),
            ("marks_used", Value::Array(marks.into_iter().collect::<Result<_, _>>()?)),
        ])
    }
}

/// The hold reason of an exit with nothing to price from (DEC-160 (12)).
pub(crate) const UNPRICED: &str = "exit_unpriced";
/// The hold reasons of an equity exit while no v1 session is open (DEC-260 (13)).
pub(crate) const SESSION_CLOSED: &str = "session_closed";
pub(crate) const SESSION_UNKNOWN: &str = "session_unknown";

/// One order a gate run is asked about.
pub(crate) struct Proposal<'s> {
    pub(crate) agent: &'s AgentId,
    pub(crate) instrument: &'s InstrumentId,
    pub(crate) side: Side,
    pub(crate) qty: Qty,
    pub(crate) purpose: Purpose,
    /// Whether the order carries protection prices: a bracket, never a plain add (§5.4).
    pub(crate) protection: Option<ProtectionPrices>,
    pub(crate) limit: Price,
    pub(crate) tif: TimeInForce,
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
    let room = available(state, proposal.instrument)?;
    let sell = proposal.side == Side::Sell;
    let left = if sell {
        room.checked_sub(between_rungs(state, proposal.instrument)?)
            .unwrap_or(Qty::ZERO)
    } else {
        room
    };
    let discretionary = proposal.purpose == Purpose::DiscretionaryExit;
    let exceeds = sell && room < proposal.qty;
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
    let mut decision = PartialGateDecision {
        verdict,
        checks,
        held,
        sized: None,
        crowded: false,
        deferred: false,
        binding: None,
    };
    if decision.verdict == GateVerdict::Allow && sell && discretionary && left < proposal.qty {
        if left == Qty::ZERO {
            decision.crowded = true;
        } else {
            decision.sized = Some(left);
        }
    }
    Ok(decision)
}

/// Runs the executor's non-substitutable binding gate after its account-stream checks have allowed.
///
/// Missing or unreadable external snapshots refuse an opening. A reducing order keeps the local
/// decision because §9.1 does not read those missing inputs for a limit denial and rule 13 forbids
/// converting the absence into a denial.
pub(crate) fn binding_checks(
    state: &ExecutorState,
    proposal: &Proposal<'_>,
    pass: GatePass,
    ports: &Ports<'_>,
    source: Option<&dyn BindingGateSource>,
) -> Result<PartialGateDecision, ExecutorError> {
    let partial = account_stream_checks(state, proposal, ports)?;
    if partial.verdict != GateVerdict::Allow {
        return Ok(partial);
    }
    let Some(source) = source else {
        return if proposal.purpose.adds_risk() {
            Err(ExecutorError::BindingGateInputMissing)
        } else {
            Ok(partial)
        };
    };
    let Some(snapshot) = source.input(proposal.agent, proposal.instrument) else {
        return if proposal.purpose.adds_risk() {
            Err(ExecutorError::BindingGateInputMissing)
        } else {
            Ok(partial)
        };
    };
    if snapshot.asset != snapshot.instrument.instrument
        || snapshot.gate_agent != snapshot.agent.agent
    {
        return if proposal.purpose.adds_risk() {
            Err(ExecutorError::BindingGateInputMissing)
        } else {
            Ok(partial)
        };
    }
    evaluate_binding(partial, proposal, pass, snapshot)
}

fn evaluate_binding(
    mut partial: PartialGateDecision,
    proposal: &Proposal<'_>,
    pass: GatePass,
    snapshot: BindingGateInput,
) -> Result<PartialGateDecision, ExecutorError> {
    let proposed = ProposedOrder {
        instrument: snapshot.asset.clone(),
        side: proposal.side,
        qty: proposal.qty,
        limit_price: proposal.limit,
        kind: if let Some(protection) = proposal.protection {
            ProposedKind::Bracket {
                take_profit: protection.take_profit.unwrap_or(proposal.limit),
                stop: protection.stop,
            }
        } else {
            ProposedKind::Plain
        },
        tif: match proposal.tif {
            TimeInForce::Day => GateTimeInForce::Day,
            TimeInForce::Gtc => GateTimeInForce::Gtc,
            TimeInForce::Ioc => GateTimeInForce::Day,
        },
        extended_hours: false,
        origin: origin(proposal.purpose),
        owner_confirmed_bid: snapshot.owner_confirmed_bid,
        client_order_id: snapshot.gate_client_order_id,
        fee_reservation: snapshot.fee_reservation,
    };
    let input = GateInput {
        now: snapshot.now,
        pass,
        config: &snapshot.config,
        mandate: &snapshot.mandate,
        risk: &snapshot.risk,
        account: &snapshot.account,
        agent: &snapshot.agent,
        instrument: &snapshot.instrument,
        market: &snapshot.market,
        conduct: &snapshot.conduct,
        universe: &snapshot.universe,
        proposed: &proposed,
    };
    let decision = match mandate_risk::evaluate(&input) {
        Ok(decision) => decision,
        Err(error) if proposal.purpose.adds_risk() => {
            return Err(ExecutorError::BindingGateFailed { code: error.code() });
        }
        Err(_) => return Ok(partial),
    };
    let reason = decision
        .reason
        .map_or_else(String::new, |reason| reason.as_str().to_owned());
    let (verdict, held, deferred) = match decision.verdict {
        Verdict::Allow => (GateVerdict::Allow, false, false),
        Verdict::Deny => (GateVerdict::Deny { reason_code: reason }, false, false),
        Verdict::Hold => (GateVerdict::Deny { reason_code: reason }, true, false),
        Verdict::Defer => (GateVerdict::Deny { reason_code: reason }, false, true),
    };
    partial.verdict = verdict;
    partial.held = held;
    partial.deferred = deferred;
    partial.sized = match (partial.sized, decision.pacing.as_ref().map(|pacing| pacing.qty)) {
        (Some(local), Some(binding)) => Some(local.min(binding)),
        (local, binding) => local.or(binding),
    };
    partial.binding = Some(BindingEvidence {
        checks: decision.checks,
        data_profile: snapshot.data_profile,
        feed: snapshot.feed,
        asset: snapshot.asset,
        market: snapshot.market,
        side: proposal.side,
        paced_limit: decision.pacing.map(|pacing| pacing.limit_price),
    });
    Ok(partial)
}

fn origin(purpose: Purpose) -> Origin {
    match purpose {
        Purpose::Open | Purpose::Increase | Purpose::DiscretionaryExit => Origin::OrderBuilder,
        Purpose::RiskExit | Purpose::Flatten => Origin::RiskEngine,
        Purpose::OwnerExit => Origin::OwnerClose,
        Purpose::Protective => Origin::ProtectiveLeg,
    }
}

fn binding_checks_value(checks: &[CheckOutcome]) -> Result<Value, ExecutorError> {
    let rows = checks
        .iter()
        .flat_map(|outcome| {
            let (check, result) = match outcome {
                CheckOutcome::Passed(check) => (*check, "pass"),
                CheckOutcome::Failed(check, _) => (*check, "fail"),
                CheckOutcome::NotReached(check) => (*check, "not_reached"),
            };
            binding_check_ids(check)
                .into_iter()
                .map(move |id| (id, result))
        })
        .map(|(id, result)| {
            object(vec![
                ("id", text(*id)),
                ("result", text(result)),
                ("inputs", object(Vec::new())?),
                ("computed", object(Vec::new())?),
            ])
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Array(rows))
}

fn binding_check_ids(check: BindingCheck) -> &'static [&'static str] {
    match check {
        BindingCheck::AccountAndMode => &["account_status", "agent_mode"],
        BindingCheck::UniverseAndLimits => &["eligibility", "concentration", "order_size"],
        BindingCheck::SessionAndHalt => &["session", "halt"],
        BindingCheck::OrderConstraints => &["order_constraints"],
        BindingCheck::MarkAndCollar => &["mark_freshness", "collar"],
        BindingCheck::ConductControls => &["conduct"],
        BindingCheck::BuyingPowerAndExposure => &["buying_power", "gross_exposure"],
        BindingCheck::DayTradeBudget => &["day_trade_budget"],
    }
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
pub(crate) fn available(
    state: &ExecutorState,
    instrument: &InstrumentId,
) -> Result<Qty, ExecutorError> {
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
                    protection: None,
                    limit: mandate_num::Price::parse("150")?,
                    tif: crate::types::TimeInForce::Day,
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

#[cfg(test)]
mod remainder_tests {
    use mandate_accounting::{InstrumentId, Side};
    use mandate_num::{Qty, SignedQty, Usd};

    use super::{PartialGateDecision, Proposal, SESSION_CLOSED, account_stream_checks};
    use crate::error::ExecutorError;
    use crate::ids::{ClientOrderId, IntentId};
    use crate::ports::Ports;
    use crate::reconcile::tests::{Everything, Ids, executor_config, fees};
    use crate::state::{ExecutorState, Ladder, LoneLadder, ObservedAccount};
    use crate::types::{
        AccountRef, AccountScope, AccountState, AgentId, EventId, Mode, Order, OrderState, Purpose,
        Seq, WorkspaceId,
    };

    /// Ten AAPL held, `agent-b`'s lone ladder parked between rungs with 4 of its rung unsold, and,
    /// where `selling` is set, a live risk exit of that many beside it.
    fn parked(selling: Option<&str>) -> Result<ExecutorState, ExecutorError> {
        let aapl = InstrumentId::new("AAPL")?;
        let agent = AgentId("agent-b".to_owned());
        let mut state = ExecutorState::new(AccountScope {
            account: AccountRef("acct-1".to_owned()),
            workspace: WorkspaceId("ws1".to_owned()),
        });
        state
            .positions
            .insert(aapl.clone(), SignedQty::parse("10")?);
        let intent = IntentId(EventId("01JABCDEFGHJKMNPQRSTV00001".to_owned()));
        let sell = |id: ClientOrderId, qty: &str, at: OrderState| -> Result<Order, ExecutorError> {
            Ok(Order {
                client_order_id: id,
                intent_id: None,
                agent: Some(agent.clone()),
                instrument: aapl.clone(),
                side: Side::Sell,
                qty: Qty::parse(qty)?,
                filled_qty: Qty::ZERO,
                state: at,
                attempt: 1,
                purpose: Purpose::RiskExit,
                absent_lookups: 0,
                first_absence_at: None,
                cancel_unconfirmed: false,
                replaced_by: None,
                created_on: None,
            })
        };
        let rung = sell(
            ClientOrderId::for_intent(&intent)?,
            "4",
            OrderState::Canceled,
        )?;
        state.orders.insert(rung.client_order_id.clone(), rung);
        if let Some(qty) = selling {
            let live = sell(ClientOrderId::parse("md-other")?, qty, OrderState::Accepted)?;
            state.orders.insert(live.client_order_id.clone(), live);
        }
        let ladder = Ladder {
            stepping: true,
            parked: true,
            ..Ladder::default()
        };
        state.ladders.insert(
            (aapl, intent.clone()),
            LoneLadder {
                intent,
                agent,
                ladder,
            },
        );
        Ok(state)
    }

    fn ask(
        state: &ExecutorState,
        side: Side,
        qty: &str,
        purpose: Purpose,
    ) -> Result<PartialGateDecision, ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        account_stream_checks(
            state,
            &Proposal {
                agent: &AgentId("agent-a".to_owned()),
                instrument: &InstrumentId::new("AAPL")?,
                side,
                qty: Qty::parse(qty)?,
                purpose,
                protection: None,
                limit: mandate_num::Price::parse("150")?,
                tif: crate::types::TimeInForce::Day,
            },
            &ports,
        )
    }

    /// #499's review, minor 2: a cancelled rung whose applied fills ran past its quantity (5 of
    /// 4, which no writer of `filled_qty` produces today) leaves nothing, as `available` reads
    /// it, so the gate still decides: a risk exit of 10 is allowed whole rather than answered
    /// with an error, and nothing between rungs counts.
    #[test]
    fn an_over_filled_rung_counts_nothing_and_the_gate_still_decides() -> Result<(), ExecutorError>
    {
        let mut state = parked(None)?;
        let rung =
            ClientOrderId::for_intent(&IntentId(EventId("01JABCDEFGHJKMNPQRSTV00001".to_owned())))?;
        if let Some(order) = state.orders.get_mut(&rung) {
            order.filled_qty = Qty::parse("5")?;
        }
        let whole = ask(&state, Side::Sell, "10", Purpose::RiskExit)?;
        assert_eq!((whole.verdict_name(), whole.sized()), ("allow", None));
        assert_eq!(
            crate::protection::between_rungs(&state, &InstrumentId::new("AAPL")?)?,
            Qty::ZERO
        );
        Ok(())
    }

    /// DEC-410 item 2: beside a parked remainder of 4, a risk exit of 8 goes whole and is not
    /// sized (the plan gives way, never the sell). Item 3: a discretionary exit of 8 is sized to
    /// the 6 left instead, and one with nothing left beside a live exit of 6 is allowed by these
    /// checks alone, then denied `sell_exceeds_available` once nothing else holds it.
    #[test]
    fn a_risk_exit_goes_whole_and_a_discretionary_exit_is_sized_to_what_is_left()
    -> Result<(), ExecutorError> {
        let state = parked(None)?;
        let whole = ask(&state, Side::Sell, "8", Purpose::RiskExit)?;
        assert_eq!((whole.verdict_name(), whole.sized()), ("allow", None));
        let crowded = whole.crowded_out();
        assert_eq!(crowded.verdict_name(), "allow", "only a discretionary exit");
        let paced = ask(&state, Side::Sell, "8", Purpose::DiscretionaryExit)?;
        assert_eq!(
            (paced.verdict_name(), paced.sized()),
            ("allow", Some(Qty::parse("6")?))
        );
        assert_eq!(paced.crowded_out().verdict_name(), "allow", "6 is left");
        let nothing = ask(
            &parked(Some("6"))?,
            Side::Sell,
            "3",
            Purpose::DiscretionaryExit,
        )?;
        assert_eq!(
            (nothing.verdict_name(), nothing.sized()),
            ("allow", None),
            "the plan's half of rule 4 comes after the holds"
        );
        let held = nothing.clone().closed(SESSION_CLOSED).crowded_out();
        assert_eq!(
            (held.verdict_name(), held.reason_code()),
            ("hold", SESSION_CLOSED),
            "a closed market holds it, to be decided again at the open"
        );
        assert!(!nothing.crowded_denial(), "allowed, not yet denied");
        let denied = nothing.crowded_out();
        assert!(denied.crowded_denial());
        assert_eq!(
            (denied.verdict_name(), denied.reason_code(), denied.sized()),
            ("deny", "sell_exceeds_available", None)
        );
        let failed: Vec<&str> = denied
            .checks
            .iter()
            .filter(|check| !check.passed)
            .map(|check| check.id)
            .collect();
        assert_eq!(failed, vec!["sell_exceeds_available"]);
        Ok(())
    }

    /// DEC-410 item 1, two agents in one instrument, the review of #494's script: `agent-b`'s
    /// lone ladder is parked with 4 unsold and `agent-b` is stopped, so nothing will send it. It
    /// counts nothing, and `agent-a`'s risk exit for the whole position goes whole.
    #[test]
    fn a_stopped_agents_parked_ladder_never_shrinks_another_agents_exit()
    -> Result<(), ExecutorError> {
        let mut state = parked(None)?;
        state
            .modes
            .insert(AgentId("agent-b".to_owned()), Mode::Stopped);
        let flatten = ask(&state, Side::Sell, "10", Purpose::RiskExit)?;
        assert_eq!((flatten.verdict_name(), flatten.sized()), ("allow", None));
        Ok(())
    }

    /// DEC-410 item 3, the remainder gone: beside the same stopped or paused agent's parked
    /// ladder, which nothing will send, a discretionary exit for the whole position counts no
    /// plan, so it is neither sized nor crowded out and goes whole.
    #[test]
    fn a_stopped_agents_parked_ladder_leaves_a_discretionary_exit_whole()
    -> Result<(), ExecutorError> {
        for mode in [Mode::Stopped, Mode::Paused] {
            let mut state = parked(None)?;
            state.modes.insert(AgentId("agent-b".to_owned()), mode);
            let whole = ask(&state, Side::Sell, "10", Purpose::DiscretionaryExit)?;
            assert_eq!(
                (whole.verdict_name(), whole.sized()),
                ("allow", None),
                "{mode:?}"
            );
            assert_eq!(whole.crowded_out().verdict_name(), "allow", "{mode:?}");
        }
        Ok(())
    }

    /// DEC-410 sizes only an allowed discretionary exit: a paused agent's exit keeps its own
    /// hold, an over-sell against the live exits alone is still denied `sell_exceeds_available`
    /// (§5.3 rule 4), and an opening buy is never sized or crowded out.
    #[test]
    fn only_an_allowed_discretionary_exit_is_sized() -> Result<(), ExecutorError> {
        let mut paused = parked(None)?;
        paused
            .modes
            .insert(AgentId("agent-a".to_owned()), Mode::Paused);
        let held = ask(&paused, Side::Sell, "8", Purpose::DiscretionaryExit)?;
        assert_eq!(
            (held.verdict_name(), held.reason_code(), held.sized()),
            ("hold", "agent_paused", None)
        );
        assert_eq!(held.crowded_out().reason_code(), "agent_paused");
        let over = ask(&parked(Some("8"))?, Side::Sell, "3", Purpose::RiskExit)?;
        assert_eq!(
            (over.verdict_name(), over.reason_code(), over.sized()),
            ("deny", "sell_exceeds_available", None)
        );
        assert!(
            !over.crowded_denial(),
            "an over-sell beside open orders is not journaled on a re-check"
        );
        let mut reconciled = parked(None)?;
        reconciled.observed = Some(ObservedAccount {
            state: AccountState::Active,
            multiplier: 1,
            equity: Usd::parse("10000")?,
            cash: Usd::parse("10000")?,
            buying_power: Usd::parse("10000")?,
            non_marginable_buying_power: Usd::parse("10000")?,
            accrued_fees: Usd::ZERO,
            complete: true,
        });
        reconciled.started_at = Some(Seq(1));
        reconciled.reconciled_through = Some(Seq(2));
        let buy = ask(&reconciled, Side::Buy, "8", Purpose::Increase)?;
        assert_eq!(
            (
                buy.verdict_name(),
                buy.sized(),
                buy.crowded_out().verdict_name()
            ),
            ("allow", None, "allow")
        );
        Ok(())
    }
}
