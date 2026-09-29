//! The protective sequences and the exit price ladder (E7-4, trading-domain spec §5.4 to §5.6).

use mandate_accounting::{AssetClass, InstrumentId};
use mandate_num::{Adverse, Fraction, Price, Qty, ShareIncrement, SignedQty};

use mandate_accounting::Side;
use mandate_canon::Value;

use crate::batch::Batch;
use crate::error::ExecutorError;
use crate::ids::{ClientOrderId, IntentId};
use crate::intent::{gate_and_submit, intent_of, journal_submission};
use crate::orders::transition;
use crate::payload::text;
use crate::ports::Ports;
use crate::state::{ExecutorState, ExitSequence, IntentOutcome};
use crate::types::{
    AgentId, BrokerRequest, EventId, ExitTier, IntentBody, MarketObservation, OcoLegs, OrderState,
    OrderType, ProtectionPrices, Purpose, RiskClock, SubmitOrder, TimeInForce,
};

/// Slice 2's entry: a **bracketed** add where protection rests is a new bracket (§5.4's tranche
/// model) and answers its stub. A plain add there never gets here (the gate denies it,
/// `add_blocked_by_protective_order`), and no exit is ever refused here (rule 13): each takes a
/// sequence instead (#174 ruling (b), 5862934909).
pub(crate) fn bracketed_add(
    state: &ExecutorState,
    instrument: &InstrumentId,
    purpose: Purpose,
    bracketed: bool,
) -> Result<(), ExecutorError> {
    if purpose.adds_risk() && bracketed && rests(state, instrument) {
        return Err(ExecutorError::Unimplemented { story: "E7-4" });
    }
    Ok(())
}

/// Only the triggered-stop watchdog's input (§5.4, slice 4), a sane mark at or below a resting
/// stop, answers its stub, and refuses this step alone: nothing else waits on it.
pub(crate) fn watched(
    state: &ExecutorState,
    observation: &MarketObservation,
) -> Result<(), ExecutorError> {
    let stop = state
        .protection
        .get(&observation.instrument)
        .filter(|protection| !protection.resting.is_empty())
        .and_then(|protection| protection.prices)
        .map(|prices| prices.stop);
    if let (true, Some(mark), Some(stop)) = (observation.sane, observation.mark, stop)
        && mark <= stop
    {
        return Err(ExecutorError::Unimplemented { story: "E7-4" });
    }
    Ok(())
}

/// A sequence's exit is priced by §5.6's ladder (its stub until slice 4, #174 ruling (A)) where
/// the exit tier and a quote are known; every other order keeps its intent's limit.
pub(crate) fn exit_limit(
    batch: &Batch<'_, '_>,
    instrument: &InstrumentId,
    purpose: Purpose,
    limit: Price,
) -> Result<Price, ExecutorError> {
    let tier = batch.ports.instruments.exit_tier(instrument);
    match (tier, batch.view.quotes.get(instrument)) {
        (Some(tier), Some(quote))
            if exits(purpose) && batch.view.exiting.contains_key(instrument) =>
        {
            ladder_price(tier, std::slice::from_ref(quote), 0, batch.at(), None)
                .map(|rung| rung.limit)
        }
        _ => Ok(limit),
    }
}

fn exits(purpose: Purpose) -> bool {
    purpose != Purpose::Protective && !purpose.adds_risk()
}

pub(crate) fn rests(state: &ExecutorState, instrument: &InstrumentId) -> bool {
    state
        .protection
        .get(instrument)
        .is_some_and(|protection| !protection.resting.is_empty())
}

/// An exit waits while a protective order in its sequence is not yet confirmed gone (§5.4), or
/// while the agent's own opening in the instrument is (§5.3 rule 5) — but never on an opening
/// whose cancel went overdue and whose query has been answered, and never twice on the same one:
/// that wait is bounded ([`overdue_openings`]; rules 3 and 13, #174 ruling 5863046153).
pub(crate) fn awaits_cancel(
    state: &ExecutorState,
    agent: &AgentId,
    instrument: &InstrumentId,
    purpose: Purpose,
) -> bool {
    let released = |id: &ClientOrderId| state.details.get(id).is_some_and(|detail| detail.answered);
    exits(purpose)
        && (state.exiting.contains_key(instrument) && rests(state, instrument)
            || openings(state, Some(agent), Some(instrument), false).any(|id| !released(&id)))
}

/// Rule 5's bound, on a tick: an opening whose cancel has gone unconfirmed for
/// `unknown_absent_window_s` while an exit waits on it is treated as an `Unknown` — marked
/// overdue and queried by id (§5.7) — and once the query is answered the exit goes, whatever the
/// answer: a terminal opening, or one still resting, whose self-cross is the broker's to refuse.
pub(crate) fn overdue_openings(batch: &mut Batch<'_, '_>) -> Result<(), ExecutorError> {
    let now = batch.at().secs();
    let window = batch.ports.config.unknown_absent_window_s;
    let due: Vec<ClientOrderId> = batch
        .view
        .orders
        .values()
        .filter(|order| order.purpose.adds_risk() && order.state == OrderState::PendingCancel)
        .filter(|order| !waiting(&batch.view, &order.instrument).is_empty())
        .filter(|order| {
            batch
                .view
                .details
                .get(&order.client_order_id)
                .filter(|detail| !detail.cancel_overdue)
                .and_then(|detail| detail.cancel_asked_at)
                .is_some_and(|asked| now.saturating_sub(asked.secs()) >= window)
        })
        .map(|order| order.client_order_id.clone())
        .collect();
    for id in due {
        overdue(batch, &id)?;
    }
    Ok(())
}

/// Marks a cancel overdue — unconfirmed past its bound, or refused by the broker — and queries
/// the order, keeping its state: the query's answer decides it (§5.7).
pub(crate) fn overdue(batch: &mut Batch<'_, '_>, id: &ClientOrderId) -> Result<(), ExecutorError> {
    let Some(state) = batch.view.orders.get(id).map(|order| order.state) else {
        return Ok(());
    };
    transition(
        batch,
        id,
        state,
        vec![("cancel_overdue", Value::Bool(true))],
    )?;
    batch.broker(BrokerRequest::GetOrderByClientId(id.clone()));
    Ok(())
}

/// The opening orders of `agent` (or every agent) in `instrument` (or every instrument) the broker
/// may hold: every live one, or with `resting` only those it accepted, which alone are cancelled
/// (an `Unknown` one is queried, never cancelled blind).
fn openings<'s>(
    state: &'s ExecutorState,
    agent: Option<&'s AgentId>,
    instrument: Option<&'s InstrumentId>,
    resting: bool,
) -> impl Iterator<Item = ClientOrderId> + 's {
    state
        .orders
        .values()
        .filter(move |order| {
            order.purpose.adds_risk()
                && !order.state.is_terminal()
                && order.state != OrderState::Intent
                && (!resting
                    || matches!(
                        order.state,
                        OrderState::Accepted | OrderState::PartiallyFilled
                    ))
                && agent.is_none_or(|agent| order.agent.as_ref() == Some(agent))
                && instrument.is_none_or(|instrument| &order.instrument == instrument)
        })
        .map(|order| order.client_order_id.clone())
}

/// Cancels the working openings `openings` names, each once until confirmed: §5.3 rule 5 before a
/// reducing sell, and mandate spec §5.9 on entering exits-only (interpretation 19).
pub(crate) fn cancel_openings(
    batch: &mut Batch<'_, '_>,
    agent: Option<&AgentId>,
    instrument: Option<&InstrumentId>,
) -> Result<(), ExecutorError> {
    let working: Vec<ClientOrderId> = openings(&batch.view, agent, instrument, true).collect();
    for id in working {
        if ask_cancel(batch, &id)? {
            batch.broker(BrokerRequest::Cancel {
                client_order_id: id,
            });
        }
    }
    Ok(())
}

/// §5.4's marketable exit sequence, once the gate allows the exit (a held or denied exit leaves
/// protection resting and opens no interval): every resting protective order, attributed or
/// not (DEC-160 3d), is cancelled by id (again on each ask), and the interval's start records the
/// sequence so a restart resumes it. A passive exit or an add is not this sequence; a crypto add
/// answers slice 5's stub, and an exit meeting its agent's own resting opening there answers
/// slice 3b's (§5.3 rule 5 cancels that opening first), never skipping rule 5. Both are reachable
/// only where protection rests, so not before slice 2 (the pin); nothing else is denied here
/// (rule 13).
pub(crate) fn begin_exit(
    batch: &mut Batch<'_, '_>,
    intent: &IntentId,
) -> Result<(), ExecutorError> {
    let (agent, body) = intent_of(batch, intent)?;
    let IntentBody::Order {
        instrument,
        limit,
        purpose,
        ..
    } = body
    else {
        return Ok(());
    };
    if exits(purpose) {
        cancel_openings(batch, Some(&agent), Some(&instrument))?;
    }
    let fresh = batch
        .at()
        .secs()
        .saturating_sub(batch.ports.config.exit_step_s);
    let bid = batch
        .view
        .quotes
        .get(&instrument)
        .filter(|quote| quote.sane && quote.observed_at.secs() >= fresh)
        .and_then(|quote| quote.bid);
    let resting = batch
        .view
        .protection
        .get(&instrument)
        .filter(|protection| !protection.resting.is_empty())
        .cloned();
    let Some(protection) = resting.filter(|_| exits(purpose)) else {
        return Ok(());
    };
    let passive = bid.is_some_and(|bid| limit > bid) && protection.prices.is_some();
    if !batch.view.exiting.contains_key(&instrument) {
        let entry = match protection
            .resting
            .iter()
            .find_map(ClientOrderId::protected_entry)
        {
            Some(entry) => entry,
            None => ClientOrderId::for_intent(intent)?,
        };
        let mut pairs = vec![
            (
                "orders",
                text(
                    protection
                        .resting
                        .iter()
                        .map(ClientOrderId::as_str)
                        .collect::<Vec<_>>()
                        .join(" "),
                ),
            ),
            ("intent_id", text(intent.0.0.clone())),
            ("entry", text(entry.as_str())),
            ("agent", text(agent.0.clone())),
        ];
        if let Some(prices) = protection.prices {
            pairs.push(("stop", text(prices.stop.to_string())));
            if let Some(take_profit) = prices.take_profit {
                pairs.push(("take_profit", text(take_profit.to_string())));
            }
        }
        let action = if passive {
            "passive_start"
        } else {
            "unprotected_start"
        };
        changed(batch, &instrument, action, pairs)?;
    }
    for id in protection.resting {
        ask_cancel(batch, &id)?;
        batch.broker(BrokerRequest::Cancel {
            client_order_id: id,
        });
    }
    Ok(())
}

/// Slice 5's entry: an add in a crypto instrument whose protection rests is §5.4's crypto
/// sequence (DEC-36) and answers its stub, before the gate — an add may always be refused.
pub(crate) fn crypto_add(batch: &Batch<'_, '_>, intent: &IntentId) -> Result<(), ExecutorError> {
    let (_, body) = intent_of(batch, intent)?;
    let IntentBody::Order {
        instrument,
        purpose,
        ..
    } = body
    else {
        return Ok(());
    };
    let crypto = batch.ports.instruments.asset_class(&instrument) == Some(AssetClass::Crypto);
    if purpose.adds_risk() && crypto && rests(&batch.view, &instrument) {
        return Err(ExecutorError::Unimplemented { story: "E7-4" });
    }
    Ok(())
}

/// Whether a live order with no cancel outstanding was moved to `PendingCancel`.
fn ask_cancel(batch: &mut Batch<'_, '_>, id: &ClientOrderId) -> Result<bool, ExecutorError> {
    let live = batch
        .view
        .orders
        .get(id)
        .is_some_and(|order| !order.cancel_unconfirmed && !order.state.is_terminal());
    if live {
        let requested = vec![("cancel_requested", Value::Bool(true))];
        transition(batch, id, OrderState::PendingCancel, requested)?;
    }
    Ok(live)
}

/// Journals one `ProtectionChanged` for `instrument` (journal spec §9).
fn changed(
    batch: &mut Batch<'_, '_>,
    instrument: &InstrumentId,
    action: &str,
    mut pairs: Vec<(&'static str, Value)>,
) -> Result<EventId, ExecutorError> {
    pairs.push(("instrument", text(instrument.as_str())));
    pairs.push(("action", text(action)));
    batch.journal("ProtectionChanged", None, pairs)
}

/// A protective order's confirmed cancel (§5.4), recorded. Releasing the waiting exits is
/// [`settle`]'s, after every step, so it follows a protective order gone by any path — confirmed,
/// filled, expired or rejected — rather than the confirmation alone.
pub(crate) fn protection_cancelled(
    batch: &mut Batch<'_, '_>,
    instrument: &InstrumentId,
    id: &ClientOrderId,
) -> Result<(), ExecutorError> {
    let orders = vec![("orders", text(id.as_str()))];
    changed(batch, instrument, "cancelled", orders).map(|_| ())
}

/// After every step (§5.4): in an instrument where a sequence runs and no protective order rests
/// any more, the waiting exits are re-gated and submitted; then, once the sequence's exit is
/// terminal, denied or abandoned **and** no other exit there is working or waiting, the position
/// is re-protected and the interval ends — so Σ resting sells never exceeds the position.
pub(crate) fn settle(batch: &mut Batch<'_, '_>) -> Result<(), ExecutorError> {
    let sequences: Vec<(InstrumentId, ExitSequence)> = batch
        .view
        .exiting
        .iter()
        .map(|(instrument, sequence)| (instrument.clone(), sequence.clone()))
        .collect();
    for (instrument, sequence) in sequences {
        if rests(&batch.view, &instrument) {
            continue;
        }
        release_waiting(batch, &instrument)?;
        if sequence.passive {
            continue;
        }
        let exit = ClientOrderId::for_intent(&sequence.intent)?;
        let finished = match batch.view.orders.get(&exit) {
            Some(order) => order.state.is_terminal(),
            None => batch
                .view
                .intents
                .get(&sequence.intent)
                .is_some_and(|record| {
                    matches!(
                        record.outcome,
                        IntentOutcome::Denied | IntentOutcome::Abandoned
                    )
                }),
        };
        let working = batch.view.orders.values().any(|order| {
            order.instrument == instrument && exits(order.purpose) && !order.state.is_terminal()
        });
        if finished && !working && waiting(&batch.view, &instrument).is_empty() {
            replace(batch, &instrument, &sequence)?;
        }
    }
    Ok(())
}

/// Re-gates, on fresh state, every exit in `instrument` still waiting for its cancels; each is
/// submitted only once nothing it waits on remains ([`awaits_cancel`]).
pub(crate) fn release_waiting(
    batch: &mut Batch<'_, '_>,
    instrument: &InstrumentId,
) -> Result<(), ExecutorError> {
    for intent in waiting(&batch.view, instrument) {
        gate_and_submit(batch, &intent)?;
    }
    Ok(())
}

/// The received exits in `instrument` with no order yet: those waiting on their cancels.
fn waiting(view: &ExecutorState, instrument: &InstrumentId) -> Vec<IntentId> {
    view.intents
        .values()
        .filter(|record| record.outcome == IntentOutcome::Received)
        .map(|record| record.intent_id.clone())
        .filter(|intent| {
            ClientOrderId::for_intent(intent).is_ok_and(|id| !view.orders.contains_key(&id))
                && matches!(view.bodies.get(intent),
                    Some(IntentBody::Order { instrument: named, purpose, .. })
                        if named == instrument && exits(*purpose))
        })
        .collect()
}

/// Re-places the shape the sequence cancelled for the held quantity (§5.4): an OCO at its prices,
/// or, with no take-profit, crypto's one GTC stop-limit at stop × (1 − the mandate's
/// `crypto_stop_limit_offset`) (DEC-36); a flat position places nothing. The interval ends
/// either way. An equity stop-only placement, or a crypto one with no offset, has no shape to
/// re-place and answers its stub (slices 2 and 5; unreachable before them, DEC-160 (2)). That
/// refusal is per-sequence and permanent by design: every input that would end this sequence is
/// refused, while ticks, the bound's alert and other instruments go on.
fn replace(
    batch: &mut Batch<'_, '_>,
    instrument: &InstrumentId,
    sequence: &ExitSequence,
) -> Result<(), ExecutorError> {
    let held = batch
        .view
        .positions
        .get(instrument)
        .copied()
        .unwrap_or(SignedQty::ZERO);
    let prices = sequence.prices.filter(|_| held > SignedQty::ZERO);
    let Some(prices) = prices else {
        changed(batch, instrument, "unprotected_end", Vec::new())?;
        return Ok(());
    };
    let qty = held.abs();
    let crypto = batch.ports.instruments.asset_class(instrument) == Some(AssetClass::Crypto);
    let offset = batch
        .ports
        .mandates
        .crypto_stop_limit_offset(&sequence.agent);
    let (order_type, limit, oco) = match (prices.take_profit, crypto, offset) {
        (Some(take_profit), _, _) => {
            let stop = prices.stop;
            (
                OrderType::Limit,
                None,
                Some(OcoLegs {
                    take_profit,
                    stop,
                    qty,
                }),
            )
        }
        (None, true, Some(offset)) => {
            let limit = prices.stop.collar_bound(offset, Adverse::Down)?;
            (OrderType::StopLimit, Some(limit), None)
        }
        _ => return Err(ExecutorError::Unimplemented { story: "E7-4" }),
    };
    let request = SubmitOrder {
        client_order_id: ClientOrderId::for_protection(&sequence.entry, &batch.id_after(1))?,
        instrument: instrument.clone(),
        side: Side::Sell,
        qty,
        order_type,
        tif: TimeInForce::Gtc,
        limit_price: limit,
        stop_price: oco.is_none().then_some(prices.stop),
        bracket: None,
        oco,
        extended_hours: false,
        purpose: Purpose::Protective,
    };
    place(batch, &request, None, &sequence.agent, prices)?;
    changed(batch, instrument, "unprotected_end", Vec::new())?;
    batch.broker(BrokerRequest::Submit(request));
    Ok(())
}

/// Journals one protective order — its `OrderSubmitted`, then the `placed` that makes it the
/// instrument's protection at `prices` — for the caller to send once its own records are written.
fn place(
    batch: &mut Batch<'_, '_>,
    request: &SubmitOrder,
    intent: Option<&IntentId>,
    agent: &AgentId,
    prices: ProtectionPrices,
) -> Result<(), ExecutorError> {
    journal_submission(batch, request, intent, agent, 1)?;
    let mut placed = vec![
        ("orders", text(request.client_order_id.as_str())),
        ("qty", text(request.qty.to_string())),
        ("stop", text(prices.stop.to_string())),
    ];
    if let Some(take_profit) = prices.take_profit {
        placed.push(("take_profit", text(take_profit.to_string())));
    }
    changed(batch, &request.instrument, "placed", placed).map(|_| ())
}

/// §5.4's passive exit, once its cancel is confirmed: the exit rests as the take-profit leg of a
/// new GTC OCO for its own quantity, keeping the stop; the rest of the position keeps protection at
/// the recorded prices, so nothing sells beyond the intent and the whole position keeps its stop
/// (#174 ruling (c), 5862934909; rule 3). Answers whether the intent was this passive sequence's.
pub(crate) fn passive_exit(
    batch: &mut Batch<'_, '_>,
    intent: &IntentId,
    instrument: &InstrumentId,
    qty: Qty,
    limit: Price,
) -> Result<bool, ExecutorError> {
    let Some(sequence) = batch.view.exiting.get(instrument).cloned() else {
        return Ok(false);
    };
    let Some(prices) = sequence
        .prices
        .filter(|_| sequence.passive && &sequence.intent == intent)
    else {
        return Ok(false);
    };
    let held = batch
        .view
        .positions
        .get(instrument)
        .copied()
        .unwrap_or(SignedQty::ZERO);
    let exit = OcoLegs {
        take_profit: limit,
        stop: prices.stop,
        qty,
    };
    let first = oco(ClientOrderId::for_intent(intent)?, instrument, exit);
    place(batch, &first, Some(intent), &sequence.agent, prices)?;
    let rest = held.abs().checked_sub(qty).unwrap_or(Qty::ZERO);
    let mut requests = vec![first];
    if let (true, Some(take_profit)) = (rest > Qty::ZERO, prices.take_profit) {
        let id = ClientOrderId::for_protection(&sequence.entry, &batch.id_after(1))?;
        let legs = OcoLegs {
            take_profit,
            stop: prices.stop,
            qty: rest,
        };
        let request = oco(id, instrument, legs);
        place(batch, &request, None, &sequence.agent, prices)?;
        requests.push(request);
    }
    for request in requests {
        batch.broker(BrokerRequest::Submit(request));
    }
    Ok(true)
}

/// A GTC OCO sell of `legs.qty`, a protective order (§5.2's capability matrix).
fn oco(id: ClientOrderId, instrument: &InstrumentId, legs: OcoLegs) -> SubmitOrder {
    SubmitOrder {
        client_order_id: id,
        instrument: instrument.clone(),
        side: Side::Sell,
        qty: legs.qty,
        order_type: OrderType::Limit,
        tif: TimeInForce::Gtc,
        limit_price: None,
        stop_price: None,
        bracket: None,
        oco: Some(legs),
        extended_hours: false,
        purpose: Purpose::Protective,
    }
}

/// §5.4's bound: at `max_unprotected_s` the exit is cancelled (its confirmation re-places
/// protection through [`settle`]) and the owner alerted once.
pub(crate) fn bound(batch: &mut Batch<'_, '_>) -> Result<(), ExecutorError> {
    let now = batch.at().secs();
    let limit = batch.ports.config.max_unprotected_s;
    let due: Vec<InstrumentId> = batch
        .view
        .unprotected
        .iter()
        .filter(|interval| {
            interval.ended_at.is_none()
                && !interval.alerted
                && now.saturating_sub(interval.started_at.secs()) >= limit
        })
        .map(|interval| interval.instrument.clone())
        .collect();
    for instrument in due {
        let exit = batch
            .view
            .exiting
            .get(&instrument)
            .map(|sequence| sequence.intent.clone());
        if let Some(intent) = exit {
            let id = ClientOrderId::for_intent(&intent)?;
            if ask_cancel(batch, &id)? {
                batch.broker(BrokerRequest::Cancel {
                    client_order_id: id,
                });
            }
        }
        let alerted = changed(batch, &instrument, "interval_limit", Vec::new())?;
        batch.notify(alerted, "unprotected_interval_limit");
    }
    Ok(())
}

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

/// Whether one instrument's position is fully covered by resting protective orders right now: the
/// public probe the tracer's step 17 reads (#171).
///
/// Only the whole-share part of a fractional equity position can be protected, and the fraction is
/// disclosed rather than hidden (§5.4, slice 5); a crypto stop-limit covers the whole position. A
/// flat instrument needs nothing. An instrument whose asset class or increment the snapshot does
/// not know is reported unprotected, never guessed covered (`AGENTS.md` rule 3).
pub fn is_protected(
    state: &ExecutorState,
    instrument: &InstrumentId,
    ports: &Ports<'_>,
) -> Result<bool, ExecutorError> {
    let held = state
        .positions
        .get(instrument)
        .copied()
        .unwrap_or(SignedQty::ZERO);
    if held == SignedQty::ZERO {
        return Ok(true);
    }
    if held.is_negative() {
        return Ok(false);
    }
    let whole = match (
        ports.instruments.asset_class(instrument),
        ports.instruments.increment(instrument),
    ) {
        (Some(AssetClass::Crypto), _) => held.abs(),
        (Some(_), Some(increment)) => match increment {
            ShareIncrement::Whole => held.abs(),
            ShareIncrement::Fractional => {
                held.abs().portion(Fraction::ONE, ShareIncrement::Whole)?
            }
        },
        _ => return Ok(false),
    };
    Ok(state.protective_sell_qty(instrument)? >= whole)
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

#[cfg(test)]
mod stub_tests {
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};

    use mandate_accounting::{InstrumentId, Side};
    use mandate_num::{Price, Qty};

    use super::{awaits_cancel, bracketed_add, watched};
    use crate::error::ExecutorError;
    use crate::ids::{ClientOrderId, IntentId};
    use crate::state::{ExecutorState, ExitSequence};
    use crate::types::{
        AccountRef, AccountScope, AgentId, EventId, MarketObservation, Order, OrderState,
        Protection, ProtectionPrices, Purpose, RiskClock, WorkspaceId,
    };

    /// Every production source under `dir`: Rust and Python files outside test directories.
    fn sources(dir: &Path, found: &mut Vec<PathBuf>) -> io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if path.is_dir() {
                if !matches!(name, "tests" | "target" | ".venv" | "__pycache__") {
                    sources(&path, found)?;
                }
            } else if (name.ends_with(".rs") || name.ends_with(".py")) && !name.starts_with("test_")
            {
                found.push(path);
            }
        }
        Ok(())
    }

    /// A source without its `#[cfg(test)]` items, so a pin reads production code alone and a test
    /// may name what it pins plainly (#258 round 2, minor 2). An item is a `#[cfg(test)]` line in
    /// column 0, the attributes after it, then either one line ending `;` (`mod doubles;`) or a
    /// line ending `{` through the next `}` in column 0: rustfmt's two shapes for a top-level
    /// item. Any other shape is refused, never stripped past, which would hide the production
    /// lines after it from the pin (#286 round 1, minor 3).
    fn production(source: &str) -> io::Result<String> {
        let refused = |at: usize, why: &str| {
            io::Error::other(format!(
                "line {}: a #[cfg(test)] item {why}; `production` strips only a one-line `;` \
                 item or a `{{` block closed by a `}}` in column 0",
                at.saturating_add(1)
            ))
        };
        let mut kept = String::new();
        let mut lines = source.lines().enumerate();
        while let Some((at, line)) = lines.next() {
            if !line.starts_with("#[cfg(test)]") {
                kept.push_str(line);
                kept.push('\n');
                continue;
            }
            if line != "#[cfg(test)]" {
                return Err(refused(at, "shares its attribute's line"));
            }
            let mut item = lines.next();
            while let Some((_, attribute)) = item.filter(|(_, next)| next.starts_with("#[")) {
                if !attribute.ends_with(']')
                    && !lines.any(|(_, next)| next.starts_with(')') || next.starts_with(']'))
                {
                    return Err(refused(at, "has an attribute that never closes"));
                }
                item = lines.next();
            }
            match item {
                Some((_, head)) if head.ends_with(';') => {}
                Some((_, head)) if head.ends_with('{') => {
                    if !lines.any(|(_, next)| next == "}") {
                        return Err(refused(at, "never closes"));
                    }
                }
                _ => return Err(refused(at, "is neither a one-line item nor a block")),
            }
        }
        Ok(kept)
    }

    /// One file's production code, or the refusal naming the file.
    fn read_production(path: &Path) -> io::Result<String> {
        production(&fs::read_to_string(path)?)
            .map_err(|refused| io::Error::other(format!("{}: {refused}", path.display())))
    }

    /// #286 round 1, minor 3: a one-line item mid-file (the `mod x;` shape `stages.rs`,
    /// `policy.rs` and `validate.rs` use), one under several attributes, and a block are each
    /// stripped exactly, keeping every production line after them; an item `production` cannot
    /// strip exactly is refused, loudly, rather than stripped past.
    #[test]
    fn production_strips_test_items_exactly_or_refuses() -> io::Result<()> {
        let source = "fn a() {}\n#[cfg(test)]\nmod doubles;\nfn b() {}\n#[cfg(test)]\n\
                      #[path = \"common.rs\"]\n#[allow(\n    dead_code,\n    reason = \"r\"\n)]\n\
                      mod common;\nfn c() {}\n#[cfg(test)]\nmod tests {\n    fn t() {}\n}\n\
                      fn d() {}\n";
        assert_eq!(
            production(source)?,
            "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\n"
        );
        for unstrippable in [
            "#[cfg(test)]\nfn f<T>()\nwhere\n    T: Sized,\n{\n}\nfn b() {}\n",
            "#[cfg(test)]\nmod tests {\n    fn t() {}\n",
            "#[cfg(test)] mod doubles;\nfn b() {}\n",
            "fn b() {}\n#[cfg(test)]\n",
            "#[cfg(test)]\n#[allow(\n    dead_code,\n",
        ] {
            assert!(
                production(unstrippable).is_err(),
                "{unstrippable:?} is refused"
            );
        }
        Ok(())
    }

    /// What a source may hold of each construction, per file; any file not named holds none.
    /// `None` is any count: the fold, which journals nothing and only reads an event back, and
    /// this module's own writer of the protection event.
    const ALLOWED: [(&str, &str, Option<usize>); 8] = [
        (
            "crates/mandate-executor/src/types.rs",
            "BracketLegs {",
            Some(1),
        ),
        ("crates/mandate-executor/src/types.rs", "OcoLegs {", Some(1)),
        ("crates/mandate-executor/src/fold.rs", "OcoLegs {", Some(1)),
        (
            "crates/mandate-executor/src/protection.rs",
            "OcoLegs {",
            Some(3),
        ),
        (
            "crates/mandate-executor/src/protection.rs",
            "\"placed\"",
            Some(1),
        ),
        ("crates/mandate-executor/src/fold.rs", "\"placed\"", None),
        (
            "crates/mandate-executor/src/fold.rs",
            "\"ProtectionChanged\"",
            None,
        ),
        (
            "crates/mandate-executor/src/protection.rs",
            "\"ProtectionChanged\"",
            None,
        ),
    ];

    /// The registries that name the protection event once each without writing it.
    const REGISTRIES: [&str; 2] = [
        "crates/mandate-journal/src/catalogue.rs",
        "crates/mandate-runtime/src/state.rs",
    ];

    /// #174 ruling 5862180909, `AGENTS.md` rule 13: the stub [`sequenced`] still answers is
    /// reached only in an instrument whose protection rests, and nothing creates protection on an
    /// unprotected position before slice 2. No source anywhere in the workspace builds a
    /// bracketed `SubmitOrder` (its only `BracketLegs` is the type's own definition), nothing
    /// builds the OCO a partial fill takes (the one OCO built is the re-placement a running exit
    /// sequence makes, which needs protection already resting, and the fold reads one back), and
    /// only this module writes the protection event, one `"placed"` among it. Each file is read
    /// without its `#[cfg(test)]` items. Slice 2 deletes this pin, landing no earlier than 3a, 3b
    /// and 4 (DEC-160).
    #[test]
    fn no_protection_is_created_on_an_unprotected_position() -> io::Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut found = Vec::new();
        sources(&root.join("crates"), &mut found)?;
        sources(&root.join("python"), &mut found)?;
        assert!(
            found.len() > 100,
            "the scan reached the workspace: {} files",
            found.len()
        );
        let needles = [
            "BracketLegs {",
            "OcoLegs {",
            "\"placed\"",
            "\"ProtectionChanged\"",
        ];
        for path in found {
            let production = read_production(&path)?;
            let relative = path
                .strip_prefix(&root)
                .map_or(path.clone(), Path::to_path_buf)
                .to_string_lossy()
                .replace('\\', "/");
            for needle in needles {
                let count = if relative.ends_with(".py") {
                    let event = needle == "\"ProtectionChanged\"";
                    usize::from(event) * production.matches(needle.trim_matches('"')).count()
                } else {
                    production.matches(needle).count()
                };
                let allowed = ALLOWED
                    .iter()
                    .find(|(file, named, _)| *file == relative && *named == needle)
                    .map_or_else(
                        || {
                            Some(usize::from(
                                needle == "\"ProtectionChanged\""
                                    && REGISTRIES.contains(&relative.as_str()),
                            ))
                        },
                        |(_, _, allowed)| *allowed,
                    );
                assert!(
                    allowed.is_none_or(|allowed| count == allowed),
                    "{relative} holds `{needle}` {count} time(s), {allowed:?} allowed: protection \
                     created on an unprotected position makes the stubs reachable, a denied \
                     exit (AGENTS.md rule 13). Slice 2 creates it no earlier \
                     than 3a, 3b and 4 (DEC-160)"
                );
            }
        }
        Ok(())
    }

    /// #174 ruling (b), 5861764910: slice 1 reads no leg id from a `BrokerOrder`. The broker's
    /// legs carry only their broker ids until the slice that reconciles legs keeps each leg's
    /// `client_order_id` (with its own `ready()` tests correction), so until then a broker-reported
    /// leg falls to the single holder or fails closed (DEC-160 3a). Every production source of
    /// this crate is scanned for a read of the field.
    #[test]
    fn no_leg_id_is_read_from_a_broker_order() -> io::Result<()> {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        for entry in fs::read_dir(&src)? {
            let path = entry?.path();
            let source = read_production(&path)?;
            assert!(
                !source.contains(".legs"),
                "{} reads a broker order's legs; leg ids from the broker wait for the leg \
                 reconciliation slice (#174 ruling (b))",
                path.display()
            );
        }
        Ok(())
    }

    fn price(raw: &str) -> Result<Price, ExecutorError> {
        Ok(Price::parse(raw)?)
    }

    /// `AAPL` with an OCO resting at a stop of 140, `MSFT` with a protection record whose last
    /// order has gone, and nothing else.
    fn protected() -> Result<(ExecutorState, InstrumentId), ExecutorError> {
        let mut state = ExecutorState::new(AccountScope {
            account: AccountRef("acct-1".to_owned()),
            workspace: WorkspaceId("ws1".to_owned()),
        });
        let aapl = InstrumentId::new("AAPL")?;
        let msft = InstrumentId::new("MSFT")?;
        for (instrument, resting) in [
            (aapl.clone(), vec![ClientOrderId::parse("md-oco-1")?]),
            (msft.clone(), Vec::new()),
        ] {
            state.protection.insert(
                instrument.clone(),
                Protection {
                    instrument,
                    resting,
                    covered_qty: Qty::parse("10")?,
                    prices: Some(ProtectionPrices {
                        stop: price("140")?,
                        take_profit: Some(price("170")?),
                    }),
                },
            );
        }
        Ok((state, aapl))
    }

    const ALL: [Purpose; 7] = [
        Purpose::Open,
        Purpose::Increase,
        Purpose::RiskExit,
        Purpose::OwnerExit,
        Purpose::DiscretionaryExit,
        Purpose::Protective,
        Purpose::Flatten,
    ];

    /// Only a bracketed add where protection rests answers slice 2's stub: never an exit or a
    /// protective order (rule 13), never a plain add, never an instrument where nothing rests.
    #[test]
    fn only_a_bracketed_add_where_protection_rests_answers_slice_2s_stub()
    -> Result<(), ExecutorError> {
        let (state, aapl) = protected()?;
        let stub = Err(ExecutorError::Unimplemented { story: "E7-4" });
        for purpose in ALL {
            for bracketed in [true, false] {
                let expected = if purpose.adds_risk() && bracketed {
                    stub.clone()
                } else {
                    Ok(())
                };
                let case = format!("{purpose:?} bracketed {bracketed}");
                assert_eq!(
                    bracketed_add(&state, &aapl, purpose, bracketed),
                    expected,
                    "{case}"
                );
                for elsewhere in ["MSFT", "TSLA"] {
                    let there = InstrumentId::new(elsewhere)?;
                    assert_eq!(
                        bracketed_add(&state, &there, purpose, bracketed),
                        Ok(()),
                        "{case}"
                    );
                }
            }
        }
        Ok(())
    }

    /// An exit waits for its sequence's cancels only while a sequence runs in its instrument and a
    /// protective order still rests there; an add and a protective order never wait on it.
    #[test]
    fn only_an_exit_in_a_running_sequence_awaits_the_cancel() -> Result<(), ExecutorError> {
        let (mut state, aapl) = protected()?;
        let msft = InstrumentId::new("MSFT")?;
        let agent = AgentId("agent-a".to_owned());
        for purpose in ALL {
            assert!(
                !awaits_cancel(&state, &agent, &aapl, purpose),
                "{purpose:?}: no sequence"
            );
        }
        for instrument in [&aapl, &msft] {
            state.exiting.insert(
                instrument.clone(),
                ExitSequence {
                    intent: IntentId(EventId("01JABCDEFGHJKMNPQRSTVWXYZ0".to_owned())),
                    entry: ClientOrderId::parse("md-held-1")?,
                    agent: AgentId("agent-a".to_owned()),
                    prices: None,
                    passive: false,
                },
            );
        }
        for purpose in ALL {
            let exit = !purpose.adds_risk() && purpose != Purpose::Protective;
            assert_eq!(
                awaits_cancel(&state, &agent, &aapl, purpose),
                exit,
                "{purpose:?}"
            );
            assert!(
                !awaits_cancel(&state, &agent, &msft, purpose),
                "{purpose:?}: nothing rests in MSFT any more"
            );
        }
        Ok(())
    }

    /// §5.3 rule 5: an exit also waits while its own agent's opening in the instrument is live,
    /// and only its own agent's, only there, and only until the opening is done.
    #[test]
    fn an_exit_awaits_its_own_agents_live_opening_in_the_instrument() -> Result<(), ExecutorError> {
        let mut state = protected()?.0;
        state.protection.clear();
        let aapl = InstrumentId::new("AAPL")?;
        let msft = InstrumentId::new("MSFT")?;
        let agent = AgentId("agent-a".to_owned());
        let other = AgentId("agent-b".to_owned());
        for (owner, at, purpose, waits) in [
            (&agent, OrderState::Accepted, Purpose::Open, true),
            (&agent, OrderState::PendingCancel, Purpose::Increase, true),
            (&agent, OrderState::Canceled, Purpose::Open, false),
            (
                &agent,
                OrderState::Accepted,
                Purpose::DiscretionaryExit,
                false,
            ),
            (&other, OrderState::Accepted, Purpose::Open, false),
        ] {
            let id = ClientOrderId::parse("md-buy-1")?;
            state.orders.insert(
                id.clone(),
                Order {
                    client_order_id: id,
                    intent_id: None,
                    agent: Some(owner.clone()),
                    instrument: aapl.clone(),
                    side: Side::Buy,
                    qty: Qty::parse("10")?,
                    filled_qty: Qty::ZERO,
                    state: at,
                    attempt: 1,
                    purpose,
                    absent_lookups: 0,
                    first_absence_at: None,
                    cancel_unconfirmed: false,
                    replaced_by: None,
                    created_on: None,
                },
            );
            let case = format!("{owner:?} {at:?} {purpose:?}");
            assert_eq!(
                awaits_cancel(&state, &agent, &aapl, Purpose::RiskExit),
                waits,
                "{case}"
            );
            assert!(
                !awaits_cancel(&state, &agent, &msft, Purpose::RiskExit),
                "{case}"
            );
            assert!(
                !awaits_cancel(&state, &agent, &aapl, Purpose::Increase),
                "{case}"
            );
        }
        Ok(())
    }

    /// The watchdog's input (slice 4) is a sane mark at or below the stop of protection that
    /// rests; every other quote passes and is kept only as the latest observation.
    #[test]
    fn only_a_sane_mark_at_or_below_a_resting_stop_answers_the_watchdog_stub()
    -> Result<(), ExecutorError> {
        let (mut state, aapl) = protected()?;
        let quote = |instrument: &InstrumentId, mark: Option<&str>, sane: bool| {
            Ok::<_, ExecutorError>(MarketObservation {
                instrument: instrument.clone(),
                bid: None,
                bid_size: None,
                ask: None,
                last_trade: None,
                mark: mark.map(price).transpose()?,
                sane,
                observed_at: RiskClock::from_secs(10),
            })
        };
        let stub = Err(ExecutorError::Unimplemented { story: "E7-4" });
        let msft = InstrumentId::new("MSFT")?;
        for (instrument, mark, sane, expected) in [
            (&aapl, Some("139.99"), true, stub.clone()),
            (&aapl, Some("140"), true, stub.clone()),
            (&aapl, Some("140.01"), true, Ok(())),
            (&aapl, Some("120"), false, Ok(())),
            (&aapl, None, true, Ok(())),
            (&msft, Some("120"), true, Ok(())),
        ] {
            assert_eq!(
                watched(&state, &quote(instrument, mark, sane)?),
                expected,
                "{instrument:?} at {mark:?}, sane {sane}"
            );
        }
        if let Some(protection) = state.protection.get_mut(&aapl) {
            protection.prices = None;
        }
        assert_eq!(
            watched(&state, &quote(&aapl, Some("120"), true)?),
            Ok(()),
            "no stop price, nothing for the watchdog to compare with"
        );
        Ok(())
    }
}

#[cfg(test)]
mod probe_tests {
    use mandate_accounting::{
        AssetClass, Config, CryptoFees, EquityFees, InstrumentId, TafCapBasis,
    };
    use mandate_num::{
        Bps, FeeCap, FeePerShare, FeeRate, Fraction, Qty, ShareIncrement, SignedQty,
    };
    use mandate_time::{Date, TradingCalendar};

    use super::is_protected;
    use crate::error::ExecutorError;
    use crate::ports::{IdGen, InstrumentSnapshot, MandateView, Ports};
    use crate::state::ExecutorState;
    use crate::types::{
        AccountRef, AccountScope, AgentId, EventId, ExecutorConfig, ExitTier, MandateVersion,
        Protection, Seq, WorkspaceId, WriterEpoch,
    };

    struct Nothing;

    impl IdGen for Nothing {
        fn event_id(&self, _epoch: WriterEpoch, _head: Seq, _ordinal: u32) -> EventId {
            EventId("e".to_owned())
        }
    }

    impl MandateView for Nothing {
        fn version(&self, _agent: &AgentId) -> Option<MandateVersion> {
            None
        }

        fn crypto_stop_limit_offset(&self, _agent: &AgentId) -> Option<Fraction> {
            None
        }

        fn covers(&self, _agent: &AgentId, _instrument: &InstrumentId) -> bool {
            false
        }
    }

    /// `AAPL` whole shares, `FRAC` fractional, `BTC/USD` crypto; anything else unknown.
    struct Snapshot;

    impl InstrumentSnapshot for Snapshot {
        fn asset_class(&self, instrument: &InstrumentId) -> Option<AssetClass> {
            match instrument.as_str() {
                "AAPL" | "FRAC" | "NOINC" => Some(AssetClass::UsEquity),
                "BTC/USD" => Some(AssetClass::Crypto),
                _ => None,
            }
        }

        fn increment(&self, instrument: &InstrumentId) -> Option<ShareIncrement> {
            match instrument.as_str() {
                "AAPL" => Some(ShareIncrement::Whole),
                "FRAC" | "BTC/USD" => Some(ShareIncrement::Fractional),
                _ => None,
            }
        }

        fn exit_tier(&self, _instrument: &InstrumentId) -> Option<ExitTier> {
            None
        }
    }

    fn fees() -> Result<Config, ExecutorError> {
        let date = |raw: &str| Date::parse(raw).map_err(ExecutorError::from);
        Ok(Config {
            equities: EquityFees {
                sec_rate: FeeRate::parse("0")?,
                taf_per_share: FeePerShare::parse("0")?,
                taf_cap: FeeCap::parse("0")?,
                taf_cap_basis: TafCapBasis::PerExecution,
                cat_per_share: FeePerShare::parse("0")?,
            },
            crypto: CryptoFees {
                maker: Bps::parse("0")?,
                taker: Bps::parse("0")?,
            },
            calendar: TradingCalendar::new(date("2026-09-01")?, date("2026-12-31")?, [], [])?,
        })
    }

    /// `held` of `name`, covered by `covered` of resting protection (none when `None`).
    fn probe(name: &str, held: &str, covered: Option<&str>) -> Result<bool, ExecutorError> {
        let fees = fees()?;
        let config = ExecutorConfig::PROPOSED;
        let ports = Ports {
            ids: &Nothing,
            mandates: &Nothing,
            instruments: &Snapshot,
            config: &config,
            fees: &fees,
        };
        let mut state = ExecutorState::new(AccountScope {
            account: AccountRef("acct-1".to_owned()),
            workspace: WorkspaceId("ws1".to_owned()),
        });
        let instrument = InstrumentId::new(name)?;
        state
            .positions
            .insert(instrument.clone(), SignedQty::parse(held)?);
        if let Some(covered) = covered {
            state.protection.insert(
                instrument.clone(),
                Protection {
                    instrument: instrument.clone(),
                    resting: Vec::new(),
                    covered_qty: Qty::parse(covered)?,
                    prices: None,
                },
            );
        }
        is_protected(&state, &instrument, &ports)
    }

    #[test]
    fn a_flat_instrument_needs_nothing_and_a_short_is_never_protected() -> Result<(), ExecutorError>
    {
        assert!(probe("AAPL", "0", None)?);
        assert!(!probe("AAPL", "-1", Some("1"))?);
        Ok(())
    }

    #[test]
    fn whole_shares_are_protected_only_when_all_of_them_are_covered() -> Result<(), ExecutorError> {
        assert!(probe("AAPL", "10", Some("10"))?);
        assert!(!probe("AAPL", "10", Some("9"))?);
        assert!(!probe("AAPL", "10", None)?);
        Ok(())
    }

    #[test]
    fn only_the_whole_share_part_of_a_fractional_position_needs_cover() -> Result<(), ExecutorError>
    {
        assert!(
            probe("FRAC", "10.5", Some("10"))?,
            "the half share is disclosed, not protected"
        );
        assert!(!probe("FRAC", "10.5", Some("9"))?);
        Ok(())
    }

    #[test]
    fn a_crypto_position_is_covered_whole() -> Result<(), ExecutorError> {
        assert!(probe("BTC/USD", "0.5", Some("0.5"))?);
        assert!(
            !probe("BTC/USD", "0.5", Some("0.4"))?,
            "no whole-share floor for crypto"
        );
        Ok(())
    }

    #[test]
    fn an_instrument_the_snapshot_does_not_know_is_reported_unprotected()
    -> Result<(), ExecutorError> {
        assert!(!probe("ZZZZ", "1", Some("1"))?, "unknown asset class");
        assert!(
            !probe("NOINC", "1", Some("1"))?,
            "an equity with no known increment"
        );
        Ok(())
    }
}

#[cfg(test)]
mod sequence_tests {
    use mandate_accounting::{AssetClass, InstrumentId, Side};
    use mandate_canon::Value;
    use mandate_num::{Fraction, Price, Qty, ShareIncrement, SignedQty, Usd};
    use mandate_time::Date;

    use super::rests;
    use crate::error::ExecutorError;
    use crate::ids::{ClientOrderId, IntentId};
    use crate::payload::{clock, object, text};
    use crate::ports::{InstrumentSnapshot, MandateView, Ports};
    use crate::reconcile::tests::{
        Everything, Executor, Ids, aapl, account, drafted, executor_config, fees, submitted,
    };
    use crate::types::{
        AgentId, BrokerFill, BrokerOrder, BrokerOutcome, BrokerReject, BrokerRequest, BrokerUpdate,
        Effect, EventDraft, EventId, ExitTier, FillId, Input, IntentBody, IntentHandoff,
        MandateVersion, MarketObservation, OcoLegs, OrderState, OrderType, Purpose,
        ReconcileReason, RiskClock, SubmitOrder, TimeInForce,
    };

    const OCO: &str = "md-held-1-p1";
    const EXIT: &str = "01JABCDEFGHJKMNPQRSTVWXYZ9";
    const SECOND: &str = "01JABCDEFGHJKMNPQRSTVWXYZ8";

    fn committed(
        executor: &mut Executor,
        event_type: &str,
        pairs: Vec<(&str, Value)>,
    ) -> Result<(), ExecutorError> {
        let mut pairs = pairs;
        pairs.push(("risk_clock", clock(RiskClock::from_secs(0))?));
        executor.commit_one(event_type, object(pairs)?)
    }

    /// Ten AAPL that `agent-a` bought, protected by one GTC OCO at 170 over 140 named for the
    /// buy (§2.3).
    fn protected(ports: &Ports<'_>) -> Result<Executor, ExecutorError> {
        let mut executor = held(ports)?;
        committed(
            &mut executor,
            "OrderSubmitted",
            vec![
                ("client_order_id", text(OCO)),
                ("agent", text("agent-a")),
                ("instrument", text("AAPL")),
                ("side", text("sell")),
                ("qty", text("10")),
                ("tif", text("gtc")),
                ("purpose", text("protective")),
                ("order_class", text("oco")),
                ("take_profit", text("170")),
                ("stop", text("140")),
            ],
        )?;
        committed(
            &mut executor,
            "OrderStateChanged",
            vec![("client_order_id", text(OCO)), ("state", text("accepted"))],
        )?;
        committed(
            &mut executor,
            "ProtectionChanged",
            vec![
                ("instrument", text("AAPL")),
                ("action", text("placed")),
                ("orders", text(OCO)),
                ("qty", text("10")),
                ("take_profit", text("170")),
                ("stop", text("140")),
            ],
        )?;
        Ok(executor)
    }

    /// The same ten AAPL with nothing protecting them.
    fn held(ports: &Ports<'_>) -> Result<Executor, ExecutorError> {
        let mut executor = Executor::opened(ports)?;
        committed(
            &mut executor,
            "OrderSubmitted",
            vec![
                ("client_order_id", text("md-held-1")),
                ("agent", text("agent-a")),
                ("instrument", text("AAPL")),
                ("side", text("buy")),
                ("qty", text("10")),
                ("limit", text("150")),
                ("purpose", text("open")),
            ],
        )?;
        committed(
            &mut executor,
            "FillApplied",
            vec![
                ("fill_id", text("f-0")),
                ("client_order_id", text("md-held-1")),
                ("instrument", text("AAPL")),
                ("side", text("buy")),
                ("qty_gross", text("10")),
                ("price", text("150")),
            ],
        )?;
        committed(
            &mut executor,
            "OrderStateChanged",
            vec![
                ("client_order_id", text("md-held-1")),
                ("state", text("filled")),
            ],
        )?;
        Ok(executor)
    }

    fn sell(
        intent: &str,
        qty: &str,
        limit: &str,
        purpose: Purpose,
    ) -> Result<Input, ExecutorError> {
        Ok(Input::Intent(IntentHandoff {
            intent_id: IntentId(EventId(intent.to_owned())),
            agent: AgentId("agent-a".to_owned()),
            body: IntentBody::Order {
                instrument: aapl()?,
                side: Side::Sell,
                qty: Qty::parse(qty)?,
                limit: Price::parse(limit)?,
                purpose,
                protection: None,
            },
        }))
    }

    /// A sane quote whose bid and mark are both `at`.
    fn quote(at: &str) -> Result<Input, ExecutorError> {
        let at = Price::parse(at)?;
        Ok(Input::Market(MarketObservation {
            instrument: aapl()?,
            bid: Some(at),
            bid_size: Some(Qty::parse("100")?),
            ask: Some(at),
            last_trade: Some(at),
            mark: Some(at),
            sane: true,
            observed_at: RiskClock::from_secs(0),
        }))
    }

    fn cancel_accepted(id: &str) -> Input {
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: id.to_owned(),
        }))
    }

    fn filled(intent: &str, qty: &str) -> Result<Input, ExecutorError> {
        Ok(Input::BrokerUpdate(BrokerUpdate::Fill(BrokerFill {
            fill_id: FillId(format!("f-{intent}")),
            client_order_id: Some(format!("md-{intent}")),
            instrument: aapl()?,
            side: Side::Sell,
            qty: Qty::parse(qty)?,
            price: Price::parse("139")?,
            fees: Usd::ZERO,
            trade_date: Date::parse("2026-09-22")?,
        })))
    }

    fn cancels(effects: &[Effect]) -> Vec<&str> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Broker(BrokerRequest::Cancel { client_order_id }) => {
                    Some(client_order_id.as_str())
                }
                _ => None,
            })
            .collect()
    }

    fn submissions(effects: &[Effect]) -> Vec<&SubmitOrder> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Broker(BrokerRequest::Submit(order)) => Some(order),
                _ => None,
            })
            .collect()
    }

    fn protection_drafts(effects: &[Effect]) -> Vec<&EventDraft> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Journal(draft) if draft.event_type == "ProtectionChanged" => Some(draft),
                _ => None,
            })
            .collect()
    }

    fn actions(effects: &[Effect]) -> Vec<&str> {
        protection_drafts(effects)
            .into_iter()
            .filter_map(|draft| draft.payload.get("action").and_then(Value::as_str))
            .collect()
    }

    fn stub<T>(result: &Result<T, ExecutorError>) -> bool {
        matches!(result, Err(ExecutorError::Unimplemented { story: "E7-4" }))
    }

    macro_rules! with_ports {
        ($ports:ident) => {
            let (config, fees) = (executor_config(), fees()?);
            let $ports = Ports {
                ids: &Ids,
                mandates: &Everything,
                instruments: &Everything,
                config: &config,
                fees: &fees,
            };
        };
    }

    /// #174 ruling 5862180909: a quote the watchdog stub refuses (a sane mark at or below the
    /// resting stop) is refused alone. It leaves the state as it found it but for the latest
    /// quote, and the exit, the cancel's confirmation and the reconciliation after it each run in
    /// full, with another such quote arriving between them.
    #[test]
    fn a_watchdog_stub_never_blocks_an_exit_a_cancel_or_a_reconciliation()
    -> Result<(), ExecutorError> {
        with_ports!(ports);
        let mut executor = protected(&ports)?;
        let before = executor.state.clone();
        assert!(stub(&executor.run(quote("139")?, &ports)));
        let mut kept = executor.state.clone();
        assert_eq!(
            kept.quotes.get(&aapl()?).and_then(|quote| quote.bid),
            Some(Price::parse("139")?),
            "the refused quote is still the latest observation"
        );
        kept.quotes = before.quotes.clone();
        assert_eq!(kept, before, "and nothing else changed");

        let started = executor.run(sell(EXIT, "5", "139", Purpose::RiskExit)?, &ports)?;
        assert_eq!(
            cancels(&started),
            vec![OCO],
            "the exit cancels the protection"
        );
        assert_eq!(actions(&started), vec!["unprotected_start"]);
        assert_eq!(submitted(&started), 0, "and waits for the confirmation");

        assert!(stub(&executor.run(quote("138")?, &ports)));
        let released = executor.run(cancel_accepted(OCO), &ports)?;
        assert_eq!(actions(&released), vec!["cancelled"]);
        assert_eq!(
            submitted(&released),
            1,
            "the confirmation releases the exit"
        );
        assert_eq!(
            executor.run(quote("137")?, &ports)?,
            Vec::new(),
            "with nothing resting there is no stop to watch"
        );

        let taken = executor.snapshot(ReconcileReason::Scheduled)?;
        let run = executor.run(Input::BrokerSnapshot(taken), &ports)?;
        assert!(
            drafted(&run).contains(&"ReconciliationRun"),
            "the reconciliation runs: {:?}",
            drafted(&run)
        );
        Ok(())
    }

    /// §5.4: an exit whose limit is at or below the latest bid, or any exit when no quote has
    /// been seen, runs the marketable sequence (the exit is never the one held back, rule 13); one
    /// above the bid runs the passive one, which keeps the stop.
    #[test]
    fn each_exit_starts_its_own_sequence() -> Result<(), ExecutorError> {
        with_ports!(ports);
        for (seen, limit, purpose, action) in [
            (None, "999", Purpose::DiscretionaryExit, "unprotected_start"),
            (
                Some("150"),
                "150",
                Purpose::DiscretionaryExit,
                "unprotected_start",
            ),
            (Some("150"), "150", Purpose::OwnerExit, "unprotected_start"),
            (Some("150"), "150", Purpose::Flatten, "unprotected_start"),
            (
                Some("150"),
                "150.01",
                Purpose::DiscretionaryExit,
                "passive_start",
            ),
        ] {
            let mut executor = protected(&ports)?;
            if let Some(bid) = seen {
                executor.run(quote(bid)?, &ports)?;
            }
            let effects = executor.run(sell(EXIT, "5", limit, purpose)?, &ports)?;
            let case = format!("{purpose:?} at {limit} against {seen:?}");
            assert!(executor.state.exiting.contains_key(&aapl()?), "{case}");
            assert_eq!(actions(&effects), vec![action], "{case}");
            assert_eq!(cancels(&effects), vec![OCO], "{case}");
        }
        Ok(())
    }

    /// A second exit joining a running sequence journals no second start and asks for no second
    /// transition, but sends the cancel again; both wait, and the one confirmation releases both.
    #[test]
    fn a_second_exit_joins_the_running_sequence() -> Result<(), ExecutorError> {
        with_ports!(ports);
        let mut executor = protected(&ports)?;
        let first = executor.run(sell(EXIT, "5", "139", Purpose::RiskExit)?, &ports)?;
        assert_eq!(
            executor
                .state
                .orders
                .get(&ClientOrderId::parse(OCO)?)
                .map(|order| (order.state, order.cancel_unconfirmed)),
            Some((OrderState::PendingCancel, true)),
            "{:?}",
            drafted(&first)
        );
        let second = executor.run(sell(SECOND, "5", "139", Purpose::RiskExit)?, &ports)?;
        assert_eq!(actions(&second), Vec::<&str>::new());
        assert!(!drafted(&second).contains(&"OrderStateChanged"));
        assert_eq!(cancels(&second), vec![OCO]);
        assert_eq!(submitted(&second), 0);
        let released = executor.run(cancel_accepted(OCO), &ports)?;
        assert_eq!(submitted(&released), 2);
        Ok(())
    }

    /// The sequence's end (§5.4): once the exit fills, an OCO at the recorded prices is placed for
    /// what the position still holds, named for the entry and for the `placed` record that
    /// follows its submission, and the interval ends; a flat position places nothing.
    #[test]
    fn a_finished_exit_re_places_protection_for_what_remains() -> Result<(), ExecutorError> {
        with_ports!(ports);
        for (qty, remains) in [("5", Some("5")), ("10", None)] {
            let mut executor = protected(&ports)?;
            executor.run(sell(EXIT, qty, "139", Purpose::RiskExit)?, &ports)?;
            executor.run(cancel_accepted(OCO), &ports)?;
            let settled = executor.run(filled(EXIT, qty)?, &ports)?;
            let placed = submissions(&settled);
            assert!(executor.state.exiting.is_empty(), "{qty}");
            assert_eq!(
                executor
                    .state
                    .unprotected
                    .last()
                    .map(|interval| interval.ended_at.is_some()),
                Some(true),
                "{qty}: the interval ends"
            );
            let Some(remains) = remains else {
                assert_eq!(actions(&settled), vec!["unprotected_end"]);
                assert!(placed.is_empty());
                continue;
            };
            assert_eq!(actions(&settled), vec!["placed", "unprotected_end"]);
            let record = protection_drafts(&settled)
                .first()
                .map(|draft| draft.event_id.0.clone())
                .unwrap_or_default();
            let expected_id = format!("md-held-1-p{record}");
            let remains = Qty::parse(remains)?;
            assert_eq!(
                placed
                    .iter()
                    .map(|order| (
                        order.client_order_id.as_str(),
                        order.side,
                        order.qty,
                        order.tif,
                        order.limit_price,
                        order.stop_price,
                        order.purpose,
                        order.oco.clone(),
                    ))
                    .collect::<Vec<_>>(),
                vec![(
                    expected_id.as_str(),
                    Side::Sell,
                    remains,
                    TimeInForce::Gtc,
                    None,
                    None,
                    Purpose::Protective,
                    Some(OcoLegs {
                        take_profit: Price::parse("170")?,
                        stop: Price::parse("140")?,
                        qty: remains,
                    }),
                )]
            );
            let protection = executor.state.protection.get(&aapl()?).cloned();
            assert_eq!(
                protection.map(|protection| (protection.resting, protection.covered_qty)),
                Some((vec![ClientOrderId::parse(&expected_id)?], remains)),
            );
        }
        Ok(())
    }

    /// An exit that grew too old while its cancel was confirmed is abandoned, and the position it
    /// never reduced is protected again in full.
    #[test]
    fn an_abandoned_exit_re_places_protection_for_the_whole_position() -> Result<(), ExecutorError>
    {
        with_ports!(ports);
        let mut executor = protected(&ports)?;
        executor.run(sell(EXIT, "5", "139", Purpose::RiskExit)?, &ports)?;
        executor.run(Input::Tick(RiskClock::from_secs(500)), &ports)?;
        let settled = executor.run(cancel_accepted(OCO), &ports)?;
        assert!(drafted(&settled).contains(&"OrderAbandoned"));
        assert_eq!(
            submissions(&settled)
                .iter()
                .map(|order| order.qty)
                .collect::<Vec<_>>(),
            vec![Qty::parse("10")?]
        );
        Ok(())
    }

    /// §5.4's bound: at `max_unprotected_s` (30 s here) the working exit is cancelled and the
    /// owner alerted, once; the cancel's confirmation then re-places protection for the position.
    #[test]
    fn an_interval_at_its_bound_cancels_the_exit_and_alerts_once() -> Result<(), ExecutorError> {
        with_ports!(ports);
        let mut executor = protected(&ports)?;
        executor.run(sell(EXIT, "5", "139", Purpose::RiskExit)?, &ports)?;
        executor.run(cancel_accepted(OCO), &ports)?;
        let exit = format!("md-{EXIT}");
        let early = executor.run(Input::Tick(RiskClock::from_secs(29)), &ports)?;
        assert_eq!(actions(&early), Vec::<&str>::new());
        let due = executor.run(Input::Tick(RiskClock::from_secs(30)), &ports)?;
        assert_eq!(cancels(&due), vec![exit.as_str()]);
        assert_eq!(actions(&due), vec!["interval_limit"]);
        assert!(
            due.iter()
                .any(|effect| matches!(effect, Effect::Notify(note) if note.message_key == "unprotected_interval_limit"))
        );
        assert_eq!(
            executor
                .state
                .unprotected
                .last()
                .map(|interval| interval.alerted),
            Some(true)
        );
        let again = executor.run(Input::Tick(RiskClock::from_secs(31)), &ports)?;
        assert!(cancels(&again).is_empty() && actions(&again).is_empty());
        let settled = executor.run(cancel_accepted(&exit), &ports)?;
        assert_eq!(actions(&settled), vec!["placed", "unprotected_end"]);
        Ok(())
    }

    /// A restart resumes the sequence from the journal: the fold rebuilds it, prices included.
    #[test]
    fn a_restart_resumes_the_sequence_from_its_start() -> Result<(), ExecutorError> {
        with_ports!(ports);
        let mut executor = protected(&ports)?;
        executor.run(sell(EXIT, "5", "139", Purpose::RiskExit)?, &ports)?;
        let restarted = executor.restarted(&ports)?;
        assert_eq!(restarted.state.exiting, executor.state.exiting);
        assert_eq!(
            restarted
                .state
                .exiting
                .get(&aapl()?)
                .and_then(|sequence| sequence.prices)
                .map(|prices| (prices.stop, prices.take_profit)),
            Some((Price::parse("140")?, Some(Price::parse("170")?)))
        );
        Ok(())
    }

    /// An OCO submission read back without both of its prices is refused, never read as a plain
    /// order.
    #[test]
    fn a_partial_oco_submission_is_refused() -> Result<(), ExecutorError> {
        with_ports!(ports);
        let mut executor = Executor::opened(&ports)?;
        let refused = committed(
            &mut executor,
            "OrderSubmitted",
            vec![
                ("client_order_id", text(OCO)),
                ("agent", text("agent-a")),
                ("instrument", text("AAPL")),
                ("qty", text("10")),
                ("order_class", text("oco")),
                ("take_profit", text("170")),
            ],
        );
        assert!(refused.is_err(), "{refused:?}");
        Ok(())
    }

    /// Whole-share equities with a liquid exit tier, so the ladder has what it prices from.
    struct Tiered;

    impl InstrumentSnapshot for Tiered {
        fn asset_class(&self, _instrument: &InstrumentId) -> Option<AssetClass> {
            Some(AssetClass::UsEquity)
        }

        fn increment(&self, _instrument: &InstrumentId) -> Option<ShareIncrement> {
            Some(ShareIncrement::Whole)
        }

        fn exit_tier(&self, _instrument: &InstrumentId) -> Option<ExitTier> {
            Some(ExitTier {
                exit_offset: Fraction::parse("0.005").ok()?,
                exit_offset_step: Fraction::parse("0.005").ok()?,
                max_exit_offset: Fraction::parse("0.03").ok()?,
            })
        }
    }

    /// #174 ruling 5862579929, (A)'s rule-13 condition: the ladder's stub is reached only by an
    /// exit a sequence runs for, where protection rested. An exit of an **unprotected** position,
    /// with an exit tier and a fresh quote on hand, is submitted at once at its own limit; the same
    /// exit of the protected position reaches the stub once its cancel is confirmed.
    #[test]
    fn an_unprotected_exit_never_reaches_the_ladder_stub() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Tiered,
            config: &config,
            fees: &fees,
        };
        for purpose in [
            Purpose::RiskExit,
            Purpose::OwnerExit,
            Purpose::DiscretionaryExit,
            Purpose::Flatten,
        ] {
            let mut executor = held(&ports)?;
            executor.run(quote("150")?, &ports)?;
            let sent = executor.run(sell(EXIT, "5", "150", purpose)?, &ports)?;
            assert_eq!(
                submissions(&sent)
                    .iter()
                    .map(|order| order.limit_price)
                    .collect::<Vec<_>>(),
                vec![Some(Price::parse("150")?)],
                "{purpose:?}"
            );

            let mut executor = protected(&ports)?;
            executor.run(quote("150")?, &ports)?;
            executor.run(sell(EXIT, "5", "150", purpose)?, &ports)?;
            assert!(
                stub(&executor.run(cancel_accepted(OCO), &ports)),
                "{purpose:?}: the sequence's exit is the ladder's"
            );
        }
        Ok(())
    }

    /// Every equity is crypto here, for the crypto sequence's stub (slice 5).
    struct Coins;

    impl InstrumentSnapshot for Coins {
        fn asset_class(&self, _instrument: &InstrumentId) -> Option<AssetClass> {
            Some(AssetClass::Crypto)
        }

        fn increment(&self, _instrument: &InstrumentId) -> Option<ShareIncrement> {
            Some(ShareIncrement::Fractional)
        }

        fn exit_tier(&self, _instrument: &InstrumentId) -> Option<ExitTier> {
            None
        }
    }

    fn order(
        intent: &str,
        name: &str,
        side: Side,
        limit: &str,
        purpose: Purpose,
    ) -> Result<Input, ExecutorError> {
        Ok(Input::Intent(IntentHandoff {
            intent_id: IntentId(EventId(intent.to_owned())),
            agent: AgentId("agent-a".to_owned()),
            body: IntentBody::Order {
                instrument: InstrumentId::new(name)?,
                side,
                qty: Qty::parse("1")?,
                limit: Price::parse(limit)?,
                purpose,
                protection: None,
            },
        }))
    }

    /// Neither an add nor a protective order starts the sequence: nothing is cancelled and no
    /// interval opens. An equity add in a protected instrument is gated as usual (here held for the
    /// startup reconciliation, never refused), while a crypto add there answers the stub of §5.4's
    /// crypto sequence (slice 5). The interval's start names the orders it cancels.
    #[test]
    fn an_add_or_a_protective_order_starts_no_sequence() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        for (side, limit, purpose) in [
            (Side::Buy, "150", Purpose::Increase),
            (Side::Sell, "150", Purpose::Protective),
        ] {
            let mut executor = protected(&ports)?;
            let ran = executor.run(order(EXIT, "AAPL", side, limit, purpose)?, &ports);
            let effects = ran.unwrap_or_default();
            assert!(
                actions(&effects).is_empty() && cancels(&effects).is_empty(),
                "{purpose:?}: {:?}",
                drafted(&effects)
            );
            assert!(executor.state.exiting.is_empty(), "{purpose:?}");
        }
        let mut executor = protected(&ports)?;
        assert!(
            executor
                .run(
                    order(EXIT, "AAPL", Side::Buy, "150", Purpose::Increase)?,
                    &ports
                )
                .is_ok(),
            "an equity add is not the crypto sequence"
        );
        let coins = Ports {
            instruments: &Coins,
            ..ports
        };
        let mut executor = protected(&coins)?;
        assert!(stub(&executor.run(
            order(EXIT, "AAPL", Side::Buy, "150", Purpose::Increase)?,
            &coins
        )));

        let mut executor = protected(&ports)?;
        let started = executor.run(sell(EXIT, "5", "139", Purpose::RiskExit)?, &ports)?;
        assert_eq!(
            protection_drafts(&started)
                .first()
                .and_then(|draft| draft.payload.get("orders"))
                .and_then(Value::as_str),
            Some(OCO)
        );
        Ok(())
    }

    /// A protective order sent while a sequence runs is not the ladder's: it goes at its own
    /// limit, never at the exit's stub.
    #[test]
    fn a_protective_order_in_a_running_sequence_is_not_the_ladders() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Tiered,
            config: &config,
            fees: &fees,
        };
        let mut executor = protected(&ports)?;
        executor.run(quote("150")?, &ports)?;
        executor.run(sell(EXIT, "5", "150", Purpose::RiskExit)?, &ports)?;
        let sent = executor.run(sell(SECOND, "5", "160", Purpose::Protective)?, &ports)?;
        assert_eq!(
            submissions(&sent)
                .iter()
                .map(|order| order.limit_price)
                .collect::<Vec<_>>(),
            vec![Some(Price::parse("160")?)]
        );
        Ok(())
    }

    /// The confirmation re-gates only the sequence's own waiting exits: an opening held in
    /// another instrument stays held, gated no second time.
    #[test]
    fn a_confirmation_re_gates_only_its_own_instruments_waiting_exits() -> Result<(), ExecutorError>
    {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = protected(&ports)?;
        executor.run(
            order(SECOND, "MSFT", Side::Buy, "100", Purpose::Open)?,
            &ports,
        )?;
        executor.run(sell(EXIT, "5", "139", Purpose::RiskExit)?, &ports)?;
        let released = executor.run(cancel_accepted(OCO), &ports)?;
        assert_eq!(
            drafted(&released)
                .iter()
                .filter(|kind| **kind == "GateDecided")
                .count(),
            1,
            "{:?}",
            drafted(&released)
        );
        Ok(())
    }

    /// At the bound, an exit whose cancel is already outstanding is not asked to cancel again; the
    /// owner is still alerted.
    #[test]
    fn the_bound_asks_no_second_cancel_of_an_exit_already_cancelling() -> Result<(), ExecutorError>
    {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = protected(&ports)?;
        executor.run(sell(EXIT, "5", "139", Purpose::RiskExit)?, &ports)?;
        executor.run(cancel_accepted(OCO), &ports)?;
        committed(
            &mut executor,
            "OrderStateChanged",
            vec![
                ("client_order_id", text(format!("md-{EXIT}"))),
                ("state", text("accepted")),
                ("cancel_requested", Value::Bool(true)),
            ],
        )?;
        let due = executor.run(Input::Tick(RiskClock::from_secs(30)), &ports)?;
        assert!(cancels(&due).is_empty(), "{:?}", drafted(&due));
        assert_eq!(actions(&due), vec!["interval_limit"]);
        Ok(())
    }

    /// Crypto with an exit tier, under a mandate whose stop-limit offset is 1%.
    struct Coin;

    impl InstrumentSnapshot for Coin {
        fn asset_class(&self, _instrument: &InstrumentId) -> Option<AssetClass> {
            Some(AssetClass::Crypto)
        }

        fn increment(&self, _instrument: &InstrumentId) -> Option<ShareIncrement> {
            Some(ShareIncrement::Fractional)
        }

        fn exit_tier(&self, instrument: &InstrumentId) -> Option<ExitTier> {
            Tiered.exit_tier(instrument)
        }
    }

    impl MandateView for Coin {
        fn version(&self, agent: &AgentId) -> Option<MandateVersion> {
            Everything.version(agent)
        }

        fn crypto_stop_limit_offset(&self, _agent: &AgentId) -> Option<Fraction> {
            Fraction::parse("0.01").ok()
        }

        fn covers(&self, _agent: &AgentId, _instrument: &InstrumentId) -> bool {
            true
        }
    }

    fn fresh_quote(bid: &str, sane: bool, at: i64) -> Result<Input, ExecutorError> {
        let bid = Price::parse(bid)?;
        Ok(Input::Market(MarketObservation {
            instrument: aapl()?,
            bid: Some(bid),
            bid_size: Some(Qty::parse("100")?),
            ask: Some(bid),
            last_trade: Some(bid),
            mark: Some(bid),
            sane,
            observed_at: RiskClock::from_secs(at),
        }))
    }

    /// #267 round 1, B1: a stop-only placement (crypto's one stop-limit, DEC-36) is re-placed in
    /// its own shape for what remains — a GTC stop-limit at stop × (1 − offset) — and the sequence
    /// ends; so a later exit, with a tier and a quote on hand, starts a fresh sequence on that
    /// protection and never meets the ladder stub at its handoff.
    #[test]
    fn a_stop_only_placement_is_re_placed_as_itself_and_the_sequence_ends()
    -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Coin,
            instruments: &Coin,
            config: &config,
            fees: &fees,
        };
        let mut executor = held(&ports)?;
        committed(
            &mut executor,
            "OrderSubmitted",
            vec![
                ("client_order_id", text(OCO)),
                ("agent", text("agent-a")),
                ("instrument", text("AAPL")),
                ("side", text("sell")),
                ("qty", text("10")),
                ("order_type", text("stop_limit")),
                ("tif", text("gtc")),
                ("stop_price", text("140")),
                ("limit", text("138.6")),
                ("purpose", text("protective")),
            ],
        )?;
        committed(
            &mut executor,
            "OrderStateChanged",
            vec![("client_order_id", text(OCO)), ("state", text("accepted"))],
        )?;
        committed(
            &mut executor,
            "ProtectionChanged",
            vec![
                ("instrument", text("AAPL")),
                ("action", text("placed")),
                ("orders", text(OCO)),
                ("qty", text("10")),
                ("stop", text("140")),
            ],
        )?;
        executor.run(sell(EXIT, "4", "150", Purpose::RiskExit)?, &ports)?;
        executor.run(cancel_accepted(OCO), &ports)?;
        let settled = executor.run(filled(EXIT, "4")?, &ports)?;
        assert_eq!(actions(&settled), vec!["placed", "unprotected_end"]);
        assert_eq!(
            submissions(&settled)
                .iter()
                .map(|order| (
                    order.order_type,
                    order.tif,
                    order.qty,
                    order.stop_price,
                    order.limit_price,
                    order.oco.is_none()
                ))
                .collect::<Vec<_>>(),
            vec![(
                OrderType::StopLimit,
                TimeInForce::Gtc,
                Qty::parse("6")?,
                Some(Price::parse("140")?),
                Some(Price::parse("138.6")?),
                true
            )]
        );
        assert!(executor.state.exiting.is_empty(), "no stale sequence");

        executor.run(fresh_quote("150", true, 0)?, &ports)?;
        let second = executor.run(sell(SECOND, "6", "150", Purpose::RiskExit)?, &ports);
        assert!(!stub(&second), "{second:?}");
        assert_eq!(actions(&second?), vec!["unprotected_start"]);
        Ok(())
    }

    /// #267 round 1, B2: a protective order that fills while its cancel is outstanding leaves the
    /// protection, and the exit waiting on it is released — nothing waits on an order the broker
    /// no longer holds.
    #[test]
    fn a_protective_order_filled_mid_sequence_releases_the_waiting_exit()
    -> Result<(), ExecutorError> {
        with_ports!(ports);
        let mut executor = protected(&ports)?;
        executor.run(sell(EXIT, "5", "139", Purpose::RiskExit)?, &ports)?;
        let filled_oco = executor.run(
            Input::BrokerUpdate(BrokerUpdate::Order(BrokerOrder {
                broker_order_id: "b-oco".to_owned(),
                client_order_id: Some(OCO.to_owned()),
                instrument: aapl()?,
                side: Side::Sell,
                qty: Qty::parse("10")?,
                filled_qty: Qty::parse("10")?,
                limit_price: None,
                stop_price: None,
                status: "filled".to_owned(),
                reject_code: None,
                replaced_by_broker_order_id: None,
                legs: Vec::new(),
                created_on: None,
            })),
            &ports,
        )?;
        assert!(!executor.state.protection.contains_key(&aapl()?));
        assert_eq!(submitted(&filled_oco), 1, "the waiting exit goes");
        Ok(())
    }

    /// #267 round 1, M3: protection is re-placed only once no exit in the sequence still works,
    /// for the position then held, so Σ resting sells never exceeds the position.
    #[test]
    fn protection_waits_for_every_exit_in_the_sequence() -> Result<(), ExecutorError> {
        with_ports!(ports);
        for (second, remains) in [("5", None), ("3", Some("2"))] {
            let mut executor = protected(&ports)?;
            executor.run(sell(EXIT, "5", "139", Purpose::RiskExit)?, &ports)?;
            executor.run(sell(SECOND, second, "139", Purpose::RiskExit)?, &ports)?;
            executor.run(cancel_accepted(OCO), &ports)?;
            let first = executor.run(filled(EXIT, "5")?, &ports)?;
            assert!(
                actions(&first).is_empty() && submitted(&first) == 0,
                "{second}: the other exit still works"
            );
            let last = executor.run(filled(SECOND, second)?, &ports)?;
            let placed: Vec<Qty> = submissions(&last).iter().map(|order| order.qty).collect();
            match remains {
                None => assert!(placed.is_empty(), "flat"),
                Some(qty) => assert_eq!(placed, vec![Qty::parse(qty)?]),
            }
            assert_eq!(actions(&last).last(), Some(&"unprotected_end"), "{second}");
        }
        Ok(())
    }

    /// #267 round 1, M4 (AGENTS.md's one bad tick): an insane or stale quote never makes an exit
    /// passive; it takes the no-quote branch, the marketable sequence.
    #[test]
    fn a_bad_or_stale_tick_never_makes_an_exit_passive() -> Result<(), ExecutorError> {
        with_ports!(ports);
        for (sane, observed, now) in [(false, 0, 0), (true, 0, 100)] {
            let mut executor = protected(&ports)?;
            executor.run(Input::Tick(RiskClock::from_secs(now)), &ports)?;
            let watched = executor.run(fresh_quote("0.01", sane, observed)?, &ports);
            assert!(
                watched.is_ok() || stub(&watched),
                "the quote is kept either way"
            );
            let started = executor.run(sell(EXIT, "5", "139", Purpose::RiskExit)?, &ports)?;
            assert_eq!(
                actions(&started),
                vec!["unprotected_start"],
                "sane {sane}, observed {observed} at {now}"
            );
        }
        Ok(())
    }

    /// #174 comment 5863814949: the gate runs first, so an exit it denies starts no sequence —
    /// nothing is cancelled, no interval opens, and the protection keeps resting.
    #[test]
    fn a_denied_exit_starts_no_sequence() -> Result<(), ExecutorError> {
        with_ports!(ports);
        let mut executor = protected(&ports)?;
        let refused = executor.run(sell(EXIT, "20", "139", Purpose::RiskExit)?, &ports)?;
        assert!(
            drafted(&refused).contains(&"GateDecided"),
            "{:?}",
            drafted(&refused)
        );
        assert!(actions(&refused).is_empty() && cancels(&refused).is_empty());
        assert!(executor.state.exiting.is_empty());
        assert!(rests(&executor.state, &aapl()?));
        Ok(())
    }

    /// #174 comment 5863814949: an exit held by an `Unknown` order in its instrument (rule 13)
    /// leaves the protection resting and opens no interval; once the `Unknown` resolves, the tick
    /// that releases the exit starts its sequence.
    #[test]
    fn a_held_exit_leaves_protection_resting_until_released() -> Result<(), ExecutorError> {
        with_ports!(ports);
        let mut executor = protected(&ports)?;
        committed(
            &mut executor,
            "OrderSubmitted",
            vec![
                ("client_order_id", text(BUY)),
                ("agent", text("agent-a")),
                ("instrument", text("AAPL")),
                ("side", text("buy")),
                ("qty", text("1")),
                ("limit", text("150")),
                ("purpose", text("increase")),
            ],
        )?;
        committed(
            &mut executor,
            "OrderStateChanged",
            vec![("client_order_id", text(BUY)), ("state", text("unknown"))],
        )?;
        let held = executor.run(sell(EXIT, "5", "139", Purpose::RiskExit)?, &ports)?;
        assert!(actions(&held).is_empty() && cancels(&held).is_empty());
        assert!(executor.state.unprotected.is_empty() && rests(&executor.state, &aapl()?));
        executor.run(buy_reported("canceled", "0")?, &ports)?;
        let released = executor.run(Input::Tick(RiskClock::from_secs(1)), &ports)?;
        assert_eq!(actions(&released), vec!["unprotected_start"]);
        assert_eq!(cancels(&released), vec![OCO]);
        Ok(())
    }

    /// #267 round 1, B1 and DEC-160 (2): a stop-only placement with no shape to re-place — an
    /// equity's (only brackets and OCOs protect equities, slice 2) or crypto's under no offset — is
    /// never ended silently: its re-placement answers the stub.
    #[test]
    fn a_stop_only_placement_with_no_shape_answers_the_stub() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let snapshots: [&dyn InstrumentSnapshot; 2] = [&Everything, &Coin];
        for instruments in snapshots {
            let ports = Ports {
                ids: &Ids,
                mandates: &Everything,
                instruments,
                config: &config,
                fees: &fees,
            };
            let mut executor = held(&ports)?;
            committed(
                &mut executor,
                "ProtectionChanged",
                vec![
                    ("instrument", text("AAPL")),
                    ("action", text("placed")),
                    ("orders", text(OCO)),
                    ("qty", text("10")),
                    ("stop", text("140")),
                ],
            )?;
            executor.run(sell(EXIT, "4", "150", Purpose::RiskExit)?, &ports)?;
            executor.run(cancel_accepted(OCO), &ports)?;
            assert!(stub(&executor.run(filled(EXIT, "4")?, &ports)));
        }
        Ok(())
    }

    fn ocos(effects: &[Effect]) -> Vec<(&str, Qty, Option<OcoLegs>, Purpose)> {
        submissions(effects)
            .into_iter()
            .map(|order| {
                (
                    order.client_order_id.as_str(),
                    order.qty,
                    order.oco.clone(),
                    order.purpose,
                )
            })
            .collect()
    }

    fn legs(take_profit: &str, stop: &str, qty: &str) -> Result<Option<OcoLegs>, ExecutorError> {
        Ok(Some(OcoLegs {
            take_profit: Price::parse(take_profit)?,
            stop: Price::parse(stop)?,
            qty: Qty::parse(qty)?,
        }))
    }

    /// §5.4: a passive exit (a sell limit above the bid) is placed as the take-profit leg of a new
    /// OCO keeping the existing stop — cancel the OCO, confirm, submit the new one — and protection
    /// is never removed: no unprotected interval opens, and the new OCO rests as the protection.
    #[test]
    fn a_passive_exit_becomes_a_new_oco_keeping_the_stop() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Tiered,
            config: &config,
            fees: &fees,
        };
        let mut executor = protected(&ports)?;
        executor.run(quote("150")?, &ports)?;
        let asked = executor.run(sell(EXIT, "10", "160", Purpose::DiscretionaryExit)?, &ports)?;
        assert_eq!(cancels(&asked), vec![OCO]);
        assert_eq!(actions(&asked), vec!["passive_start"]);
        assert_eq!(
            submitted(&asked),
            0,
            "the new OCO waits for the confirmation"
        );

        let placed = executor.run(cancel_accepted(OCO), &ports)?;
        let exit = format!("md-{EXIT}");
        assert_eq!(
            ocos(&placed),
            vec![(
                exit.as_str(),
                Qty::parse("10")?,
                legs("160", "140", "10")?,
                Purpose::Protective
            )],
            "the exit's price as the take-profit, the stop kept"
        );
        assert_eq!(actions(&placed), vec!["cancelled", "placed"]);
        assert!(
            executor.state.exiting.is_empty(),
            "the passive sequence ends at its placement"
        );
        assert!(
            executor.state.unprotected.is_empty(),
            "no interval ever opened"
        );
        assert_eq!(
            executor
                .state
                .protection
                .get(&aapl()?)
                .map(|protection| (protection.resting.clone(), protection.covered_qty)),
            Some((vec![ClientOrderId::parse(&exit)?], Qty::parse("10")?)),
            "the new OCO is the instrument's protection"
        );
        Ok(())
    }

    /// DEC-160 draft (#174 comment 5862931870, (c)): a passive exit of part of the position sells
    /// no more than the intent — its OCO covers the exit's quantity — and the remainder keeps its
    /// protection at the original prices, so the whole position keeps the stop (rule 3).
    #[test]
    fn a_partial_passive_exit_re_protects_the_remainder_at_the_original_prices()
    -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Tiered,
            config: &config,
            fees: &fees,
        };
        let mut executor = protected(&ports)?;
        executor.run(quote("150")?, &ports)?;
        executor.run(sell(EXIT, "4", "160", Purpose::DiscretionaryExit)?, &ports)?;
        let placed = executor.run(cancel_accepted(OCO), &ports)?;
        let exit = format!("md-{EXIT}");
        let remainder = protection_drafts(&placed)
            .iter()
            .filter(|draft| draft.payload.get("action").and_then(Value::as_str) == Some("placed"))
            .nth(1)
            .map(|draft| format!("md-held-1-p{}", draft.event_id.0))
            .unwrap_or_default();
        assert_eq!(
            ocos(&placed),
            vec![
                (
                    exit.as_str(),
                    Qty::parse("4")?,
                    legs("160", "140", "4")?,
                    Purpose::Protective
                ),
                (
                    remainder.as_str(),
                    Qty::parse("6")?,
                    legs("170", "140", "6")?,
                    Purpose::Protective
                ),
            ]
        );
        assert_eq!(
            executor
                .state
                .protection
                .get(&aapl()?)
                .map(|protection| protection.covered_qty),
            Some(Qty::parse("10")?)
        );
        Ok(())
    }

    /// A restart between the cancel and its confirmation resumes the passive sequence from the
    /// journal: the confirmation still places the OCO, never a plain sell.
    #[test]
    fn a_restart_resumes_a_passive_exit() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Tiered,
            config: &config,
            fees: &fees,
        };
        let mut executor = protected(&ports)?;
        executor.run(quote("150")?, &ports)?;
        executor.run(sell(EXIT, "10", "160", Purpose::DiscretionaryExit)?, &ports)?;
        let mut restarted = executor.restarted(&ports)?;
        restarted.run(quote("150")?, &ports)?;
        let placed = restarted.run(cancel_accepted(OCO), &ports)?;
        assert_eq!(
            ocos(&placed)
                .into_iter()
                .map(|(_, _, oco, _)| oco)
                .collect::<Vec<_>>(),
            vec![legs("160", "140", "10")?]
        );
        Ok(())
    }

    /// Rule 13 and rule 3: with no stop on record there is nothing to keep, so a passive exit is
    /// not refused but runs the marketable sequence, bounded like any other.
    #[test]
    fn a_passive_exit_without_a_known_stop_runs_the_marketable_sequence()
    -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = held(&ports)?;
        committed(
            &mut executor,
            "ProtectionChanged",
            vec![
                ("instrument", text("AAPL")),
                ("action", text("placed")),
                ("orders", text(OCO)),
                ("qty", text("10")),
            ],
        )?;
        executor.run(quote("150")?, &ports)?;
        let asked = executor.run(sell(EXIT, "10", "160", Purpose::DiscretionaryExit)?, &ports)?;
        assert_eq!(actions(&asked), vec!["unprotected_start"]);
        assert_eq!(cancels(&asked), vec![OCO]);
        Ok(())
    }

    const BUY: &str = "md-buy-2";

    /// The unprotected ten AAPL with `agent-a`'s own opening buy of 5 resting beside them.
    fn held_with_a_resting_buy(ports: &Ports<'_>) -> Result<Executor, ExecutorError> {
        let mut executor = held(ports)?;
        committed(
            &mut executor,
            "OrderSubmitted",
            vec![
                ("client_order_id", text(BUY)),
                ("agent", text("agent-a")),
                ("instrument", text("AAPL")),
                ("side", text("buy")),
                ("qty", text("5")),
                ("limit", text("150")),
                ("purpose", text("open")),
            ],
        )?;
        committed(
            &mut executor,
            "OrderStateChanged",
            vec![("client_order_id", text(BUY)), ("state", text("accepted"))],
        )?;
        Ok(executor)
    }

    fn buy_reported(status: &str, filled: &str) -> Result<Input, ExecutorError> {
        Ok(Input::Broker(Ok(BrokerOutcome::Order(BrokerOrder {
            broker_order_id: "b-buy".to_owned(),
            client_order_id: Some(BUY.to_owned()),
            instrument: aapl()?,
            side: Side::Buy,
            qty: Qty::parse("5")?,
            filled_qty: Qty::parse(filled)?,
            limit_price: Some(Price::parse("150")?),
            stop_price: None,
            status: status.to_owned(),
            reject_code: None,
            replaced_by_broker_order_id: None,
            legs: Vec::new(),
            created_on: None,
        }))))
    }

    fn queried(effects: &[Effect]) -> Vec<&str> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Broker(BrokerRequest::GetOrderByClientId(id)) => Some(id.as_str()),
                _ => None,
            })
            .collect()
    }

    /// §5.3 rule 5, ruling (ii): a reducing sell first cancels its agent's own resting opening
    /// buy and goes once the cancel is confirmed.
    #[test]
    fn an_exit_cancels_its_agents_resting_opening_first() -> Result<(), ExecutorError> {
        with_ports!(ports);
        let mut executor = held_with_a_resting_buy(&ports)?;
        let asked = executor.run(sell(EXIT, "5", "139", Purpose::RiskExit)?, &ports)?;
        assert_eq!(cancels(&asked), vec![BUY]);
        assert_eq!(submitted(&asked), 0, "the exit waits for the confirmation");
        let released = executor.run(cancel_accepted(BUY), &ports)?;
        assert_eq!(submitted(&released), 1);
        Ok(())
    }

    /// Ruling 5863046153: a cancel never confirmed ends the wait at its bound
    /// (`unknown_absent_window_s`, 15 s here) — the buy is queried as an `Unknown` would be, and
    /// once answered, still resting, the exit goes; a later exit never waits on that buy again.
    #[test]
    fn a_cancel_never_confirmed_ends_the_wait_at_its_bound() -> Result<(), ExecutorError> {
        with_ports!(ports);
        let mut executor = held_with_a_resting_buy(&ports)?;
        executor.run(sell(EXIT, "5", "139", Purpose::RiskExit)?, &ports)?;
        let early = executor.run(Input::Tick(RiskClock::from_secs(14)), &ports)?;
        assert!(queried(&early).is_empty() && submitted(&early) == 0);
        let due = executor.run(Input::Tick(RiskClock::from_secs(15)), &ports)?;
        assert_eq!(queried(&due), vec![BUY]);
        assert!(
            due.iter().any(|effect| matches!(
                effect,
                Effect::Journal(draft) if draft.payload.get("cancel_overdue") == Some(&Value::Bool(true))
            )),
            "the overdue cancel is journaled"
        );
        assert_eq!(submitted(&due), 0, "and the exit waits for the answer");
        let answered = executor.run(buy_reported("new", "0")?, &ports)?;
        assert_eq!(
            submitted(&answered),
            1,
            "still resting: the exit goes anyway"
        );
        let again = executor.run(sell(SECOND, "5", "139", Purpose::RiskExit)?, &ports)?;
        assert_eq!(submitted(&again), 1, "never a second wait on the same buy");
        Ok(())
    }

    /// Ruling 5863046153: a cancel the broker refuses because the buy filled is overdue at once;
    /// the buy is queried, and the answer — filled — releases the exit.
    #[test]
    fn a_cancel_refused_because_the_buy_filled_ends_the_wait() -> Result<(), ExecutorError> {
        with_ports!(ports);
        let mut executor = held_with_a_resting_buy(&ports)?;
        executor.run(sell(EXIT, "5", "139", Purpose::RiskExit)?, &ports)?;
        let refused = executor.run(
            Input::Broker(Ok(BrokerOutcome::Rejected(BrokerReject {
                client_order_id: Some(BUY.to_owned()),
                http_status: 422,
                code: None,
                message: "order is already filled".to_owned(),
            }))),
            &ports,
        )?;
        assert_eq!(queried(&refused), vec![BUY]);
        assert_eq!(submitted(&refused), 0);
        let answered = executor.run(buy_reported("filled", "5")?, &ports)?;
        assert_eq!(submitted(&answered), 1);
        Ok(())
    }

    /// The quantity resting protective sells cover, counted from the order set, independently of
    /// the protection record: every live protective order's unfilled quantity.
    fn stop_covered(executor: &Executor) -> Result<Qty, ExecutorError> {
        executor
            .state
            .orders
            .values()
            .filter(|order| order.purpose == Purpose::Protective && !order.state.is_terminal())
            .try_fold(Qty::ZERO, |total, order| {
                Ok(total.checked_add(order.qty.checked_sub(order.filled_qty)?)?)
            })
    }

    fn position(executor: &Executor) -> Result<Qty, ExecutorError> {
        Ok(executor
            .state
            .positions
            .get(&aapl()?)
            .copied()
            .unwrap_or(SignedQty::ZERO)
            .abs())
    }

    /// Ruling (c) (#174, 5862934909): for every size of passive exit, once its OCOs are placed the
    /// stop-protected quantity equals the position, and after the exit's take-profit fills it still
    /// equals it — never more, never less.
    #[test]
    fn a_passive_exit_keeps_the_stop_on_exactly_the_position() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Tiered,
            config: &config,
            fees: &fees,
        };
        for size in 1..=10 {
            let qty = size.to_string();
            let mut executor = protected(&ports)?;
            executor.run(quote("150")?, &ports)?;
            executor.run(sell(EXIT, &qty, "160", Purpose::DiscretionaryExit)?, &ports)?;
            executor.run(cancel_accepted(OCO), &ports)?;
            assert_eq!(
                stop_covered(&executor)?,
                position(&executor)?,
                "exit of {qty}"
            );
            executor.run(filled(EXIT, &qty)?, &ports)?;
            assert_eq!(
                stop_covered(&executor)?,
                position(&executor)?,
                "exit of {qty} filled: the remainder keeps its stop, and no more"
            );
        }
        Ok(())
    }

    /// `AGENTS.md` rule 13: buying power never holds or denies a protective order or an exit. With
    /// the account at zero buying power, the exit and the re-placement both go.
    #[test]
    fn a_protective_order_goes_out_with_no_buying_power() -> Result<(), ExecutorError> {
        with_ports!(ports);
        let mut executor = protected(&ports)?;
        executor.run(
            Input::BrokerUpdate(BrokerUpdate::Account(account("0")?)),
            &ports,
        )?;
        assert!(
            executor
                .state
                .buying_power()
                .is_some_and(|power| power <= Usd::ZERO),
            "{:?}",
            executor.state.buying_power()
        );
        executor.run(sell(EXIT, "4", "139", Purpose::RiskExit)?, &ports)?;
        let exit = executor.run(cancel_accepted(OCO), &ports)?;
        assert_eq!(submitted(&exit), 1, "the exit");
        let placed = executor.run(filled(EXIT, "4")?, &ports)?;
        assert_eq!(
            submissions(&placed)
                .iter()
                .map(|order| (order.purpose, order.qty))
                .collect::<Vec<_>>(),
            vec![(Purpose::Protective, Qty::parse("6")?)]
        );
        Ok(())
    }

    /// Rule 5's bound is an opening's: a protective order whose cancel is unconfirmed for as long
    /// is not queried as overdue — the sequence's own bound, `max_unprotected_s`, governs it.
    #[test]
    fn the_rule_5_bound_never_touches_a_protective_cancel() -> Result<(), ExecutorError> {
        with_ports!(ports);
        let mut executor = protected(&ports)?;
        executor.run(sell(EXIT, "5", "139", Purpose::RiskExit)?, &ports)?;
        let tick = executor.run(Input::Tick(RiskClock::from_secs(20)), &ports)?;
        assert!(queried(&tick).is_empty(), "{:?}", drafted(&tick));
        Ok(())
    }

    /// Rule 5 replaces 3a's stub (94ac122; #267, comment 5863704223): where protection rests, an
    /// exit cancels its own agent's resting opening as well as the protection, and goes only once
    /// both are confirmed; another agent's opening, or a done one, is left alone.
    #[test]
    fn an_exit_where_protection_rests_cancels_its_agents_opening_too() -> Result<(), ExecutorError>
    {
        with_ports!(ports);
        for (agent, state, cancelled_too) in [
            ("agent-a", "accepted", true),
            ("agent-a", "partially_filled", true),
            ("agent-a", "canceled", false),
            ("agent-b", "accepted", false),
        ] {
            let mut executor = protected(&ports)?;
            committed(
                &mut executor,
                "OrderSubmitted",
                vec![
                    ("client_order_id", text(BUY)),
                    ("agent", text(agent)),
                    ("instrument", text("AAPL")),
                    ("side", text("buy")),
                    ("qty", text("5")),
                    ("limit", text("150")),
                    ("purpose", text("increase")),
                ],
            )?;
            committed(
                &mut executor,
                "OrderStateChanged",
                vec![("client_order_id", text(BUY)), ("state", text(state))],
            )?;
            let case = format!("{agent} {state}");
            let asked = executor.run(sell(EXIT, "5", "139", Purpose::RiskExit)?, &ports)?;
            let expected = if cancelled_too {
                vec![BUY, OCO]
            } else {
                vec![OCO]
            };
            assert_eq!(cancels(&asked), expected, "{case}");
            assert_eq!(submitted(&asked), 0, "{case}");
            let protection = executor.run(cancel_accepted(OCO), &ports)?;
            assert_eq!(
                submitted(&protection),
                usize::from(!cancelled_too),
                "{case}"
            );
            if cancelled_too {
                let opening = executor.run(cancel_accepted(BUY), &ports)?;
                assert_eq!(
                    submitted(&opening),
                    1,
                    "{case}: both confirmed, the exit goes"
                );
            }
        }
        Ok(())
    }

    /// #174 ruling 5863867565 (2), rule 13: with the gate first, the shares resting protection
    /// holds still count as available to an exit, since its sequence cancels that protection
    /// first — an exit for the whole protected quantity is allowed and starts the sequence, never
    /// `sell_exceeds_available`.
    #[test]
    fn an_exit_for_the_whole_protected_quantity_is_allowed() -> Result<(), ExecutorError> {
        with_ports!(ports);
        let mut executor = protected(&ports)?;
        let started = executor.run(sell(EXIT, "10", "139", Purpose::RiskExit)?, &ports)?;
        let verdict = started.iter().find_map(|effect| match effect {
            Effect::Journal(draft) if draft.event_type == "GateDecided" => {
                draft.payload.get("verdict").and_then(Value::as_str)
            }
            _ => None,
        });
        assert_eq!(verdict, Some("allow"));
        assert_eq!(actions(&started), vec!["unprotected_start"]);
        let released = executor.run(cancel_accepted(OCO), &ports)?;
        assert_eq!(
            submissions(&released)
                .iter()
                .map(|order| order.qty)
                .collect::<Vec<_>>(),
            vec![Qty::parse("10")?]
        );
        Ok(())
    }
}
