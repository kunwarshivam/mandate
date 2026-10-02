//! The protective sequences and the exit price ladder (E7-4, trading-domain spec §5.4 to §5.6).

use std::collections::BTreeSet;

use mandate_accounting::{AssetClass, InstrumentId};
use mandate_num::{Adverse, Fraction, Price, Qty, ShareIncrement, SignedQty, TickRule};

use mandate_accounting::Side;
use mandate_canon::Value;

use crate::batch::Batch;
use crate::error::ExecutorError;
use crate::fold::single_holder;
use crate::ids::{ClientOrderId, IntentId, WATCHDOG};
use crate::intent::{
    abandon, gate_and_submit, intent_of, journal_rung, journal_submission, order_tif, received,
};
use crate::orders::transition;
use crate::payload::{int, text};
use crate::ports::Ports;
use crate::session::{
    Venue, closed_hold, extended_hours, same_session, stops_trigger_since, venue,
};
use crate::state::{EVERY_AGENT, ExecutorState, ExitSequence, IntentOutcome, LoneLadder};
use crate::types::{
    AgentId, BracketLegs, BrokerRequest, EventId, ExitTier, IntentBody, IntentHandoff,
    MarketObservation, Mode, OcoLegs, OrderState, OrderType, ProtectionPrices, Purpose, RiskClock,
    SubmitOrder, TimeInForce,
};

/// The triggered-stop watchdog's clock (§5.4): a sane mark at or below the instrument's resting
/// stop starts a breach, unless one is running; any other sane mark ends it, above the stop or
/// with no stop resting, as in the interval a watchdog's own sequence opens, so a breach never
/// outlives the protection it was measured against (#385 round 2, blocker 2). An insane quote, or
/// one with no mark, changes nothing. A breach starts when the executor learns of it, never
/// earlier than its clock: a stale quote cannot claim the mark has been there for
/// `stop_watchdog_s` already (DEC-260 (9)). A breach the watchdog has already fired on is spent:
/// the next sane breaching mark starts a new one.
pub(crate) fn breach(state: &mut ExecutorState, observation: &MarketObservation) {
    let stop = state
        .protection
        .get(&observation.instrument)
        .filter(|protection| !protection.resting.is_empty())
        .and_then(|protection| protection.prices)
        .map(|prices| prices.stop);
    let (true, Some(mark)) = (observation.sane, observation.mark) else {
        return;
    };
    let Some(stop) = stop else {
        state.breaches.remove(&observation.instrument);
        return;
    };
    if mark <= stop {
        let learned = state.now.map_or(observation.observed_at, |now| {
            now.max(observation.observed_at)
        });
        let fired = state.watchdogged.get(&observation.instrument).copied();
        let start = state
            .breaches
            .entry(observation.instrument.clone())
            .or_insert(learned);
        if let Some(fired) = fired.filter(|fired| spent(*start, *fired)) {
            *start = learned.max(RiskClock::from_secs(fired.secs().saturating_add(1)));
        }
    } else {
        state.breaches.remove(&observation.instrument);
    }
}

/// Whether a breach that began at `since` is the one the instrument's last watchdog, at `fired`,
/// already fired on (or an older one): a spent breach fires no second watchdog, and the next sane
/// breaching mark starts a new one, never earlier than the second after the fire (DEC-260 (11)).
fn spent(since: RiskClock, fired: RiskClock) -> bool {
    since <= fired
}

/// §5.4's triggered-stop watchdog (DEC-160 (11)). Where a breach has lasted `stop_watchdog_s` and
/// the resting stop has no fill, the executor journals the watchdog record (`ProtectionChanged
/// watchdog`), alerts the owner, and hands itself an intent: `w-<record>`, a `risk_exit` of the
/// quantity the protection covers, its agent the position's single holder or none (`*`). That
/// intent takes the marketable sequence like any exit (cancel, confirm, re-gate, the ladder), so
/// it is never refused (rule 13). Its limit, the fallback for when nothing prices it, is the
/// lowest of the latest quote's mark and bid, if that quote is sane, and the last sane bid's mark
/// and bid: an insane quote never sets it, and it is never above a fresh bid, so never placed as
/// a passive take-profit (#385 round 1, blocker 1). With no sane observation to take it from, the
/// watchdog does not fire: the breach persists, the stop keeps resting and nothing is cancelled,
/// so no exit goes and none waits, and §5.6 step 4's alert, which is for an exit at its fallback
/// or held, has nothing to report; the next sane quote prices it. One watchdog runs per
/// instrument at a time: none fires while a sequence runs there or a watchdog intent waits, and a
/// breach it has fired on fires no second time. It exits what the protection still covers, never more than the long
/// position (§5.4's "the held quantity").
pub(crate) fn watchdog(batch: &mut Batch<'_, '_>) -> Result<(), ExecutorError> {
    let now = batch.at().secs();
    let wait = batch.ports.config.stop_watchdog_s;
    let due: Vec<InstrumentId> = batch
        .view
        .breaches
        .iter()
        .filter(|(instrument, since)| {
            stops_trigger_since(batch.ports, instrument, batch.at())
                .is_some_and(|open| now.saturating_sub(since.secs().max(open)) >= wait)
                && batch
                    .view
                    .watchdogged
                    .get(*instrument)
                    .is_none_or(|fired| !spent(**since, *fired))
                && untouched(&batch.view, instrument)
                && !batch.view.exiting.contains_key(*instrument)
                && !watching(&batch.view, instrument)
        })
        .map(|(instrument, _)| instrument.clone())
        .collect();
    for instrument in due {
        let held = covered(&batch.view, &instrument)?.min(long(&batch.view, &instrument));
        let fresh = batch
            .view
            .quotes
            .get(&instrument)
            .filter(|quote| quote.sane);
        let kept = batch.view.sane_bids.get(&instrument);
        let limit = [
            fresh.and_then(|quote| quote.mark),
            fresh.and_then(|quote| quote.bid),
            kept.and_then(|quote| quote.mark),
            kept.and_then(|quote| quote.bid),
        ]
        .into_iter()
        .flatten()
        .min();
        let (true, Some(limit)) = (held > Qty::ZERO, limit) else {
            continue;
        };
        let agent = single_holder(&batch.view, &instrument)
            .unwrap_or_else(|| AgentId(EVERY_AGENT.to_owned()));
        let pairs = vec![
            ("agent", text(agent.0.clone())),
            ("qty", text(held.to_string())),
        ];
        let record = changed(batch, &instrument, "watchdog", pairs)?;
        batch.notify(record.clone(), "stop_watchdog");
        let intent_id = IntentId(EventId(format!("{WATCHDOG}{}", record.0)));
        received(
            batch,
            IntentHandoff {
                intent_id,
                agent,
                body: IntentBody::Order {
                    instrument,
                    side: Side::Sell,
                    qty: held,
                    limit,
                    purpose: Purpose::RiskExit,
                    protection: None,
                },
            },
        )?;
    }
    Ok(())
}

/// Whether every protective order resting in `instrument` is unfilled: §5.4's "with no fill".
fn untouched(view: &ExecutorState, instrument: &InstrumentId) -> bool {
    view.protection.get(instrument).is_some_and(|protection| {
        !protection.resting.is_empty()
            && protection.resting.iter().all(|id| {
                view.orders
                    .get(id)
                    .is_some_and(|order| order.filled_qty == Qty::ZERO)
            })
    })
}

/// Whether a watchdog intent in `instrument` is still waiting: received, and not yet submitted,
/// denied or abandoned.
fn watching(view: &ExecutorState, instrument: &InstrumentId) -> bool {
    view.intents.iter().any(|(intent, record)| {
        intent.0.0.starts_with(WATCHDOG)
            && record.outcome == IntentOutcome::Received
            && view.bodies.get(intent).is_some_and(
                |body| matches!(body, IntentBody::Order { instrument: of, .. } if of == instrument),
            )
    })
}

/// How an order is priced (§5.6, DEC-160 (12)). Never a refusal: an exit is never denied for
/// want of a price (rules 3 and 13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExitPrice {
    /// The intent's own limit: not an exit §5.4's sequence runs for, or no exit tier to ladder.
    Own(Price),
    /// A rung of the ladder.
    Laddered(Price),
    /// Nothing to price from: the intent's own limit, never below an owner exit's floor,
    /// journaled with an owner alert.
    Fallback(Price),
    /// Nothing to price from, for a discretionary exit: held, protection resting, until a price
    /// arrives.
    Held,
}

/// §5.6 for one exit, with DEC-160 (12)'s answer where there is nothing to price from (step 4).
pub(crate) fn exit_price(
    tier: Option<ExitTier>,
    observations: &[MarketObservation],
    step: u32,
    now: RiskClock,
    floor: Option<Price>,
    purpose: Purpose,
    limit: Price,
) -> ExitPrice {
    let Some(tier) = tier.filter(|_| exits(purpose)) else {
        return ExitPrice::Own(limit);
    };
    match ladder_price(tier, observations, step, now, floor) {
        Ok(rung) => ExitPrice::Laddered(rung.limit),
        Err(_) if purpose == Purpose::DiscretionaryExit => ExitPrice::Held,
        Err(_) => ExitPrice::Fallback(floor.map_or(limit, |floor| limit.max(floor))),
    }
}

/// Whether §5.6 ladders an exit with no protection to cancel first (DEC-260 (14)): one that must be
/// marketable where no market order may go — in extended hours, in the closing auction window, or
/// in a presumed halt (§4.4: the latest quote stale or not sane) — and whose purpose may trade
/// there: a risk exit, a protective order or a flatten in any of them, and an owner exit in the
/// regular session. A discretionary exit is paced (§9.6), an instrument with no tier has no
/// ladder, and an exit priced above a fresh sane bid is passive: each keeps its own limit.
pub(crate) fn alone(
    batch: &Batch<'_, '_>,
    instrument: &InstrumentId,
    purpose: Purpose,
    limit: Price,
) -> bool {
    let fresh = batch
        .at()
        .secs()
        .saturating_sub(batch.ports.config.exit_step_s);
    let latest = batch.view.quotes.get(instrument);
    let current = latest.filter(|quote| quote.sane && quote.observed_at.secs() >= fresh);
    let may = purpose.exempt_from_pacing()
        || purpose == Purpose::OwnerExit
            && venue(batch.ports, instrument, batch.at()) != Venue::Extended;
    let needed = match venue(batch.ports, instrument, batch.at()) {
        Venue::Extended | Venue::Closing => true,
        Venue::Regular => latest.is_some() && current.is_none(),
        Venue::Closed => false,
    };
    may && needed
        && batch.ports.instruments.exit_tier(instrument).is_some()
        && !current
            .and_then(|quote| quote.bid)
            .is_some_and(|bid| limit > bid)
}

/// An exit where §5.4's sequence runs, or will once the gate allows it (protection rests), or one
/// [`alone`] ladders, is priced by §5.6's ladder from the last sane bid and the latest quote,
/// oldest first; every other order keeps its intent's limit, and so does the exit of a passive
/// sequence, which is placed as its new OCO's take-profit at that limit (§5.4) and needs nothing
/// to price from.
pub(crate) fn exit_limit(
    batch: &Batch<'_, '_>,
    intent: &IntentId,
    instrument: &InstrumentId,
    purpose: Purpose,
    limit: Price,
) -> ExitPrice {
    let passive = batch
        .view
        .exiting
        .get(instrument)
        .is_some_and(|sequence| sequence.passive && &sequence.intent == intent);
    let unsequenced =
        !batch.view.exiting.contains_key(instrument) && !rests(&batch.view, instrument);
    if passive || unsequenced && !alone(batch, instrument, purpose, limit) {
        return ExitPrice::Own(limit);
    }
    let tier = batch.ports.instruments.exit_tier(instrument);
    match exit_price(
        tier,
        &observed(batch, instrument),
        0,
        batch.at(),
        None,
        purpose,
        limit,
    ) {
        ExitPrice::Laddered(rung) => ExitPrice::Laddered(ticked(
            batch.ports.instruments.asset_class(instrument),
            rung,
        )),
        priced => priced,
    }
}

/// A sell rung on its instrument's price grid (§2.1): an equity's limit rounded up to the Reg NMS
/// tick of the rounded price, so the ladder never sends a sub-penny limit the broker refuses; a
/// crypto price is left as priced, as the gate's collar leaves it (`mandate-risk`'s `on_grid`).
fn ticked(asset_class: Option<AssetClass>, price: Price) -> Price {
    match asset_class {
        Some(AssetClass::Crypto) => price,
        _ => price
            .on_tick(TickRule::RegNmsEquity, Adverse::Down)
            .unwrap_or(price),
    }
}

/// DEC-160 (12), rule 3: an exit held for want of a price after its sequence cancelled the
/// protection never waits unprotected for the interval's bound. Its sequence ends and the
/// protection is re-placed at once for the held quantity; the exit, held, starts a fresh
/// sequence at the first tick it is priceable.
pub(crate) fn reprotect_unpriced(
    batch: &mut Batch<'_, '_>,
    intent: &IntentId,
) -> Result<(), ExecutorError> {
    let (_, body) = intent_of(batch, intent)?;
    let IntentBody::Order { instrument, .. } = body else {
        return Ok(());
    };
    let Some(sequence) = batch.view.exiting.get(&instrument).cloned() else {
        return Ok(());
    };
    if &sequence.intent == intent && !rests(&batch.view, &instrument) {
        replace(batch, &instrument, &sequence)?;
    }
    Ok(())
}

/// Whether the current rung's confirmed cancel is a step's (§5.6 step 2): asked by
/// [`ladder_steps`], not overtaken by the interval's bound, whose cancel ends the sequence, and
/// not after the agent was paused or stopped, which ends it too: the rung is not replaced, and
/// protection returns for what is left (rules 3 and 13 let a pause hold an exit).
fn steps(view: &ExecutorState, sequence: &ExitSequence) -> bool {
    sequence.ladder.stepping && climbs(view, sequence)
}

/// Whether the sequence's ladder may still climb: neither bounded nor its agent paused or stopped.
fn climbs(view: &ExecutorState, sequence: &ExitSequence) -> bool {
    view.effective_mode(&sequence.agent) < Mode::Paused
        && !view.unprotected.iter().any(|interval| {
            interval.ended_at.is_none()
                && interval.alerted
                && view
                    .exiting
                    .get(&interval.instrument)
                    .is_some_and(|running| running.intent == sequence.intent)
        })
}

/// §5.6 step 2, on every tick: a laddered rung left unfilled for `exit_step_s` is cancelled to
/// step, once, unless it rests at the floor (step 3), its instrument has no exit tier to ladder,
/// or the ladder no longer climbs ([`climbs`]). Its confirmation submits the next rung
/// ([`settle`]).
pub(crate) fn ladder_steps(batch: &mut Batch<'_, '_>) -> Result<(), ExecutorError> {
    let now = batch.at().secs();
    let step_s = batch.ports.config.exit_step_s;
    let due: Vec<ClientOrderId> = batch
        .view
        .exiting
        .iter()
        .filter(|(instrument, sequence)| {
            !sequence.passive
                && batch.ports.instruments.exit_tier(instrument).is_some()
                && climbs(&batch.view, sequence)
        })
        .map(|(_, sequence)| (&sequence.intent, sequence.ladder))
        .chain(
            batch
                .view
                .ladders
                .iter()
                .filter(|(instrument, lone)| {
                    batch.ports.instruments.exit_tier(instrument).is_some()
                        && batch.view.effective_mode(&lone.agent) < Mode::Paused
                })
                .map(|(_, lone)| (&lone.intent, lone.ladder)),
        )
        .filter(|(_, ladder)| {
            !ladder.floored
                && !ladder.stepping
                && ladder
                    .since
                    .is_some_and(|since| now.saturating_sub(since.secs()) >= step_s)
        })
        .map(|(intent, ladder)| ClientOrderId::for_intent(intent)?.rung(ladder.rung))
        .collect::<Result<Vec<_>, _>>()?;
    for id in due {
        let live = batch.view.orders.get(&id).is_some_and(|order| {
            matches!(
                order.state,
                OrderState::Submitting | OrderState::Accepted | OrderState::PartiallyFilled
            )
        });
        if live {
            let asked = vec![
                ("cancel_requested", Value::Bool(true)),
                ("ladder_step", Value::Bool(true)),
            ];
            transition(batch, &id, OrderState::PendingCancel, asked)?;
            batch.broker(BrokerRequest::Cancel {
                client_order_id: id,
            });
        }
    }
    Ok(())
}

/// §5.6 step 2's resubmission: the next rung, a new order under `-l{n}` (§2.3), for what the last
/// rung left unsold, priced one offset step lower from the current reference. With nothing to
/// price from it goes at the intent's own limit, journaled and alerted (DEC-160 (12)), never
/// refused; at `max_exit_offset` it rests and the owner is alerted once (step 3).
fn next_rung(
    batch: &mut Batch<'_, '_>,
    instrument: &InstrumentId,
    (intent, rung): (&IntentId, u32),
    qty: Qty,
    lone: bool,
) -> Result<(), ExecutorError> {
    let (agent, body) = intent_of(batch, intent)?;
    let IntentBody::Order {
        side,
        limit,
        purpose,
        ..
    } = body
    else {
        return Ok(());
    };
    let step = rung.saturating_add(1);
    let tier = batch.ports.instruments.exit_tier(instrument);
    let observations = observed(batch, instrument);
    let rung = tier.map(|tier| ladder_price(tier, &observations, step, batch.at(), None));
    let (price, at_floor) = match rung {
        Some(Ok(rung)) => (
            ticked(batch.ports.instruments.asset_class(instrument), rung.limit),
            rung.at_floor,
        ),
        _ => {
            fallback(batch, intent, instrument, limit)?;
            (limit, false)
        }
    };
    let request = SubmitOrder {
        client_order_id: ClientOrderId::for_intent(intent)?.rung(step)?,
        instrument: instrument.clone(),
        side,
        qty,
        order_type: OrderType::Limit,
        tif: order_tif(batch, instrument),
        limit_price: Some(price),
        stop_price: None,
        bracket: None,
        oco: None,
        extended_hours: extended_hours(batch.ports, instrument, batch.at(), purpose),
        purpose,
    };
    let mut extra = vec![
        ("rung", int(u64::from(step))?),
        ("at_floor", Value::Bool(at_floor)),
    ];
    if lone {
        extra.push(("laddered", Value::Bool(true)));
    }
    journal_rung(batch, &request, Some(intent), &agent, 1, extra)?;
    if at_floor {
        let alerted = changed(batch, instrument, "ladder_floor", Vec::new())?;
        batch.notify(alerted, "exit_ladder_floor");
    }
    batch.broker(BrokerRequest::Submit(request));
    Ok(())
}

/// What §5.6 prices `instrument` from, oldest to newest by when each was observed: the last sane
/// quote with a trade, the last sane bid, and the latest quote. A trade printed in another session
/// prices nothing (§8.2: a last trade counts only in-session).
fn observed(batch: &Batch<'_, '_>, instrument: &InstrumentId) -> Vec<MarketObservation> {
    let mut seen: Vec<MarketObservation> = [
        &batch.view.trades,
        &batch.view.sane_bids,
        &batch.view.quotes,
    ]
    .into_iter()
    .filter_map(|kept| kept.get(instrument))
    .cloned()
    .map(|mut quote| {
        if !same_session(batch.ports, instrument, quote.observed_at, batch.at()) {
            quote.last_trade = None;
        }
        quote
    })
    .collect();
    seen.sort_by_key(|quote| quote.observed_at);
    seen.dedup();
    seen
}

/// DEC-160 (12): an exit going at its own limit for want of a price, journaled and alerted.
pub(crate) fn fallback(
    batch: &mut Batch<'_, '_>,
    intent: &IntentId,
    instrument: &InstrumentId,
    limit: Price,
) -> Result<(), ExecutorError> {
    let pairs = vec![
        ("intent_id", text(intent.0.0.clone())),
        ("limit", text(limit.to_string())),
    ];
    let alerted = changed(batch, instrument, "exit_unpriced", pairs)?;
    batch.notify(alerted, "exit_unpriced");
    Ok(())
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

/// Whether a sequence runs in `instrument` and the protection resting there holds its exits back
/// (§5.4): any of it, in a sequence that cancelled it. A handed-on one's placement
/// ([`passive_exit`]) was sized net of every exit selling beside it and rests by design, so it
/// holds them only once its stop and their sells no longer fit within the position ([`fits`]),
/// which is when [`begin_exit`] cancels it; an exit it was sized around goes rather than waiting
/// on a cancel nobody asked, and a failed count holds (#286 round 2, M1′).
fn protection_holds(state: &ExecutorState, instrument: &InstrumentId) -> bool {
    state.exiting.get(instrument).is_some_and(|sequence| {
        rests(state, instrument)
            && (!handed(state, sequence) || !matches!(fits(state, instrument), Ok(true)))
    })
}

/// An exit waits while a protective order in its sequence is not yet confirmed gone (§5.4), or
/// while the agent's own opening in the instrument is live, in any state the broker may hold it
/// in (§5.3 rule 5) — but never on an opening whose cancel went overdue, answered or not, and never
/// twice on the same submission of it: that wait is bounded ([`overdue_openings`]; rules 3 and 13,
/// #174 ruling 5863046153, DEC-160 (13), (18)). A broker that never answers the query, or 404s an
/// opening it acknowledged, ends no wait; an opening it may still hold (`Unknown`) holds the exit
/// at the gate (`unknown_order_in_flight`), which is rule 13's hold, not this wait.
pub(crate) fn awaits_cancel(
    state: &ExecutorState,
    agent: &AgentId,
    instrument: &InstrumentId,
    purpose: Purpose,
) -> bool {
    let released = |id: &ClientOrderId| {
        state
            .details
            .get(id)
            .is_some_and(|detail| detail.cancel_overdue)
    };
    exits(purpose)
        && (protection_holds(state, instrument)
            || openings(state, Some(agent), Some(instrument), false).any(|id| !released(&id)))
}

/// Rule 5's bound, on a tick: once an exit has waited `unknown_absent_window_s` from the moment
/// its gate allowed it, every opening of its agent in the instrument it still waits on, whatever
/// its state, is treated as an `Unknown` — marked overdue and queried by id (§5.7) — and the exit
/// goes to the gate at once ([`release_waiting`]), without waiting for the answer: an opening
/// the broker may still hold as `Unknown` holds it there until the query or reconciliation decides
/// it, and one still resting is a self-cross the broker's to refuse (DEC-160 (7), (13), (18)).
pub(crate) fn overdue_openings(batch: &mut Batch<'_, '_>) -> Result<(), ExecutorError> {
    let now = batch.at().secs();
    let window = batch.ports.config.unknown_absent_window_s;
    let view = &batch.view;
    let mut due = BTreeSet::new();
    for exit in waiting_exits(view) {
        if now.saturating_sub(exit.since.secs()) >= window {
            due.extend(
                openings(view, Some(&exit.agent), Some(&exit.instrument), false).filter(|id| {
                    !view
                        .details
                        .get(id)
                        .is_some_and(|detail| detail.cancel_overdue)
                }),
            );
        }
    }
    for id in due {
        overdue(batch, &id)?;
    }
    Ok(())
}

/// Marks a wait on an order overdue — past rule 5's bound, or its cancel refused by the broker —
/// and queries the order: the query's answer decides it (§5.7). An order the broker has not yet
/// acknowledged (`Submitting`) becomes `Unknown`, which is what a submission unanswered that long
/// is (DEC-160 (13)); any other keeps its state.
pub(crate) fn overdue(batch: &mut Batch<'_, '_>, id: &ClientOrderId) -> Result<(), ExecutorError> {
    let Some(state) = batch.view.orders.get(id).map(|order| order.state) else {
        return Ok(());
    };
    let to = match state {
        OrderState::Submitting => OrderState::Unknown,
        kept => kept,
    };
    transition(batch, id, to, vec![("cancel_overdue", Value::Bool(true))])?;
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
/// not (DEC-160 3d), is cancelled by id (again on each ask) and the interval's start records the
/// sequence so a restart resumes it. In a handed-on sequence ([`passive_exit`]) the placement is
/// left resting while its stop and every exit still selling fit within the position ([`fits`]):
/// it was sized around them. An exit it was not sized around cancels it all, the passive exit's
/// own OCO too, and [`replace`] re-places what is left through the same interval (#286 round 2,
/// M1′); a passive exit starts the passive sequence instead. Any exit
/// first cancels its agent's own resting openings in the instrument (§5.3 rule 5). An add is no
/// sequence, and nothing is denied here (rule 13).
///
/// A passive exit where the resting protection has no take-profit — crypto's one stop-limit
/// (DEC-36) — answers slice 5's stub before anything is cancelled, so the stop keeps resting: it
/// never becomes an OCO in a crypto instrument (§5.4, simple orders only) and never leaves part
/// of the position unprotected (#286 round 1, M1), like [`replace`] and [`crypto_add`]. It is
/// reachable only where protection rests.
pub(crate) fn begin_exit(
    batch: &mut Batch<'_, '_>,
    intent: &IntentId,
) -> Result<(), ExecutorError> {
    begin(batch, intent, true)
}

/// [`begin_exit`], where `may_rest` false starts the marketable sequence whatever the exit's
/// limit: a parked ladder resuming at the open was marketable when it stepped (DEC-260 (18)).
fn begin(
    batch: &mut Batch<'_, '_>,
    intent: &IntentId,
    may_rest: bool,
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
        .filter(|protection| !protection.resting.is_empty() && exits(purpose))
        .cloned();
    let kept = resting
        .as_ref()
        .and_then(|protection| protection.prices)
        .filter(|_| may_rest && bid.is_some_and(|bid| limit > bid));
    if kept.is_some_and(|prices| prices.take_profit.is_none()) {
        return Err(ExecutorError::Unimplemented { story: "E7-4" });
    }
    if exits(purpose) {
        cancel_openings(batch, Some(&agent), Some(&instrument))?;
    }
    let Some(protection) = resting else {
        return Ok(());
    };
    let passive = kept.is_some();
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
    let sized_around = batch
        .view
        .exiting
        .get(&instrument)
        .is_some_and(|sequence| handed(&batch.view, sequence))
        && fits(&batch.view, &instrument)?;
    if sized_around {
        return Ok(());
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

/// §5.6 step 2 for an exit [`alone`] ladders: a rung's confirmed step cancel submits the next
/// rung for what it left unsold. The step was asked while the ladder climbed, and no protection
/// returns for the remainder, so a pause since then does not end it (DEC-260 (14)).
fn lone_steps(batch: &mut Batch<'_, '_>) -> Result<(), ExecutorError> {
    let ladders: Vec<(InstrumentId, LoneLadder)> = batch
        .view
        .ladders
        .iter()
        .map(|(instrument, lone)| (instrument.clone(), lone.clone()))
        .collect();
    for (instrument, lone) in ladders.into_iter().filter(|(_, lone)| lone.ladder.stepping) {
        let rung = ClientOrderId::for_intent(&lone.intent)?.rung(lone.ladder.rung)?;
        let left = match batch.view.orders.get(&rung) {
            Some(order) if order.state == OrderState::Canceled => {
                order.qty.checked_sub(order.filled_qty)?
            }
            _ => Qty::ZERO,
        };
        if left == Qty::ZERO {
            continue;
        }
        let paused = batch.view.effective_mode(&lone.agent) >= Mode::Paused;
        if let Some(reason) = closed_hold(batch.ports, &instrument, batch.at()) {
            park(batch, &lone.intent, reason, lone.ladder.parked)?;
        } else if lone.ladder.parked && paused {
            continue;
        } else if rests(&batch.view, &instrument) {
            begin(batch, &lone.intent, false)?;
        } else {
            let at = (&lone.intent, lone.ladder.rung);
            next_rung(batch, &instrument, at, left, true)?;
        }
    }
    Ok(())
}

/// The coordinator's ruling on #400 round 1, blocker 1 (DEC-260 (18)): a ladder step whose next
/// rung would fall while no v1 session is open submits nothing. The exit is journaled held, once,
/// `session_closed` (`session_unknown` where the calendar names no open), `parked` so the fold
/// neither gates it again nor resends its first rung; a sequence's protection is re-placed
/// meanwhile, and at the first tick of the next open the ladder resumes from the next rung, priced
/// fresh, its clock starting there ([`lone_steps`]). The park alerts the owner once, with its
/// record's id and its reason as the key, and is `held_long` from the start, since it lasts until
/// the next open: no second alert follows at `max_intent_age_s` (the ruling on #400 round 2,
/// DEC-260 (19); `AGENTS.md` rule 6).
fn park(
    batch: &mut Batch<'_, '_>,
    intent: &IntentId,
    reason: &'static str,
    parked: bool,
) -> Result<(), ExecutorError> {
    if parked {
        return Ok(());
    }
    let pairs = vec![
        ("intent_id", text(intent.0.0.clone())),
        ("verdict", text("hold")),
        ("reason_code", text(reason)),
        ("parked", Value::Bool(true)),
        ("held_long", Value::Bool(true)),
        ("evaluation", text("account_stream_only")),
    ];
    let decided = batch.journal("GateDecided", None, pairs)?;
    batch.notify(decided, reason);
    Ok(())
}

/// After every step (§5.4): the waiting exits are re-evaluated ([`release_waiting`]); then, in an
/// instrument where a sequence runs and no protective order rests any more, once the sequence's
/// exit is terminal, denied or abandoned **and** no other exit there is working or waiting, the
/// position is re-protected and the interval ends — so Σ resting sells never exceeds the position.
/// A passive sequence is ended the same way when its exit is denied or abandoned rather than
/// placed as the new OCO. While its exit still waits once nothing rests — on an opening, or held
/// at the gate — the passive sequence opens its interval, once, so `max_unprotected_s` bounds and
/// alerts it like any other ([`bound`]); [`passive_exit`]'s placement or [`replace`] ends it, and
/// the old OCO is never re-placed while the exit waits (#286 round 2, M2′; rule 3).
///
/// A passive exit placed while other exits in the instrument were still selling hands its
/// sequence on ([`passive_exit`]): the protection it placed rests by design, so the sequence
/// waits on it only once a later exit did not fit beside it ([`protection_holds`]), and on the
/// other exits, and [`replace`] then covers what they left unsold (#286 round 2, M1′).
pub(crate) fn settle(batch: &mut Batch<'_, '_>) -> Result<(), ExecutorError> {
    release_waiting(batch)?;
    lone_steps(batch)?;
    let sequences: Vec<(InstrumentId, ExitSequence)> = batch
        .view
        .exiting
        .iter()
        .map(|(instrument, sequence)| (instrument.clone(), sequence.clone()))
        .collect();
    for (instrument, sequence) in sequences {
        if protection_holds(&batch.view, &instrument) {
            continue;
        }
        let exit = ClientOrderId::for_intent(&sequence.intent)?.rung(sequence.ladder.rung)?;
        let finished = match batch.view.orders.get(&exit) {
            Some(order) if order.state == OrderState::Canceled && steps(&batch.view, &sequence) => {
                let left = order.qty.checked_sub(order.filled_qty)?;
                if left == Qty::ZERO {
                    true
                } else if let Some(reason) = closed_hold(batch.ports, &instrument, batch.at()) {
                    park(batch, &sequence.intent, reason, sequence.ladder.parked)?;
                    true
                } else {
                    let at = (&sequence.intent, sequence.ladder.rung);
                    next_rung(batch, &instrument, at, left, false)?;
                    continue;
                }
            }
            Some(order) => handed(&batch.view, &sequence) || order.state.is_terminal(),
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
        if sequence.passive && !finished {
            if !interval_open(&batch.view, &instrument) {
                changed(batch, &instrument, "unprotected_start", Vec::new())?;
            }
            continue;
        }
        let working = batch.view.orders.values().any(|order| {
            order.instrument == instrument && exits(order.purpose) && !order.state.is_terminal()
        });
        if finished && !working && waiting(&batch.view, &instrument).is_empty() {
            replace(batch, &instrument, &sequence)?;
        }
    }
    brackets(batch)
}

/// §5.4's bracket entries, after every step: each entry sent as a bracket that has filled anything
/// and is not yet protected ([`bracket`]).
fn brackets(batch: &mut Batch<'_, '_>) -> Result<(), ExecutorError> {
    let entries: Vec<(ClientOrderId, BracketLegs)> = batch
        .view
        .orders
        .values()
        .filter(|order| order.purpose.adds_risk())
        .filter_map(|order| {
            let detail = batch.view.details.get(&order.client_order_id)?;
            let legs = detail.request.as_ref()?.bracket.clone()?;
            (!detail.bracket_placed).then(|| (order.client_order_id.clone(), legs))
        })
        .collect();
    for (entry, legs) in entries {
        bracket(batch, &entry, &legs)?;
    }
    Ok(())
}

/// One bracket entry (§5.4, DEC-346). Its legs are held until it is completely filled, so the
/// unprotected interval starts, journaled once and naming the entry, at its first fill, by the
/// broker's own report or an ingested fill, whichever comes first. Complete, its legs are active at
/// the broker: they are recorded `placed` for the entry's whole quantity at its prices, named
/// `{entry}-p{record}` (§2.3), and the interval ends. Still working at
/// `bracket_partial_fill_timeout_s` from that start, or at `max_unprotected_s` if that bound is
/// shorter (§5.4's bound cancels the unfilled order), or once its venue is no longer the regular
/// session before the closing auction window, its remainder is cancelled, once. Terminal partly
/// filled — after that cancel or by any other path — it gets one GTC OCO at its prices for the
/// filled quantity, capped at what the position, as the broker reports it, leaves after every live
/// protective order and every exit still selling (rule 12: Σ protective sells never exceed the
/// position), and the interval ends; nothing left to cover places nothing and still ends it.
/// Unlike [`crate::intent::send`], the OCO's `OrderSubmitted` is not the effect immediately before
/// its request: the `placed` and `unprotected_end` records sit between them, so the interval's end
/// is journaled before the send. Rule 5 needs only that the draft comes first in the same list.
fn bracket(
    batch: &mut Batch<'_, '_>,
    entry: &ClientOrderId,
    legs: &BracketLegs,
) -> Result<(), ExecutorError> {
    let Some(order) = batch.view.orders.get(entry).cloned() else {
        return Ok(());
    };
    let detail = batch.view.details.get(entry).cloned().unwrap_or_default();
    let filled = order
        .filled_qty
        .max(detail.reported_filled.unwrap_or(Qty::ZERO));
    if filled == Qty::ZERO {
        return Ok(());
    }
    let prices = ProtectionPrices {
        stop: legs.stop,
        take_profit: Some(legs.take_profit),
    };
    let named = vec![("bracket", text(entry.as_str()))];
    let instrument = order.instrument.clone();
    if order.state == OrderState::Filled {
        let id = ClientOrderId::for_protection(entry, &batch.next_id())?;
        let mut placed = recorded_placement(&id, order.qty, prices);
        placed.extend(named.clone());
        changed(batch, &instrument, "placed", placed)?;
        if detail.bracket_since.is_some() {
            changed(batch, &instrument, "unprotected_end", named)?;
        }
        return Ok(());
    }
    if detail.bracket_since.is_none() {
        changed(batch, &instrument, "unprotected_start", named.clone())?;
    }
    if order.state.is_terminal() {
        let unapplied = filled.checked_sub(order.filled_qty)?;
        let committed = covered(&batch.view, &instrument)?
            .checked_add(still_selling(&batch.view, &instrument)?)?;
        let room = long(&batch.view, &instrument)
            .checked_add(unapplied)?
            .checked_sub(committed)
            .unwrap_or(Qty::ZERO);
        let qty = filled.min(room);
        let mut request = None;
        if qty > Qty::ZERO {
            let id = ClientOrderId::for_protection(entry, &batch.id_after(1))?;
            let oco = oco(
                id,
                &instrument,
                OcoLegs {
                    take_profit: legs.take_profit,
                    stop: legs.stop,
                    qty,
                },
            );
            let agent = order
                .agent
                .clone()
                .unwrap_or_else(|| AgentId(EVERY_AGENT.to_owned()));
            journal_submission(batch, &oco, None, &agent, 1)?;
            let mut placed = recorded_placement(&oco.client_order_id, qty, prices);
            placed.extend(named.clone());
            changed(batch, &instrument, "placed", placed)?;
            request = Some(oco);
        }
        changed(batch, &instrument, "unprotected_end", named)?;
        if let Some(request) = request {
            batch.broker(BrokerRequest::Submit(request));
        }
        return Ok(());
    }
    let since = batch
        .view
        .details
        .get(entry)
        .and_then(|detail| detail.bracket_since)
        .unwrap_or(batch.at());
    let config = batch.ports.config;
    let bound = config
        .bracket_partial_fill_timeout_s
        .min(config.max_unprotected_s);
    let due = batch.at().secs().saturating_sub(since.secs()) >= bound
        || venue(batch.ports, &instrument, batch.at()) != Venue::Regular;
    let working = matches!(
        order.state,
        OrderState::Accepted | OrderState::PartiallyFilled
    );
    if due && working && ask_cancel(batch, entry)? {
        batch.broker(BrokerRequest::Cancel {
            client_order_id: entry.clone(),
        });
    }
    Ok(())
}

/// A `placed` record's fields for one protective order covering `qty` at `prices`.
fn recorded_placement(
    id: &ClientOrderId,
    qty: Qty,
    prices: ProtectionPrices,
) -> Vec<(&'static str, Value)> {
    let mut placed = vec![
        ("orders", text(id.as_str())),
        ("qty", text(qty.to_string())),
        ("stop", text(prices.stop.to_string())),
    ];
    if let Some(take_profit) = prices.take_profit {
        placed.push(("take_profit", text(take_profit.to_string())));
    }
    placed
}

/// Rule 5 and §5.4, after every step — every tick and every order-state change: each exit waiting
/// on its cancels asks the cancel of any opening of its agent in the instrument the broker has
/// since accepted (a `Submitting` one is cancelled once it is accepted), and each exit nothing
/// holds back any more ([`awaits_cancel`]) is re-gated on fresh state and submitted, whatever
/// ended its wait: a confirmed cancel, a fill, an expiry, a reject, the broker reporting the
/// opening gone by any path, or the bound marking it overdue (#286 round 1, B1; DEC-160 (18)).
pub(crate) fn release_waiting(batch: &mut Batch<'_, '_>) -> Result<(), ExecutorError> {
    for exit in waiting_exits(&batch.view) {
        cancel_openings(batch, Some(&exit.agent), Some(&exit.instrument))?;
        if !awaits_cancel(&batch.view, &exit.agent, &exit.instrument, exit.purpose) {
            gate_and_submit(batch, &exit.intent)?;
        }
    }
    Ok(())
}

/// One exit the gate allowed that has no order yet: it waits on its cancels.
struct WaitingExit {
    intent: IntentId,
    agent: AgentId,
    instrument: InstrumentId,
    purpose: Purpose,
    since: RiskClock,
}

/// The exits waiting on their cancels: received, allowed by the gate at `since` and not held
/// since, with no order yet. A held exit has not started its wait (DEC-160 (8)), and an intent
/// the gate has not yet decided is [`crate::intent::resume`]'s.
fn waiting_exits(view: &ExecutorState) -> Vec<WaitingExit> {
    view.intents
        .values()
        .filter(|record| record.outcome == IntentOutcome::Received)
        .filter(|record| !view.held.contains(&record.intent_id))
        .filter(|record| {
            ClientOrderId::for_intent(&record.intent_id)
                .is_ok_and(|id| !view.orders.contains_key(&id))
        })
        .filter_map(|record| match view.bodies.get(&record.intent_id) {
            Some(IntentBody::Order {
                instrument,
                purpose,
                ..
            }) if exits(*purpose) => Some(WaitingExit {
                intent: record.intent_id.clone(),
                agent: record.agent.clone(),
                instrument: instrument.clone(),
                purpose: *purpose,
                since: record.allowed_at?,
            }),
            _ => None,
        })
        .collect()
}

/// Whether the sequence's exit has been placed as the take-profit leg of its own protective OCO:
/// a passive exit's sequence handed on to the exits still selling beside it ([`passive_exit`]).
/// A marketable sequence's exit is never protective.
fn handed(view: &ExecutorState, sequence: &ExitSequence) -> bool {
    ClientOrderId::for_intent(&sequence.intent).is_ok_and(|id| {
        view.orders
            .get(&id)
            .is_some_and(|order| order.purpose == Purpose::Protective)
    })
}

/// What the other exits in `instrument` may still sell: every live non-protective sell's unfilled
/// quantity — the set `gate::available` walks, and one back in `Intent` for its resubmission — and
/// every exit received that has no order yet. Stop coverage beyond the position less this would
/// outlast the position once they fill (#286 round 2, M1′).
fn still_selling(view: &ExecutorState, instrument: &InstrumentId) -> Result<Qty, ExecutorError> {
    let live = view
        .orders
        .values()
        .filter(|order| {
            &order.instrument == instrument
                && order.side == Side::Sell
                && order.purpose != Purpose::Protective
                && !order.state.is_terminal()
        })
        .try_fold(Qty::ZERO, |total, order| {
            total.checked_add(order.qty.checked_sub(order.filled_qty).unwrap_or(Qty::ZERO))
        })?;
    waiting(view, instrument)
        .iter()
        .filter_map(|intent| match view.bodies.get(intent) {
            Some(IntentBody::Order { qty, .. }) => Some(*qty),
            _ => None,
        })
        .try_fold(live, Qty::checked_add)
        .map_err(ExecutorError::from)
}

/// Whether what protective orders cover in `instrument` and what its exits may still sell fit
/// within the long position together, so no stop outlasts the position once they fill.
fn fits(view: &ExecutorState, instrument: &InstrumentId) -> Result<bool, ExecutorError> {
    let committed = covered(view, instrument)?.checked_add(still_selling(view, instrument)?)?;
    Ok(committed <= long(view, instrument))
}

fn long(view: &ExecutorState, instrument: &InstrumentId) -> Qty {
    view.positions
        .get(instrument)
        .copied()
        .unwrap_or(SignedQty::ZERO)
        .max(SignedQty::ZERO)
        .abs()
}

/// The unfilled quantity every live protective order in `instrument` covers.
fn covered(view: &ExecutorState, instrument: &InstrumentId) -> Result<Qty, ExecutorError> {
    view.orders
        .values()
        .filter(|order| {
            &order.instrument == instrument
                && order.purpose == Purpose::Protective
                && !order.state.is_terminal()
        })
        .try_fold(Qty::ZERO, |total, order| {
            total.checked_add(order.qty.checked_sub(order.filled_qty).unwrap_or(Qty::ZERO))
        })
        .map_err(ExecutorError::from)
}

fn interval_open(view: &ExecutorState, instrument: &InstrumentId) -> bool {
    view.unprotected
        .iter()
        .any(|interval| &interval.instrument == instrument && interval.ended_at.is_none())
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

/// Re-places the shape the sequence cancelled (§5.4) for the held quantity less what protective
/// orders still cover: all of it after a marketable sequence, whose cancels are confirmed by then,
/// and what the exits beside a handed-on passive exit left unsold (#286 round 2, M1′). The shape
/// is an OCO at its prices, or, with no take-profit, crypto's one GTC stop-limit at stop × (1 −
/// the mandate's `crypto_stop_limit_offset`) (DEC-36); nothing left to cover places nothing. The
/// interval ends either way. An equity stop-only placement, or a crypto one with no offset, has no shape to
/// re-place and answers its stub (slices 2 and 5; unreachable before them, DEC-160 (2)). That
/// refusal is per-sequence and permanent by design: every input that would end this sequence is
/// refused, while ticks, the bound's alert and other instruments go on.
fn replace(
    batch: &mut Batch<'_, '_>,
    instrument: &InstrumentId,
    sequence: &ExitSequence,
) -> Result<(), ExecutorError> {
    let qty = long(&batch.view, instrument)
        .checked_sub(covered(&batch.view, instrument)?)
        .unwrap_or(Qty::ZERO);
    let prices = sequence.prices.filter(|_| qty > Qty::ZERO);
    let Some(prices) = prices else {
        changed(batch, instrument, "unprotected_end", Vec::new())?;
        return Ok(());
    };
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
    let placed = recorded_placement(&request.client_order_id, request.qty, prices);
    changed(batch, &request.instrument, "placed", placed).map(|_| ())
}

/// §5.4's passive exit, once its cancel is confirmed: the exit rests as the take-profit leg of a
/// new GTC OCO for its own quantity, keeping the stop; the rest of the position keeps protection at
/// the recorded prices, so nothing sells beyond the intent and the whole position keeps its stop
/// (#174 ruling (c), 5862934909; rule 3), and the interval its wait opened, if any, ends ([`settle`]).
///
/// The rest is the position less the exit and less what every other exit in the instrument may
/// still sell ([`still_selling`]), so the stop never covers more than the position once they fill
/// (#286 round 2, M1′). The passive exit is never refused for them (rule 13): where they may sell
/// anything, its sequence is handed on to them — a new interval, bounded like any other — and
/// [`replace`] covers what they leave unsold once they end. An exit the placement was not sized
/// around cancels it like any protection ([`begin_exit`]), and [`replace`] re-places what is left
/// at the smaller size through the same interval. Answers whether the intent was this passive
/// sequence's.
/// A passive sequence on the journal with no take-profit, which [`begin_exit`] never starts,
/// answers slice 5's stub rather than an OCO without one or a remainder left unprotected (#286
/// round 1, M1).
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
    let Some(take_profit) = prices.take_profit else {
        return Err(ExecutorError::Unimplemented { story: "E7-4" });
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
    let selling = still_selling(&batch.view, instrument)?;
    let rest = held
        .abs()
        .checked_sub(qty)
        .and_then(|rest| rest.checked_sub(selling))
        .unwrap_or(Qty::ZERO);
    let mut requests = vec![first];
    if rest > Qty::ZERO {
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
    if interval_open(&batch.view, instrument) {
        changed(batch, instrument, "unprotected_end", Vec::new())?;
    }
    if selling > Qty::ZERO {
        let pairs = recorded(intent, &sequence.entry, &sequence.agent, prices);
        changed(batch, instrument, "unprotected_start", pairs)?;
    }
    for request in requests {
        batch.broker(BrokerRequest::Submit(request));
    }
    Ok(true)
}

/// The fields a handed-on sequence's start records so a restart resumes it (journal spec §9),
/// as [`begin_exit`]'s do.
fn recorded(
    intent: &IntentId,
    entry: &ClientOrderId,
    agent: &AgentId,
    prices: ProtectionPrices,
) -> Vec<(&'static str, Value)> {
    let mut pairs = vec![
        ("intent_id", text(intent.0.0.clone())),
        ("entry", text(entry.as_str())),
        ("agent", text(agent.0.clone())),
        ("stop", text(prices.stop.to_string())),
    ];
    if let Some(take_profit) = prices.take_profit {
        pairs.push(("take_profit", text(take_profit.to_string())));
    }
    pairs
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

/// §5.4's bound: at `max_unprotected_s` the sequence's exits are ended ([`end_exits`]) — their
/// confirmations re-place protection through [`settle`] — and the owner alerted once.
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
        if batch.view.exiting.contains_key(&instrument) {
            end_exits(batch, &instrument)?;
        }
        let alerted = changed(batch, &instrument, "interval_limit", Vec::new())?;
        batch.notify(alerted, "unprotected_interval_limit");
    }
    Ok(())
}

/// The bound's end of every exit in `instrument`, since [`settle`] re-places protection only once
/// none is working or waiting: each working one is cancelled, and each with no order yet —
/// waiting on an opening, or held at the gate — has nothing to cancel at the broker and is
/// abandoned, so protection returns in the same step rather than after the hold (#286 round 2,
/// M2′; rule 3). A handed-on passive exit ([`passive_exit`]) rests as protection and is left
/// alone (M1′). An order back in `Intent` has nothing at the broker to cancel and ends with its
/// resubmission.
fn end_exits(batch: &mut Batch<'_, '_>, instrument: &InstrumentId) -> Result<(), ExecutorError> {
    let working: Vec<ClientOrderId> = batch
        .view
        .orders
        .values()
        .filter(|order| {
            &order.instrument == instrument
                && exits(order.purpose)
                && !order.state.is_terminal()
                && order.state != OrderState::Intent
        })
        .map(|order| order.client_order_id.clone())
        .collect();
    for id in working {
        if ask_cancel(batch, &id)? {
            batch.broker(BrokerRequest::Cancel {
                client_order_id: id,
            });
        }
    }
    for intent in waiting(&batch.view, instrument) {
        abandon(batch, &intent, "unprotected_interval_limit")?;
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
/// within five minutes; failing that the last sane trade within five minutes (DEC-260 (5)). The
/// limit is reference × (1 − offset), rounded per §2.1. A step happens only after `exit_step_s`
/// has elapsed, with the offset raised by `exit_offset_step` and repriced from the **current**
/// reference, and the offset never exceeds `max_exit_offset`.
///
/// An owner exit outside the regular session prices from the bid the owner confirmed and never
/// below the floor that `OwnerExitRequested` carries (§5.5, mandate spec §6.1).
///
/// `observations` run oldest to newest. "Fresh" is the newest observation, sane; "within five
/// minutes" is [`SANE_BID_WINDOW_S`] of `now`, for a bid and a trade alike: an insane print or an
/// old one prices nothing. With no such bid and no such trade there is no rung, and the refusal
/// says so rather than guessing a price (rule 3).
pub(crate) fn ladder_price(
    tier: ExitTier,
    observations: &[MarketObservation],
    step: u32,
    now: RiskClock,
    floor: Option<Price>,
) -> Result<LadderPrice, ExecutorError> {
    let recent = |quote: &&MarketObservation| {
        quote.sane && now.secs().saturating_sub(quote.observed_at.secs()) <= SANE_BID_WINDOW_S
    };
    let newest = observations.last();
    let (bid, reference) = match (
        newest.filter(recent).and_then(|quote| quote.bid),
        observations
            .iter()
            .rev()
            .filter(recent)
            .find_map(|quote| quote.bid),
        observations
            .iter()
            .rev()
            .filter(recent)
            .find_map(|quote| quote.last_trade),
    ) {
        (Some(bid), _, _) => (bid, LadderReference::FreshQuote),
        (None, Some(bid), _) => (bid, LadderReference::LastSaneBid),
        (None, None, Some(trade)) => (trade, LadderReference::LastTrade),
        (None, None, None) => {
            return Err(ExecutorError::NotInterpreted {
                what: "an exit with no bid and no trade to price from".to_owned(),
                story: "E7-4",
            });
        }
    };
    let offset = tier
        .exit_offset
        .stepped(tier.exit_offset_step, step, tier.max_exit_offset)?;
    let priced = bid.collar_bound(offset, Adverse::Down)?;
    Ok(LadderPrice {
        limit: floor.map_or(priced, |floor| priced.max(floor)),
        reference,
        step,
        at_floor: offset == tier.max_exit_offset,
    })
}

/// §5.6's "the last sane bid within the last 5 minutes".
const SANE_BID_WINDOW_S: i64 = 300;

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
    use std::path::Path;

    use mandate_accounting::{InstrumentId, Side};
    use mandate_num::{Price, Qty};

    use super::{awaits_cancel, breach};
    use crate::error::ExecutorError;
    use crate::ids::{ClientOrderId, IntentId};
    use crate::state::{ExecutorState, ExitSequence};
    use crate::types::{
        AccountRef, AccountScope, AgentId, EventId, MarketObservation, Order, OrderState,
        Protection, ProtectionPrices, Purpose, RiskClock, WorkspaceId,
    };

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
                    ladder: crate::state::Ladder::default(),
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

    /// The watchdog's clock (§5.4): only a sane mark at or below the stop of protection that rests
    /// starts a breach, which keeps its start while it runs; a sane mark above the stop ends it,
    /// and an insane quote, a quote with no mark, a quote elsewhere or a stop with no price
    /// changes nothing.
    #[test]
    fn only_a_sane_mark_at_or_below_a_resting_stop_starts_a_breach() -> Result<(), ExecutorError> {
        let (mut state, aapl) = protected()?;
        let quote = |instrument: &InstrumentId, mark: Option<&str>, sane: bool, at: i64| {
            Ok::<_, ExecutorError>(MarketObservation {
                instrument: instrument.clone(),
                bid: None,
                bid_size: None,
                ask: None,
                last_trade: None,
                mark: mark.map(price).transpose()?,
                sane,
                observed_at: RiskClock::from_secs(at),
            })
        };
        let msft = InstrumentId::new("MSFT")?;
        let started = Some(RiskClock::from_secs(10));
        for (instrument, mark, sane, at, expected) in [
            (&aapl, Some("140.01"), true, 5, None),
            (&aapl, Some("140"), true, 10, started),
            (&aapl, Some("139.99"), true, 11, started),
            (&aapl, Some("150"), false, 12, started),
            (&aapl, None, true, 13, started),
            (&msft, Some("120"), true, 14, started),
            (&aapl, Some("140.01"), true, 15, None),
            (&aapl, Some("120"), false, 16, None),
        ] {
            breach(&mut state, &quote(instrument, mark, sane, at)?);
            assert_eq!(
                state.breaches.get(&aapl).copied(),
                expected,
                "{instrument:?} at {mark:?}, sane {sane}, at {at}"
            );
        }
        assert!(
            !state.breaches.contains_key(&msft),
            "no protection rests there"
        );
        if let Some(protection) = state.protection.get_mut(&aapl) {
            protection.prices = None;
        }
        breach(&mut state, &quote(&aapl, Some("120"), true, 17)?);
        assert!(
            state.breaches.is_empty(),
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
    use std::collections::{BTreeMap, VecDeque};

    use mandate_accounting::{AssetClass, InstrumentId, Side};
    use mandate_canon::Value;
    use mandate_num::{Fraction, Price, Qty, ShareIncrement, SignedQty, Usd};
    use mandate_time::Date;
    use proptest::prelude::{Just, ProptestConfig, Strategy, TestCaseError, any, prop};
    use proptest::prop_oneof;
    use proptest::test_runner::TestRunner;

    use super::rests;
    use crate::error::ExecutorError;
    use crate::fold::fold;
    use crate::ids::{ClientOrderId, IntentId};
    use crate::payload::{clock, object, text};
    use crate::ports::{InstrumentSnapshot, MandateView, Ports};
    use crate::reconcile::tests::{
        Everything, Executor, Ids, aapl, account, drafted, executor_config, fees, missing,
        submitted,
    };
    use crate::state::ExecutorState;
    use crate::types::{
        AgentId, BrokerFill, BrokerOrder, BrokerOutcome, BrokerReject, BrokerRequest,
        BrokerUnknown, BrokerUpdate, Effect, EventDraft, EventId, ExecutorConfig, ExitTier, FillId,
        Input, IntentBody, IntentHandoff, MandateVersion, MarketObservation, Mode, OcoLegs,
        OrderState, OrderType, Purpose, ReconcileReason, RiskClock, SubmitOrder, TimeInForce,
    };

    const OCO: &str = "md-held-1-p1";
    const EXIT: &str = "01JABCDEFGHJKMNPQRSTVWXYZ9";
    const SECOND: &str = "01JABCDEFGHJKMNPQRSTVWXYZ8";
    /// An intent that sorts after [`EXIT`], where [`SECOND`] sorts before it, so a step that
    /// releases both takes the passive exit first or second.
    const LATER: &str = "01JABCDEFGHJKMNPQRSTVWXZZ0";

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
        sell_as("agent-a", intent, qty, limit, purpose)
    }

    fn sell_as(
        agent: &str,
        intent: &str,
        qty: &str,
        limit: &str,
        purpose: Purpose,
    ) -> Result<Input, ExecutorError> {
        Ok(Input::Intent(IntentHandoff {
            intent_id: IntentId(EventId(intent.to_owned())),
            agent: AgentId(agent.to_owned()),
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

    /// #174 ruling 5862180909, carried into slice 4b: a breaching quote (a sane mark at or below
    /// the resting stop) only starts the watchdog's clock. It leaves the state as it found it but
    /// for the latest quote, the last sane bid and trade, and the breach, and the exit, the
    /// cancel's confirmation and the reconciliation after it each run in full, with another such
    /// quote arriving between them.
    #[test]
    fn a_breach_never_blocks_an_exit_a_cancel_or_a_reconciliation() -> Result<(), ExecutorError> {
        with_ports!(ports);
        let mut executor = protected(&ports)?;
        let before = executor.state.clone();
        assert_eq!(executor.run(quote("139")?, &ports)?, Vec::new());
        let mut kept = executor.state.clone();
        assert_eq!(
            kept.quotes.get(&aapl()?).and_then(|quote| quote.bid),
            Some(Price::parse("139")?),
            "the breaching quote is the latest observation"
        );
        assert_eq!(
            kept.sane_bids.get(&aapl()?).and_then(|quote| quote.bid),
            Some(Price::parse("139")?),
            "and the last sane bid"
        );
        assert_eq!(
            kept.breaches.get(&aapl()?),
            Some(&RiskClock::from_secs(0)),
            "and starts the watchdog's clock"
        );
        kept.quotes = before.quotes.clone();
        kept.sane_bids = before.sane_bids.clone();
        kept.trades = before.trades.clone();
        kept.breaches = before.breaches.clone();
        assert_eq!(kept, before, "and nothing else changed");

        let started = executor.run(sell(EXIT, "5", "139", Purpose::RiskExit)?, &ports)?;
        assert_eq!(
            cancels(&started),
            vec![OCO],
            "the exit cancels the protection"
        );
        assert_eq!(actions(&started), vec!["unprotected_start"]);
        assert_eq!(submitted(&started), 0, "and waits for the confirmation");

        executor.run(quote("138")?, &ports)?;
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

    /// The coordinator's ruling D2 (rule 13, DEC-160 (12)): an exit that grew older than
    /// `max_intent_age_s` while its cancel was confirmed is never abandoned; it goes once the
    /// cancel is confirmed. The interval's own bound is set past the exit's age here, so that the
    /// age alone is tested.
    #[test]
    fn an_old_exit_still_goes_once_its_cancel_is_confirmed() -> Result<(), ExecutorError> {
        let (config, fees) = (
            ExecutorConfig {
                max_unprotected_s: 600,
                ..executor_config()
            },
            fees()?,
        );
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = protected(&ports)?;
        executor.run(sell(EXIT, "5", "139", Purpose::RiskExit)?, &ports)?;
        executor.run(Input::Tick(RiskClock::from_secs(500)), &ports)?;
        let settled = executor.run(cancel_accepted(OCO), &ports)?;
        assert!(!drafted(&settled).contains(&"OrderAbandoned"));
        assert_eq!(
            submissions(&settled)
                .iter()
                .map(|order| (order.client_order_id.as_str().to_owned(), order.qty))
                .collect::<Vec<_>>(),
            vec![(format!("md-{EXIT}"), Qty::parse("5")?)]
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

    /// #174 ruling 5862579929, (A)'s rule-13 condition: only an exit a sequence runs for, where
    /// protection rested, is laddered (§5.6). An exit of an **unprotected** position, with an exit
    /// tier and a fresh quote on hand, is submitted at once at its own limit; the same exit of the
    /// protected position is submitted at the ladder's first rung, 150 × (1 − 0.5%), once its
    /// cancel is confirmed.
    #[test]
    fn only_the_sequences_exit_is_laddered() -> Result<(), ExecutorError> {
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
            let laddered = executor.run(cancel_accepted(OCO), &ports)?;
            assert_eq!(
                submissions(&laddered)
                    .iter()
                    .map(|order| order.limit_price)
                    .collect::<Vec<_>>(),
                vec![Some(Price::parse("149.25")?)],
                "{purpose:?}: the sequence's exit is the ladder's"
            );
        }
        Ok(())
    }

    /// §5.6's second reference: a newer quote that is not sane, with no trade on it, leaves the
    /// last sane bid within five minutes to price from, so the executor keeps it past the newer
    /// quote rather than refusing the exit.
    #[test]
    fn the_sequences_exit_prices_from_the_last_sane_bid() -> Result<(), ExecutorError> {
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
        executor.run(
            Input::Market(MarketObservation {
                instrument: aapl()?,
                bid: Some(Price::parse("120")?),
                bid_size: None,
                ask: None,
                last_trade: None,
                mark: None,
                sane: false,
                observed_at: RiskClock::from_secs(0),
            }),
            &ports,
        )?;
        executor.run(sell(EXIT, "5", "150", Purpose::RiskExit)?, &ports)?;
        let laddered = executor.run(cancel_accepted(OCO), &ports)?;
        assert_eq!(
            submissions(&laddered)
                .iter()
                .map(|order| order.limit_price)
                .collect::<Vec<_>>(),
            vec![Some(Price::parse("149.25")?)]
        );
        Ok(())
    }

    fn tiered_ports<'p>(
        config: &'p crate::types::ExecutorConfig,
        fees: &'p mandate_accounting::Config,
    ) -> Ports<'p> {
        Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Tiered,
            config,
            fees,
        }
    }

    /// A sane quote with no bid and no trade: nothing §5.6 can price from.
    fn unpriceable() -> Result<Input, ExecutorError> {
        Ok(Input::Market(MarketObservation {
            instrument: aapl()?,
            bid: None,
            bid_size: None,
            ask: None,
            last_trade: None,
            mark: None,
            sane: true,
            observed_at: RiskClock::from_secs(0),
        }))
    }

    fn alerts(effects: &[Effect]) -> Vec<&str> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Notify(note) => Some(note.message_key),
                _ => None,
            })
            .collect()
    }

    fn verdicts_with_reasons(effects: &[Effect]) -> Vec<(String, String)> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Journal(draft) if draft.event_type == "GateDecided" => Some((
                    draft
                        .payload
                        .get("verdict")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned(),
                    draft
                        .payload
                        .get("reason_code")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned(),
                )),
                _ => None,
            })
            .collect()
    }

    /// DEC-160 (12), §5.6 step 4: a risk exit, an owner exit or a flatten with nothing to price
    /// from goes at its intent's own limit, never refused, the fallback journaled with an owner
    /// alert.
    #[test]
    fn an_exit_with_nothing_to_price_from_goes_at_its_own_limit() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        for purpose in [Purpose::RiskExit, Purpose::OwnerExit, Purpose::Flatten] {
            let mut executor = protected(&ports)?;
            executor.run(unpriceable()?, &ports)?;
            let started = executor.run(sell(EXIT, "5", "150", purpose)?, &ports)?;
            assert_eq!(
                cancels(&started),
                vec![OCO],
                "{purpose:?}: the sequence starts"
            );
            let sent = executor.run(cancel_accepted(OCO), &ports)?;
            assert_eq!(
                submissions(&sent)
                    .iter()
                    .map(|order| order.limit_price)
                    .collect::<Vec<_>>(),
                vec![Some(Price::parse("150")?)],
                "{purpose:?}: at its own limit"
            );
            assert!(
                actions(&sent).contains(&"exit_unpriced"),
                "{purpose:?}: the fallback is journaled"
            );
            assert_eq!(
                alerts(&sent),
                vec!["exit_unpriced"],
                "{purpose:?}: and alerted"
            );
        }
        Ok(())
    }

    /// DEC-160 (12): a discretionary exit with nothing to price from is held with its protection
    /// resting, the hold journaled and alerted once; each tick re-evaluates it silently, and the
    /// first tick after a priceable quote releases it into the sequence, laddered.
    #[test]
    fn a_discretionary_exit_with_nothing_to_price_from_is_held_then_goes()
    -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let mut executor = protected(&ports)?;
        executor.run(unpriceable()?, &ports)?;
        let held = executor.run(sell(EXIT, "5", "150", Purpose::DiscretionaryExit)?, &ports)?;
        assert!(cancels(&held).is_empty(), "protection stays resting");
        assert_eq!(
            verdicts_with_reasons(&held),
            vec![("hold".to_owned(), "exit_unpriced".to_owned())]
        );
        assert_eq!(alerts(&held), vec!["exit_unpriced"]);
        let quiet = executor.run(Input::Tick(RiskClock::from_secs(1)), &ports)?;
        assert!(
            quiet.is_empty(),
            "re-evaluated, still unpriced, journaled once: {quiet:?}"
        );
        executor.run(quote("150")?, &ports)?;
        let released = executor.run(Input::Tick(RiskClock::from_secs(2)), &ports)?;
        assert_eq!(cancels(&released), vec![OCO], "released into the sequence");
        assert_eq!(
            verdicts_with_reasons(&released),
            vec![("allow".to_owned(), String::new())]
        );
        let sent = executor.run(cancel_accepted(OCO), &ports)?;
        assert_eq!(
            submissions(&sent)
                .iter()
                .map(|order| order.limit_price)
                .collect::<Vec<_>>(),
            vec![Some(Price::parse("149.25")?)]
        );
        Ok(())
    }

    /// A sane quote with a bid and no trade, observed at `at`.
    fn bid_only(bid: &str, at: i64) -> Result<Input, ExecutorError> {
        Ok(Input::Market(MarketObservation {
            instrument: aapl()?,
            bid: Some(Price::parse(bid)?),
            bid_size: Some(Qty::parse("100")?),
            ask: None,
            last_trade: None,
            mark: None,
            sane: true,
            observed_at: RiskClock::from_secs(at),
        }))
    }

    /// Rule 3, DEC-160 (12): a discretionary exit whose price vanishes after its protection was
    /// cancelled never leaves the position unprotected until the interval's bound. The sequence
    /// ends and protection is re-placed at once for the held quantity, the exit is held
    /// `exit_unpriced` and alerted, and a priceable quote releases it into a fresh sequence.
    #[test]
    fn an_exit_whose_price_vanishes_mid_sequence_re_protects_at_once() -> Result<(), ExecutorError>
    {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let mut executor = protected(&ports)?;
        executor.run(bid_only("150", 0)?, &ports)?;
        executor.run(Input::Tick(RiskClock::from_secs(280)), &ports)?;
        let started = executor.run(sell(EXIT, "5", "150", Purpose::DiscretionaryExit)?, &ports)?;
        assert_eq!(
            cancels(&started),
            vec![OCO],
            "priced from the last sane bid, it starts"
        );
        executor.run(Input::Tick(RiskClock::from_secs(301)), &ports)?;

        let vanished = executor.run(cancel_accepted(OCO), &ports)?;
        assert_eq!(
            verdicts_with_reasons(&vanished),
            vec![("hold".to_owned(), "exit_unpriced".to_owned())],
            "the bid is now older than five minutes and there is no trade"
        );
        assert_eq!(alerts(&vanished), vec!["exit_unpriced"]);
        let replaced = submissions(&vanished);
        assert_eq!(
            replaced
                .iter()
                .map(|order| (order.purpose, order.oco.as_ref().map(|legs| legs.qty)))
                .collect::<Vec<_>>(),
            vec![(Purpose::Protective, Some(Qty::parse("10")?))],
            "protection is re-placed at once for the whole held quantity, and the exit is not sent"
        );
        assert!(actions(&vanished).contains(&"unprotected_end"));
        assert!(executor.state.exiting.is_empty(), "the sequence has ended");
        let new_oco = replaced
            .first()
            .map(|order| order.client_order_id.as_str().to_owned())
            .unwrap_or_default();

        executor.run(bid_only("150", 302)?, &ports)?;
        let released = executor.run(Input::Tick(RiskClock::from_secs(302)), &ports)?;
        assert_eq!(
            cancels(&released),
            vec![new_oco.as_str()],
            "released into a fresh sequence"
        );
        let sent = executor.run(cancel_accepted(&new_oco), &ports)?;
        assert_eq!(
            submissions(&sent)
                .iter()
                .map(|order| order.limit_price)
                .collect::<Vec<_>>(),
            vec![Some(Price::parse("149.25")?)]
        );
        Ok(())
    }

    /// The first rung of a risk exit of 5 AAPL, bid 150 at 0: 149.25 under the intent's own id.
    fn first_rung(executor: &mut Executor, ports: &Ports<'_>) -> Result<(), ExecutorError> {
        executor.run(quote("150")?, ports)?;
        executor.run(sell(EXIT, "5", "150", Purpose::RiskExit)?, ports)?;
        let sent = executor.run(cancel_accepted(OCO), ports)?;
        assert_eq!(
            submissions(&sent)
                .iter()
                .map(|order| (order.client_order_id.as_str(), order.limit_price))
                .collect::<Vec<_>>(),
            vec![(format!("md-{EXIT}").as_str(), Some(Price::parse("149.25")?))]
        );
        Ok(())
    }

    fn rung_id(step: u32) -> String {
        match step {
            0 => format!("md-{EXIT}"),
            step => format!("md-{EXIT}-l{step}"),
        }
    }

    /// §5.6 step 2, §2.3 (DEC-160 (10)): an unfilled rung is cancelled only once `exit_step_s`
    /// has passed, and only the confirmation releases the next rung: a new order under
    /// `-l1`, one offset step lower, for what is left to sell.
    #[test]
    fn a_rung_steps_after_exit_step_s_under_a_new_id() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let mut executor = protected(&ports)?;
        first_rung(&mut executor, &ports)?;
        let early = executor.run(Input::Tick(RiskClock::from_secs(9)), &ports)?;
        assert!(cancels(&early).is_empty() && submissions(&early).is_empty());
        let stepped = executor.run(Input::Tick(RiskClock::from_secs(10)), &ports)?;
        assert_eq!(cancels(&stepped), vec![rung_id(0).as_str()]);
        assert!(
            submissions(&stepped).is_empty(),
            "the next rung waits for the confirmation"
        );
        let again = executor.run(Input::Tick(RiskClock::from_secs(11)), &ports)?;
        assert!(cancels(&again).is_empty(), "one cancel per rung");
        let next = executor.run(cancel_accepted(&rung_id(0)), &ports)?;
        assert_eq!(
            submissions(&next)
                .iter()
                .map(|order| (
                    order.client_order_id.as_str(),
                    order.qty,
                    order.limit_price,
                    order.purpose
                ))
                .collect::<Vec<_>>(),
            vec![(
                rung_id(1).as_str(),
                Qty::parse("5")?,
                Some(Price::parse("148.5")?),
                Purpose::RiskExit
            )]
        );
        assert!(
            actions(&next).is_empty(),
            "a step is not the sequence's end: nothing is re-placed"
        );
        Ok(())
    }

    /// §5.6: a rung the broker partly filled steps for the remainder only.
    #[test]
    fn a_partly_filled_rung_steps_for_the_remainder() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let mut executor = protected(&ports)?;
        first_rung(&mut executor, &ports)?;
        executor.run(filled(EXIT, "2")?, &ports)?;
        executor.run(Input::Tick(RiskClock::from_secs(10)), &ports)?;
        let next = executor.run(cancel_accepted(&rung_id(0)), &ports)?;
        assert_eq!(
            submissions(&next)
                .iter()
                .map(|order| (order.client_order_id.as_str(), order.qty))
                .collect::<Vec<_>>(),
            vec![(rung_id(1).as_str(), Qty::parse("3")?)]
        );
        Ok(())
    }

    /// The later-rung reading (#174, comment 5864659765 item 2; DEC-160 (23)): once an exit is
    /// working, a later rung with nothing to price from goes at the intent's own limit for every
    /// purpose, a discretionary exit's included, journaled `exit_unpriced` with an owner alert. It
    /// is never held: no gate verdict is drafted for it. The bid that priced the first rung at 290
    /// is more than five minutes old when the step's cancel is confirmed at 301, and there is no
    /// trade.
    #[test]
    fn a_later_rung_with_nothing_to_price_from_goes_at_its_own_limit() -> Result<(), ExecutorError>
    {
        let fees = fees()?;
        let config = ExecutorConfig {
            max_unprotected_s: 600,
            ..executor_config()
        };
        let ports = tiered_ports(&config, &fees);
        for purpose in [
            Purpose::DiscretionaryExit,
            Purpose::RiskExit,
            Purpose::OwnerExit,
        ] {
            let mut executor = protected(&ports)?;
            executor.run(bid_only("150", 0)?, &ports)?;
            executor.run(Input::Tick(RiskClock::from_secs(290)), &ports)?;
            executor.run(sell(EXIT, "5", "151", purpose)?, &ports)?;
            let first = executor.run(cancel_accepted(OCO), &ports)?;
            assert_eq!(
                submissions(&first)
                    .iter()
                    .map(|order| (order.client_order_id.as_str(), order.limit_price))
                    .collect::<Vec<_>>(),
                vec![(rung_id(0).as_str(), Some(Price::parse("149.25")?))],
                "{purpose:?}: the first rung prices from the last sane bid"
            );
            let stepped = executor.run(Input::Tick(RiskClock::from_secs(300)), &ports)?;
            assert_eq!(cancels(&stepped), vec![rung_id(0).as_str()], "{purpose:?}");
            executor.run(Input::Tick(RiskClock::from_secs(301)), &ports)?;
            let next = executor.run(cancel_accepted(&rung_id(0)), &ports)?;
            assert_eq!(
                submissions(&next)
                    .iter()
                    .map(|order| (order.client_order_id.as_str(), order.qty, order.limit_price))
                    .collect::<Vec<_>>(),
                vec![(
                    rung_id(1).as_str(),
                    Qty::parse("5")?,
                    Some(Price::parse("151")?)
                )],
                "{purpose:?}: the later rung goes at the intent's own limit"
            );
            assert_eq!(actions(&next), vec!["exit_unpriced"], "{purpose:?}");
            assert_eq!(alerts(&next), vec!["exit_unpriced"], "{purpose:?}");
            assert!(verdicts(&next).is_empty(), "{purpose:?}: never held");
        }
        Ok(())
    }

    /// Rules 3 and 13: a pause ends the ladder rather than climbing it. No tick steps a rung while
    /// its agent is paused, and a step's cancel the pause overtakes submits no next rung: its
    /// confirmation ends the sequence and re-places protection for the ten held.
    #[test]
    fn a_pause_ends_the_ladder_and_re_protects() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let agent = AgentId("agent-a".to_owned());
        let mut executor = protected(&ports)?;
        first_rung(&mut executor, &ports)?;
        executor.state.modes.insert(agent.clone(), Mode::Paused);
        let paused = executor.run(Input::Tick(RiskClock::from_secs(10)), &ports)?;
        assert!(cancels(&paused).is_empty(), "{:?}", drafted(&paused));
        executor.state.modes.insert(agent.clone(), Mode::Normal);
        let stepped = executor.run(Input::Tick(RiskClock::from_secs(11)), &ports)?;
        assert_eq!(cancels(&stepped), vec![rung_id(0).as_str()]);
        executor.state.modes.insert(agent, Mode::Paused);
        let ended = executor.run(cancel_accepted(&rung_id(0)), &ports)?;
        assert_eq!(
            submissions(&ended)
                .iter()
                .map(|order| (order.purpose, order.qty))
                .collect::<Vec<_>>(),
            vec![(Purpose::Protective, Qty::parse("10")?)],
            "{:?}",
            drafted(&ended)
        );
        Ok(())
    }

    /// §2.1 (DEC-260 (6)): an equity's sell rung rounds up to the Reg NMS tick, as does one whose
    /// class is not known; a crypto rung is left as priced.
    #[test]
    fn only_a_crypto_rung_is_left_off_the_equity_tick() -> Result<(), ExecutorError> {
        let priced = Price::parse("140.295")?;
        assert_eq!(
            super::ticked(Some(AssetClass::UsEquity), priced),
            Price::parse("140.3")?
        );
        assert_eq!(super::ticked(None, priced), Price::parse("140.3")?);
        assert_eq!(super::ticked(Some(AssetClass::Crypto), priced), priced);
        Ok(())
    }

    /// §5.4's bound overtakes a step in flight (DEC-160 (23)): a step's cancel the bound reaches
    /// before its confirmation submits no next rung; the confirmation re-protects the ten held.
    #[test]
    fn the_bound_overtakes_a_step_in_flight() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let mut executor = protected(&ports)?;
        first_rung(&mut executor, &ports)?;
        let stepped = executor.run(Input::Tick(RiskClock::from_secs(10)), &ports)?;
        assert_eq!(cancels(&stepped), vec![rung_id(0).as_str()]);
        let bound = executor.run(Input::Tick(RiskClock::from_secs(30)), &ports)?;
        assert_eq!(
            actions(&bound),
            vec!["interval_limit"],
            "{:?}",
            drafted(&bound)
        );
        let ended = executor.run(cancel_accepted(&rung_id(0)), &ports)?;
        assert_eq!(
            submissions(&ended)
                .iter()
                .map(|order| (order.purpose, order.qty))
                .collect::<Vec<_>>(),
            vec![(Purpose::Protective, Qty::parse("10")?)],
            "{:?}",
            drafted(&ended)
        );
        Ok(())
    }

    /// Only the sequence's own exit sets its ladder's clock: another exit submitted beside it, at
    /// 5, leaves the first rung's `exit_step_s` running from 0, so the step comes at 10.
    #[test]
    fn another_exits_submission_leaves_the_ladder_alone() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let mut executor = protected(&ports)?;
        first_rung(&mut executor, &ports)?;
        executor.run(Input::Tick(RiskClock::from_secs(5)), &ports)?;
        let beside = executor.run(sell(SECOND, "2", "150", Purpose::RiskExit)?, &ports)?;
        assert_eq!(submissions(&beside).len(), 1, "{:?}", drafted(&beside));
        let stepped = executor.run(Input::Tick(RiskClock::from_secs(10)), &ports)?;
        assert_eq!(cancels(&stepped), vec![rung_id(0).as_str()]);
        Ok(())
    }

    /// DEC-160 (23) (b): a discretionary exit held for want of a price beside another exit's
    /// working sequence re-places nothing: protection returns only when that sequence ends, never
    /// beside a working exit, so Σ sells stays within the position.
    #[test]
    fn an_unpriced_exit_beside_another_sequence_re_places_nothing() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let mut executor = protected(&ports)?;
        executor.run(sell(EXIT, "5", "150", Purpose::RiskExit)?, &ports)?;
        let working = executor.run(cancel_accepted(OCO), &ports)?;
        assert_eq!(submissions(&working).len(), 1, "{:?}", drafted(&working));
        let held = executor.run(
            sell(SECOND, "2", "150", Purpose::DiscretionaryExit)?,
            &ports,
        )?;
        assert_eq!(verdicts(&held), vec!["hold"]);
        assert!(submissions(&held).is_empty(), "{:?}", drafted(&held));
        Ok(())
    }

    /// A step's confirmation folded before a restart, with a late fill that completed the rung:
    /// nothing is left to sell, so no next rung goes; the sequence ends and the five still held
    /// are re-protected.
    #[test]
    fn a_step_on_a_rung_a_late_fill_completed_ends_the_sequence() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let mut executor = protected(&ports)?;
        first_rung(&mut executor, &ports)?;
        executor.run(Input::Tick(RiskClock::from_secs(10)), &ports)?;
        executor.commit_one(
            "OrderStateChanged",
            object(vec![
                ("client_order_id", text(rung_id(0))),
                ("state", text("canceled")),
                ("cancel_confirmed", Value::Bool(true)),
                ("risk_clock", clock(RiskClock::from_secs(10))?),
            ])?,
        )?;
        executor.commit_one(
            "LateFillApplied",
            object(vec![
                ("fill_id", text("f-late")),
                ("client_order_id", text(rung_id(0))),
                ("instrument", text("AAPL")),
                ("side", text("sell")),
                ("qty_gross", text("5")),
                ("price", text("149.25")),
                ("risk_clock", clock(RiskClock::from_secs(10))?),
            ])?,
        )?;
        let after = executor.run(Input::Tick(RiskClock::from_secs(11)), &ports)?;
        assert_eq!(
            submissions(&after)
                .iter()
                .map(|order| (order.purpose, order.qty))
                .collect::<Vec<_>>(),
            vec![(Purpose::Protective, Qty::parse("5")?)],
            "{:?}",
            drafted(&after)
        );
        Ok(())
    }

    /// The watchdog's own intent, as journaled: its id, agent, purpose and quantity.
    fn watchdog_intents(effects: &[Effect]) -> Vec<(String, String, String, String)> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Journal(draft) if draft.event_type == "IntentReceived" => {
                    let field = |name: &str| {
                        draft
                            .payload
                            .get(name)
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned()
                    };
                    Some((
                        field("intent_id"),
                        field("agent"),
                        field("purpose"),
                        field("qty"),
                    ))
                }
                _ => None,
            })
            .filter(|(intent, ..)| intent.starts_with("w-"))
            .collect()
    }

    /// The id of the journaled watchdog record, `ProtectionChanged watchdog`.
    fn watchdog_record(effects: &[Effect]) -> Option<String> {
        effects.iter().find_map(|effect| match effect {
            Effect::Journal(draft)
                if draft.event_type == "ProtectionChanged"
                    && draft.payload.get("action").and_then(Value::as_str) == Some("watchdog") =>
            {
                Some(draft.event_id.0.clone())
            }
            _ => None,
        })
    }

    /// §5.4, DEC-160 (11): a sane mark at or below the resting stop for `stop_watchdog_s` (30 s
    /// here) with no fill is watchdogged, and not a second sooner. The record is journaled and the
    /// owner alerted; the executor's own intent `w-<record>` is a `risk_exit` of the ten the stop
    /// covers, its agent the position's single holder; the protection is cancelled, and the
    /// confirmation sends `md-w-<record>` through the ladder, 139 × 0.995 on the cent. A restart
    /// between the cancel and its confirmation derives the same id.
    #[test]
    fn a_breached_stop_is_watchdogged_into_the_holders_risk_exit() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        for restart in [false, true] {
            let mut executor = protected(&ports)?;
            executor.run(observation(Some(139), None, true, 0)?, &ports)?;
            let early = executor.run(Input::Tick(RiskClock::from_secs(29)), &ports)?;
            assert!(watchdog_record(&early).is_none(), "{:?}", drafted(&early));
            let fired = executor.run(Input::Tick(RiskClock::from_secs(30)), &ports)?;
            let record = watchdog_record(&fired).ok_or_else(|| missing("the watchdog record"))?;
            assert!(alerts(&fired).contains(&"stop_watchdog"));
            assert_eq!(
                watchdog_intents(&fired),
                vec![(
                    format!("w-{record}"),
                    "agent-a".to_owned(),
                    "risk_exit".to_owned(),
                    "10".to_owned()
                )]
            );
            assert_eq!(cancels(&fired), vec![OCO]);
            if restart {
                executor = executor.restarted(&ports)?;
                executor.run(observation(Some(139), None, true, 30)?, &ports)?;
            }
            let sent = executor.run(cancel_accepted(OCO), &ports)?;
            assert_eq!(
                submissions(&sent)
                    .iter()
                    .map(|order| (
                        order.client_order_id.as_str().to_owned(),
                        order.purpose,
                        order.qty,
                        order.limit_price
                    ))
                    .collect::<Vec<_>>(),
                vec![(
                    format!("md-w-{record}"),
                    Purpose::RiskExit,
                    Qty::parse("10")?,
                    Some(Price::parse("138.31")?)
                )],
                "restart {restart}"
            );
        }
        Ok(())
    }

    /// DEC-260 (11): the watchdog's exit takes the lower of the latest mark and sane bid as its
    /// limit, so a mark above a fresh bid (139.9 against 139) never makes it a passive
    /// take-profit; it runs the marketable sequence.
    #[test]
    fn a_watchdog_exit_is_never_passive() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let mut executor = protected(&ports)?;
        executor.run(
            Input::Market(MarketObservation {
                instrument: aapl()?,
                bid: Some(Price::parse("139")?),
                bid_size: Some(Qty::parse("100")?),
                ask: Some(Price::parse("140")?),
                last_trade: None,
                mark: Some(Price::parse("139.9")?),
                sane: true,
                observed_at: RiskClock::from_secs(28),
            }),
            &ports,
        )?;
        executor.run(
            Input::Market(MarketObservation {
                instrument: aapl()?,
                bid: Some(Price::parse("139")?),
                bid_size: Some(Qty::parse("100")?),
                ask: Some(Price::parse("140")?),
                last_trade: None,
                mark: Some(Price::parse("139.9")?),
                sane: true,
                observed_at: RiskClock::from_secs(57),
            }),
            &ports,
        )?;
        let fired = executor.run(Input::Tick(RiskClock::from_secs(58)), &ports)?;
        assert!(watchdog_record(&fired).is_some(), "{:?}", drafted(&fired));
        assert_eq!(
            actions(&fired),
            vec!["watchdog", "unprotected_start"],
            "{:?}",
            drafted(&fired)
        );
        Ok(())
    }

    /// The watchdog exits what is held: with the position already sold and the stop still
    /// resting, there is nothing to exit, and no watchdog fires.
    #[test]
    fn a_flat_position_is_never_watchdogged() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let mut executor = protected(&ports)?;
        committed(
            &mut executor,
            "FillApplied",
            vec![
                ("fill_id", text("f-out")),
                ("client_order_id", text("md-held-1")),
                ("instrument", text("AAPL")),
                ("side", text("sell")),
                ("qty_gross", text("10")),
                ("price", text("150")),
            ],
        )?;
        executor.run(observation(Some(139), None, true, 0)?, &ports)?;
        let ticked = executor.run(Input::Tick(RiskClock::from_secs(60)), &ports)?;
        assert!(watchdog_record(&ticked).is_none(), "{:?}", drafted(&ticked));
        Ok(())
    }

    /// One watchdog per breach (#385 round 1): once the first watchdog's exit has ended (four
    /// filled, the rest cancelled at the bound) and protection is back for the six held, the
    /// breach it fired on is spent; a new sane breaching mark, at 62, starts a new one, which
    /// fires a second watchdog for those six `stop_watchdog_s` later.
    #[test]
    fn a_breach_that_outlasts_the_first_exit_fires_again() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let mut executor = protected(&ports)?;
        executor.run(observation(Some(139), None, true, 0)?, &ports)?;
        let first = executor.run(Input::Tick(RiskClock::from_secs(30)), &ports)?;
        let record = watchdog_record(&first).ok_or_else(|| missing("the first watchdog"))?;
        let exit = format!("md-w-{record}");
        executor.run(cancel_accepted(OCO), &ports)?;
        executor.run(
            Input::BrokerUpdate(BrokerUpdate::Fill(BrokerFill {
                fill_id: FillId("f-w".to_owned()),
                client_order_id: Some(exit.clone()),
                instrument: aapl()?,
                side: Side::Sell,
                qty: Qty::parse("4")?,
                price: Price::parse("138.31")?,
                fees: Usd::ZERO,
                trade_date: Date::parse("2026-09-22")?,
            })),
            &ports,
        )?;
        executor.run(Input::Tick(RiskClock::from_secs(40)), &ports)?;
        executor.run(Input::Tick(RiskClock::from_secs(60)), &ports)?;
        let ended = executor.run(cancel_accepted(&exit), &ports)?;
        assert_eq!(
            submissions(&ended)
                .iter()
                .map(|order| (order.purpose, order.qty))
                .collect::<Vec<_>>(),
            vec![(Purpose::Protective, Qty::parse("6")?)],
            "{:?}",
            drafted(&ended)
        );
        let spent = executor.run(Input::Tick(RiskClock::from_secs(61)), &ports)?;
        assert!(
            watchdog_record(&spent).is_none(),
            "the breach it fired on is spent: {:?}",
            drafted(&spent)
        );
        executor.run(observation(Some(139), None, true, 62)?, &ports)?;
        let early = executor.run(Input::Tick(RiskClock::from_secs(91)), &ports)?;
        assert!(watchdog_record(&early).is_none(), "{:?}", drafted(&early));
        let again = executor.run(Input::Tick(RiskClock::from_secs(92)), &ports)?;
        let second = watchdog_record(&again).ok_or_else(|| missing("the second watchdog"))?;
        assert_eq!(
            watchdog_intents(&again),
            vec![(
                format!("w-{second}"),
                "agent-a".to_owned(),
                "risk_exit".to_owned(),
                "6".to_owned()
            )]
        );
        Ok(())
    }

    /// #385 round 1, blocker 1 (rule 3): an insane quote never prices the watchdog exit. Sane 139
    /// at 0, then an insane quote at 200 at 400: the watchdog fires at 430 and its exit goes at
    /// 139, its fallback from the last sane bid (alerted, the bid being past five minutes), never
    /// at 200; and the protection does not cycle: once the bound has ended the exit and
    /// re-protected, the spent breach fires no second watchdog.
    #[test]
    fn an_insane_quote_never_prices_the_watchdog_exit() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let mut executor = protected(&ports)?;
        executor.run(observation(Some(139), None, true, 0)?, &ports)?;
        executor.run(observation(Some(200), Some(200), false, 400)?, &ports)?;
        let fired = executor.run(Input::Tick(RiskClock::from_secs(430)), &ports)?;
        assert!(watchdog_record(&fired).is_some(), "{:?}", drafted(&fired));
        let sent = executor.run(cancel_accepted(OCO), &ports)?;
        assert_eq!(
            submissions(&sent)
                .iter()
                .map(|order| order.limit_price)
                .collect::<Vec<_>>(),
            vec![Some(Price::parse("139")?)]
        );
        assert_eq!(alerts(&sent), vec!["exit_unpriced"]);
        let mut records = 0usize;
        for at in (431..=600).step_by(7) {
            let tick = executor.run(Input::Tick(RiskClock::from_secs(at)), &ports)?;
            records = records.saturating_add(usize::from(watchdog_record(&tick).is_some()));
            for id in cancels(&tick) {
                if id.starts_with("md-w-") {
                    executor.run(cancel_accepted(id), &ports)?;
                }
            }
        }
        assert_eq!(records, 0, "no second watchdog on the spent breach");
        assert!(
            rests(&executor.state, &aapl()?),
            "the protection is back and stays"
        );
        Ok(())
    }

    /// #385 round 1, minor 1: with no sane observation to price the exit from (the breach came
    /// from a sane mark with no bid, and the latest quote is insane), the watchdog does not fire:
    /// the breach persists, the stop keeps resting and nothing is cancelled. The next sane quote
    /// prices it, and the watchdog fires on the breach it has been running since 0.
    #[test]
    fn a_watchdog_with_nothing_sane_to_price_from_waits() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let mut executor = protected(&ports)?;
        executor.run(
            Input::Market(MarketObservation {
                instrument: aapl()?,
                bid: None,
                bid_size: None,
                ask: None,
                last_trade: None,
                mark: Some(Price::parse("139")?),
                sane: true,
                observed_at: RiskClock::from_secs(0),
            }),
            &ports,
        )?;
        executor.run(observation(Some(200), None, false, 10)?, &ports)?;
        for at in [30, 60, 90] {
            let tick = executor.run(Input::Tick(RiskClock::from_secs(at)), &ports)?;
            assert!(
                watchdog_record(&tick).is_none() && cancels(&tick).is_empty(),
                "at {at}: {:?}",
                drafted(&tick)
            );
        }
        assert!(executor.state.breaches.contains_key(&aapl()?));
        assert!(rests(&executor.state, &aapl()?));
        let priced = executor.run(observation(Some(139), None, true, 91)?, &ports)?;
        assert!(watchdog_record(&priced).is_some(), "{:?}", drafted(&priced));
        assert_eq!(cancels(&priced), vec![OCO]);
        Ok(())
    }

    /// DEC-260 (11), the boundary: with `stop_watchdog_s` at 0 the watchdog fires in the second the
    /// breach begins, and that breach is spent at once: a tick and a breaching quote in the same
    /// second fire nothing, and the new breach starts the second after the fire.
    #[test]
    fn a_breach_is_spent_in_the_second_it_fires() -> Result<(), ExecutorError> {
        let fees = fees()?;
        let config = ExecutorConfig {
            stop_watchdog_s: 0,
            max_unprotected_s: 600,
            ..executor_config()
        };
        let ports = tiered_ports(&config, &fees);
        let mut executor = protected(&ports)?;
        executor
            .state
            .modes
            .insert(AgentId("agent-a".to_owned()), Mode::Paused);
        let fired = executor.run(observation(Some(139), None, true, 0)?, &ports)?;
        assert!(watchdog_record(&fired).is_some(), "{:?}", drafted(&fired));
        for input in [
            Input::Tick(RiskClock::from_secs(0)),
            observation(Some(139), None, true, 0)?,
        ] {
            let again = executor.run(input, &ports)?;
            assert!(watchdog_record(&again).is_none(), "{:?}", drafted(&again));
        }
        assert_eq!(
            executor.state.breaches.get(&aapl()?),
            Some(&RiskClock::from_secs(1)),
            "the new breach starts the second after the fire"
        );
        Ok(())
    }

    /// #385 round 2, blocker 2 (rule 3): a breach never outlives the recovery that ends it. Sane
    /// 139 at 0 fires the watchdog at 30; sane 139 at 30 re-arms a breach while the cancel waits;
    /// once the cancel is confirmed no stop rests, and sane 155 at 35, 40 and 50 end the breach.
    /// The bound at 61 re-places the protection for the unfilled exit, and at 62 no second
    /// watchdog fires: the restored protection rests and nothing sells at 154.23.
    #[test]
    fn a_recovery_during_the_interval_ends_the_breach() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let mut executor = protected(&ports)?;
        executor.run(observation(Some(139), None, true, 0)?, &ports)?;
        let fired = executor.run(Input::Tick(RiskClock::from_secs(30)), &ports)?;
        assert!(watchdog_record(&fired).is_some(), "{:?}", drafted(&fired));
        executor.run(observation(Some(139), None, true, 30)?, &ports)?;
        executor.run(cancel_accepted(OCO), &ports)?;
        for at in [35, 40, 50] {
            executor.run(observation(Some(155), None, true, at)?, &ports)?;
        }
        assert!(executor.state.breaches.is_empty(), "the recovery ended it");
        let bound = executor.run(Input::Tick(RiskClock::from_secs(61)), &ports)?;
        for id in cancels(&bound) {
            if id.starts_with("md-w-") {
                executor.run(cancel_accepted(id), &ports)?;
            }
        }
        assert!(rests(&executor.state, &aapl()?), "the protection is back");
        let after = executor.run(Input::Tick(RiskClock::from_secs(62)), &ports)?;
        assert!(watchdog_record(&after).is_none(), "{:?}", drafted(&after));
        assert!(cancels(&after).is_empty(), "{:?}", drafted(&after));
        assert!(
            submissions(&after)
                .iter()
                .all(|order| order.purpose == Purpose::Protective),
            "nothing sells: {:?}",
            drafted(&after)
        );
        assert!(rests(&executor.state, &aapl()?));
        Ok(())
    }

    /// §5.4: the breach must last. A sane mark back above the stop before `stop_watchdog_s` ends
    /// it, and the next breach starts its own clock; an insane quote neither starts nor ends one.
    #[test]
    fn a_breach_must_last_stop_watchdog_s() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let mut executor = protected(&ports)?;
        executor.run(observation(Some(139), None, true, 0)?, &ports)?;
        executor.run(observation(Some(141), None, true, 20)?, &ports)?;
        executor.run(observation(Some(139), None, true, 25)?, &ports)?;
        executor.run(observation(Some(150), None, false, 40)?, &ports)?;
        let early = executor.run(Input::Tick(RiskClock::from_secs(54)), &ports)?;
        assert!(watchdog_record(&early).is_none(), "{:?}", drafted(&early));
        let fired = executor.run(Input::Tick(RiskClock::from_secs(55)), &ports)?;
        assert!(watchdog_record(&fired).is_some(), "{:?}", drafted(&fired));
        let sent = executor.run(cancel_accepted(OCO), &ports)?;
        assert_eq!(
            submissions(&sent)
                .iter()
                .map(|order| order.limit_price)
                .collect::<Vec<_>>(),
            vec![Some(Price::parse("138.31")?)],
            "priced from the last sane bid, 139, never from the insane 150"
        );
        Ok(())
    }

    /// DEC-260 (9): a stale breaching quote starts the clock when the executor learns of it, not
    /// at its own timestamp, so it never fires the watchdog at once.
    #[test]
    fn a_stale_breach_starts_the_clock_when_it_arrives() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let mut executor = protected(&ports)?;
        executor.run(Input::Tick(RiskClock::from_secs(100)), &ports)?;
        let arrived = executor.run(observation(Some(139), None, true, 0)?, &ports)?;
        assert!(
            watchdog_record(&arrived).is_none(),
            "{:?}",
            drafted(&arrived)
        );
        let early = executor.run(Input::Tick(RiskClock::from_secs(129)), &ports)?;
        assert!(watchdog_record(&early).is_none(), "{:?}", drafted(&early));
        let fired = executor.run(Input::Tick(RiskClock::from_secs(130)), &ports)?;
        assert!(watchdog_record(&fired).is_some(), "{:?}", drafted(&fired));
        Ok(())
    }

    /// §5.4's "with no fill": a stop that has filled at all is working, and is not watchdogged.
    #[test]
    fn a_stop_that_has_filled_is_not_watchdogged() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let mut executor = protected(&ports)?;
        committed(
            &mut executor,
            "FillApplied",
            vec![
                ("fill_id", text("f-stop")),
                ("client_order_id", text(OCO)),
                ("instrument", text("AAPL")),
                ("side", text("sell")),
                ("qty_gross", text("2")),
                ("price", text("140")),
            ],
        )?;
        executor.run(observation(Some(139), None, true, 0)?, &ports)?;
        let ticked = executor.run(Input::Tick(RiskClock::from_secs(60)), &ports)?;
        assert!(watchdog_record(&ticked).is_none(), "{:?}", drafted(&ticked));
        Ok(())
    }

    /// Rule 13: a watchdog exit the gate holds (its agent paused) leaves the stop resting and is
    /// journaled once: no second watchdog fires while it waits, not even on new breaching quotes
    /// past `stop_watchdog_s`, which would sell the same shares twice once released; the tick after
    /// the pause lifts releases it into the sequence.
    #[test]
    fn a_held_watchdog_exit_fires_once_and_goes_when_released() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let agent = AgentId("agent-a".to_owned());
        let mut executor = protected(&ports)?;
        executor.state.modes.insert(agent.clone(), Mode::Paused);
        executor.run(observation(Some(139), None, true, 0)?, &ports)?;
        let fired = executor.run(Input::Tick(RiskClock::from_secs(30)), &ports)?;
        assert!(watchdog_record(&fired).is_some());
        assert_eq!(verdicts(&fired), vec!["hold"]);
        assert!(cancels(&fired).is_empty(), "the stop keeps resting");
        for at in [31, 45, 60, 90] {
            let quoted = executor.run(observation(Some(139), None, true, at)?, &ports)?;
            let tick = executor.run(Input::Tick(RiskClock::from_secs(at)), &ports)?;
            assert!(
                watchdog_record(&quoted).is_none() && watchdog_record(&tick).is_none(),
                "at {at}: a new breach while the watchdog's exit waits fires nothing: {:?}",
                drafted(&tick)
            );
        }
        executor.state.modes.insert(agent, Mode::Normal);
        let released = executor.run(Input::Tick(RiskClock::from_secs(91)), &ports)?;
        assert_eq!(cancels(&released), vec![OCO], "{:?}", drafted(&released));
        assert!(watchdog_record(&released).is_none());
        Ok(())
    }

    /// DEC-160 (11), §2.3, §5.5: with no single holder (agent-b bought five beside agent-a's ten)
    /// the watchdog exit belongs to no agent, `*`. It is never any agent's own, and the
    /// instrument-wide bound cancels it by its own id like any exit working there.
    #[test]
    fn a_watchdog_exit_with_no_single_holder_is_nobodys_and_the_bound_ends_it()
    -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let mut executor = protected(&ports)?;
        committed(
            &mut executor,
            "OrderSubmitted",
            vec![
                ("client_order_id", text("md-held-2")),
                ("agent", text("agent-b")),
                ("instrument", text("AAPL")),
                ("side", text("buy")),
                ("qty", text("5")),
                ("limit", text("150")),
                ("purpose", text("open")),
            ],
        )?;
        committed(
            &mut executor,
            "FillApplied",
            vec![
                ("fill_id", text("f-b")),
                ("client_order_id", text("md-held-2")),
                ("instrument", text("AAPL")),
                ("side", text("buy")),
                ("qty_gross", text("5")),
                ("price", text("150")),
            ],
        )?;
        executor.run(observation(Some(139), None, true, 0)?, &ports)?;
        let fired = executor.run(Input::Tick(RiskClock::from_secs(30)), &ports)?;
        let record = watchdog_record(&fired).ok_or_else(|| missing("the watchdog record"))?;
        assert_eq!(
            watchdog_intents(&fired),
            vec![(
                format!("w-{record}"),
                "*".to_owned(),
                "risk_exit".to_owned(),
                "10".to_owned()
            )],
            "the ten the stop covers, owned by no agent"
        );
        executor.run(cancel_accepted(OCO), &ports)?;
        let exit = ClientOrderId::parse(&format!("md-w-{record}"))?;
        let order = executor
            .state
            .orders
            .get(&exit)
            .ok_or_else(|| missing("the watchdog exit"))?;
        assert_eq!(order.agent, Some(AgentId("*".to_owned())));
        assert_ne!(order.agent, Some(AgentId("agent-a".to_owned())));
        assert_ne!(order.agent, Some(AgentId("agent-b".to_owned())));
        let bound = executor.run(Input::Tick(RiskClock::from_secs(60)), &ports)?;
        assert!(
            cancels(&bound).contains(&exit.as_str()),
            "{:?}",
            cancels(&bound)
        );
        Ok(())
    }

    /// §5.6 ladders an exit only where its instrument has an exit tier: without one the exit goes
    /// at its intent's own limit and is never cancelled to step.
    #[test]
    fn an_exit_with_no_tier_never_steps() -> Result<(), ExecutorError> {
        with_ports!(ports);
        let mut executor = protected(&ports)?;
        executor.run(quote("150")?, &ports)?;
        executor.run(sell(EXIT, "5", "150", Purpose::RiskExit)?, &ports)?;
        let sent = executor.run(cancel_accepted(OCO), &ports)?;
        assert_eq!(
            submissions(&sent)
                .iter()
                .map(|order| order.limit_price)
                .collect::<Vec<_>>(),
            vec![Some(Price::parse("150")?)]
        );
        for at in [10, 15, 20] {
            let tick = executor.run(Input::Tick(RiskClock::from_secs(at)), &ports)?;
            assert!(cancels(&tick).is_empty(), "at {at}: {:?}", drafted(&tick));
        }
        Ok(())
    }

    /// §5.6 step 3: at `max_exit_offset` the rung rests, the owner is alerted once, and no tick
    /// steps it again; a restart resumes the ladder where the journal left it.
    #[test]
    fn the_ladder_rests_at_its_floor_and_alerts_once() -> Result<(), ExecutorError> {
        let fees = fees()?;
        let config = crate::types::ExecutorConfig {
            max_unprotected_s: 600,
            ..executor_config()
        };
        let ports = tiered_ports(&config, &fees);
        let mut executor = protected(&ports)?;
        first_rung(&mut executor, &ports)?;
        let mut prices = Vec::new();
        let mut floor_alerts = 0usize;
        for step in 1..=5u32 {
            let at = i64::from(step).saturating_mul(10);
            executor.run(Input::Tick(RiskClock::from_secs(at)), &ports)?;
            let next = executor.run(cancel_accepted(&rung_id(step.saturating_sub(1))), &ports)?;
            floor_alerts = floor_alerts.saturating_add(
                alerts(&next)
                    .iter()
                    .filter(|key| **key == "exit_ladder_floor")
                    .count(),
            );
            prices.extend(
                submissions(&next)
                    .iter()
                    .map(|order| (order.client_order_id.as_str().to_owned(), order.limit_price)),
            );
        }
        assert_eq!(
            prices,
            [
                (1, "148.5"),
                (2, "147.75"),
                (3, "147"),
                (4, "146.25"),
                (5, "145.5")
            ]
            .map(|(step, price)| (rung_id(step), Price::parse(price).ok()))
            .to_vec()
        );
        assert_eq!(floor_alerts, 1, "the owner is alerted once, at the floor");
        let restarted = executor.restarted(&ports)?;
        assert_eq!(restarted.state.exiting, executor.state.exiting);
        for at in [60, 70, 200] {
            let rested = executor.run(Input::Tick(RiskClock::from_secs(at)), &ports)?;
            assert!(
                cancels(&rested).is_empty()
                    && submissions(&rested).is_empty()
                    && alerts(&rested).is_empty(),
                "at the floor the rung rests ({at})"
            );
        }
        Ok(())
    }

    /// The interval's bound still ends a laddered exit: the current rung is the one cancelled,
    /// and its confirmation re-protects rather than stepping.
    #[test]
    fn the_intervals_bound_cancels_the_current_rung_and_re_protects() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let mut executor = protected(&ports)?;
        first_rung(&mut executor, &ports)?;
        executor.run(Input::Tick(RiskClock::from_secs(10)), &ports)?;
        executor.run(cancel_accepted(&rung_id(0)), &ports)?;
        let due = executor.run(Input::Tick(RiskClock::from_secs(30)), &ports)?;
        assert_eq!(cancels(&due), vec![rung_id(1).as_str()]);
        assert!(actions(&due).contains(&"interval_limit"));
        let ended = executor.run(cancel_accepted(&rung_id(1)), &ports)?;
        assert_eq!(
            submissions(&ended)
                .iter()
                .map(|order| order.purpose)
                .collect::<Vec<_>>(),
            vec![Purpose::Protective],
            "re-protected, no further rung"
        );
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
            executor.run(fresh_quote("0.01", sane, observed)?, &ports)?;
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
        assert_eq!(
            journal_order(&released),
            vec!["unprotected_start", OCO, "GateDecided"],
            "DEC-160 (8): the released exit's sequence is journaled before its GateDecided, as on \
             arrival (#286 round 1, minor 1)"
        );
        Ok(())
    }

    /// The drafts that order an exit's start: each `ProtectionChanged` by its action, each cancel
    /// asked by the order it names, and each `GateDecided`.
    fn journal_order(effects: &[Effect]) -> Vec<&str> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Journal(draft) if draft.event_type == "GateDecided" => Some("GateDecided"),
                Effect::Journal(draft) if draft.event_type == "ProtectionChanged" => {
                    draft.payload.get("action").and_then(Value::as_str)
                }
                Effect::Journal(draft)
                    if draft.payload.get("cancel_requested") == Some(&Value::Bool(true)) =>
                {
                    draft.payload.get("client_order_id").and_then(Value::as_str)
                }
                _ => None,
            })
            .collect()
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

    /// The coordinator's ruling D2 (rule 13): a passive exit older than `max_intent_age_s` when
    /// the old OCO's cancel is confirmed is never abandoned; it is placed as its new OCO's
    /// take-profit at its own limit, the stop kept, for the whole position (§5.4).
    #[test]
    fn an_old_passive_exit_still_places_its_oco() -> Result<(), ExecutorError> {
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
        executor.run(Input::Tick(RiskClock::from_secs(121)), &ports)?;
        let late = executor.run(cancel_accepted(OCO), &ports)?;
        assert!(
            !drafted(&late).contains(&"OrderAbandoned"),
            "{:?}",
            drafted(&late)
        );
        assert_eq!(
            ocos(&late)
                .into_iter()
                .map(|(_, qty, legs, purpose)| (qty, legs, purpose))
                .collect::<Vec<_>>(),
            vec![(
                Qty::parse("10")?,
                legs("160", "140", "10")?,
                Purpose::Protective
            )],
            "the exit's price as the take-profit, the stop kept"
        );
        assert_eq!(stop_covered(&executor)?, position(&executor)?);
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

    /// The broker's description of an opening buy of 5 at 150.
    fn opening_order(id: &str, status: &str, filled: &str) -> Result<BrokerOrder, ExecutorError> {
        Ok(BrokerOrder {
            broker_order_id: format!("b-{id}"),
            client_order_id: Some(id.to_owned()),
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
        })
    }

    fn buy_reported(status: &str, filled: &str) -> Result<Input, ExecutorError> {
        Ok(Input::Broker(Ok(BrokerOutcome::Order(opening_order(
            BUY, status, filled,
        )?))))
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

    /// Ruling 5863046153, DEC-160 (18): a cancel never confirmed ends the wait at its bound
    /// (`unknown_absent_window_s`, 15 s here) — the buy is queried as an `Unknown` would be, and
    /// the exit goes at once, without waiting for an answer that may never come; a later exit
    /// never waits on that buy again.
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
        assert_eq!(submitted(&due), 1, "the exit goes with the query");
        let again = executor.run(sell(SECOND, "5", "139", Purpose::RiskExit)?, &ports)?;
        assert_eq!(submitted(&again), 1, "never a second wait on the same buy");
        Ok(())
    }

    /// Ruling 5863046153, DEC-160 (18): a cancel the broker refuses because the buy filled is
    /// overdue at once; the buy is queried and the exit goes with the query.
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
        assert_eq!(submitted(&refused), 1);
        let answered = executor.run(buy_reported("filled", "5")?, &ports)?;
        assert_eq!(submitted(&answered), 0, "the answer re-sends nothing");
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
    /// equals it — never more, never less. With a second, marketable exit released beside it in
    /// either order (#286 round 2, M1′), the stop covers the position less what that exit may
    /// sell, never more than the position before or after it fills; and once it ends, filled or
    /// not, the stop covers exactly the position again.
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
        with_ports!(beside);
        let sizes = (1..=9_u8).flat_map(|size| {
            (1..=9_u8)
                .filter(move |other| size.checked_add(*other).is_some_and(|total| total <= 10))
                .map(move |other| (size, other))
        });
        for (size, other) in sizes {
            for second in [SECOND, LATER] {
                for fills in [true, false] {
                    let case = format!("passive {size} beside {second} of {other}, fills {fills}");
                    let mut executor = protected(&beside)?;
                    executor.run(quote("150")?, &beside)?;
                    let passive = sell(EXIT, &size.to_string(), "160", Purpose::DiscretionaryExit);
                    executor.run(passive?, &beside)?;
                    let marketable = sell(second, &other.to_string(), "139", Purpose::RiskExit);
                    executor.run(marketable?, &beside)?;
                    executor.run(cancel_accepted(OCO), &beside)?;
                    let held = position(&executor)?;
                    assert!(stop_covered(&executor)? <= held, "{case}");
                    assert_eq!(
                        stop_covered(&executor)?.checked_add(Qty::parse(&other.to_string())?)?,
                        held,
                        "{case}: the stop covers the position less what the other exit may sell"
                    );
                    let end = if fills {
                        filled(second, &other.to_string())?
                    } else {
                        cancel_accepted(&format!("md-{second}"))
                    };
                    executor.run(end, &beside)?;
                    assert!(stop_covered(&executor)? <= position(&executor)?, "{case}");
                    assert_eq!(
                        stop_covered(&executor)?,
                        position(&executor)?,
                        "{case}: what the other exit left unsold is protected again, and no more"
                    );
                    assert!(
                        executor.state.exiting.is_empty() && open_interval(&executor).is_none(),
                        "{case}: the handed-on sequence ends"
                    );
                }
            }
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

    /// The ten AAPL, as crypto under a 1% offset, protected by crypto's one GTC stop-limit at 140
    /// (DEC-36): a stop-only placement, with no take-profit.
    fn stop_only(ports: &Ports<'_>) -> Result<Executor, ExecutorError> {
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
        Ok(executor)
    }

    /// #286 round 1, M1: a passive exit (a sell limit above a fresh sane bid) where the resting
    /// protection is a stop-only placement answers slice 5's stub before anything is journaled or
    /// cancelled — never an OCO in a crypto instrument, never a remainder left unprotected — and
    /// the stop keeps resting. The same exit at or below the bid is marketable and runs its
    /// sequence.
    #[test]
    fn a_passive_exit_over_a_stop_only_placement_answers_the_stub() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Coin,
            instruments: &Coin,
            config: &config,
            fees: &fees,
        };
        for (limit, passive) in [
            ("160", true),
            ("150.01", true),
            ("150", false),
            ("139", false),
        ] {
            let mut executor = stop_only(&ports)?;
            executor.run(fresh_quote("150", true, 0)?, &ports)?;
            let exit = executor.run(sell(EXIT, "4", limit, Purpose::DiscretionaryExit)?, &ports);
            if passive {
                assert!(stub(&exit), "{limit}: {exit:?}");
                assert!(
                    rests(&executor.state, &aapl()?),
                    "{limit}: the stop still rests"
                );
                assert!(
                    executor.state.exiting.is_empty(),
                    "{limit}: no sequence began"
                );
                assert_eq!(
                    executor
                        .state
                        .orders
                        .get(&ClientOrderId::parse(OCO)?)
                        .map(|order| (order.state, order.cancel_unconfirmed)),
                    Some((OrderState::Accepted, false)),
                    "{limit}: the stop was never asked to cancel"
                );
            } else {
                let exit = exit?;
                assert_eq!(actions(&exit), vec!["unprotected_start"], "{limit}");
                assert_eq!(cancels(&exit), vec![OCO], "{limit}");
            }
        }
        Ok(())
    }

    /// #286 round 1, M1, in [`passive_exit`](super::passive_exit): a passive sequence on the
    /// journal whose recorded protection has no take-profit — which [`begin_exit`](super::begin_exit)
    /// no longer starts — answers the stub at the confirmation, rather than an OCO without a
    /// take-profit or a remainder left unprotected.
    #[test]
    fn a_journaled_passive_sequence_with_no_take_profit_answers_the_stub()
    -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Coin,
            instruments: &Coin,
            config: &config,
            fees: &fees,
        };
        let mut executor = stop_only(&ports)?;
        committed(
            &mut executor,
            "IntentReceived",
            vec![
                ("intent_id", text(EXIT)),
                ("agent", text("agent-a")),
                ("kind", text("order")),
                ("instrument", text("AAPL")),
                ("side", text("sell")),
                ("qty", text("4")),
                ("limit", text("160")),
                ("purpose", text("discretionary_exit")),
            ],
        )?;
        committed(
            &mut executor,
            "GateDecided",
            vec![("intent_id", text(EXIT)), ("verdict", text("allow"))],
        )?;
        committed(
            &mut executor,
            "ProtectionChanged",
            vec![
                ("instrument", text("AAPL")),
                ("action", text("passive_start")),
                ("orders", text(OCO)),
                ("intent_id", text(EXIT)),
                ("entry", text("md-held-1")),
                ("agent", text("agent-a")),
                ("stop", text("140")),
            ],
        )?;
        committed(
            &mut executor,
            "OrderStateChanged",
            vec![
                ("client_order_id", text(OCO)),
                ("state", text("pending_cancel")),
                ("cancel_requested", Value::Bool(true)),
            ],
        )?;
        assert!(
            executor
                .state
                .exiting
                .get(&aapl()?)
                .is_some_and(|sequence| sequence.passive),
            "the journal carries a passive sequence"
        );
        let confirmed = executor.run(cancel_accepted(OCO), &ports);
        assert!(stub(&confirmed), "{confirmed:?}");
        Ok(())
    }

    /// DEC-160 (8), (13): an exit's wait on its cancels starts at the `GateDecided` that allows
    /// it, the latest one — an exit held on an `Unknown` and released later waits its full bound
    /// on the opening it then cancels, rather than being overdue from its first allow and going
    /// before rule 5's cancel is confirmed.
    #[test]
    fn a_wait_starts_at_the_allow_that_ends_a_hold() -> Result<(), ExecutorError> {
        with_ports!(ports);
        let mut executor = held_with_a_resting_buy(&ports)?;
        let other = "md-other-1";
        committed(
            &mut executor,
            "OrderSubmitted",
            vec![
                ("client_order_id", text(other)),
                ("agent", text("agent-b")),
                ("instrument", text("AAPL")),
                ("side", text("buy")),
                ("qty", text("1")),
                ("limit", text("150")),
                ("purpose", text("open")),
            ],
        )?;
        committed(
            &mut executor,
            "OrderStateChanged",
            vec![
                ("client_order_id", text(other)),
                ("state", text("accepted")),
            ],
        )?;
        executor.run(sell(EXIT, "5", "139", Purpose::RiskExit)?, &ports)?;
        committed(
            &mut executor,
            "OrderStateChanged",
            vec![("client_order_id", text(other)), ("state", text("unknown"))],
        )?;
        let held = executor.run(cancel_accepted(BUY), &ports)?;
        assert_eq!(verdicts(&held), vec!["hold"], "{:?}", drafted(&held));
        let second = "md-buy-3";
        committed(
            &mut executor,
            "OrderSubmitted",
            vec![
                ("client_order_id", text(second)),
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
            vec![
                ("client_order_id", text(second)),
                ("state", text("accepted")),
            ],
        )?;
        executor.run(
            Input::Broker(Ok(BrokerOutcome::Order(opening_order(
                other, "canceled", "0",
            )?))),
            &ports,
        )?;
        let released = executor.run(Input::Tick(RiskClock::from_secs(40)), &ports)?;
        assert_eq!(verdicts(&released), vec!["allow"]);
        assert_eq!(cancels(&released), vec![second]);
        assert!(queried(&released).is_empty() && submitted(&released) == 0);
        let early = executor.run(Input::Tick(RiskClock::from_secs(54)), &ports)?;
        assert!(queried(&early).is_empty() && submitted(&early) == 0);
        let due = executor.run(Input::Tick(RiskClock::from_secs(55)), &ports)?;
        assert_eq!(queried(&due), vec![second]);
        Ok(())
    }

    /// Rule 5's release pass takes only exits for waiting ones (rule 2): an intent that adds risk,
    /// allowed on the journal with no order — which no batch writes, a submission following its
    /// allow in the same batch — never has its agent's openings cancelled for it, nor is it
    /// submitted, by a tick.
    #[test]
    fn the_release_pass_never_takes_an_add_for_a_waiting_exit() -> Result<(), ExecutorError> {
        with_ports!(ports);
        let mut executor = held_with_a_resting_buy(&ports)?;
        committed(
            &mut executor,
            "IntentReceived",
            vec![
                ("intent_id", text(SECOND)),
                ("agent", text("agent-a")),
                ("kind", text("order")),
                ("instrument", text("AAPL")),
                ("side", text("buy")),
                ("qty", text("5")),
                ("limit", text("150")),
                ("purpose", text("increase")),
            ],
        )?;
        committed(
            &mut executor,
            "GateDecided",
            vec![("intent_id", text(SECOND)), ("verdict", text("allow"))],
        )?;
        let tick = executor.run(Input::Tick(RiskClock::from_secs(20)), &ports)?;
        assert!(
            cancels(&tick).is_empty() && submitted(&tick) == 0 && queried(&tick).is_empty(),
            "{:?}",
            drafted(&tick)
        );
        Ok(())
    }

    /// The unprotected ten AAPL with `agent-a`'s opening buy of 5 submitted and not yet
    /// acknowledged: `Submitting`.
    fn held_with_a_submitted_buy(ports: &Ports<'_>) -> Result<Executor, ExecutorError> {
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
        Ok(executor)
    }

    /// The broker's fill of `qty` of an opening buy at 150.
    fn opening_filled(id: &str, qty: &str) -> Result<Input, ExecutorError> {
        Ok(Input::BrokerUpdate(BrokerUpdate::Fill(BrokerFill {
            fill_id: FillId(format!("f-{id}")),
            client_order_id: Some(id.to_owned()),
            instrument: aapl()?,
            side: Side::Buy,
            qty: Qty::parse(qty)?,
            price: Price::parse("150")?,
            fees: Usd::ZERO,
            trade_date: Date::parse("2026-09-22")?,
        })))
    }

    fn refused(id: &str, message: &str) -> Input {
        Input::Broker(Ok(BrokerOutcome::Rejected(BrokerReject {
            client_order_id: Some(id.to_owned()),
            http_status: 422,
            code: None,
            message: message.to_owned(),
        })))
    }

    fn verdicts(effects: &[Effect]) -> Vec<&str> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Journal(draft) if draft.event_type == "GateDecided" => {
                    draft.payload.get("verdict").and_then(Value::as_str)
                }
                _ => None,
            })
            .collect()
    }

    /// #286 round 1, B1, the reviewer's first path: an exit waits on its agent's opening the
    /// broker has not yet acknowledged, with nothing to cancel yet, and asks its cancel in the
    /// step the acknowledgment arrives; the confirmation then releases the exit.
    #[test]
    fn an_exit_cancels_a_submitting_opening_once_it_is_accepted() -> Result<(), ExecutorError> {
        with_ports!(ports);
        let mut executor = held_with_a_submitted_buy(&ports)?;
        let waiting = executor.run(sell(EXIT, "5", "139", Purpose::RiskExit)?, &ports)?;
        assert_eq!(verdicts(&waiting), vec!["allow"]);
        assert!(
            cancels(&waiting).is_empty() && submitted(&waiting) == 0,
            "{:?}",
            drafted(&waiting)
        );
        let accepted = executor.run(
            Input::Broker(Ok(BrokerOutcome::Submitted(opening_order(
                BUY, "new", "0",
            )?))),
            &ports,
        )?;
        assert_eq!(cancels(&accepted), vec![BUY], "cancelled once accepted");
        assert_eq!(
            submitted(&accepted),
            0,
            "and the exit waits for the confirmation"
        );
        let released = executor.run(cancel_accepted(BUY), &ports)?;
        assert_eq!(submitted(&released), 1);
        Ok(())
    }

    /// #286 round 1, B1, the first path at its bound: an opening never acknowledged is, at
    /// `unknown_absent_window_s` from the exit's allow, an `Unknown`, marked overdue and queried
    /// (§5.7), and the exit's wait ends in that step: the gate holds it on the `Unknown` in its
    /// instrument — rule 13's hold — rather than sending it past an order that may yet appear
    /// (DEC-160 (18)). After an answer that it rests, the next tick releases the hold, cancels it
    /// and sends the exit; after an answer that the broker never had it, the hold stays for §5.7's
    /// lookups to end.
    #[test]
    fn a_submitting_opening_never_acknowledged_is_queried_at_the_bound() -> Result<(), ExecutorError>
    {
        with_ports!(ports);
        for rests_at_broker in [true, false] {
            let mut executor = held_with_a_submitted_buy(&ports)?;
            executor.run(sell(EXIT, "5", "139", Purpose::RiskExit)?, &ports)?;
            let early = executor.run(Input::Tick(RiskClock::from_secs(14)), &ports)?;
            assert!(queried(&early).is_empty() && submitted(&early) == 0);
            let due = executor.run(Input::Tick(RiskClock::from_secs(15)), &ports)?;
            assert_eq!(queried(&due), vec![BUY]);
            assert!(
                due.iter().any(|effect| matches!(
                    effect,
                    Effect::Journal(draft)
                        if draft.payload.get("cancel_overdue") == Some(&Value::Bool(true))
                            && draft.payload.get("state").and_then(Value::as_str) == Some("unknown")
                )),
                "{:?}",
                drafted(&due)
            );
            let exit = IntentId(EventId(EXIT.to_owned()));
            assert_eq!(verdicts(&due), vec!["hold"], "{:?}", drafted(&due));
            assert_eq!(submitted(&due), 0);
            assert!(executor.state.held.contains(&exit), "held on the Unknown");
            let answer = if rests_at_broker {
                buy_reported("new", "0")?
            } else {
                Input::Broker(Ok(BrokerOutcome::Absent {
                    client_order_id: BUY.to_owned(),
                }))
            };
            executor.run(answer, &ports)?;
            let next = executor.run(Input::Tick(RiskClock::from_secs(16)), &ports)?;
            if rests_at_broker {
                assert_eq!(cancels(&next), vec![BUY], "{:?}", drafted(&next));
                assert_eq!(submitted(&next), 1, "the exit goes past the resting buy");
            } else {
                assert_eq!(submitted(&next), 0);
                assert!(
                    executor.state.held.contains(&exit),
                    "still held on the Unknown"
                );
            }
        }
        Ok(())
    }

    /// #286 round 1, B1: a `Submitting` opening that ends by any path — the broker's reject,
    /// expiry, cancel or fill, or the submission refused outright — releases the waiting exit in
    /// that step.
    #[test]
    fn a_submitting_opening_gone_by_any_path_releases_the_exit_at_once() -> Result<(), ExecutorError>
    {
        with_ports!(ports);
        for (status, filled) in [
            ("rejected", "0"),
            ("expired", "0"),
            ("canceled", "0"),
            ("filled", "5"),
            ("refused", "0"),
        ] {
            let mut executor = held_with_a_submitted_buy(&ports)?;
            executor.run(sell(EXIT, "5", "139", Purpose::RiskExit)?, &ports)?;
            let report = if status == "refused" {
                refused(BUY, "insufficient qty")
            } else {
                buy_reported(status, filled)?
            };
            let gone = executor.run(report, &ports)?;
            assert_eq!(submitted(&gone), 1, "{status}: {:?}", drafted(&gone));
        }
        Ok(())
    }

    /// #286 round 1, B1, the reviewer's second path: the broker reports the opening whose cancel
    /// is outstanding canceled, or filled, with no `CancelAccepted`; the exit goes in that step.
    /// An expiry reported then is no edge out of `PendingCancel` in §5.7's table and is journaled
    /// and ignored, so the bound ends that wait: the exit goes with the query, and the answer
    /// re-sends nothing (DEC-160 (18)).
    #[test]
    fn an_opening_reported_gone_without_a_confirmation_releases_the_exit()
    -> Result<(), ExecutorError> {
        with_ports!(ports);
        for (status, filled) in [("canceled", "0"), ("filled", "5"), ("expired", "0")] {
            let mut executor = held_with_a_resting_buy(&ports)?;
            let asked = executor.run(sell(EXIT, "5", "139", Purpose::RiskExit)?, &ports)?;
            assert_eq!(cancels(&asked), vec![BUY]);
            let pushed = executor.run(
                Input::BrokerUpdate(BrokerUpdate::Order(opening_order(BUY, status, filled)?)),
                &ports,
            )?;
            if status == "expired" {
                assert_eq!(submitted(&pushed), 0, "{:?}", drafted(&pushed));
                let due = executor.run(Input::Tick(RiskClock::from_secs(15)), &ports)?;
                assert_eq!(queried(&due), vec![BUY]);
                assert_eq!(submitted(&due), 1, "{status}");
                let answered = executor.run(buy_reported(status, filled)?, &ports)?;
                assert_eq!(submitted(&answered), 0, "{status}");
            } else {
                assert_eq!(submitted(&pushed), 1, "{status}: {:?}", drafted(&pushed));
            }
        }
        Ok(())
    }

    /// #286 round 1, B1, the reviewer's third path: the opening fills in full while its cancel is
    /// outstanding, and the exit goes in that step; the broker's later refusal of the cancel, on
    /// an order that has since filled, sends nothing again and queries nothing.
    #[test]
    fn an_opening_filled_while_its_cancel_is_outstanding_releases_the_exit()
    -> Result<(), ExecutorError> {
        with_ports!(ports);
        let mut executor = held_with_a_resting_buy(&ports)?;
        executor.run(sell(EXIT, "5", "139", Purpose::RiskExit)?, &ports)?;
        let fill = executor.run(opening_filled(BUY, "5")?, &ports)?;
        assert_eq!(submitted(&fill), 1, "{:?}", drafted(&fill));
        let late = executor.run(refused(BUY, "order is already filled"), &ports)?;
        assert!(
            submitted(&late) == 0 && queried(&late).is_empty(),
            "{:?}",
            drafted(&late)
        );
        Ok(())
    }

    /// #286 round 1, B1: a cancel refused while the broker has moved the opening out of
    /// `PendingCancel` — back to resting, or part filled — is refused all the same: the opening is
    /// queried at once and never marked rejected, and the exit goes with the query (DEC-160 (18)).
    #[test]
    fn a_cancel_refused_after_the_opening_moved_on_is_queried_at_once() -> Result<(), ExecutorError>
    {
        with_ports!(ports);
        for (status, filled) in [("new", "0"), ("partially_filled", "2")] {
            let mut executor = held_with_a_resting_buy(&ports)?;
            executor.run(sell(EXIT, "5", "139", Purpose::RiskExit)?, &ports)?;
            executor.run(buy_reported(status, filled)?, &ports)?;
            let refusal = executor.run(refused(BUY, "order is not cancelable"), &ports)?;
            assert_eq!(queried(&refusal), vec![BUY], "{status}");
            assert_ne!(
                executor
                    .state
                    .orders
                    .get(&ClientOrderId::parse(BUY)?)
                    .map(|order| order.state),
                Some(OrderState::Rejected),
                "{status}"
            );
            assert_eq!(submitted(&refusal), 1, "{status}");
            let answered = executor.run(buy_reported(status, filled)?, &ports)?;
            assert_eq!(submitted(&answered), 0, "{status}");
        }
        Ok(())
    }

    /// The protected ten AAPL with an opening buy of 5 by `agent`, acknowledged or not.
    fn protected_with_an_opening(
        ports: &Ports<'_>,
        agent: &str,
        acknowledged: bool,
    ) -> Result<Executor, ExecutorError> {
        let mut executor = protected(ports)?;
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
                ("purpose", text("open")),
            ],
        )?;
        if acknowledged {
            committed(
                &mut executor,
                "OrderStateChanged",
                vec![("client_order_id", text(BUY)), ("state", text("accepted"))],
            )?;
        }
        executor.run(quote("150")?, ports)?;
        Ok(executor)
    }

    /// The instrument's open interval: when it started, and whether it has been alerted.
    fn open_interval(executor: &Executor) -> Option<(i64, bool)> {
        executor
            .state
            .unprotected
            .iter()
            .find(|interval| interval.ended_at.is_none())
            .map(|interval| (interval.started_at.secs(), interval.alerted))
    }

    fn abandoned_for(effects: &[Effect]) -> Vec<&str> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Journal(draft) if draft.event_type == "OrderAbandoned" => {
                    draft.payload.get("reason").and_then(Value::as_str)
                }
                _ => None,
            })
            .collect()
    }

    /// §5.4's bound, reached by a passive exit that waited with nothing resting: the exit is
    /// abandoned, the owner alerted, and the recorded OCO re-placed for the whole position in the
    /// same step, which ends the interval.
    fn assert_bound_fired_at_30(
        executor: &mut Executor,
        ports: &Ports<'_>,
    ) -> Result<(), ExecutorError> {
        let early = executor.run(Input::Tick(RiskClock::from_secs(29)), ports)?;
        assert!(
            actions(&early).is_empty() && ocos(&early).is_empty(),
            "no bound, and the old OCO is never re-placed while the exit waits: {:?}",
            drafted(&early)
        );
        let due = executor.run(Input::Tick(RiskClock::from_secs(30)), ports)?;
        assert_eq!(abandoned_for(&due), vec!["unprotected_interval_limit"]);
        assert_eq!(
            actions(&due),
            vec!["interval_limit", "placed", "unprotected_end"]
        );
        assert!(due.iter().any(|effect| matches!(
            effect,
            Effect::Notify(note) if note.message_key == "unprotected_interval_limit"
        )));
        assert_eq!(
            ocos(&due)
                .into_iter()
                .map(|(_, qty, legs, purpose)| (qty, legs, purpose))
                .collect::<Vec<_>>(),
            vec![(
                Qty::parse("10")?,
                legs("170", "140", "10")?,
                Purpose::Protective
            )]
        );
        assert_eq!(open_interval(executor), None);
        assert!(executor.state.exiting.is_empty(), "no stale sequence");
        assert_eq!(stop_covered(executor)?, position(executor)?);
        Ok(())
    }

    /// #286 round 2, M2′, the reviewer's first path: a passive exit waits on its agent's opening,
    /// which the broker never had. Once the OCO's cancel is confirmed the passive sequence opens
    /// its interval; at rule 5's bound the opening is queried as an `Unknown`, the query 404s and
    /// the gate holds the exit on the `Unknown`; at `max_unprotected_s` the bound fires. An
    /// interval the instrument had before, ended, is no open one.
    #[test]
    fn a_passive_exit_waiting_on_an_opening_that_404s_is_bounded() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Tiered,
            config: &config,
            fees: &fees,
        };
        for earlier in [false, true] {
            let mut executor = protected_with_an_opening(&ports, "agent-a", false)?;
            if earlier {
                for action in ["unprotected_start", "unprotected_end"] {
                    committed(
                        &mut executor,
                        "ProtectionChanged",
                        vec![("instrument", text("AAPL")), ("action", text(action))],
                    )?;
                }
            }
            a_passive_exit_waits_on_a_404_until_the_bound(executor, &ports)?;
        }
        Ok(())
    }

    fn a_passive_exit_waits_on_a_404_until_the_bound(
        mut executor: Executor,
        ports: &Ports<'_>,
    ) -> Result<(), ExecutorError> {
        let asked = executor.run(sell(EXIT, "5", "160", Purpose::DiscretionaryExit)?, ports)?;
        assert_eq!(actions(&asked), vec!["passive_start"]);
        assert_eq!(open_interval(&executor), None, "the OCO still rests");
        let confirmed = executor.run(cancel_accepted(OCO), ports)?;
        assert_eq!(actions(&confirmed), vec!["cancelled", "unprotected_start"]);
        assert_eq!(submitted(&confirmed), 0, "the exit waits on the opening");
        assert_eq!(open_interval(&executor), Some((0, false)));
        let overdue = executor.run(Input::Tick(RiskClock::from_secs(15)), ports)?;
        assert_eq!(queried(&overdue), vec![BUY]);
        assert_eq!(verdicts(&overdue), vec!["hold"]);
        assert!(actions(&overdue).is_empty(), "{:?}", drafted(&overdue));
        let absent = executor.run(
            Input::Broker(Ok(BrokerOutcome::Absent {
                client_order_id: BUY.to_owned(),
            })),
            ports,
        )?;
        assert!(submitted(&absent) == 0 && actions(&absent).is_empty());
        assert_eq!(open_interval(&executor), Some((0, false)), "opened once");
        assert_bound_fired_at_30(&mut executor, ports)
    }

    /// #286 round 2, M2′, the reviewer's second path: another agent's opening in the instrument
    /// goes `Unknown` on a transport failure while the passive exit's cancel is outstanding, and
    /// the query's answer is lost. The confirmation re-gates the exit, which rule 5 does not hold
    /// back, and the gate holds it on the `Unknown`; the interval opens in that step, and the bound
    /// fires at `max_unprotected_s`.
    #[test]
    fn a_passive_exit_held_on_an_unknown_is_bounded() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Tiered,
            config: &config,
            fees: &fees,
        };
        let mut executor = protected_with_an_opening(&ports, "agent-b", false)?;
        executor.run(sell(EXIT, "5", "160", Purpose::DiscretionaryExit)?, &ports)?;
        let lost = executor.run(Input::Broker(Err(BrokerUnknown::Transport)), &ports)?;
        assert_eq!(queried(&lost), vec![BUY]);
        let confirmed = executor.run(cancel_accepted(OCO), &ports)?;
        assert_eq!(verdicts(&confirmed), vec!["hold"]);
        assert_eq!(actions(&confirmed), vec!["cancelled", "unprotected_start"]);
        assert!(
            executor
                .state
                .held
                .contains(&IntentId(EventId(EXIT.to_owned())))
        );
        assert_eq!(open_interval(&executor), Some((0, false)));
        assert_bound_fired_at_30(&mut executor, &ports)
    }

    /// #286 round 2, M2′: a passive exit waiting on an opening the broker acknowledged and then
    /// lost goes at rule 5's bound, and its placement ends the interval the wait opened; the
    /// interval's own bound never fires.
    #[test]
    fn a_passive_exits_placement_ends_the_interval_its_wait_opened() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Tiered,
            config: &config,
            fees: &fees,
        };
        let mut executor = protected_with_an_opening(&ports, "agent-a", true)?;
        let asked = executor.run(sell(EXIT, "5", "160", Purpose::DiscretionaryExit)?, &ports)?;
        assert_eq!(cancels(&asked), vec![BUY, OCO]);
        let confirmed = executor.run(cancel_accepted(OCO), &ports)?;
        assert_eq!(actions(&confirmed), vec!["cancelled", "unprotected_start"]);
        let placed = executor.run(Input::Tick(RiskClock::from_secs(15)), &ports)?;
        assert_eq!(queried(&placed), vec![BUY]);
        assert_eq!(
            actions(&placed),
            vec!["placed", "placed", "unprotected_end"]
        );
        assert_eq!(
            executor
                .state
                .unprotected
                .last()
                .map(|interval| (interval.started_at.secs(), interval.ended_at)),
            Some((0, Some(RiskClock::from_secs(15))))
        );
        executor.run(
            Input::Broker(Ok(BrokerOutcome::Absent {
                client_order_id: BUY.to_owned(),
            })),
            &ports,
        )?;
        let later = executor.run(Input::Tick(RiskClock::from_secs(30)), &ports)?;
        assert!(actions(&later).is_empty(), "{:?}", drafted(&later));
        assert_eq!(stop_covered(&executor)?, position(&executor)?);
        Ok(())
    }

    /// §5.4's bound ends every exit in the instrument, not the sequence's alone: `settle`
    /// re-places protection only once none is working, so one left working would leave the
    /// position unprotected past the bound (#286 round 2, M1′ and M2′).
    #[test]
    fn the_bound_ends_every_exit_in_the_instrument() -> Result<(), ExecutorError> {
        with_ports!(ports);
        let mut executor = protected(&ports)?;
        executor.run(quote("150")?, &ports)?;
        executor.run(sell(EXIT, "3", "139", Purpose::RiskExit)?, &ports)?;
        executor.run(sell(SECOND, "2", "139", Purpose::RiskExit)?, &ports)?;
        let gone = executor.run(cancel_accepted(OCO), &ports)?;
        assert_eq!(submitted(&gone), 2);
        let due = executor.run(Input::Tick(RiskClock::from_secs(30)), &ports)?;
        let mut cancelled = cancels(&due);
        cancelled.sort_unstable();
        let (first, second) = (format!("md-{SECOND}"), format!("md-{EXIT}"));
        assert_eq!(cancelled, vec![first.as_str(), second.as_str()]);
        assert_eq!(actions(&due), vec!["interval_limit"]);
        executor.run(cancel_accepted(&first), &ports)?;
        assert_eq!(stop_covered(&executor)?, Qty::ZERO, "one exit still works");
        let ended = executor.run(cancel_accepted(&second), &ports)?;
        assert_eq!(actions(&ended), vec!["placed", "unprotected_end"]);
        assert_eq!(stop_covered(&executor)?, position(&executor)?);
        Ok(())
    }

    fn submitted_ids(effects: &[Effect]) -> Vec<&str> {
        submissions(effects)
            .into_iter()
            .map(|order| order.client_order_id.as_str())
            .collect()
    }

    /// `agent-b`'s passive exit of 4 placed at 0 while `agent-a`'s marketable exit of 3 still
    /// waits on its own acknowledged opening (§5.3 rule 5): the placement is sized around the
    /// waiting exit (4 + 3 of stop over ten shares) and hands the sequence on.
    fn placed_around_a_waiting_exit(
        ports: &Ports<'_>,
    ) -> Result<(Executor, Vec<Effect>), ExecutorError> {
        let mut executor = protected_with_an_opening(ports, "agent-a", true)?;
        let passive = sell_as("agent-b", EXIT, "4", "160", Purpose::DiscretionaryExit)?;
        let asked = executor.run(passive, ports)?;
        assert_eq!(actions(&asked), vec!["passive_start"]);
        let waiting = executor.run(sell(SECOND, "3", "139", Purpose::RiskExit)?, ports)?;
        assert_eq!(cancels(&waiting), vec![BUY, OCO]);
        assert_eq!(submitted(&waiting), 0);
        let placed = executor.run(cancel_accepted(OCO), ports)?;
        assert_eq!(
            actions(&placed),
            vec!["cancelled", "placed", "placed", "unprotected_start"],
            "the placement hands the sequence on to the exit still waiting"
        );
        assert_eq!(
            ocos(&placed)
                .into_iter()
                .map(|(_, qty, legs, _)| (qty, legs))
                .collect::<Vec<_>>(),
            vec![
                (Qty::parse("4")?, legs("160", "140", "4")?),
                (Qty::parse("3")?, legs("170", "140", "3")?),
            ]
        );
        assert_eq!(stop_covered(&executor)?, Qty::parse("7")?);
        assert_eq!(open_interval(&executor), Some((0, false)));
        Ok((executor, placed))
    }

    /// #286 round 2, M1′: an exit waiting on its opening when a passive exit is placed beside it is
    /// netted out of the remainder, so once its opening's cancel is confirmed it goes without
    /// cancelling the placement; the stop never covers more than the position, and once the exit
    /// ends, filled or not, it covers exactly the position again.
    #[test]
    fn an_exit_the_passive_placement_was_sized_around_goes_without_cancelling_it()
    -> Result<(), ExecutorError> {
        with_ports!(ports);
        for fills in [true, false] {
            let (mut executor, _) = placed_around_a_waiting_exit(&ports)?;
            let gone = executor.run(cancel_accepted(BUY), &ports)?;
            let exit = format!("md-{SECOND}");
            assert!(cancels(&gone).is_empty(), "{:?}", cancels(&gone));
            assert_eq!(submitted_ids(&gone), vec![exit.as_str()]);
            assert_eq!(stop_covered(&executor)?, Qty::parse("7")?);
            let end = if fills {
                filled(SECOND, "3")?
            } else {
                cancel_accepted(&exit)
            };
            let ended = executor.run(end, &ports)?;
            assert!(stop_covered(&executor)? <= position(&executor)?, "{fills}");
            assert_eq!(stop_covered(&executor)?, position(&executor)?, "{fills}");
            let topped_up: Vec<Qty> = ocos(&ended).into_iter().map(|(_, qty, ..)| qty).collect();
            let expected = if fills {
                vec![]
            } else {
                vec![Qty::parse("3")?]
            };
            assert_eq!(topped_up, expected, "{fills}");
            assert!(executor.state.exiting.is_empty() && open_interval(&executor).is_none());
        }
        Ok(())
    }

    /// #286 round 2, M1′, the conservative interim: an exit the placement was not sized around
    /// cancels it all, the passive exit's own OCO too, and goes once the cancels are confirmed;
    /// the stop never covers more than the position, and once the exits end the remaining
    /// position is protected again through the same interval, at the smaller size.
    #[test]
    fn an_exit_the_placement_was_not_sized_around_cancels_it_and_protection_returns_smaller()
    -> Result<(), ExecutorError> {
        with_ports!(ports);
        let (mut executor, placed) = placed_around_a_waiting_exit(&ports)?;
        let placement: Vec<String> = submitted_ids(&placed)
            .into_iter()
            .map(str::to_owned)
            .collect();
        executor.run(cancel_accepted(BUY), &ports)?;
        let late = executor.run(sell(LATER, "2", "139", Purpose::RiskExit)?, &ports)?;
        assert_eq!(verdicts(&late), vec!["allow"]);
        let mut asked: Vec<&str> = cancels(&late);
        asked.sort_unstable();
        let mut expected: Vec<&str> = placement.iter().map(String::as_str).collect();
        expected.sort_unstable();
        assert_eq!(asked, expected, "the whole placement is cancelled");
        assert_eq!(
            submitted(&late),
            0,
            "the exit waits on those cancels: {:?}",
            drafted(&late)
        );
        for id in &placement {
            executor.run(cancel_accepted(id), &ports)?;
            assert!(stop_covered(&executor)? <= position(&executor)?);
        }
        assert!(
            executor
                .state
                .orders
                .contains_key(&ClientOrderId::parse(&format!("md-{LATER}"))?),
            "the late exit goes once the placement is gone"
        );
        executor.run(filled(SECOND, "3")?, &ports)?;
        assert_eq!(stop_covered(&executor)?, Qty::ZERO);
        let ended = executor.run(filled(LATER, "2")?, &ports)?;
        assert_eq!(
            ocos(&ended)
                .into_iter()
                .map(|(_, qty, legs, _)| (qty, legs))
                .collect::<Vec<_>>(),
            vec![(Qty::parse("5")?, legs("170", "140", "5")?)]
        );
        assert_eq!(stop_covered(&executor)?, position(&executor)?);
        assert!(executor.state.exiting.is_empty() && open_interval(&executor).is_none());
        Ok(())
    }

    /// #286 round 2, M1′ with M2′'s bound: a handed-on sequence's interval is bounded like any
    /// other. At `max_unprotected_s` the exit beside the placement is cancelled if it has an
    /// order, or abandoned if it still waits (here on an opening whose rule-5 bound is set past
    /// the interval's); the placement itself is left alone, and what the exit leaves unsold is
    /// protected again.
    #[test]
    fn a_handed_on_sequence_is_bounded_by_ending_the_exit_beside_it() -> Result<(), ExecutorError> {
        let (config, fees) = (
            ExecutorConfig {
                unknown_absent_window_s: 60,
                ..executor_config()
            },
            fees()?,
        );
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        for waits in [false, true] {
            let (mut executor, _) = placed_around_a_waiting_exit(&ports)?;
            let exit = format!("md-{SECOND}");
            if !waits {
                executor.run(cancel_accepted(BUY), &ports)?;
            }
            let early = executor.run(Input::Tick(RiskClock::from_secs(29)), &ports)?;
            assert!(actions(&early).is_empty() && cancels(&early).is_empty());
            let due = executor.run(Input::Tick(RiskClock::from_secs(30)), &ports)?;
            assert!(due.iter().any(|effect| matches!(
                effect,
                Effect::Notify(note) if note.message_key == "unprotected_interval_limit"
            )));
            if waits {
                assert_eq!(abandoned_for(&due), vec!["unprotected_interval_limit"]);
                assert!(cancels(&due).is_empty(), "{:?}", cancels(&due));
                assert_eq!(
                    actions(&due),
                    vec!["interval_limit", "placed", "unprotected_end"]
                );
            } else {
                assert!(abandoned_for(&due).is_empty());
                assert_eq!(cancels(&due), vec![exit.as_str()]);
                assert_eq!(actions(&due), vec!["interval_limit"]);
                let confirmed = executor.run(cancel_accepted(&exit), &ports)?;
                assert_eq!(actions(&confirmed), vec!["placed", "unprotected_end"]);
            }
            assert_eq!(stop_covered(&executor)?, position(&executor)?, "{waits}");
            assert!(
                executor.state.exiting.is_empty() && open_interval(&executor).is_none(),
                "{waits}"
            );
        }
        Ok(())
    }

    /// One step of a random script: the clock moves, an exit of one share arrives, the broker
    /// speaks about one of the openings, loses one it had acknowledged (every later query of it
    /// 404s), or starts or stops dropping the answers to queries of one.
    #[derive(Clone, Copy, Debug)]
    enum Step {
        Tick(i64),
        Exit,
        Report(usize),
        Become(usize, &'static str),
        Fill(usize),
        Confirm(usize),
        Refuse(usize),
        Forget(usize),
        Mute(usize),
    }

    /// `agent-a`'s openings as the script starts, each `(acknowledged, reached the broker)`, and
    /// its steps.
    #[derive(Clone, Debug)]
    struct Script {
        openings: Vec<(bool, bool)>,
        steps: Vec<Step>,
    }

    fn steps() -> impl Strategy<Value = Step> {
        let statuses = vec![
            "partially_filled",
            "filled",
            "canceled",
            "expired",
            "rejected",
        ];
        prop_oneof![
            4 => (1_i64..=6).prop_map(Step::Tick),
            2 => Just(Step::Exit),
            1 => (0_usize..2).prop_map(Step::Report),
            1 => (0_usize..2, prop::sample::select(statuses))
                .prop_map(|(which, status)| Step::Become(which, status)),
            1 => (0_usize..2).prop_map(Step::Fill),
            2 => (0_usize..2).prop_map(Step::Confirm),
            1 => (0_usize..2).prop_map(Step::Refuse),
            1 => (0_usize..2).prop_map(Step::Forget),
            1 => (0_usize..2).prop_map(Step::Mute),
        ]
    }

    fn scripts() -> impl Strategy<Value = Script> {
        (
            prop::collection::vec((any::<bool>(), any::<bool>()), 1..=2),
            prop::collection::vec(steps(), 1..48),
        )
            .prop_map(|(openings, steps)| Script { openings, steps })
    }

    /// The broker's side of one opening: its status (`None` for a submission that never reached
    /// it, or one it has since lost), the quantity it filled, whether a cancel of it is
    /// outstanding, and whether the answers to queries of it are dropped.
    struct Venue {
        id: &'static str,
        status: Option<&'static str>,
        filled: u8,
        asked: bool,
        mute: bool,
    }

    impl Venue {
        fn live(&self) -> bool {
            matches!(self.status, Some("new" | "partially_filled"))
        }

        fn answer(&self) -> Result<Input, ExecutorError> {
            Ok(Input::Broker(Ok(match self.status {
                Some(status) => {
                    BrokerOutcome::Order(opening_order(self.id, status, &self.filled.to_string())?)
                }
                None => BrokerOutcome::Absent {
                    client_order_id: self.id.to_owned(),
                },
            })))
        }
    }

    /// One exit as the journal tells it: when its wait began — its latest `GateDecided` allowing
    /// it, or its arrival — its latest verdict and reason, and whether it has been submitted,
    /// denied or abandoned.
    struct Watched {
        intent: String,
        since: i64,
        verdict: String,
        reason: String,
        done: bool,
    }

    /// The gate's reasons for holding an exit, which alone `AGENTS.md` rule 13 lets hold one: an
    /// `Unknown` order in its instrument, the agent `paused` or `stopped`, or the broker (the
    /// code a blocked account holds an exit with, gate.rs `account_failure`).
    const RULE_13_HOLDS: [&str; 4] = [
        "unknown_order_in_flight",
        "agent_paused",
        "agent_stopped",
        "broker",
    ];

    /// One opening as the journal tells it: its state, and whether a wait on it went overdue.
    struct Seen {
        state: String,
        overdue: bool,
    }

    /// The oracle, derived from the drafts alone and independently of the fold.
    struct Watch {
        now: i64,
        exits: BTreeMap<String, Watched>,
        openings: BTreeMap<String, Seen>,
    }

    impl Watch {
        fn saw(&mut self, draft: &EventDraft) -> Result<(), String> {
            let field = |name: &str| draft.payload.get(name).and_then(Value::as_str);
            match draft.event_type.as_str() {
                "GateDecided" => {
                    let verdict = field("verdict").unwrap_or_default();
                    for exit in self.exits.values_mut() {
                        if field("intent_id") == Some(exit.intent.as_str()) {
                            verdict.clone_into(&mut exit.verdict);
                            field("reason_code")
                                .unwrap_or_default()
                                .clone_into(&mut exit.reason);
                            exit.since = if verdict == "allow" {
                                self.now
                            } else {
                                exit.since
                            };
                            exit.done = exit.done || verdict == "deny";
                        }
                    }
                }
                "OrderAbandoned" => {
                    for exit in self.exits.values_mut() {
                        exit.done = exit.done || field("intent_id") == Some(exit.intent.as_str());
                    }
                }
                "OrderSubmitted" => {
                    let id = field("client_order_id").unwrap_or_default();
                    if let Some(exit) = self.exits.get_mut(id) {
                        let waited_on: Vec<&String> = self
                            .openings
                            .iter()
                            .filter(|(_, seen)| {
                                !seen.overdue
                                    && !matches!(
                                        seen.state.as_str(),
                                        "filled" | "canceled" | "rejected" | "expired" | "intent"
                                    )
                            })
                            .map(|(id, _)| id)
                            .collect();
                        if !waited_on.is_empty() {
                            return Err(format!("{id} went while {waited_on:?} were live"));
                        }
                        exit.done = true;
                    }
                    if let Some(seen) = self.openings.get_mut(id) {
                        "submitting".clone_into(&mut seen.state);
                    }
                }
                "OrderStateChanged" => {
                    let id = field("client_order_id").unwrap_or_default();
                    if let Some(seen) = self.openings.get_mut(id) {
                        seen.overdue = seen.overdue
                            || draft.payload.get("cancel_overdue") == Some(&Value::Bool(true));
                        field("state")
                            .unwrap_or_default()
                            .clone_into(&mut seen.state);
                    }
                }
                _ => {}
            }
            Ok(())
        }

        /// No exit is neither submitted nor denied once its wait has lasted
        /// `unknown_absent_window_s`, except one the gate holds for a reason rule 13 names.
        fn bounded(&self, window: i64) -> Result<(), String> {
            for (id, exit) in &self.exits {
                let exempt =
                    exit.verdict == "hold" && RULE_13_HOLDS.contains(&exit.reason.as_str());
                if !exit.done && !exempt && self.now.saturating_sub(exit.since) >= window {
                    return Err(format!(
                        "{id} ({} {}) waited from {} to {}",
                        exit.verdict, exit.reason, exit.since, self.now
                    ));
                }
            }
            Ok(())
        }
    }

    /// Hands one input to the executor, and the broker answers every query of an opening at
    /// once, unless it drops that opening's answers, until nothing more is asked.
    fn deliver(
        executor: &mut Executor,
        ports: &Ports<'_>,
        venues: &mut [Venue],
        watch: &mut Watch,
        input: Input,
    ) -> Result<(), String> {
        let failed = |error: ExecutorError| format!("{error:?}");
        let mut inputs = VecDeque::from([input]);
        let mut budget = 64_u32;
        while let Some(input) = inputs.pop_front() {
            budget = budget
                .checked_sub(1)
                .ok_or("the broker's answers never settle")?;
            for effect in executor.run(input, ports).map_err(failed)? {
                match effect {
                    Effect::Journal(draft) => watch.saw(&draft)?,
                    Effect::Broker(BrokerRequest::Cancel { client_order_id }) => {
                        for venue in venues
                            .iter_mut()
                            .filter(|venue| venue.id == client_order_id.as_str())
                        {
                            venue.asked = true;
                        }
                    }
                    Effect::Broker(BrokerRequest::Submit(order)) => {
                        for venue in venues
                            .iter_mut()
                            .filter(|venue| venue.id == order.client_order_id.as_str())
                        {
                            venue.status = venue.status.or(Some("new"));
                        }
                    }
                    Effect::Broker(BrokerRequest::GetOrderByClientId(id)) => {
                        for venue in venues
                            .iter()
                            .filter(|venue| venue.id == id.as_str() && !venue.mute)
                        {
                            inputs.push_back(venue.answer().map_err(failed)?);
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn played(script: &Script) -> Result<(), String> {
        let failed = |error: ExecutorError| format!("{error:?}");
        let (config, fees) = (executor_config(), fees().map_err(failed)?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = held(&ports).map_err(failed)?;
        let mut venues = Vec::new();
        let mut watch = Watch {
            now: 0,
            exits: BTreeMap::new(),
            openings: BTreeMap::new(),
        };
        for (id, (acknowledged, reached)) in
            ["md-buy-2", "md-buy-3"].into_iter().zip(&script.openings)
        {
            committed(
                &mut executor,
                "OrderSubmitted",
                vec![
                    ("client_order_id", text(id)),
                    ("agent", text("agent-a")),
                    ("instrument", text("AAPL")),
                    ("side", text("buy")),
                    ("qty", text("5")),
                    ("limit", text("150")),
                    ("purpose", text("open")),
                ],
            )
            .map_err(failed)?;
            let state = if *acknowledged {
                "accepted"
            } else {
                "submitting"
            };
            if *acknowledged {
                committed(
                    &mut executor,
                    "OrderStateChanged",
                    vec![("client_order_id", text(id)), ("state", text(state))],
                )
                .map_err(failed)?;
            }
            venues.push(Venue {
                id,
                status: (*acknowledged || *reached).then_some("new"),
                filled: 0,
                asked: false,
                mute: false,
            });
            watch.openings.insert(
                id.to_owned(),
                Seen {
                    state: state.to_owned(),
                    overdue: false,
                },
            );
        }
        let mut exits = 0_u32;
        for step in &script.steps {
            let input = match *step {
                Step::Tick(by) => {
                    watch.now = watch.now.saturating_add(by);
                    Some(Input::Tick(RiskClock::from_secs(watch.now)))
                }
                Step::Exit => {
                    exits = exits.saturating_add(1);
                    let intent = format!("01JABCDEFGHJKMNPQRSTVWX{exits:03}");
                    watch.exits.insert(
                        format!("md-{intent}"),
                        Watched {
                            intent: intent.clone(),
                            since: watch.now,
                            verdict: String::new(),
                            reason: String::new(),
                            done: false,
                        },
                    );
                    Some(sell(&intent, "1", "139", Purpose::RiskExit).map_err(failed)?)
                }
                Step::Report(which) => venues
                    .get(which)
                    .map(Venue::answer)
                    .transpose()
                    .map_err(failed)?,
                Step::Become(which, status) => match venues.get_mut(which) {
                    Some(venue) if venue.live() => {
                        venue.status = Some(status);
                        venue.filled = match status {
                            "filled" => 5,
                            "partially_filled" => venue.filled.max(2),
                            _ => venue.filled,
                        };
                        let order = opening_order(venue.id, status, &venue.filled.to_string());
                        Some(Input::BrokerUpdate(BrokerUpdate::Order(
                            order.map_err(failed)?,
                        )))
                    }
                    _ => None,
                },
                Step::Fill(which) => match venues.get_mut(which) {
                    Some(venue) if venue.live() => {
                        let rest = 5_u8.saturating_sub(venue.filled);
                        venue.status = Some("filled");
                        venue.filled = 5;
                        Some(opening_filled(venue.id, &rest.to_string()).map_err(failed)?)
                    }
                    _ => None,
                },
                Step::Confirm(which) => match venues.get_mut(which) {
                    Some(venue) if venue.asked => {
                        venue.asked = false;
                        Some(if venue.live() {
                            venue.status = Some("canceled");
                            cancel_accepted(venue.id)
                        } else {
                            refused(venue.id, "order is not cancelable")
                        })
                    }
                    _ => None,
                },
                Step::Refuse(which) => match venues.get_mut(which) {
                    Some(venue) if venue.asked => {
                        venue.asked = false;
                        Some(refused(venue.id, "order is not cancelable"))
                    }
                    _ => None,
                },
                Step::Forget(which) => {
                    if let Some(venue) = venues.get_mut(which) {
                        venue.status = None;
                    }
                    None
                }
                Step::Mute(which) => {
                    if let Some(venue) = venues.get_mut(which) {
                        venue.mute = !venue.mute;
                    }
                    None
                }
            };
            if let Some(input) = input {
                deliver(&mut executor, &ports, &mut venues, &mut watch, input)?;
            }
            watch.bounded(config.unknown_absent_window_s)?;
        }
        Ok(())
    }

    /// #286 round 1, B1, and round 2, B1′: over random scripts of ticks, exits, acknowledgments,
    /// broker reports, fills, cancels confirmed or refused, openings the broker loses after
    /// acknowledging them, and query answers dropped, against openings acknowledged or not and
    /// submissions that reached the broker or not, no exit stays neither submitted nor denied
    /// past `unknown_absent_window_s` plus one tick, except one the gate holds for a reason rule
    /// 13 names; and no exit is submitted while an opening of its agent is live, unless the wait
    /// on it went overdue. The oracle reads the journal's drafts, not the fold.
    #[test]
    fn no_exit_waits_past_the_bound_but_under_a_rule_13_hold() -> Result<(), String> {
        let config = ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        };
        TestRunner::new(config)
            .run(&scripts(), |script| {
                played(&script).map_err(TestCaseError::fail)
            })
            .map_err(|error| error.to_string())
    }
    /// DEC-260 (2): a trade seen on an earlier quote is still "the last trade" (§5.6) after a newer
    /// quote that carries none and no sane bid, so the exit is laddered from it, not fallen back.
    #[test]
    fn the_last_trade_outlives_a_newer_quote_without_one() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let mut executor = protected(&ports)?;
        executor.run(observation(None, Some(150), true, 0)?, &ports)?;
        executor.run(observation(Some(152), None, false, 5)?, &ports)?;
        executor.run(sell(EXIT, "5", "160", Purpose::RiskExit)?, &ports)?;
        let sent = executor.run(cancel_accepted(OCO), &ports)?;
        assert_eq!(
            submissions(&sent)
                .iter()
                .map(|order| order.limit_price)
                .collect::<Vec<_>>(),
            vec![Some(Price::parse("149.25")?)]
        );
        assert!(alerts(&sent).is_empty(), "{:?}", drafted(&sent));
        Ok(())
    }

    /// §2.1: a rung is a sell limit on the equity tick, rounded up. 141 × (1 − 0.5%) is 140.295,
    /// which goes as 140.30, never as a sub-penny limit the broker would refuse.
    #[test]
    fn a_rung_is_on_the_equity_tick() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let mut executor = protected(&ports)?;
        executor.run(observation(Some(141), None, true, 0)?, &ports)?;
        executor.run(sell(EXIT, "5", "141", Purpose::RiskExit)?, &ports)?;
        let sent = executor.run(cancel_accepted(OCO), &ports)?;
        assert_eq!(
            submissions(&sent)
                .iter()
                .map(|order| order.limit_price)
                .collect::<Vec<_>>(),
            vec![Some(Price::parse("140.3")?)]
        );
        executor.run(Input::Tick(RiskClock::from_secs(10)), &ports)?;
        let next = executor.run(cancel_accepted(&rung_id(0)), &ports)?;
        assert_eq!(
            submissions(&next)
                .iter()
                .map(|order| order.limit_price)
                .collect::<Vec<_>>(),
            vec![Some(Price::parse("139.59")?)],
            "141 × (1 − 1%) is 139.59 exactly"
        );
        Ok(())
    }

    /// §5.3 rules 3 and 4 (DEC-260): a partly filled exit counts only what it may still sell, since
    /// its fills already left the position. Ten held, an exit of 4 filled 2, so 8 are held and 2
    /// still selling: a second exit of 6 crosses nothing and is allowed; one of 7 would cross zero.
    #[test]
    fn a_partly_filled_exit_counts_only_what_it_may_still_sell() -> Result<(), ExecutorError> {
        with_ports!(ports);
        for (qty, verdict) in [("6", "allow"), ("7", "deny")] {
            let mut executor = held(&ports)?;
            executor.run(sell(EXIT, "4", "139", Purpose::RiskExit)?, &ports)?;
            executor.run(
                Input::Broker(Ok(BrokerOutcome::Submitted(BrokerOrder {
                    side: Side::Sell,
                    qty: Qty::parse("4")?,
                    ..opening_order(&format!("md-{EXIT}"), "new", "0")?
                }))),
                &ports,
            )?;
            executor.run(filled(EXIT, "2")?, &ports)?;
            let second = executor.run(sell(SECOND, qty, "139", Purpose::RiskExit)?, &ports)?;
            assert_eq!(
                verdicts(&second),
                vec![verdict],
                "{qty}: {:?}",
                drafted(&second)
            );
        }
        Ok(())
    }

    /// DEC-260 (5), #373 round 1, blocker 1: an insane print is no last trade. A sane market at
    /// 150 more than five minutes ago, then a print at 1000 on an insane quote: the risk exit
    /// prices nothing from the print and goes at its own limit at once, with the alert.
    #[test]
    fn an_insane_print_never_prices_a_rung() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let mut executor = protected(&ports)?;
        executor.run(observation(Some(150), Some(150), true, 0)?, &ports)?;
        executor.run(Input::Tick(RiskClock::from_secs(400)), &ports)?;
        executor.run(observation(None, Some(1000), false, 400)?, &ports)?;
        executor.run(sell(EXIT, "5", "151", Purpose::RiskExit)?, &ports)?;
        let sent = executor.run(cancel_accepted(OCO), &ports)?;
        assert_eq!(
            submissions(&sent)
                .iter()
                .map(|order| order.limit_price)
                .collect::<Vec<_>>(),
            vec![Some(Price::parse("151")?)]
        );
        assert_eq!(alerts(&sent), vec!["exit_unpriced"], "{:?}", drafted(&sent));
        Ok(())
    }

    /// DEC-260 (5), #373 round 2, minor B: an insane print never displaces the last sane trade.
    /// A sane trade at 150 at 0, then an insane print at 1000 at 5: an exit at 5 still prices
    /// from 150 (149.25) with no alert; once that trade is more than five minutes old, at 400, an
    /// exit takes the fallback at its own limit with the alert.
    #[test]
    fn an_insane_print_never_displaces_the_last_sane_trade() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        for (at, limit, alerted) in [(5, "149.25", false), (400, "151", true)] {
            let mut executor = protected(&ports)?;
            executor.run(observation(None, Some(150), true, 0)?, &ports)?;
            executor.run(Input::Tick(RiskClock::from_secs(5)), &ports)?;
            executor.run(observation(None, Some(1000), false, 5)?, &ports)?;
            executor.run(Input::Tick(RiskClock::from_secs(at)), &ports)?;
            executor.run(sell(EXIT, "5", "151", Purpose::RiskExit)?, &ports)?;
            let sent = executor.run(cancel_accepted(OCO), &ports)?;
            assert_eq!(
                submissions(&sent)
                    .iter()
                    .map(|order| order.limit_price)
                    .collect::<Vec<_>>(),
                vec![Some(Price::parse(limit)?)],
                "at {at}"
            );
            assert_eq!(
                alerts(&sent),
                if alerted {
                    vec!["exit_unpriced"]
                } else {
                    Vec::new()
                },
                "at {at}: {:?}",
                drafted(&sent)
            );
        }
        Ok(())
    }

    /// DEC-260 (5), #373 round 1, blocker 1: a trade 100,000 s old prices nothing either, though
    /// its quote was sane; the exit falls back at once, alerted.
    #[test]
    fn an_old_trade_never_prices_a_rung() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let mut executor = protected(&ports)?;
        executor.run(observation(None, Some(150), true, 0)?, &ports)?;
        executor.run(Input::Tick(RiskClock::from_secs(100_000)), &ports)?;
        executor.run(sell(EXIT, "5", "151", Purpose::RiskExit)?, &ports)?;
        let sent = executor.run(cancel_accepted(OCO), &ports)?;
        assert_eq!(
            submissions(&sent)
                .iter()
                .map(|order| order.limit_price)
                .collect::<Vec<_>>(),
            vec![Some(Price::parse("151")?)]
        );
        assert_eq!(alerts(&sent), vec!["exit_unpriced"], "{:?}", drafted(&sent));
        Ok(())
    }

    /// #373 round 1, minor 2: market data that arrives out of order is read by when it was
    /// observed. A sane bid of 150 observed at 10 arrives first, then one of 141 observed at 5:
    /// the newer-observed 150 prices the rung (149.25), not the later-arrived 141.
    #[test]
    fn the_newer_observed_sane_bid_wins_over_a_later_arrival() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let mut executor = protected(&ports)?;
        executor.run(Input::Tick(RiskClock::from_secs(10)), &ports)?;
        executor.run(observation(Some(150), None, true, 10)?, &ports)?;
        executor.run(observation(Some(141), None, true, 5)?, &ports)?;
        executor.run(sell(EXIT, "5", "141", Purpose::RiskExit)?, &ports)?;
        let sent = executor.run(cancel_accepted(OCO), &ports)?;
        assert_eq!(
            submissions(&sent)
                .iter()
                .map(|order| order.limit_price)
                .collect::<Vec<_>>(),
            vec![Some(Price::parse("149.25")?)]
        );
        Ok(())
    }

    /// A quote observed at `at`: its bid (also its mark, so a bid at or below the stop at 140 is a
    /// breach the watchdog counts), its last trade, and whether it is sane.
    fn observation(
        bid: Option<u32>,
        trade: Option<u32>,
        sane: bool,
        at: i64,
    ) -> Result<Input, ExecutorError> {
        let price = |value: Option<u32>| {
            value
                .map(|value| Price::parse(&value.to_string()))
                .transpose()
        };
        Ok(Input::Market(MarketObservation {
            instrument: aapl()?,
            bid: price(bid)?,
            bid_size: bid.map(|_| Qty::parse("100")).transpose()?,
            ask: None,
            last_trade: price(trade)?,
            mark: price(bid)?,
            sane,
            observed_at: RiskClock::from_secs(at),
        }))
    }

    /// One move of a rule-13 script.
    #[derive(Clone, Copy, Debug)]
    enum Move {
        Tick(i64),
        Quote(Option<u32>, Option<u32>, bool),
        Exit(usize, u32, u32),
        Ack,
        Fill(u32),
        Confirm,
        Pause(bool),
    }

    const EXIT_PURPOSES: [Purpose; 4] = [
        Purpose::RiskExit,
        Purpose::OwnerExit,
        Purpose::DiscretionaryExit,
        Purpose::Flatten,
    ];

    fn moves() -> impl Strategy<Value = Move> {
        let price = || prop::option::of(135_u32..=160);
        prop_oneof![
            4 => (1_i64..=20).prop_map(Move::Tick),
            1 => prop::sample::select(vec![1_800_i64, 3_600, 7_200]).prop_map(Move::Tick),
            3 => (price(), price(), any::<bool>()).prop_map(|(bid, trade, sane)| Move::Quote(bid, trade, sane)),
            2 => (0_usize..4, 1_u32..=4, 141_u32..=165).prop_map(|(purpose, qty, limit)| Move::Exit(purpose, qty, limit)),
            2 => Just(Move::Ack),
            2 => (1_u32..=3).prop_map(Move::Fill),
            3 => Just(Move::Confirm),
            1 => any::<bool>().prop_map(Move::Pause),
        ]
    }

    /// One order at the broker, as the script's broker holds it.
    struct Held {
        purpose: Purpose,
        qty: u32,
        filled: u32,
        acked: bool,
        live: bool,
    }

    /// One exit as the oracle follows it from the drafts: its purpose and quantity, when its
    /// wait began (its arrival or its latest allow), its latest verdict and reason, and whether
    /// it was submitted, denied or abandoned.
    struct Followed {
        purpose: Purpose,
        qty: u32,
        arrived: i64,
        since: i64,
        verdict: String,
        reason: String,
        done: bool,
        watchdog: bool,
    }

    /// The rule-13 oracle's own record: what the script fed in, what the broker holds, and each
    /// exit's fate, none of it read from the executor's state.
    struct Desk {
        now: i64,
        quotes: Vec<(i64, Option<u32>, Option<u32>, bool)>,
        position: u32,
        venue: BTreeMap<String, Held>,
        asked: Vec<String>,
        exits: BTreeMap<String, Followed>,
        fills: u32,
        paused: bool,
        quiet_since: i64,
    }

    /// 2018-01-01 00:00 ET, the calendar's first date: before it (the 1970 base, the eve of 2018)
    /// every instant reads as the regular session (DEC-260 (13)); from it on, the calendar names
    /// the session, even in a script that started before it (DEC-392).
    const CALENDAR_FROM: i64 = 1_514_782_800;

    /// §4.3 written out for the oracle, never read from the calendar file: the New York midnight
    /// of each trading day its scripts can reach (the calendar's first week, 2018-01-02 to 05
    /// after the New Year holiday, 2026-09-01 to 10-16 without Labor Day, and the calendar's last
    /// week, 2028-12-26 to 29), from which each day's overnight (20:00 the day before to 04:00),
    /// pre-market, regular (09:30 to 16:00) and after-hours (to 20:00) follow.
    const TRADING_DAYS: [i64; 41] = [
        1_514_869_200,
        1_514_955_600,
        1_515_042_000,
        1_515_128_400,
        1_788_235_200,
        1_788_321_600,
        1_788_408_000,
        1_788_494_400,
        1_788_840_000,
        1_788_926_400,
        1_789_012_800,
        1_789_099_200,
        1_789_358_400,
        1_789_444_800,
        1_789_531_200,
        1_789_617_600,
        1_789_704_000,
        1_789_963_200,
        1_790_049_600,
        1_790_136_000,
        1_790_222_400,
        1_790_308_800,
        1_790_568_000,
        1_790_654_400,
        1_790_740_800,
        1_790_827_200,
        1_790_913_600,
        1_791_172_800,
        1_791_259_200,
        1_791_345_600,
        1_791_432_000,
        1_791_518_400,
        1_791_777_600,
        1_791_864_000,
        1_791_950_400,
        1_792_036_800,
        1_792_123_200,
        1_861_419_600,
        1_861_506_000,
        1_861_592_400,
        1_861_678_800,
    ];

    impl Desk {
        /// The session `at` falls in by the oracle's own reading: its trading day and 0 overnight,
        /// 1 pre-market, 2 regular, 3 after-hours; `None` while the market is closed.
        fn segment(at: i64) -> Option<(i64, u8)> {
            const HOUR: i64 = 3_600;
            TRADING_DAYS.iter().find_map(|day| {
                let at_hour = |hours: i64, halves: i64| {
                    day.saturating_add(hours.saturating_mul(HOUR))
                        .saturating_add(halves.saturating_mul(HOUR / 2))
                };
                [
                    (at_hour(-4, 0), at_hour(4, 0), 0),
                    (at_hour(4, 0), at_hour(9, 1), 1),
                    (at_hour(9, 1), at_hour(16, 0), 2),
                    (at_hour(16, 0), at_hour(20, 0), 3),
                ]
                .into_iter()
                .find(|(from, to, _)| (*from..*to).contains(&at))
                .map(|(_, _, segment)| (*day, segment))
            })
        }

        /// Whether the calendar covers `at`.
        fn covered(at: i64) -> bool {
            at >= CALENDAR_FROM
        }

        /// Whether `then` and now are one session (§8.2's in-session trade).
        fn same_session(&self, then: i64) -> bool {
            !Self::covered(self.now) || Self::segment(then) == Self::segment(self.now)
        }

        /// Whether no v1 session is open now (DEC-30: the overnight session is none).
        fn closed(&self) -> bool {
            Self::covered(self.now) && !matches!(Self::segment(self.now), Some((_, 1..=3)))
        }

        /// Whether now is the regular session, where nothing goes extended-hours.
        fn regular(&self) -> bool {
            !Self::covered(self.now) || matches!(Self::segment(self.now), Some((_, 2)))
        }

        /// Rule 13's session duties on every order the executor sends or journals: no exit goes
        /// while no session is open, and `extended_hours` only in pre-market or after-hours.
        fn sent(&self, id: &str, purpose: Purpose, extended: bool) -> Result<(), String> {
            if self.closed() && purpose != Purpose::Protective {
                return Err(format!(
                    "{id} ({purpose:?}) sent at {} with no session open",
                    self.now
                ));
            }
            if extended && (self.closed() || self.regular()) {
                return Err(format!("{id} sent extended-hours at {}", self.now));
            }
            Ok(())
        }

        /// §5.6 by the oracle's own reading: there is something to price from when some sane
        /// quote with a bid, or with a trade printed in the session it is now (§8.2), was seen
        /// within five minutes (DEC-260 (5), (16)).
        fn priceable(&self) -> bool {
            self.quotes.iter().any(|(at, bid, trade, sane)| {
                *sane
                    && (bid.is_some() || trade.is_some() && self.same_session(*at))
                    && self.now.saturating_sub(*at) <= 300
            })
        }

        /// Whether `qty` more on top of what every other exit may still sell exceeds the
        /// position: the only denial an exit may meet (§5.3 rules 3 and 4).
        fn oversells(&self, intent: &str, qty: u32) -> bool {
            let working: u32 = self
                .venue
                .values()
                .filter(|held| held.live && held.purpose != Purpose::Protective)
                .map(|held| held.qty.saturating_sub(held.filled))
                .sum();
            let waiting: u32 = self
                .exits
                .iter()
                .filter(|(other, exit)| other.as_str() != intent && !exit.done)
                .map(|(_, exit)| exit.qty)
                .sum();
            qty.saturating_add(working).saturating_add(waiting) > self.position
        }

        fn saw(&mut self, draft: &EventDraft) -> Result<(), String> {
            let field = |name: &str| draft.payload.get(name).and_then(Value::as_str);
            let intent = field("intent_id").unwrap_or_default().to_owned();
            match (draft.event_type.as_str(), field("action")) {
                ("IntentReceived", _) if intent.starts_with("w-") => {
                    let qty = field("qty")
                        .unwrap_or_default()
                        .parse::<u32>()
                        .map_err(|error| format!("the watchdog's qty: {error}"))?;
                    if field("purpose") != Some("risk_exit") {
                        return Err(format!("{intent}: the watchdog's exit is not a risk_exit"));
                    }
                    if self.exits.values().any(|exit| exit.watchdog && !exit.done) {
                        return Err(format!("{intent}: a second watchdog while one waits"));
                    }
                    self.exits.insert(
                        intent.clone(),
                        Followed {
                            purpose: Purpose::RiskExit,
                            qty,
                            arrived: self.now,
                            since: self.now,
                            verdict: String::new(),
                            reason: String::new(),
                            done: false,
                            watchdog: true,
                        },
                    );
                }
                ("GateDecided", _) => {
                    let verdict = field("verdict").unwrap_or_default();
                    let reason = field("reason_code").unwrap_or_default();
                    let oversells = self
                        .exits
                        .get(&intent)
                        .is_some_and(|exit| self.oversells(&intent, exit.qty));
                    let priceable = self.priceable();
                    let closed = self.closed();
                    let now = self.now;
                    let Some(exit) = self.exits.get_mut(&intent) else {
                        return Ok(());
                    };
                    let unpriced = reason == "exit_unpriced"
                        && exit.purpose == Purpose::DiscretionaryExit
                        && !priceable;
                    match verdict {
                        "deny" if reason != "sell_exceeds_available" || !oversells => {
                            return Err(format!("{intent} denied for {reason}"));
                        }
                        "hold"
                            if !RULE_13_HOLDS.contains(&reason)
                                && !unpriced
                                && !(matches!(reason, "session_closed" | "session_unknown")
                                    && closed) =>
                        {
                            return Err(format!("{intent} held for {reason}"));
                        }
                        _ => {}
                    }
                    verdict.clone_into(&mut exit.verdict);
                    reason.clone_into(&mut exit.reason);
                    exit.since = if verdict == "allow" { now } else { exit.since };
                    exit.done = exit.done || verdict == "deny";
                }
                ("ProtectionChanged", Some("exit_unpriced")) if self.priceable() => {
                    return Err(format!("{intent} fell back with something to price from"));
                }
                ("OrderSubmitted", _) => {
                    let purpose = match field("purpose") {
                        Some("protective") => Purpose::Protective,
                        _ => Purpose::RiskExit,
                    };
                    let extended = draft.payload.get("extended_hours") == Some(&Value::Bool(true));
                    let id = field("client_order_id").unwrap_or_default();
                    self.sent(id, purpose, extended)?;
                    if let Some(exit) = self.exits.get_mut(&intent) {
                        exit.done = true;
                    }
                }
                ("OrderAbandoned", _) => {
                    let reason = field("reason_code").or(field("reason")).unwrap_or_default();
                    if let Some(exit) = self.exits.get_mut(&intent) {
                        if reason != "unprotected_interval_limit" {
                            return Err(format!("{intent} abandoned for {reason}"));
                        }
                        exit.done = true;
                    }
                }
                _ => {}
            }
            Ok(())
        }

        /// Checked at every tick, where held exits are re-evaluated: no exit stays neither
        /// submitted, denied nor abandoned past the interval's bound and rule 5's window, except
        /// while the agent is paused, while a protective cancel waits on the broker's
        /// confirmation (the broker's own hold: an exit never goes before it), or under a hold
        /// that still stands by the oracle's own record: an `Unknown` order or the broker, or a
        /// discretionary exit with nothing to price it (DEC-160 (12)). An exit that would cross
        /// zero beside the others still selling waits for room, never denied while it waits; and
        /// every exit the broker is not holding is done by `max_intent_age_s`, which abandons it.
        /// A broker silent on a protective cancel holds the exit with the protection still
        /// resting; nothing bounds that hold yet (backlog), and the bound runs from when the
        /// broker last confirmed.
        fn bounded(&self, config: &ExecutorConfig) -> Result<(), String> {
            let limit = config
                .max_unprotected_s
                .saturating_add(config.unknown_absent_window_s)
                .saturating_add(20);
            let broker = self.asked.iter().any(|id| {
                self.venue
                    .get(id)
                    .is_some_and(|held| held.purpose == Purpose::Protective)
            });
            for (intent, exit) in &self.exits {
                let held = self.paused
                    || self.closed()
                    || exit.verdict == "hold"
                        && (exit.purpose == Purpose::DiscretionaryExit && !self.priceable()
                            || matches!(
                                exit.reason.as_str(),
                                "unknown_order_in_flight" | "broker"
                            ));
                let waits = held || broker || self.oversells(intent, exit.qty);
                let aged = !waits
                    && self.now.saturating_sub(exit.arrived)
                        > config.max_intent_age_s.saturating_add(20);
                let since = exit.since.max(self.quiet_since);
                if !exit.done
                    && !broker
                    && (aged || !waits && self.now.saturating_sub(since) > limit)
                {
                    return Err(format!(
                        "{intent} ({} {}) waited from {} to {}",
                        exit.verdict, exit.reason, exit.since, self.now
                    ));
                }
            }
            Ok(())
        }

        /// Hands one input to the executor, answering every query from the broker's record,
        /// until nothing more is asked. A refusal of any input is a rule-13 failure.
        fn deliver(
            &mut self,
            executor: &mut Executor,
            ports: &Ports<'_>,
            input: Input,
        ) -> Result<(), String> {
            let failed = |error: ExecutorError| format!("{error:?}");
            let mut inputs = VecDeque::from([input]);
            let mut budget = 64_u32;
            while let Some(input) = inputs.pop_front() {
                budget = budget
                    .checked_sub(1)
                    .ok_or("the broker's answers never settle")?;
                let effects = executor.run(input, ports).map_err(failed)?;
                for effect in effects {
                    match effect {
                        Effect::Journal(draft) => self.saw(&draft)?,
                        Effect::Broker(BrokerRequest::Cancel { client_order_id }) => {
                            self.asked.push(client_order_id.as_str().to_owned());
                        }
                        Effect::Broker(BrokerRequest::Submit(order)) => {
                            self.sent(
                                order.client_order_id.as_str(),
                                order.purpose,
                                order.extended_hours,
                            )?;
                            let qty = order
                                .qty
                                .to_string()
                                .parse::<u32>()
                                .map_err(|error| error.to_string())?;
                            self.venue.insert(
                                order.client_order_id.as_str().to_owned(),
                                Held {
                                    purpose: order.purpose,
                                    qty,
                                    filled: 0,
                                    acked: false,
                                    live: true,
                                },
                            );
                        }
                        Effect::Broker(BrokerRequest::GetOrderByClientId(id)) => {
                            inputs.push_back(Input::Broker(Ok(
                                match self.venue.get(id.as_str()) {
                                    Some(held) => BrokerOutcome::Order(
                                        reported(id.as_str(), held).map_err(failed)?,
                                    ),
                                    None => BrokerOutcome::Absent {
                                        client_order_id: id.as_str().to_owned(),
                                    },
                                },
                            )));
                        }
                        _ => {}
                    }
                }
            }
            Ok(())
        }
    }

    /// One order as the broker reports it.
    fn reported(id: &str, held: &Held) -> Result<BrokerOrder, ExecutorError> {
        let status = match (held.live, held.filled) {
            (false, filled) if filled == held.qty => "filled",
            (false, _) => "canceled",
            (true, 0) => "new",
            (true, _) => "partially_filled",
        };
        Ok(BrokerOrder {
            broker_order_id: format!("b-{id}"),
            client_order_id: Some(id.to_owned()),
            instrument: aapl()?,
            side: Side::Sell,
            qty: Qty::parse(&held.qty.to_string())?,
            filled_qty: Qty::parse(&held.filled.to_string())?,
            limit_price: Some(Price::parse("150")?),
            stop_price: None,
            status: status.to_owned(),
            reject_code: None,
            replaced_by_broker_order_id: None,
            legs: Vec::new(),
            created_on: None,
        })
    }

    fn rule_13_script(start: i64, script: &[Move]) -> Result<(), String> {
        let failed = |error: ExecutorError| format!("{error:?}");
        let (config, fees) = (executor_config(), fees().map_err(failed)?);
        let ports = tiered_ports(&config, &fees);
        let mut executor = protected(&ports).map_err(failed)?;
        let mut desk = Desk {
            now: start,
            quotes: Vec::new(),
            position: 10,
            venue: BTreeMap::from([(
                OCO.to_owned(),
                Held {
                    purpose: Purpose::Protective,
                    qty: 10,
                    filled: 0,
                    acked: true,
                    live: true,
                },
            )]),
            asked: Vec::new(),
            exits: BTreeMap::new(),
            fills: 0,
            paused: false,
            quiet_since: 0,
        };
        let agent = AgentId("agent-a".to_owned());
        desk.deliver(
            &mut executor,
            &ports,
            Input::Tick(RiskClock::from_secs(start)),
        )?;
        for (step, next) in script.iter().enumerate() {
            let input = match *next {
                Move::Tick(by) => {
                    desk.now = desk.now.saturating_add(by);
                    Some(Input::Tick(RiskClock::from_secs(desk.now)))
                }
                Move::Quote(bid, trade, sane) => {
                    desk.quotes.push((desk.now, bid, trade, sane));
                    Some(observation(bid, trade, sane, desk.now).map_err(failed)?)
                }
                Move::Exit(purpose, qty, limit) => {
                    let intent = format!("01JABCDEFGHJKMNPQRSTV{step:05}");
                    let purpose = EXIT_PURPOSES
                        .get(purpose)
                        .copied()
                        .unwrap_or(Purpose::RiskExit);
                    desk.exits.insert(
                        intent.clone(),
                        Followed {
                            purpose,
                            qty,
                            arrived: desk.now,
                            since: desk.now,
                            verdict: String::new(),
                            reason: String::new(),
                            done: false,
                            watchdog: false,
                        },
                    );
                    Some(
                        sell(&intent, &qty.to_string(), &limit.to_string(), purpose)
                            .map_err(failed)?,
                    )
                }
                Move::Ack => {
                    let first = desk.venue.iter_mut().find(|(_, held)| {
                        held.live && !held.acked && held.purpose != Purpose::Protective
                    });
                    match first {
                        Some((id, held)) => {
                            held.acked = true;
                            Some(Input::Broker(Ok(BrokerOutcome::Submitted(
                                reported(id, held).map_err(failed)?,
                            ))))
                        }
                        None => None,
                    }
                }
                Move::Fill(qty) => {
                    let first = desk.venue.iter_mut().find(|(_, held)| {
                        held.live && held.acked && held.purpose != Purpose::Protective
                    });
                    match first {
                        Some((id, held)) => {
                            let qty = qty.min(held.qty.saturating_sub(held.filled));
                            held.filled = held.filled.saturating_add(qty);
                            held.live = held.filled < held.qty;
                            desk.position = desk.position.saturating_sub(qty);
                            desk.fills = desk.fills.saturating_add(1);
                            Some(Input::BrokerUpdate(BrokerUpdate::Fill(BrokerFill {
                                fill_id: FillId(format!("f-{}", desk.fills)),
                                client_order_id: Some(id.clone()),
                                instrument: aapl().map_err(failed)?,
                                side: Side::Sell,
                                qty: Qty::parse(&qty.to_string())
                                    .map_err(|error| error.to_string())?,
                                price: Price::parse("150").map_err(|error| error.to_string())?,
                                fees: Usd::ZERO,
                                trade_date: Date::parse("2026-09-22")
                                    .map_err(|error| error.to_string())?,
                            })))
                        }
                        None => None,
                    }
                }
                Move::Confirm => {
                    if desk.asked.is_empty() {
                        None
                    } else {
                        let id = desk.asked.remove(0);
                        let protective = |desk: &Desk, id: &String| {
                            desk.venue
                                .get(id)
                                .is_some_and(|held| held.purpose == Purpose::Protective)
                        };
                        let cleared = protective(&desk, &id)
                            && !desk.asked.iter().any(|other| protective(&desk, other));
                        if cleared {
                            desk.quiet_since = desk.now;
                        }
                        match desk.venue.get_mut(&id) {
                            Some(held) if held.live => {
                                held.live = false;
                                Some(cancel_accepted(&id))
                            }
                            Some(_) => Some(refused(&id, "order is not cancelable")),
                            None => Some(cancel_accepted(&id)),
                        }
                    }
                }
                Move::Pause(paused) => {
                    let mode = if paused { Mode::Paused } else { Mode::Normal };
                    desk.paused = paused;
                    executor.state.modes.insert(agent.clone(), mode);
                    None
                }
            };
            if let Some(input) = input {
                desk.deliver(&mut executor, &ports, input)?;
            }
            if matches!(next, Move::Tick(_)) {
                desk.bounded(&config)?;
            }
        }
        let mut restarted = ExecutorState::new(executor.state.scope.clone());
        for event in &executor.journal {
            fold(&mut restarted, event).map_err(failed)?;
        }
        restarted.quotes.clone_from(&executor.state.quotes);
        restarted.sane_bids.clone_from(&executor.state.sane_bids);
        restarted.trades.clone_from(&executor.state.trades);
        restarted.breaches.clone_from(&executor.state.breaches);
        restarted.modes.clone_from(&executor.state.modes);
        restarted.now = executor.state.now;
        restarted.epoch = executor.state.epoch;
        restarted.started = executor.state.started;
        restarted.started_at = executor.state.started_at;
        if restarted != executor.state {
            return Err(format!(
                "a restart from the journal differs from the live state:\n{restarted:#?}\n{:#?}",
                executor.state
            ));
        }
        Ok(())
    }

    /// Rule 13, by an oracle of its own over random scripts of ticks, quotes (a bid, a trade,
    /// either, neither; sane or not; marks at, above and below the stop, so the watchdog fires), exits of every purpose and size, acknowledgments, partial
    /// fills, cancel confirmations and pauses, against ten protected AAPL with an exit tier, from
    /// an instant in each session (none the calendar covers, after-hours, just before the closing
    /// auction window, the morning, overnight, and just before the open): no
    /// input is refused; no exit is denied but for a genuine over-sell; no exit is held but for
    /// rule 13's four, or a discretionary exit with nothing to price from (DEC-160 (12)); no
    /// exit falls back to its own limit while something prices it; and none stays neither
    /// submitted, denied nor abandoned past the bounds but under those holds or the broker's
    /// silence on a protective cancel. The oracle reads the drafts and its own record, never
    /// the fold; and at the end the journal, folded afresh, equals the live state but for what is
    /// process-local (quotes, the watchdog's clock, the clock, the writer's epoch, and the pause set
    /// directly here).
    #[test]
    fn rule_13_holds_over_random_scripts() -> Result<(), String> {
        let config = ProptestConfig {
            cases: 512,
            failure_persistence: None,
            ..ProptestConfig::default()
        };
        let starts = prop::sample::select(vec![
            0,
            AFTER_HOURS,
            CLOSING.saturating_sub(30),
            MORNING,
            OVERNIGHT,
            OPEN.saturating_sub(30),
            TEN_SECONDS_BEFORE_THE_NIGHT,
            TEN_SECONDS_BEFORE_THE_NIGHT,
            TEN_SECONDS_BEFORE_THE_NIGHT,
            SATURDAY,
            1_788_789_600,
            EVE_OF_2018,
            1_861_754_400,
            1_862_150_400,
        ]);
        TestRunner::new(config)
            .run(
                &(starts, prop::collection::vec(moves(), 1..64)),
                |(start, script)| rule_13_script(start, &script).map_err(TestCaseError::fail),
            )
            .map_err(|error| error.to_string())
    }

    /// #400 round 2's nit: the random oracle reached the composed plant (an exit's step into the
    /// close sent as an extended-hours rung) in 7 of 9 runs, so a run could miss it. This script
    /// reaches it on every run: a risk exit at 19:59:50 ET goes after-hours once the OCO's cancel
    /// is confirmed, its first rung is acknowledged, its step is asked at 20:00:05, and the step's
    /// confirmation falls in the overnight session, where the oracle wants no rung sent and the
    /// exit held `session_closed` and parked (DEC-260 (18)); it stays parked through the night.
    #[test]
    fn rule_13_holds_for_a_step_into_the_close() -> Result<(), String> {
        let script = [
            Move::Quote(Some(150), None, true),
            Move::Exit(0, 4, 150),
            Move::Confirm,
            Move::Ack,
            Move::Tick(15),
            Move::Confirm,
            Move::Tick(3_600),
            Move::Quote(Some(150), None, true),
            Move::Tick(3_600),
        ];
        rule_13_script(TEN_SECONDS_BEFORE_THE_NIGHT, &script)
    }

    /// CI's minimal input on main (reported on #445): a script from the eve of 2018, before the
    /// calendar's first date, ticks twelve hours to 2018-01-01 00:00 ET, the New Year holiday's
    /// overnight, and places a risk exit with no quote. The calendar covers that instant and names
    /// no session open, so the exit is held `session_closed`, rule 13's broker hold (DEC-260 (13),
    /// DEC-392); the oracle reads the calendar's range at each instant, not at the script's start.
    #[test]
    fn rule_13_holds_for_a_risk_exit_on_the_new_year_holiday_overnight() -> Result<(), String> {
        let script = [
            Move::Tick(7_200),
            Move::Tick(3_600),
            Move::Tick(3_600),
            Move::Tick(7_200),
            Move::Tick(7_200),
            Move::Tick(7_200),
            Move::Tick(7_200),
            Move::Exit(0, 1, 141),
        ];
        rule_13_script(EVE_OF_2018, &script)
    }

    /// 2026-09-22, a Tuesday, at 19:59:50 ET: ten seconds before the after-hours session ends.
    const TEN_SECONDS_BEFORE_THE_NIGHT: i64 = 1_790_121_590;
    /// 2017-12-31, a Sunday, at 12:00 ET: seventeen hours before the calendar's first date.
    const EVE_OF_2018: i64 = 1_514_739_600;
    /// 2026-09-22, a Tuesday, at 17:00 ET: after-hours.
    const AFTER_HOURS: i64 = 1_790_110_800;
    /// 11:00 ET, the regular session.
    const MORNING: i64 = 1_790_089_200;
    /// 15:50:00 ET, the first second of the closing auction window, and the second before it.
    const CLOSING: i64 = 1_790_106_600;
    /// 02:00 ET, the overnight session v1 disables (DEC-30).
    const OVERNIGHT: i64 = 1_790_056_800;
    /// 08:00 ET, pre-market, and 09:30 ET, the regular open.
    const PRE_MARKET: i64 = 1_790_078_400;
    const OPEN: i64 = 1_790_083_800;
    /// Saturday 2026-09-26, 12:00 ET: the market is closed.
    const SATURDAY: i64 = 1_790_438_400;

    /// An unprotected position's exit at `at`, priced 150 with a quote observed `age` seconds
    /// before: what it was submitted as.
    fn alone_at(
        purpose: Purpose,
        limit: &str,
        at: i64,
        age: i64,
    ) -> Result<(Executor, SubmitOrder, EventDraft), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let mut executor = held(&ports)?;
        executor.run(Input::Tick(RiskClock::from_secs(at)), &ports)?;
        executor.run(
            observation(Some(150), None, true, at.saturating_sub(age))?,
            &ports,
        )?;
        let ran = executor.run(sell(EXIT, "10", limit, purpose)?, &ports)?;
        let order = submissions(&ran)
            .first()
            .copied()
            .cloned()
            .ok_or_else(|| missing("the exit's submission"))?;
        let draft = ran
            .iter()
            .find_map(|effect| match effect {
                Effect::Journal(draft) if draft.event_type == "OrderSubmitted" => Some(draft),
                _ => None,
            })
            .cloned()
            .ok_or_else(|| missing("its OrderSubmitted"))?;
        Ok((executor, order, draft))
    }

    /// §4.3, §5.6, DEC-260 (13), (14): in after-hours an unprotected position's risk exit goes as an
    /// extended-hours limit at the ladder's first rung, 150 × (1 − 0.5%), journaled `laddered`;
    /// a discretionary or owner exit keeps its own limit as a regular-session limit, which the
    /// broker queues to the session (§5.5, §9.6), and so does an exit priced above the bid.
    #[test]
    fn an_after_hours_risk_exit_is_laddered_and_extended() -> Result<(), ExecutorError> {
        let laddered = Some(Value::Bool(true));
        let (_, order, draft) = alone_at(Purpose::RiskExit, "150", AFTER_HOURS, 0)?;
        assert_eq!(order.limit_price, Some(Price::parse("149.25")?));
        assert!(order.extended_hours);
        assert_eq!(draft.payload.get("laddered").cloned(), laddered);
        for purpose in [Purpose::DiscretionaryExit, Purpose::OwnerExit] {
            let (_, order, draft) = alone_at(purpose, "150", AFTER_HOURS, 0)?;
            assert_eq!(order.limit_price, Some(Price::parse("150")?), "{purpose:?}");
            assert!(!order.extended_hours, "{purpose:?}");
            assert_eq!(draft.payload.get("laddered"), None, "{purpose:?}");
        }
        let (_, passive, _) = alone_at(Purpose::RiskExit, "151", AFTER_HOURS, 0)?;
        assert_eq!(
            passive.limit_price,
            Some(Price::parse("151")?),
            "above the bid"
        );
        assert!(passive.extended_hours, "still an extended-hours exit");
        Ok(())
    }

    /// §4.4, §5.6: in the regular session a fresh quote leaves the exit at its own limit, and a
    /// quote 120 s old is a presumed halt that ladders a risk or owner exit, but not a discretionary
    /// one; in the closing auction window the risk and owner exits are laddered from a fresh quote,
    /// and in the second before it they are not.
    #[test]
    fn a_presumed_halt_or_the_close_window_ladders_the_exit() -> Result<(), ExecutorError> {
        let rung = Some(Price::parse("149.25")?);
        let own = Some(Price::parse("150")?);
        for (purpose, at, age, expected) in [
            (Purpose::RiskExit, MORNING, 0, own),
            (Purpose::RiskExit, MORNING, 120, rung),
            (Purpose::OwnerExit, MORNING, 120, rung),
            (Purpose::DiscretionaryExit, MORNING, 120, own),
            (Purpose::RiskExit, CLOSING, 0, rung),
            (Purpose::OwnerExit, CLOSING, 0, rung),
            (Purpose::DiscretionaryExit, CLOSING, 0, own),
            (Purpose::RiskExit, CLOSING.saturating_sub(1), 0, own),
        ] {
            let (_, order, _) = alone_at(purpose, "150", at, age)?;
            assert_eq!(
                order.limit_price, expected,
                "{purpose:?} at {at}, {age} s old"
            );
            assert!(!order.extended_hours, "{purpose:?} at {at}");
        }
        Ok(())
    }

    /// The gate's reason on the `GateDecided` drafts in `effects`, newest last.
    fn gate_reasons(effects: &[Effect]) -> Vec<String> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Journal(draft) if draft.event_type == "GateDecided" => draft
                    .payload
                    .get("reason_code")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                _ => None,
            })
            .collect()
    }

    /// DEC-260 (13), the coordinator's ruling D1 (rule 13's broker hold): with no v1 session open,
    /// an equity exit of any purpose is held `session_closed`, nothing sent, until the calendar's
    /// next pre-market open: overnight to 04:00, a Friday evening to Monday 04:00, the Friday
    /// before Labor Day (2026-09-07) to Tuesday 04:00. At the open it is priced fresh, 151 ×
    /// (1 − 0.5%) on the tick, as an extended-hours rung, and its ladder's clock starts there.
    #[test]
    fn a_closed_market_holds_the_exit_for_the_next_pre_market_open() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let exit = format!("md-{EXIT}");
        for (closed, open) in [
            (OVERNIGHT, 1_790_064_000),
            (1_790_382_600, 1_790_582_400),
            (1_788_568_200, 1_788_854_400),
        ] {
            for purpose in [Purpose::RiskExit, Purpose::DiscretionaryExit] {
                let mut executor = held(&ports)?;
                executor.run(Input::Tick(RiskClock::from_secs(closed)), &ports)?;
                executor.run(observation(Some(150), None, true, closed)?, &ports)?;
                let ran = executor.run(sell(EXIT, "10", "150", purpose)?, &ports)?;
                assert!(submissions(&ran).is_empty(), "{purpose:?} at {closed}");
                assert_eq!(gate_reasons(&ran), vec!["session_closed"], "{purpose:?}");
                let before = executor.run(Input::Tick(RiskClock::from_secs(open - 1)), &ports)?;
                assert!(submissions(&before).is_empty(), "{purpose:?} before {open}");
                executor.run(observation(Some(151), None, true, open)?, &ports)?;
                let released = executor.run(Input::Tick(RiskClock::from_secs(open)), &ports)?;
                let order = submissions(&released)
                    .first()
                    .copied()
                    .cloned()
                    .ok_or_else(|| missing("the released exit"))?;
                if purpose == Purpose::RiskExit {
                    assert_eq!(
                        order.limit_price,
                        Some(Price::parse("150.25")?),
                        "at {open}"
                    );
                    assert!(order.extended_hours);
                    let early =
                        executor.run(Input::Tick(RiskClock::from_secs(open + 9)), &ports)?;
                    assert!(
                        cancels(&early).is_empty(),
                        "the ladder's clock starts at release"
                    );
                    let step =
                        executor.run(Input::Tick(RiskClock::from_secs(open + 10)), &ports)?;
                    assert_eq!(cancels(&step), vec![exit.as_str()]);
                } else {
                    assert_eq!(order.limit_price, Some(Price::parse("150")?));
                    assert!(!order.extended_hours, "paced to the regular session (§9.6)");
                }
            }
        }
        Ok(())
    }

    /// D1 holds only an exit the gate allows: one that would sell more than the position is still
    /// denied `sell_exceeds_available` in a closed market (§5.3 rule 4), never held for the open.
    #[test]
    fn a_closed_market_never_holds_an_exit_the_gate_denies() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let mut executor = held(&ports)?;
        executor.run(Input::Tick(RiskClock::from_secs(OVERNIGHT)), &ports)?;
        executor.run(observation(Some(150), None, true, OVERNIGHT)?, &ports)?;
        let ran = executor.run(sell(EXIT, "20", "150", Purpose::RiskExit)?, &ports)?;
        assert!(submissions(&ran).is_empty());
        assert_eq!(gate_reasons(&ran), vec!["sell_exceeds_available"]);
        Ok(())
    }

    /// D1: the protection resting when a closed market holds the exit stays untouched through the
    /// hold; the sequence starts, cancelling it, only at the release.
    #[test]
    fn a_closed_market_hold_leaves_the_protection_resting() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let mut executor = protected(&ports)?;
        executor.run(Input::Tick(RiskClock::from_secs(OVERNIGHT)), &ports)?;
        executor.run(observation(Some(150), None, true, OVERNIGHT)?, &ports)?;
        let ran = executor.run(sell(EXIT, "10", "150", Purpose::RiskExit)?, &ports)?;
        assert!(cancels(&ran).is_empty() && submissions(&ran).is_empty());
        assert!(rests(&executor.state, &aapl()?));
        executor.run(observation(Some(150), None, true, 1_790_064_000)?, &ports)?;
        let open = executor.run(Input::Tick(RiskClock::from_secs(1_790_064_000)), &ports)?;
        assert_eq!(
            cancels(&open),
            vec![OCO],
            "the sequence starts at the release"
        );
        Ok(())
    }

    /// D1 in a running sequence: an exit allowed at 19:59:50 ET, after-hours, whose protective
    /// cancel is confirmed after 20:00 finds no session open; it is held `session_closed`, and
    /// the protection is re-placed at once for the position rather than left cancelled through
    /// the night.
    #[test]
    fn a_sequence_that_runs_into_the_close_re_protects() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let late = 1_790_121_590;
        let mut executor = protected(&ports)?;
        executor.run(Input::Tick(RiskClock::from_secs(late)), &ports)?;
        executor.run(observation(Some(150), None, true, late)?, &ports)?;
        let ran = executor.run(sell(EXIT, "10", "150", Purpose::RiskExit)?, &ports)?;
        assert_eq!(cancels(&ran), vec![OCO]);
        executor.run(Input::Tick(RiskClock::from_secs(late + 15)), &ports)?;
        let closed = executor.run(cancel_accepted(OCO), &ports)?;
        assert!(
            submissions(&closed)
                .iter()
                .all(|order| order.purpose == Purpose::Protective)
        );
        assert!(!ocos(&closed).is_empty(), "{:?}", drafted(&closed));
        assert_eq!(stop_covered(&executor)?, position(&executor)?);
        Ok(())
    }

    /// #400 round 1, blocker 1 (DEC-260 (18)), the reviewer's witness: a 19:59:55 ET risk exit
    /// goes after-hours at 149.25; its step is asked at 20:00:05, and the cancel's confirmation, in
    /// the overnight session, submits nothing: the exit is held `session_closed`, parked. At
    /// Wednesday's 04:00 open the ladder resumes from `-l1`, priced fresh, 151 × (1 − 1%), as an
    /// extended-hours rung, and its clock starts there.
    #[test]
    fn a_step_into_the_close_parks_the_ladder_until_the_open() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let (mut executor, first, _) = alone_at(Purpose::RiskExit, "150", 1_790_121_595, 0)?;
        assert_eq!(first.limit_price, Some(Price::parse("149.25")?));
        let exit = format!("md-{EXIT}");
        let step = executor.run(Input::Tick(RiskClock::from_secs(1_790_121_605)), &ports)?;
        assert_eq!(cancels(&step), vec![exit.as_str()]);
        let night = executor.run(cancel_accepted(&exit), &ports)?;
        assert!(submissions(&night).is_empty(), "{:?}", drafted(&night));
        assert_eq!(gate_reasons(&night), vec!["session_closed"]);
        assert!(
            !alerts(&night).contains(&"session_unknown"),
            "{:?}",
            alerts(&night)
        );
        for at in [1_790_121_700, 1_790_140_000, 1_790_150_399] {
            let quiet = executor.run(Input::Tick(RiskClock::from_secs(at)), &ports)?;
            assert!(submissions(&quiet).is_empty(), "at {at}");
            assert!(gate_reasons(&quiet).is_empty(), "journaled once, at {at}");
        }
        executor.run(observation(Some(151), None, true, 1_790_150_400)?, &ports)?;
        let open = executor.run(Input::Tick(RiskClock::from_secs(1_790_150_400)), &ports)?;
        let rung = submissions(&open)
            .first()
            .copied()
            .cloned()
            .ok_or_else(|| missing("the resumed rung"))?;
        assert_eq!(rung.client_order_id.as_str(), format!("{exit}-l1"));
        assert_eq!(rung.limit_price, Some(Price::parse("149.49")?));
        assert!(rung.extended_hours);
        let early = executor.run(Input::Tick(RiskClock::from_secs(1_790_150_409)), &ports)?;
        assert!(
            cancels(&early).is_empty(),
            "the ladder's clock restarts at the release"
        );
        let again = executor.run(Input::Tick(RiskClock::from_secs(1_790_150_410)), &ports)?;
        assert_eq!(cancels(&again), vec![format!("{exit}-l1").as_str()]);
        Ok(())
    }

    /// Blocker 1 for a sequence's ladder: the step confirmed in the overnight session submits no
    /// rung; the protection is re-placed for the night, and at the 04:00 open the sequence starts
    /// again, cancelling it, and resumes from `-l1` priced fresh.
    #[test]
    fn a_sequence_step_into_the_close_re_protects_and_resumes_at_the_open()
    -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let late = 1_790_121_595;
        let mut executor = protected(&ports)?;
        executor.run(Input::Tick(RiskClock::from_secs(late)), &ports)?;
        executor.run(observation(Some(150), None, true, late)?, &ports)?;
        executor.run(sell(EXIT, "10", "150", Purpose::RiskExit)?, &ports)?;
        let first = executor.run(cancel_accepted(OCO), &ports)?;
        assert_eq!(submissions(&first).len(), 1);
        let exit = format!("md-{EXIT}");
        let step = executor.run(Input::Tick(RiskClock::from_secs(late + 10)), &ports)?;
        assert_eq!(cancels(&step), vec![exit.as_str()]);
        let night = executor.run(cancel_accepted(&exit), &ports)?;
        assert!(
            submissions(&night)
                .iter()
                .all(|order| order.purpose == Purpose::Protective),
            "{:?}",
            drafted(&night)
        );
        assert!(
            !ocos(&night).is_empty(),
            "the protection is re-placed for the night"
        );
        assert_eq!(gate_reasons(&night), vec!["session_closed"]);
        assert_eq!(stop_covered(&executor)?, position(&executor)?);
        executor.run(observation(Some(151), None, true, 1_790_150_400)?, &ports)?;
        let open = executor.run(Input::Tick(RiskClock::from_secs(1_790_150_400)), &ports)?;
        let replaced = cancels(&open)
            .first()
            .map(|id| (*id).to_owned())
            .ok_or_else(|| missing("the night's protection cancelled at the open"))?;
        let resumed = executor.run(cancel_accepted(&replaced), &ports)?;
        let rung = submissions(&resumed)
            .first()
            .copied()
            .cloned()
            .ok_or_else(|| missing("the resumed rung"))?;
        assert_eq!(rung.client_order_id.as_str(), format!("{exit}-l1"));
        assert_eq!(rung.limit_price, Some(Price::parse("149.49")?));
        assert!(rung.extended_hours);
        assert!(
            executor.state.ladders.is_empty() && executor.state.exiting.contains_key(&aapl()?),
            "the parked ladder resumes inside the new sequence, not beside it"
        );
        Ok(())
    }

    /// The coordinator's ruling on #400 round 2 (5930410998) for a sequence's ladder: the step
    /// confirmed in the overnight session parks it and re-protects the position, and the park
    /// alerts the owner **once**, `session_closed`, naming the park's own record; nothing else is
    /// raised before the 04:00 open, swept every half hour, and the exit is never abandoned.
    #[test]
    fn a_sequence_parked_overnight_alerts_the_owner_exactly_once() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let late = 1_790_121_595;
        let open = 1_790_150_400;
        let mut executor = protected(&ports)?;
        executor.run(Input::Tick(RiskClock::from_secs(late)), &ports)?;
        executor.run(observation(Some(150), None, true, late)?, &ports)?;
        executor.run(sell(EXIT, "10", "150", Purpose::RiskExit)?, &ports)?;
        executor.run(cancel_accepted(OCO), &ports)?;
        let exit = format!("md-{EXIT}");
        executor.run(Input::Tick(RiskClock::from_secs(late + 10)), &ports)?;
        let night = executor.run(cancel_accepted(&exit), &ports)?;
        let park = night
            .iter()
            .find_map(|effect| match effect {
                Effect::Journal(draft) if draft.event_type == "GateDecided" => {
                    Some(draft.event_id.clone())
                }
                _ => None,
            })
            .ok_or_else(|| missing("the park"))?;
        let mut notes: Vec<(EventId, &str)> = night
            .iter()
            .filter_map(|effect| match effect {
                Effect::Notify(note) => Some((note.subject_event.clone(), note.message_key)),
                _ => None,
            })
            .collect();
        let mut at = late + 60;
        while at < open {
            for input in [
                observation(Some(150), None, true, at)?,
                Input::Tick(RiskClock::from_secs(at)),
            ] {
                let ran = executor.run(input, &ports)?;
                assert!(
                    submissions(&ran).is_empty(),
                    "nothing is sent overnight, at {at}"
                );
                assert!(
                    !ran.iter().any(|effect| matches!(
                        effect,
                        Effect::Journal(draft) if draft.event_type == "OrderAbandoned"
                    )),
                    "the exit is never abandoned, at {at}"
                );
                notes.extend(ran.iter().filter_map(|effect| match effect {
                    Effect::Notify(note) => Some((note.subject_event.clone(), note.message_key)),
                    _ => None,
                }));
            }
            at += 1_800;
        }
        assert_eq!(notes, vec![(park, "session_closed")]);
        assert_eq!(stop_covered(&executor)?, position(&executor)?);
        let mut folded = ExecutorState::new(executor.state.scope.clone());
        for event in &executor.journal {
            fold(&mut folded, event)?;
        }
        let intent = IntentId(EventId(EXIT.to_owned()));
        assert!(
            folded.held_long.contains(&intent),
            "the park is what `exit_held_long` reports: the journal folds it into the held-long set"
        );
        assert!(
            !folded.held.contains(&intent),
            "and it is never in the held set `exit_held_long`'s re-journal walks, which is what keeps \
             the night to one alert"
        );
        Ok(())
    }

    /// DEC-260 (18): a sequence that starts in an instrument takes a parked ladder only if it is
    /// that ladder's own exit; another exit's sequence starts its own ladder, and the parked one
    /// stays for its exit.
    #[test]
    fn a_parked_ladder_resumes_only_in_its_own_exits_sequence() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let parked = crate::state::Ladder {
            rung: 2,
            since: None,
            floored: false,
            stepping: true,
            parked: true,
        };
        for (intent, takes) in [(SECOND, false), (EXIT, true)] {
            let mut executor = held(&ports)?;
            executor.state.ladders.insert(
                aapl()?,
                crate::state::LoneLadder {
                    intent: IntentId(EventId(EXIT.to_owned())),
                    agent: AgentId("agent-a".to_owned()),
                    ladder: parked,
                },
            );
            committed(
                &mut executor,
                "ProtectionChanged",
                vec![
                    ("instrument", text("AAPL")),
                    ("action", text("unprotected_start")),
                    ("orders", text(OCO)),
                    ("intent_id", text(intent)),
                    ("entry", text("md-held-1")),
                    ("agent", text("agent-a")),
                    ("stop", text("140")),
                ],
            )?;
            let started = executor
                .state
                .exiting
                .get(&aapl()?)
                .map(|sequence| sequence.ladder)
                .ok_or_else(|| missing("the sequence"))?;
            assert_eq!(started == parked, takes, "{intent}");
            assert_eq!(executor.state.ladders.is_empty(), takes, "{intent}");
        }
        Ok(())
    }

    /// #400 round 1, minor 1: in the overnight session of the calendar's last trading day
    /// (2028-12-29, 02:00 ET), that day's 04:00 open is named, so the hold reads `session_closed`.
    #[test]
    fn the_last_trading_days_overnight_reads_session_closed() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let at = 1_861_686_000;
        let mut executor = held(&ports)?;
        executor.run(Input::Tick(RiskClock::from_secs(at)), &ports)?;
        executor.run(observation(Some(150), None, true, at)?, &ports)?;
        let ran = executor.run(sell(EXIT, "10", "150", Purpose::RiskExit)?, &ports)?;
        assert_eq!(gate_reasons(&ran), vec!["session_closed"]);
        Ok(())
    }

    /// D1's fail-safe: on the calendar's last Friday evening (2028-12-29, 21:00 ET) no next
    /// pre-market open can be named, and past its last date (2029-01-03, 11:00 ET) no session at
    /// all, so the exit is held `session_unknown` and alerted, and stays held; never sent as a
    /// queued limit or an overnight extended-hours one.
    #[test]
    fn a_closed_market_with_no_next_open_holds_and_alerts() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        for at in [1_861_754_400, 1_862_150_400] {
            let mut executor = held(&ports)?;
            executor.run(Input::Tick(RiskClock::from_secs(at)), &ports)?;
            executor.run(observation(Some(150), None, true, at)?, &ports)?;
            let ran = executor.run(sell(EXIT, "10", "150", Purpose::RiskExit)?, &ports)?;
            assert!(submissions(&ran).is_empty());
            assert_eq!(gate_reasons(&ran), vec!["session_unknown"]);
            assert!(
                alerts(&ran).contains(&"session_unknown"),
                "{:?}",
                alerts(&ran)
            );
            let later = executor.run(Input::Tick(RiskClock::from_secs(at + 86_400)), &ports)?;
            assert!(submissions(&later).is_empty());
        }
        Ok(())
    }

    /// #400 round 1, minor 2: a `session_unknown` hold past `max_intent_age_s` is never abandoned
    /// and alerts `exit_held_long` once, as every hold does (D2).
    #[test]
    fn a_session_unknown_hold_is_never_abandoned() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let at = 1_862_150_400;
        let mut executor = held(&ports)?;
        executor.run(Input::Tick(RiskClock::from_secs(at)), &ports)?;
        executor.run(observation(Some(150), None, true, at)?, &ports)?;
        executor.run(sell(EXIT, "10", "150", Purpose::RiskExit)?, &ports)?;
        let long = executor.run(Input::Tick(RiskClock::from_secs(at + 121)), &ports)?;
        assert!(!drafted(&long).contains(&"OrderAbandoned"));
        assert!(alerts(&long).contains(&"exit_held_long"));
        assert_eq!(gate_reasons(&long), vec!["session_unknown"]);
        let later = executor.run(Input::Tick(RiskClock::from_secs(at + 86_400)), &ports)?;
        assert!(!drafted(&later).contains(&"OrderAbandoned"));
        assert!(!alerts(&later).contains(&"exit_held_long"));
        Ok(())
    }

    /// The coordinator's ruling D2 (rule 13, DEC-160 (12)): an exit held past `max_intent_age_s`
    /// (120 s) for any reason — paused, nothing to price from where protection rests, the market
    /// closed — is never
    /// abandoned; the hold is journaled once more, `held_long`, and alerted once, and the exit goes
    /// when the hold clears.
    #[test]
    fn a_long_hold_alerts_once_and_the_exit_still_goes() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let agent = AgentId("agent-a".to_owned());
        for (reason, start) in [
            ("agent_paused", 0),
            ("exit_unpriced", 0),
            ("session_closed", OVERNIGHT),
        ] {
            let protects = reason == "exit_unpriced";
            let mut executor = if protects {
                protected(&ports)?
            } else {
                held(&ports)?
            };
            executor.run(Input::Tick(RiskClock::from_secs(start)), &ports)?;
            let purpose = match reason {
                "agent_paused" => {
                    executor.state.modes.insert(agent.clone(), Mode::Paused);
                    executor.run(observation(Some(150), None, true, start)?, &ports)?;
                    Purpose::RiskExit
                }
                "exit_unpriced" => {
                    executor.run(unpriceable()?, &ports)?;
                    Purpose::DiscretionaryExit
                }
                _ => {
                    executor.run(observation(Some(150), None, true, start)?, &ports)?;
                    Purpose::RiskExit
                }
            };
            let ran = executor.run(sell(EXIT, "10", "150", purpose)?, &ports)?;
            assert_eq!(gate_reasons(&ran), vec![reason]);
            let long = executor.run(Input::Tick(RiskClock::from_secs(start + 121)), &ports)?;
            assert!(!drafted(&long).contains(&"OrderAbandoned"), "{reason}");
            assert!(alerts(&long).contains(&"exit_held_long"), "{reason}");
            assert_eq!(
                gate_reasons(&long),
                vec![reason],
                "journaled with its reason"
            );
            let again = executor.run(Input::Tick(RiskClock::from_secs(start + 200)), &ports)?;
            assert!(
                !alerts(&again).contains(&"exit_held_long"),
                "once: {reason}"
            );
            let clear = if reason == "session_closed" {
                1_790_064_000
            } else {
                start + 201
            };
            executor.state.modes.clear();
            executor.run(observation(Some(150), None, true, clear)?, &ports)?;
            let mut released = executor.run(Input::Tick(RiskClock::from_secs(clear)), &ports)?;
            if protects {
                assert_eq!(
                    cancels(&released),
                    vec![OCO],
                    "the sequence starts at the release"
                );
                released = executor.run(cancel_accepted(OCO), &ports)?;
            }
            assert_eq!(submissions(&released).len(), 1, "{reason}");
        }
        Ok(())
    }

    /// §5.6 step 2 for a lone ladder: unfilled after `exit_step_s` (10 s here) the rung is cancelled
    /// to step, and the confirmation submits `-l1` one offset step lower, 148.50, extended-hours.
    /// A pause after the step was asked does not end it (DEC-260 (14)), but while paused no new
    /// step is asked.
    #[test]
    fn a_lone_ladder_steps_and_a_pause_stops_only_new_steps() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let (mut executor, _, _) = alone_at(Purpose::RiskExit, "150", AFTER_HOURS, 0)?;
        let early = executor.run(Input::Tick(RiskClock::from_secs(AFTER_HOURS + 9)), &ports)?;
        assert!(cancels(&early).is_empty(), "not before exit_step_s");
        let step = executor.run(Input::Tick(RiskClock::from_secs(AFTER_HOURS + 10)), &ports)?;
        let exit = format!("md-{EXIT}");
        assert_eq!(cancels(&step), vec![exit.as_str()]);
        executor
            .state
            .modes
            .insert(AgentId("agent-a".to_owned()), Mode::Paused);
        let next = executor.run(cancel_accepted(&exit), &ports)?;
        let rung = submissions(&next)
            .first()
            .copied()
            .cloned()
            .ok_or_else(|| missing("the next rung"))?;
        assert_eq!(rung.client_order_id.as_str(), format!("{exit}-l1"));
        assert_eq!(rung.limit_price, Some(Price::parse("148.5")?));
        assert!(rung.extended_hours);
        let paused = executor.run(Input::Tick(RiskClock::from_secs(AFTER_HOURS + 30)), &ports)?;
        assert!(
            cancels(&paused).is_empty(),
            "paused: {:?}",
            drafted(&paused)
        );
        executor.state.modes.clear();
        let resumed = executor.run(Input::Tick(RiskClock::from_secs(AFTER_HOURS + 31)), &ports)?;
        assert_eq!(cancels(&resumed), vec![format!("{exit}-l1").as_str()]);
        Ok(())
    }

    /// §8.2, #373 round 2 minor A: a last trade counts only in its own session. At 16:01 ET with
    /// no bid at all, a print at 15:59, two minutes before but in the regular session, prices the
    /// after-hours exit nothing, so it falls back to its own limit; a print at 16:00:30 prices it,
    /// 149 × (1 − 0.5%) rounded up to the tick.
    #[test]
    fn a_trade_from_another_session_prices_no_rung() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let now = 1_790_107_260;
        for (printed, expected) in [(now - 30, "148.26"), (now - 120, "150")] {
            let mut executor = held(&ports)?;
            executor.run(Input::Tick(RiskClock::from_secs(now)), &ports)?;
            executor.run(observation(None, Some(149), true, printed)?, &ports)?;
            let ran = executor.run(sell(EXIT, "10", "150", Purpose::RiskExit)?, &ports)?;
            let order = submissions(&ran)
                .first()
                .copied()
                .cloned()
                .ok_or_else(|| missing("the exit"))?;
            assert_eq!(
                order.limit_price,
                Some(Price::parse(expected)?),
                "printed {printed}"
            );
        }
        Ok(())
    }

    /// §5.4, DEC-260 (12): stops do not trigger outside the regular session, so a breach that began
    /// in pre-market is watchdogged only `stop_watchdog_s` (30 s) after the open, one in
    /// after-hours or on a weekend never.
    #[test]
    fn the_watchdog_waits_for_the_regular_session() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = tiered_ports(&config, &fees);
        let mut executor = protected(&ports)?;
        executor.run(observation(Some(139), None, true, PRE_MARKET)?, &ports)?;
        for at in [PRE_MARKET + 60, OPEN, OPEN + 29] {
            let tick = executor.run(Input::Tick(RiskClock::from_secs(at)), &ports)?;
            assert!(
                watchdog_record(&tick).is_none(),
                "at {at}: {:?}",
                drafted(&tick)
            );
        }
        let fired = executor.run(Input::Tick(RiskClock::from_secs(OPEN + 30)), &ports)?;
        assert!(watchdog_record(&fired).is_some(), "{:?}", drafted(&fired));
        for at in [AFTER_HOURS, SATURDAY] {
            let mut executor = protected(&ports)?;
            executor.run(observation(Some(139), None, true, at)?, &ports)?;
            let tick = executor.run(Input::Tick(RiskClock::from_secs(at + 600)), &ports)?;
            assert!(
                watchdog_record(&tick).is_none(),
                "at {at}: {:?}",
                drafted(&tick)
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod ladder_tests {
    use mandate_accounting::InstrumentId;
    use mandate_num::{Fraction, Price, Qty};

    use proptest::prelude::*;

    use super::{ExitPrice, LadderReference, exit_price, ladder_price};
    use crate::error::ExecutorError;
    use crate::types::{ExitTier, MarketObservation, Purpose, RiskClock};

    fn liquid() -> Result<ExitTier, ExecutorError> {
        Ok(ExitTier {
            exit_offset: Fraction::parse("0.005")?,
            exit_offset_step: Fraction::parse("0.005")?,
            max_exit_offset: Fraction::parse("0.03")?,
        })
    }

    fn seen(
        bid: Option<&str>,
        trade: Option<&str>,
        sane: bool,
        at: i64,
    ) -> Result<MarketObservation, ExecutorError> {
        Ok(MarketObservation {
            instrument: InstrumentId::new("AAPL")?,
            bid: bid.map(Price::parse).transpose()?,
            bid_size: Some(Qty::parse("100")?),
            ask: bid.map(Price::parse).transpose()?,
            last_trade: trade.map(Price::parse).transpose()?,
            mark: bid.map(Price::parse).transpose()?,
            sane,
            observed_at: RiskClock::from_secs(at),
        })
    }

    /// §5.6, RC-24: 150 × (1 − offset), the offset 0.5% raised 0.5% a step and held at 3%, where
    /// the rung is at the floor.
    #[test]
    fn each_step_raises_the_offset_until_the_floor() -> Result<(), ExecutorError> {
        let quotes = [seen(Some("150"), Some("150"), true, 0)?];
        let rungs = (0..8)
            .map(|step| ladder_price(liquid()?, &quotes, step, RiskClock::from_secs(0), None))
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            rungs
                .iter()
                .map(|rung| (rung.limit.to_string(), rung.at_floor, rung.step))
                .collect::<Vec<_>>(),
            [
                ("149.25", false, 0),
                ("148.5", false, 1),
                ("147.75", false, 2),
                ("147", false, 3),
                ("146.25", false, 4),
                ("145.5", true, 5),
                ("145.5", true, 6),
                ("145.5", true, 7),
            ]
            .map(|(limit, floor, step)| (limit.to_owned(), floor, step))
            .to_vec()
        );
        assert!(
            rungs
                .iter()
                .all(|rung| rung.reference == LadderReference::FreshQuote)
        );
        Ok(())
    }

    /// §5.6: the newest sane quote first; else the last sane bid within five minutes; else the
    /// last sane trade within five minutes, a newer insane print skipped (DEC-260 (5)); and no
    /// price at all is refused, never guessed.
    #[test]
    fn the_reference_falls_back_in_the_spec_order() -> Result<(), ExecutorError> {
        let now = RiskClock::from_secs(400);
        let cases = [
            (
                vec![seen(Some("150"), Some("151"), true, 390)?],
                LadderReference::FreshQuote,
                "149.25",
            ),
            (
                vec![
                    seen(Some("150"), Some("151"), true, 100)?,
                    seen(Some("148"), Some("152"), false, 399)?,
                ],
                LadderReference::LastSaneBid,
                "149.25",
            ),
            (
                vec![
                    seen(None, Some("152"), true, 350)?,
                    seen(Some("148"), Some("1000"), false, 399)?,
                ],
                LadderReference::LastTrade,
                "151.24",
            ),
            (
                vec![seen(None, Some("152"), true, 399)?],
                LadderReference::LastTrade,
                "151.24",
            ),
        ];
        for (quotes, reference, limit) in cases {
            let rung = ladder_price(liquid()?, &quotes, 0, now, None)?;
            assert_eq!(
                (rung.reference, rung.limit),
                (reference, Price::parse(limit)?),
                "{quotes:?}"
            );
        }
        for nothing in [
            vec![seen(None, None, true, 399)?],
            vec![seen(Some("148"), Some("1000"), false, 399)?],
            vec![seen(Some("150"), Some("151"), true, 99)?],
        ] {
            assert!(
                matches!(
                    ladder_price(liquid()?, &nothing, 0, now, None),
                    Err(ExecutorError::NotInterpreted { .. })
                ),
                "an insane print or one older than five minutes prices nothing: {nothing:?}"
            );
        }
        Ok(())
    }

    /// DEC-160 (12): an owner exit's fallback, like its rungs, never goes below the owner's floor.
    #[test]
    fn a_fallback_never_prices_below_the_owners_floor() -> Result<(), ExecutorError> {
        let now = RiskClock::from_secs(0);
        let nothing = [seen(None, None, true, 0)?];
        let (limit, floor) = (Price::parse("140")?, Price::parse("145")?);
        assert_eq!(
            (
                exit_price(
                    Some(liquid()?),
                    &nothing,
                    0,
                    now,
                    Some(floor),
                    Purpose::OwnerExit,
                    limit
                ),
                exit_price(
                    Some(liquid()?),
                    &nothing,
                    0,
                    now,
                    None,
                    Purpose::OwnerExit,
                    limit
                ),
            ),
            (ExitPrice::Fallback(floor), ExitPrice::Fallback(limit))
        );
        Ok(())
    }

    fn purposes() -> impl Strategy<Value = Purpose> {
        prop::sample::select(vec![
            Purpose::Open,
            Purpose::Increase,
            Purpose::RiskExit,
            Purpose::OwnerExit,
            Purpose::DiscretionaryExit,
            Purpose::Protective,
            Purpose::Flatten,
        ])
    }

    fn observations() -> impl Strategy<Value = Vec<(Option<u32>, Option<u32>, bool, i64)>> {
        prop::collection::vec(
            (
                prop::option::of(1u32..100_000),
                prop::option::of(1u32..100_000),
                any::<bool>(),
                0i64..1_000,
            ),
            0..4,
        )
    }

    proptest! {
        /// DEC-160 (12): `exit_price` answers every purpose, for any observations, step, floor and
        /// limit, and never with a refusal. Its oracle is its own: a sequence's exit prices when
        /// some sane bid or sane trade is within five minutes (DEC-260 (5)); otherwise a discretionary
        /// exit is held and every other exit falls back to its limit, raised to the floor.
        #[test]
        fn exit_price_never_refuses_any_purpose(
            purpose in purposes(),
            seen_quotes in observations(),
            step in 0u32..20,
            limit in 1u32..100_000,
            floor in prop::option::of(1u32..100_000),
            tiered in any::<bool>(),
        ) {
            let cents = |value: u32| {
                let raw = format!("{}.{:02}", value / 100, value % 100);
                Price::parse(raw.trim_end_matches('0').trim_end_matches('.'))
            };
            let now = RiskClock::from_secs(1_000);
            let quotes = seen_quotes
                .iter()
                .map(|(bid, trade, sane, at)| {
                    Ok(MarketObservation {
                        instrument: InstrumentId::new("AAPL")?,
                        bid: bid.map(cents).transpose()?,
                        bid_size: None,
                        ask: None,
                        last_trade: trade.map(cents).transpose()?,
                        mark: None,
                        sane: *sane,
                        observed_at: RiskClock::from_secs(*at),
                    })
                })
                .collect::<Result<Vec<_>, ExecutorError>>()
                .map_err(|error| TestCaseError::fail(format!("{error:?}")))?;
            let limit = cents(limit).map_err(|error| TestCaseError::fail(format!("{error:?}")))?;
            let floor = floor
                .map(cents)
                .transpose()
                .map_err(|error| TestCaseError::fail(format!("{error:?}")))?;
            let tier = if tiered {
                Some(liquid().map_err(|error| TestCaseError::fail(format!("{error:?}")))?)
            } else {
                None
            };
            let priced = exit_price(tier, &quotes, step, now, floor, purpose, limit);
            let exit = !purpose.adds_risk() && purpose != Purpose::Protective;
            let priceable = seen_quotes
                .iter()
                .any(|(bid, trade, sane, at)| (bid.is_some() || trade.is_some()) && *sane && *at >= 700);
            let raised = floor.map_or(limit, |floor| limit.max(floor));
            match (exit && tiered, priceable, purpose) {
                (false, _, _) => prop_assert_eq!(priced, ExitPrice::Own(limit)),
                (true, true, _) => prop_assert!(
                    matches!(priced, ExitPrice::Laddered(price) if floor.is_none_or(|floor| price >= floor)),
                    "{:?}", priced
                ),
                (true, false, Purpose::DiscretionaryExit) => prop_assert_eq!(priced, ExitPrice::Held),
                (true, false, _) => prop_assert_eq!(priced, ExitPrice::Fallback(raised)),
            }
        }
    }

    /// An owner exit's floor (§5.5): the rung never prices below it, and a rung above it keeps
    /// its own price.
    #[test]
    fn a_rung_never_prices_below_the_owners_floor() -> Result<(), ExecutorError> {
        let quotes = [seen(Some("150"), None, true, 0)?];
        let now = RiskClock::from_secs(0);
        let floor = Price::parse("148")?;
        let low = ladder_price(liquid()?, &quotes, 4, now, Some(floor))?;
        let high = ladder_price(liquid()?, &quotes, 0, now, Some(floor))?;
        assert_eq!((low.limit, high.limit), (floor, Price::parse("149.25")?));
        Ok(())
    }
}

#[cfg(test)]
mod bracket_tests {
    use mandate_accounting::Side;
    use mandate_canon::Value;
    use mandate_num::{Price, Qty};

    use crate::error::ExecutorError;
    use crate::ids::IntentId;
    use crate::payload::{clock, object};
    use crate::ports::Ports;
    use crate::reconcile::tests::{
        Everything, Executor, Ids, aapl, drafted, executor_config, fees, missing, reporting,
        submitted,
    };
    use crate::types::{
        AgentId, BrokerOrder, BrokerRequest, BrokerUpdate, Effect, EventDraft, EventId,
        ExecutorConfig, Input, IntentBody, IntentHandoff, ProtectionPrices, Purpose, RiskClock,
        TimeInForce,
    };

    const ENTRY: &str = "01JABCDEFGHJKMNPQRSTVWXYZ1";

    fn text(raw: &str) -> Value {
        Value::Str(raw.to_owned())
    }

    fn entry_id() -> String {
        format!("md-{ENTRY}")
    }

    /// The broker's report of the bracket entry: `status`, with `filled` of its 10 filled.
    fn report(status: &str, filled: &str) -> Result<Input, ExecutorError> {
        Ok(Input::BrokerUpdate(BrokerUpdate::Order(BrokerOrder {
            broker_order_id: "b-1".to_owned(),
            client_order_id: Some(entry_id()),
            instrument: aapl()?,
            side: Side::Buy,
            qty: Qty::parse("10")?,
            filled_qty: Qty::parse(filled)?,
            limit_price: Some(Price::parse("150")?),
            stop_price: None,
            status: status.to_owned(),
            reject_code: None,
            replaced_by_broker_order_id: None,
            legs: Vec::new(),
            created_on: None,
        })))
    }

    fn cancels(effects: &[Effect]) -> usize {
        effects
            .iter()
            .filter(|effect| matches!(effect, Effect::Broker(BrokerRequest::Cancel { .. })))
            .count()
    }

    /// A ready executor that has sent one bracket entry of 10 `AAPL` at 150, stop 140, take-profit
    /// 170, at risk-clock second 10.
    fn bracketed(ports: &Ports<'_>) -> Result<Executor, ExecutorError> {
        bracketed_at(ports, 10)
    }

    /// [`bracketed`], sent at risk-clock second `at`.
    fn bracketed_at(ports: &Ports<'_>, at: i64) -> Result<Executor, ExecutorError> {
        let mut executor = reporting(ports)?;
        executor.run(Input::Tick(RiskClock::from_secs(at)), ports)?;
        let sent = executor.run(
            Input::Intent(IntentHandoff {
                intent_id: IntentId(EventId(ENTRY.to_owned())),
                agent: AgentId("agent-a".to_owned()),
                body: IntentBody::Order {
                    instrument: aapl()?,
                    side: Side::Buy,
                    qty: Qty::parse("10")?,
                    limit: Price::parse("150")?,
                    purpose: Purpose::Open,
                    protection: Some(ProtectionPrices {
                        stop: Price::parse("140")?,
                        take_profit: Some(Price::parse("170")?),
                    }),
                },
            }),
            ports,
        )?;
        let bracket = sent.iter().find_map(|effect| match effect {
            Effect::Broker(BrokerRequest::Submit(order)) => order.bracket.clone(),
            _ => None,
        });
        if bracket.is_none() {
            return Err(missing("the entry sent as a bracket"));
        }
        Ok(executor)
    }

    /// §5.4: the first partial fill opens the interval and cancels nothing; the remainder is
    /// cancelled at the shorter of `bracket_partial_fill_timeout_s` and `max_unprotected_s` from
    /// it, not a second before, and once (DEC-346 item 5).
    #[test]
    fn a_partly_filled_entry_is_cancelled_at_the_shorter_bound_and_not_before()
    -> Result<(), ExecutorError> {
        let fees = fees()?;
        for (timeout, limit, due_at) in [(60, 90, 70), (60, 30, 40), (20, 60, 30)] {
            let config = ExecutorConfig {
                bracket_partial_fill_timeout_s: timeout,
                max_unprotected_s: limit,
                ..executor_config()
            };
            let ports = Ports {
                ids: &Ids,
                mandates: &Everything,
                instruments: &Everything,
                config: &config,
                fees: &fees,
            };
            let case = format!("timeout {timeout}, bound {limit}");
            let mut executor = bracketed(&ports)?;
            let partial = executor.run(report("partially_filled", "4")?, &ports)?;
            assert!(
                drafted(&partial).contains(&"ProtectionChanged"),
                "{case}: {partial:?}"
            );
            assert_eq!(
                cancels(&partial),
                0,
                "{case}: the first fill cancels nothing"
            );
            let early = executor.run(Input::Tick(RiskClock::from_secs(due_at - 1)), &ports)?;
            assert_eq!(
                cancels(&early),
                0,
                "{case}: a second early, the legs are held"
            );
            let due = executor.run(Input::Tick(RiskClock::from_secs(due_at)), &ports)?;
            assert_eq!(cancels(&due), 1, "{case}: the remainder is cancelled");
            let again = executor.run(Input::Tick(RiskClock::from_secs(due_at + 1)), &ports)?;
            assert_eq!(cancels(&again), 0, "{case}: once");
        }
        Ok(())
    }

    fn with_ports<T>(
        run: impl FnOnce(&Ports<'_>) -> Result<T, ExecutorError>,
    ) -> Result<T, ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        run(&Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        })
    }

    /// Every `ProtectionChanged` draft in `effects`.
    fn records(effects: &[Effect]) -> Vec<&EventDraft> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Journal(draft) if draft.event_type == "ProtectionChanged" => Some(draft),
                _ => None,
            })
            .collect()
    }

    /// The `placed` record in `effects`, and the protective order it names.
    fn placed(effects: &[Effect]) -> Result<(&EventDraft, String), ExecutorError> {
        let record = records(effects)
            .into_iter()
            .find(|draft| draft.payload.get("action").and_then(Value::as_str) == Some("placed"))
            .ok_or_else(|| missing("the placed record"))?;
        let named = record
            .payload
            .get("orders")
            .and_then(Value::as_str)
            .ok_or_else(|| missing("the placed order's id"))?
            .to_owned();
        Ok((record, named))
    }

    /// §2.3 (DEC-346 item 4): the protective order a `placed` record names is `{entry}-p{record}`,
    /// the record being that `ProtectionChanged` itself, on the completed entry's branch and the
    /// OCO's alike.
    fn assert_named_for_its_record(effects: &[Effect], case: &str) -> Result<(), ExecutorError> {
        let (record, named) = placed(effects)?;
        assert_eq!(
            named,
            format!("{}-p{}", entry_id(), record.event_id.0),
            "{case}: the id's suffix is the placed record's own event id"
        );
        Ok(())
    }

    /// Rule 5 and §5.4 (DEC-346 items 4 and 6, #446 round 1 B1, M2, M3): the OCO for a partly
    /// filled entry is a GTC order whose `OrderSubmitted` comes before its `Submit` in the same
    /// effect list, named for its own `placed` record.
    #[test]
    fn the_partial_fill_oco_is_journaled_before_it_is_sent() -> Result<(), ExecutorError> {
        with_ports(|ports| {
            let mut executor = bracketed(ports)?;
            executor.run(report("partially_filled", "4")?, ports)?;
            let ended = executor.run(report("canceled", "4")?, ports)?;
            let (_, named) = placed(&ended)?;
            let journaled = ended.iter().position(|effect| {
                matches!(effect, Effect::Journal(draft) if draft.event_type == "OrderSubmitted"
                    && draft.payload.get("client_order_id").and_then(Value::as_str) == Some(named.as_str()))
            });
            let sent = ended.iter().position(|effect| {
                matches!(effect, Effect::Broker(BrokerRequest::Submit(order))
                    if order.client_order_id.as_str() == named)
            });
            assert!(
                journaled.is_some() && sent.is_some() && journaled < sent,
                "the journal names the OCO before the broker sees it (rule 5): {:?}",
                drafted(&ended)
            );
            let tif = ended.iter().find_map(|effect| match effect {
                Effect::Broker(BrokerRequest::Submit(order)) => Some(order.tif),
                _ => None,
            });
            assert_eq!(tif, Some(TimeInForce::Gtc), "the OCO is GTC (§5.4)");
            assert_named_for_its_record(&ended, "the OCO")
        })
    }

    /// Rule 12 (DEC-346 item 4, #446 round 1 B2): an entry is protected once. After its completed
    /// legs are recorded, or its OCO sent, later steps record and send nothing more.
    #[test]
    fn an_entry_is_protected_once() -> Result<(), ExecutorError> {
        with_ports(|ports| {
            for terminal in ["filled", "canceled"] {
                let mut executor = bracketed(ports)?;
                let filled = if terminal == "filled" { "10" } else { "4" };
                if terminal == "canceled" {
                    executor.run(report("partially_filled", "4")?, ports)?;
                }
                let ended = executor.run(report(terminal, filled)?, ports)?;
                assert_named_for_its_record(&ended, terminal)?;
                for second in 11..15 {
                    let later = executor.run(Input::Tick(RiskClock::from_secs(second)), ports)?;
                    assert!(
                        records(&later).is_empty() && submitted(&later) == 0,
                        "{terminal}, second {second}: nothing more is recorded or sent: {:?}",
                        drafted(&later)
                    );
                }
            }
            Ok(())
        })
    }

    /// §5.4 (DEC-346 item 5, #446 round 1 M1): an entry still working when its venue reaches the
    /// closing auction window has its remainder cancelled at once, inside the timeout, and the OCO
    /// for the filled quantity goes at once too: no session holds protection (rule 13).
    #[test]
    fn an_entry_at_the_closing_window_is_cancelled_and_oco_d_at_once() -> Result<(), ExecutorError>
    {
        const AT_THREE: i64 = 1_790_017_200;
        const REGULAR: i64 = 1_790_020_190;
        const CLOSING: i64 = 1_790_020_200;
        with_ports(|ports| {
            let mut executor = bracketed_at(ports, AT_THREE)?;
            executor.run(Input::Tick(RiskClock::from_secs(REGULAR)), ports)?;
            let partial = executor.run(report("partially_filled", "4")?, ports)?;
            assert_eq!(cancels(&partial), 0, "the regular session holds the legs");
            let closing = executor.run(Input::Tick(RiskClock::from_secs(CLOSING)), ports)?;
            assert_eq!(
                cancels(&closing),
                1,
                "ten seconds in, well inside the timeout, the closing window cancels the remainder"
            );
            let ended = executor.run(report("canceled", "4")?, ports)?;
            assert_eq!(
                submitted(&ended),
                1,
                "the OCO goes in the closing window: {:?}",
                drafted(&ended)
            );
            Ok(())
        })
    }

    /// Rule 12: the OCO for a partly filled entry never covers more than the position leaves once
    /// every exit still selling has sold; with nothing left it places nothing, and the interval
    /// still ends.
    #[test]
    fn nothing_left_to_cover_places_no_oco_and_ends_the_interval() -> Result<(), ExecutorError> {
        with_ports(|ports| {
            let (_, ended) = nothing_left(ports)?;
            assert_eq!(submitted(&ended), 0, "nothing to cover: {ended:?}");
            let actions: Vec<&str> = records(&ended)
                .into_iter()
                .filter_map(|draft| draft.payload.get("action").and_then(Value::as_str))
                .collect();
            assert!(actions.contains(&"unprotected_end"), "{actions:?}");
            assert!(!actions.contains(&"placed"), "{actions:?}");
            Ok(())
        })
    }

    /// A bracket entry partly filled for 4 while `agent-b`'s exit of 4 is still selling, reported
    /// cancelled: nothing is left to cover. Answers the executor and that report's effects.
    fn nothing_left(ports: &Ports<'_>) -> Result<(Executor, Vec<Effect>), ExecutorError> {
        let mut executor = bracketed(ports)?;
        let at = || clock(RiskClock::from_secs(10));
        executor.commit_one(
            "FillApplied",
            object(vec![
                ("fill_id", text("f-1")),
                ("client_order_id", text(&entry_id())),
                ("instrument", text("AAPL")),
                ("side", text("buy")),
                ("qty_gross", text("4")),
                ("price", text("150")),
                ("risk_clock", at()?),
            ])?,
        )?;
        executor.commit_one(
            "OrderSubmitted",
            object(vec![
                ("client_order_id", text("md-sell-b")),
                ("agent", text("agent-b")),
                ("instrument", text("AAPL")),
                ("side", text("sell")),
                ("qty", text("4")),
                ("limit", text("150")),
                ("purpose", text("risk_exit")),
                ("risk_clock", at()?),
            ])?,
        )?;
        let ended = executor.run(report("canceled", "4")?, ports)?;
        Ok((executor, ended))
    }

    /// DEC-346 item 4 (#446 round 2, M1): with nothing left to cover no `placed` record is drafted,
    /// so the interval's `unprotected_end`, naming the entry, is all that marks it protected. Later
    /// steps record and send nothing; and once the competing exit stops selling and room opens, no
    /// OCO goes for an entry whose interval has already ended.
    #[test]
    fn an_entry_with_nothing_left_to_cover_is_protected_once() -> Result<(), ExecutorError> {
        with_ports(|ports| {
            let (mut executor, _) = nothing_left(ports)?;
            for second in 11..15 {
                let later = executor.run(Input::Tick(RiskClock::from_secs(second)), ports)?;
                assert!(
                    records(&later).is_empty() && submitted(&later) == 0,
                    "second {second}: nothing more is recorded or sent: {:?}",
                    drafted(&later)
                );
            }
            executor.commit_one(
                "OrderStateChanged",
                object(vec![
                    ("client_order_id", text("md-sell-b")),
                    ("state", text("canceled")),
                    ("risk_clock", clock(RiskClock::from_secs(15))?),
                ])?,
            )?;
            let opened = executor.run(Input::Tick(RiskClock::from_secs(16)), ports)?;
            assert!(
                records(&opened).is_empty() && submitted(&opened) == 0,
                "room opens, and still no late OCO for an entry whose interval ended: {:?}",
                drafted(&opened)
            );
            Ok(())
        })
    }

    /// A bracket's `OrderSubmitted` that names its class without both prices is refused on the
    /// fold, never read back as a plain order that a resubmission would send unprotected.
    #[test]
    fn a_bracket_submission_without_its_prices_is_refused() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = reporting(&ports)?;
        let refused = executor.commit_one(
            "OrderSubmitted",
            object(vec![
                ("client_order_id", text("md-buy-x")),
                ("agent", text("agent-a")),
                ("instrument", text("AAPL")),
                ("side", text("buy")),
                ("qty", text("10")),
                ("limit", text("150")),
                ("order_class", text("bracket")),
                ("stop", text("140")),
                ("risk_clock", clock(RiskClock::from_secs(0))?),
            ])?,
        );
        assert!(refused.is_err(), "{refused:?}");
        Ok(())
    }
}
