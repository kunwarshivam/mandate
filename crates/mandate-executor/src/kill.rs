//! The kill switch (trading-domain spec §5.5, `AGENTS.md` rule 13, DEC-100): one command, one
//! ordered sequence — the final mode first, then the cancels, each confirmed, then the close —
//! each step journaled, so a crash inside it leaves the journal saying where it stopped and
//! [`crate::Input::Started`] resumes from there rather than restarting the sequence
//! (task brief interpretation 20).
//!
//! The scopes differ in what they may reach. The account and workspace scopes are the only two
//! that may use the broker's `cancel-all` and `close-position` endpoints, and they are the only
//! two that can build an [`AccountWideScope`]: its constructor is `pub(crate)` and this module's
//! switch and close steps are its only callers, so no agent-scoped code path can name those
//! endpoints (trading-domain spec §5.5, task brief interpretation 18). The agent scope cancels
//! only that agent's orders, and any unattributed protective leg in an instrument it closes, each
//! by `client_order_id`, and sells exactly the agent's sub-ledger quantity.
//!
//! Automated switches (mandate limits) sell equities only in the regular session, leaving
//! protection in place until then, and crypto at once; their orders are `risk_exit`. Owner
//! switches sell outside the regular session once the owner has confirmed the displayed bid, bid
//! size, and floor, through the exit price ladder, never below the floor; their orders are
//! `owner_exit`. Kill-switch orders are exempt from the agent's mode, never wait for approval,
//! and never depend on model state (§5.5), so they run no gate and no pacing control: the sell is
//! journaled and sent from the plan itself, under the id the switch's own record derives
//! (`md-k-<record>-<ordinal>`, §2.3's pattern for an order the executor originates, DEC-160 (11)).

use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::{AssetClass, InstrumentId, Side};
use mandate_canon::Value;
use mandate_num::{Adverse, Price, Qty, SignedQty};

use crate::batch::Batch;
use crate::codec::{mode_name, purpose_name};
use crate::error::ExecutorError;
use crate::fold::held_by;
use crate::ids::{ClientOrderId, IntentId};
use crate::intent::journal_submission;
use crate::payload::{object, text};
use crate::protection::{ask_cancel, changed, interval_open, long, rests, ticked};
use crate::session::{Venue, venue};
use crate::state::{ConfirmedBid, ExecutorState, PendingFlatten};
use crate::types::{
    AccountWide, AccountWideScope, AgentId, BrokerRequest, EventId, Initiator, KillScope,
    OrderType, OwnerConfirmation, Purpose, SubmitOrder, TimeInForce,
};

/// The kill switch's own step (§5.5): the final mode, the switch's record, the cancels, and every
/// sell or close that needs nothing further. Everything that waits — a sell behind its cancels, an
/// equity close behind the session — waits as folded state, so a restart resumes it.
pub(crate) fn switch(
    batch: &mut Batch<'_, '_>,
    scope: KillScope,
    initiator: Initiator,
    confirmation: Option<OwnerConfirmation>,
) -> Result<(), ExecutorError> {
    for agent in agents_of(&batch.view, &scope) {
        batch.journal(
            "AgentModeApplied",
            None,
            vec![
                ("agent", text(agent.0.clone())),
                ("to", text(mode_name(initiator.final_mode()))),
                ("reason", text("kill_switch")),
                ("originated", Value::Bool(true)),
            ],
        )?;
    }
    let mut canceled: Vec<String> = Vec::new();
    let mut plans: Vec<Plan> = Vec::new();
    match &scope {
        KillScope::Agent(agent) => {
            for instrument in instruments_of_agent(&batch.view, agent) {
                for id in live_orders_of_agent(&batch.view, agent, &instrument)
                    .into_iter()
                    .chain(unattributed_legs(&batch.view, &instrument))
                {
                    canceled.push(id.as_str().to_owned());
                }
                let lots = held_by(&batch.view, agent, &instrument)?;
                let spoken = spoken_for(&batch.view, agent, &instrument)?;
                let remaining = lots.checked_sub(spoken).unwrap_or(Qty::ZERO);
                if remaining > Qty::ZERO {
                    let deferred = session_deferred(batch, &instrument, &initiator, &confirmation);
                    plans.push(Plan {
                        instrument,
                        qty: remaining,
                        purpose: initiator.sell_purpose(),
                        agent: Some(agent.clone()),
                        deferred,
                        close: false,
                        wide: None,
                        confirmed: confirmed_pricing(&confirmation),
                    });
                }
            }
        }
        KillScope::Account(_) | KillScope::Workspace(_) => {
            let wide = wide_of(&scope);
            for (instrument, held) in batch.view.positions() {
                if held.is_negative() || *held == SignedQty::ZERO {
                    continue;
                }
                let deferred = session_deferred(batch, instrument, &initiator, &confirmation);
                plans.push(Plan {
                    instrument: instrument.clone(),
                    qty: held.abs(),
                    purpose: initiator.sell_purpose(),
                    agent: None,
                    deferred,
                    close: true,
                    wide: Some(wide.clone()),
                    confirmed: confirmed_pricing(&confirmation),
                });
            }
        }
    }
    let deferred: Vec<Value> = plans
        .iter()
        .filter(|plan| plan.deferred)
        .map(|plan| text(plan.instrument.as_str()))
        .collect();
    let sells = plans
        .iter()
        .map(|plan| -> Result<Value, ExecutorError> {
            let mut pairs = vec![
                ("instrument", text(plan.instrument.as_str())),
                ("qty", text(plan.qty.to_string())),
                ("purpose", text(purpose_name(plan.purpose))),
                ("deferred", Value::Bool(plan.deferred)),
                ("close", Value::Bool(plan.close)),
            ];
            if let Some(agent) = &plan.agent {
                pairs.push(("agent", text(agent.0.clone())));
            }
            if let Some(wide) = &plan.wide {
                pairs.push(("wide", text(wide_name(wide))));
            }
            if let Some(confirmed) = &plan.confirmed {
                pairs.push(("bid", text(confirmed.bid.to_string())));
                pairs.push(("bid_size", text(confirmed.bid_size.to_string())));
                pairs.push(("floor", text(confirmed.floor.to_string())));
            }
            object(pairs)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let (kind, subject) = subject_of(&scope);
    batch.journal(
        "KillSwitchActivated",
        None,
        vec![
            ("scope", text(kind)),
            ("subject", text(subject)),
            ("initiator", text(initiator_name(&initiator))),
            ("canceled", text(canceled.join(" "))),
            ("confirmed", Value::Bool(confirmation.is_some())),
            ("deferred", Value::Array(deferred)),
            ("sells", Value::Array(sells)),
        ],
    )?;
    let mut touched: BTreeSet<InstrumentId> = BTreeSet::new();
    match &scope {
        KillScope::Agent(agent) => {
            for instrument in instruments_of_agent(&batch.view, agent) {
                for id in live_orders_of_agent(&batch.view, agent, &instrument)
                    .into_iter()
                    .chain(unattributed_legs(&batch.view, &instrument))
                {
                    if ask_cancel(batch, &id)? {
                        batch.broker(BrokerRequest::Cancel {
                            client_order_id: id,
                        });
                        touched.insert(instrument.clone());
                    }
                }
            }
        }
        KillScope::Account(_) | KillScope::Workspace(_) => {
            let wide = wide_of(&scope);
            batch.view.wide_confirmed.remove(wide_name(&wide));
            batch.broker(BrokerRequest::CancelAll(AccountWideScope::for_scope(wide)));
        }
    }
    for instrument in touched {
        open_interval(batch, &instrument)?;
    }
    for plan in plans {
        if plan.close || waiting_on_cancels(&batch.view, &plan.instrument) {
            continue;
        }
        release_plan(batch, &plan.instrument)?;
    }
    Ok(())
}

/// The broker's account-wide `cancel-all` answered: the cancel step is confirmed, so the close
/// step goes — the broker's `close-position` per instrument the account still holds, at once for
/// crypto and in the regular session for equities (§5.5). An equity close outside the session
/// waits as folded state and goes at the first tick of the session. The scope's confirmation is
/// process-local, so a restart asks the idempotent `cancel-all` again and reconfirms from its
/// answer rather than closing on a memory.
pub(crate) fn account_wide_accepted(batch: &mut Batch<'_, '_>) -> Result<(), ExecutorError> {
    batch.view.wide_confirmed.insert("account".to_owned());
    batch.view.wide_confirmed.insert("workspace".to_owned());
    for instrument in batch.view.flattens.keys().cloned().collect::<Vec<_>>() {
        close_due_for(batch, &instrument)?;
    }
    Ok(())
}

/// A restart's resume of an account- or workspace-scoped switch whose close has not gone: the
/// `cancel-all` is asked again, which cancels nothing that is already gone, and its answer
/// reconfirms the scope before any close is built (§5.5's cancel → confirm → close).
pub(crate) fn resume(batch: &mut Batch<'_, '_>) -> Result<(), ExecutorError> {
    let scopes: BTreeSet<&'static str> = batch
        .view
        .flattens
        .values()
        .filter(|plan| plan.close)
        .filter_map(|plan| plan.wide.as_ref().map(wide_name))
        .collect();
    for scope in scopes {
        batch.view.wide_confirmed.remove(scope);
        let wide = match scope {
            "account" => AccountWide::Account(batch.view.scope.account.clone()),
            _ => AccountWide::Workspace(batch.view.scope.workspace.clone()),
        };
        batch.broker(BrokerRequest::CancelAll(AccountWideScope::for_scope(wide)));
    }
    Ok(())
}

/// Every step's tail: the sells and closes that waited on a cancel's confirmation or on the
/// session, once what they waited on is gone (§5.5's cancel → confirm → close).
pub(crate) fn release(batch: &mut Batch<'_, '_>) -> Result<(), ExecutorError> {
    let pending: Vec<InstrumentId> = batch.view.flattens.keys().cloned().collect();
    for instrument in pending {
        let Some(plan) = batch.view.flattens.get(&instrument).cloned() else {
            continue;
        };
        if plan.close {
            close_due_for(batch, &instrument)?;
            continue;
        }
        if waiting_on_cancels(&batch.view, &instrument) {
            continue;
        }
        if sell_waits_for_session(batch, &instrument, &plan) {
            continue;
        }
        release_plan(batch, &instrument)?;
    }
    Ok(())
}

/// Whether a planned sell still waits for the regular session (§5.5): an automated equity sell
/// does while no v1 session is open, and an unconfirmed owner equity sell until the session —
/// the plan's `deferred` was journaled when the switch was made, and the venue is read fresh at
/// every release so the first tick of the session is the first release.
fn sell_waits_for_session(
    batch: &Batch<'_, '_>,
    instrument: &InstrumentId,
    plan: &PendingFlatten,
) -> bool {
    if batch.ports.instruments.asset_class(instrument) == Some(AssetClass::Crypto) {
        return false;
    }
    if !plan.deferred {
        return false;
    }
    venue(batch.ports, instrument, batch.at()) != Venue::Regular
}

/// The close of one instrument, once its scope's `cancel-all` is confirmed and its session allows
/// it: journaled first (§5.5's journal each step), then requested through the only path that may
/// name the endpoint. The close-step record is what the fold reads to end the plan, so a replay
/// ends it too.
fn close_due_for(
    batch: &mut Batch<'_, '_>,
    instrument: &InstrumentId,
) -> Result<(), ExecutorError> {
    let Some(plan) = batch.view.flattens.get(instrument).cloned() else {
        return Ok(());
    };
    if !plan.close {
        return Ok(());
    }
    if plan.deferred
        && batch.ports.instruments.asset_class(instrument) != Some(AssetClass::Crypto)
        && venue(batch.ports, instrument, batch.at()) != Venue::Regular
    {
        return Ok(());
    }
    let wide = plan
        .wide
        .clone()
        .ok_or_else(|| ExecutorError::NotInterpreted {
            what: "an account-wide close with no scope".to_owned(),
            story: "E7-4",
        })?;
    if !batch.view.wide_confirmed.contains(wide_name(&wide)) {
        return Ok(());
    }
    batch.journal(
        "KillSwitchActivated",
        None,
        vec![
            ("step", text("close")),
            ("instrument", text(instrument.as_str())),
            ("scope", text(wide_name(&wide))),
        ],
    )?;
    batch.broker(BrokerRequest::ClosePosition(
        AccountWideScope::for_scope(wide),
        instrument.clone(),
    ));
    Ok(())
}

/// One planned sell, sent (§5.5): exactly the planned quantity, capped at what the position still
/// holds, priced by the switch's kind.
///
/// An automated sell is a market order in the regular session, or a ladder rung from the current
/// quotes in the closing window or extended hours, and crypto goes at once. An owner's confirmed
/// equity sell goes through the exit price ladder from the confirmed bid, rounded up to the tick
/// (§2.1, DEC-260 (6)), never below the confirmed floor, and extended-hours, which is what the
/// confirmation is for — the same limit trades at once in the regular session, so the flag never
/// delays the exit. An unconfirmed owner's equity sell is a market order in the regular session
/// and waits elsewhere (§5.5). Nothing to price from holds the sell for the next quote, never a
/// price nobody saw (§5.6 step 4, DEC-160 (12)).
fn release_plan(batch: &mut Batch<'_, '_>, instrument: &InstrumentId) -> Result<(), ExecutorError> {
    let Some(plan) = batch.view.flattens.get(instrument).cloned() else {
        return Ok(());
    };
    if plan.close {
        return close_due_for(batch, instrument);
    }
    let Some(agent) = plan.agent.clone() else {
        return Ok(());
    };
    let held = long(&batch.view, instrument)?;
    let qty = plan.qty.min(held);
    if qty == Qty::ZERO {
        return Ok(());
    }
    let crypto = batch.ports.instruments.asset_class(instrument) == Some(AssetClass::Crypto);
    let confirmed = plan.confirmed.clone();
    let at = batch.at();
    let shape = match (plan.purpose, &confirmed) {
        (Purpose::OwnerExit, Some(confirmation)) if !crypto => {
            let limit = match batch.ports.instruments.exit_tier(instrument) {
                Some(tier) => confirmation
                    .bid
                    .collar_bound(tier.exit_offset, Adverse::Down)?,
                None => confirmation.bid,
            };
            Shape::Limit {
                limit: ticked(
                    batch.ports.instruments.asset_class(instrument),
                    limit.max(confirmation.floor),
                ),
                extended_hours: true,
            }
        }
        (Purpose::OwnerExit, None) if !crypto => {
            if venue(batch.ports, instrument, at) != Venue::Regular {
                return Ok(());
            }
            Shape::Market
        }
        _ if crypto || venue(batch.ports, instrument, at) == Venue::Regular => Shape::Market,
        _ => match batch.ports.instruments.exit_tier(instrument) {
            Some(tier) => match crate::protection::ladder_price(
                tier,
                &crate::protection::observed(batch, instrument),
                0,
                at,
                None,
            ) {
                Ok(rung) => Shape::Limit {
                    limit: ticked(batch.ports.instruments.asset_class(instrument), rung.limit),
                    extended_hours: false,
                },
                Err(_) => return Ok(()),
            },
            None => return Ok(()),
        },
    };
    let request = SubmitOrder {
        client_order_id: kill_order_id(&plan.record, plan.ordinal)?,
        instrument: instrument.clone(),
        side: Side::Sell,
        qty,
        order_type: shape.order_type(),
        tif: if crypto {
            TimeInForce::Gtc
        } else {
            TimeInForce::Day
        },
        limit_price: shape.limit(),
        stop_price: None,
        bracket: None,
        oco: None,
        extended_hours: shape.extended_hours(),
        purpose: plan.purpose,
    };
    journal_submission(batch, &request, None, &agent, 1)?;
    batch.broker(BrokerRequest::Submit(request));
    Ok(())
}

/// The shape one sell takes: a market order, or a limit at the price it was priced at.
enum Shape {
    Market,
    Limit { limit: Price, extended_hours: bool },
}

impl Shape {
    fn order_type(&self) -> OrderType {
        match self {
            Self::Market => OrderType::Market,
            Self::Limit { .. } => OrderType::Limit,
        }
    }

    fn limit(&self) -> Option<Price> {
        match self {
            Self::Market => None,
            Self::Limit { limit, .. } => Some(*limit),
        }
    }

    fn extended_hours(&self) -> bool {
        matches!(
            self,
            Self::Limit {
                extended_hours: true,
                ..
            }
        )
    }
}

/// The sell's own id: `md-k-<record>-<ordinal>` from the `KillSwitchActivated` that planned it
/// and the plan's place in it, so every process and every restart derives the same id, two plans
/// of one switch never share one, and a broker that already has the order refuses the duplicate
/// instead of taking it twice (E7-2's chain, step 6).
fn kill_order_id(record: &str, ordinal: u32) -> Result<ClientOrderId, ExecutorError> {
    ClientOrderId::for_intent(&IntentId(EventId(format!("k-{record}-{ordinal}"))))
}

/// The unprotected interval a switch's cancel of protection opens, journaled when the cancel is
/// asked (§5.4, DEC-348 item 1; R-23). It names no intent and no entry: a kill switch is no exit
/// sequence, and the fold must not read one into it.
fn open_interval(
    batch: &mut Batch<'_, '_>,
    instrument: &InstrumentId,
) -> Result<(), ExecutorError> {
    if rests(&batch.view, instrument) && !interval_open(&batch.view, instrument) {
        changed(batch, instrument, "unprotected_start", Vec::new())?;
    }
    Ok(())
}

/// Whether an equity sell or close of this kind waits for the regular session where the switch is
/// made outside it (§5.5): an automated one always does, an owner's does until they confirm a
/// bid, and crypto never does.
fn session_deferred(
    batch: &Batch<'_, '_>,
    instrument: &InstrumentId,
    initiator: &Initiator,
    confirmation: &Option<OwnerConfirmation>,
) -> bool {
    if batch.ports.instruments.asset_class(instrument) == Some(AssetClass::Crypto) {
        return false;
    }
    let outside = venue(batch.ports, instrument, batch.at()) != Venue::Regular;
    match initiator {
        Initiator::Owner => outside && confirmation.is_none(),
        Initiator::RiskLimit | Initiator::PlatformOperator => outside,
    }
}

/// One instrument's planned flatten, as the command named it, before anything is journaled.
struct Plan {
    instrument: InstrumentId,
    qty: Qty,
    purpose: Purpose,
    agent: Option<AgentId>,
    deferred: bool,
    close: bool,
    wide: Option<AccountWide>,
    confirmed: Option<ConfirmedBid>,
}

/// The pricing facts of a confirmed bid, for the plan that sells on it.
fn confirmed_pricing(confirmation: &Option<OwnerConfirmation>) -> Option<ConfirmedBid> {
    confirmation.as_ref().map(|confirmed| ConfirmedBid {
        bid: confirmed.bid,
        bid_size: confirmed.bid_size,
        floor: confirmed.floor,
    })
}

/// The agents a switch reaches: its own for the agent scope; every agent the fold knows, whose
/// final mode is the switch's to apply, for the account and workspace scopes (§5.5).
fn agents_of(view: &ExecutorState, scope: &KillScope) -> Vec<AgentId> {
    match scope {
        KillScope::Agent(agent) => vec![agent.clone()],
        KillScope::Account(_) | KillScope::Workspace(_) => {
            let mut agents: BTreeMap<AgentId, ()> = BTreeMap::new();
            for agent in view.orders.values().filter_map(|order| order.agent.clone()) {
                agents.insert(agent, ());
            }
            for agent in view.modes.keys().cloned() {
                agents.insert(agent, ());
            }
            agents.into_keys().collect()
        }
    }
}

fn subject_of(scope: &KillScope) -> (&'static str, String) {
    match scope {
        KillScope::Agent(agent) => ("agent", agent.0.clone()),
        KillScope::Account(account) => ("account", account.0.clone()),
        KillScope::Workspace(workspace) => ("workspace", workspace.0.clone()),
    }
}

fn initiator_name(initiator: &Initiator) -> &'static str {
    match initiator {
        Initiator::Owner => "owner",
        Initiator::RiskLimit => "risk_limit",
        Initiator::PlatformOperator => "platform_operator",
    }
}

fn wide_name(wide: &AccountWide) -> &'static str {
    match wide {
        AccountWide::Account(_) => "account",
        AccountWide::Workspace(_) => "workspace",
    }
}

fn wide_of(scope: &KillScope) -> AccountWide {
    match scope {
        KillScope::Account(account) => AccountWide::Account(account.clone()),
        KillScope::Workspace(workspace) => AccountWide::Workspace(workspace.clone()),
        KillScope::Agent(_) => AccountWide::Account(crate::types::AccountRef(String::new())),
    }
}

/// What of an agent's sub-ledger in one instrument its own working sells and a switch's pending
/// plan already speak for: a second switch plans only what is left, so two switches never sell the
/// same lots twice (§5.5 sells exactly the sub-ledger quantity).
fn spoken_for(
    view: &ExecutorState,
    agent: &AgentId,
    instrument: &InstrumentId,
) -> Result<Qty, ExecutorError> {
    let own = view
        .orders
        .values()
        .filter(|order| {
            &order.instrument == instrument
                && order.agent.as_ref() == Some(agent)
                && order.side == Side::Sell
                && order.purpose != Purpose::Protective
                && !order.state.is_terminal()
        })
        .try_fold(Qty::ZERO, |total, order| {
            total.checked_add(order.qty.checked_sub(order.filled_qty).unwrap_or(Qty::ZERO))
        })?;
    let planned = view
        .flattens
        .get(instrument)
        .filter(|plan| !plan.close && plan.agent.as_ref() == Some(agent))
        .map(|plan| plan.qty)
        .unwrap_or(Qty::ZERO);
    own.checked_add(planned).map_err(ExecutorError::from)
}

/// The instruments an agent holds or has working in, in id order (ES-21).
fn instruments_of_agent(view: &ExecutorState, agent: &AgentId) -> Vec<InstrumentId> {
    let mut instruments: BTreeMap<InstrumentId, ()> = BTreeMap::new();
    for order in view.orders.values() {
        if order.agent.as_ref() == Some(agent) && !order.state.is_terminal() {
            instruments.insert(order.instrument.clone(), ());
        }
    }
    for (instrument, held) in view.positions.iter() {
        if !held.is_negative()
            && held.abs() > Qty::ZERO
            && held_by(view, agent, instrument).is_ok_and(|lots| lots > Qty::ZERO)
        {
            instruments.insert(instrument.clone(), ());
        }
    }
    instruments.into_keys().collect()
}

/// The agent's own live orders in one instrument, each cancelled by its own `client_order_id`
/// (§5.5): never another agent's, and never an account-wide endpoint.
fn live_orders_of_agent(
    view: &ExecutorState,
    agent: &AgentId,
    instrument: &InstrumentId,
) -> Vec<ClientOrderId> {
    view.orders
        .values()
        .filter(|order| {
            &order.instrument == instrument
                && order.agent.as_ref() == Some(agent)
                && !order.state.is_terminal()
        })
        .map(|order| order.client_order_id.clone())
        .collect()
}

/// The unattributed protective legs in one instrument, which an agent-scoped switch in an
/// instrument it closes cancels by their own ids (§5.5, DEC-160 (3)(d)).
fn unattributed_legs(view: &ExecutorState, instrument: &InstrumentId) -> Vec<ClientOrderId> {
    view.orders
        .values()
        .filter(|order| {
            &order.instrument == instrument
                && order.agent.is_none()
                && order.purpose == Purpose::Protective
                && !order.state.is_terminal()
        })
        .map(|order| order.client_order_id.clone())
        .collect()
}

/// Whether any live order in the instrument still has a cancel the broker has not confirmed,
/// which is what a planned sell waits on (§5.5's cancel → confirm → close, and the property that
/// nothing is submitted while an unconfirmed cancel is outstanding).
fn waiting_on_cancels(view: &ExecutorState, instrument: &InstrumentId) -> bool {
    view.orders.values().any(|order| {
        &order.instrument == instrument && !order.state.is_terminal() && order.cancel_unconfirmed
    })
}
