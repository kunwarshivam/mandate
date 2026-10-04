//! The intent protocol (E7-2): received, gated, submitted under a derived id, and — after a
//! confirmed absence — resubmitted under the same id or abandoned.

use mandate_accounting::{AssetClass, InstrumentId};
use mandate_canon::Value;

use crate::batch::Batch;
use crate::codec::{order_type_name, purpose_name, side_name, tif_name};
use crate::error::ExecutorError;
use crate::gate::{
    PartialGateDecision, Proposal, SESSION_CLOSED, SESSION_UNKNOWN, UNPRICED, account_stream_checks,
};
use crate::ids::{ClientOrderId, IntentId};
use crate::payload::{int, text};
use crate::protection::{
    ExitPrice, alone, awaits_cancel, begin_exit, crypto_add, exit_limit, fallback, passive_exit,
    reprotect_unpriced, rests,
};
use crate::session::{closed_hold, extended_hours};
use crate::state::IntentOutcome;
use crate::types::{
    AgentId, BracketLegs, BrokerRequest, EventId, GateVerdict, IntentBody, IntentHandoff,
    OrderType, ProtectionPrices, Purpose, SubmitOrder, TimeInForce,
};

/// `Input::Intent`: deduplicated by the fold lookup, journaled as `IntentReceived`, then gated.
/// An intent the fold already carries costs nothing (task brief interpretation 8).
pub(crate) fn received(
    batch: &mut Batch<'_, '_>,
    handoff: IntentHandoff,
) -> Result<(), ExecutorError> {
    if batch.view.intents.contains_key(&handoff.intent_id) {
        return Ok(());
    }
    let IntentBody::Order {
        instrument,
        protection,
        ..
    } = &handoff.body
    else {
        return flatten_plan();
    };
    batch.journal(
        "IntentReceived",
        None,
        intent_received_fields(
            &handoff.intent_id,
            &handoff.agent,
            &handoff.body,
            handoff.tif,
        )?,
    )?;
    if let Some(prices) = protection {
        batch.journal(
            "ProtectionChanged",
            None,
            intended_fields(instrument, &handoff.intent_id, *prices)?,
        )?;
    }
    gate_and_submit(batch, &handoff.intent_id)
}

/// The `intended` `ProtectionChanged` (§9.5, DEC-446 item 5): the receipt-time protective prices
/// an opening was handed in with, journaled beside its `IntentReceived` in the same batch, so
/// replay restores the protection (`AGENTS.md` rule 13). Every member present (§4.2); the prices
/// leave `IntentReceived`, never a member there at either version.
fn intended_fields(
    instrument: &InstrumentId,
    intent: &IntentId,
    prices: ProtectionPrices,
) -> Result<Vec<(&'static str, Value)>, ExecutorError> {
    Ok(vec![
        ("instrument_id", text(instrument.as_str())),
        ("action", text("intended")),
        ("orders", Value::Array(Vec::new())),
        ("awaiting", Value::Array(Vec::new())),
        ("qty", Value::Null),
        ("stop", text(prices.stop.to_string())),
        (
            "take_profit",
            prices
                .take_profit
                .map_or(Value::Null, |price| text(price.to_string())),
        ),
        ("intent_id", text(intent.0.0.clone())),
        ("bracket", Value::Null),
        ("entry", Value::Null),
        ("agent_id", Value::Null),
        ("replacing", Value::Null),
        ("created_on", Value::Null),
        ("sent", Value::Null),
        ("uncovered", Value::Null),
        ("acknowledged", Value::Null),
    ])
}

/// A flatten plan handed over as an intent: the agent-scoped flatten's sells are the protective
/// sequence's (E7-4, the coordinator's ruling (d) on #174).
fn flatten_plan() -> Result<(), ExecutorError> {
    Err(ExecutorError::Unimplemented { story: "E7-4" })
}

/// Whether an intent is past `max_intent_age`, measured from its `IntentReceived`. Checked at
/// **every** `Intent → Submitting` transition, the first included (§5.7, interpretation 11).
/// An exit is never too old: it is held, never dropped (rule 13, DEC-160 (12), the coordinator's
/// ruling D2), and priced at its release from the quotes then (§5.6), so the stale price the age
/// guards against is never acted on. Only an opening is abandoned for age.
fn too_old(batch: &Batch<'_, '_>, intent: &IntentId) -> bool {
    !exit(batch, intent) && aged(batch, intent)
}

fn aged(batch: &Batch<'_, '_>, intent: &IntentId) -> bool {
    batch.view.intents.get(intent).is_none_or(|record| {
        batch.at().secs().saturating_sub(record.received_at.secs())
            > batch.ports.config.max_intent_age_s
    })
}

fn exit(batch: &Batch<'_, '_>, intent: &IntentId) -> bool {
    batch.view.bodies.get(intent).is_some_and(
        |body| !matches!(body, IntentBody::Order { purpose, .. } if purpose.adds_risk()),
    )
}

/// Gates an intent the fold carries as received and not yet submitted: it is submitted, held, or
/// denied, and one that has grown too old is abandoned without being gated at all.
pub(crate) fn gate_and_submit(
    batch: &mut Batch<'_, '_>,
    intent: &IntentId,
) -> Result<(), ExecutorError> {
    if too_old(batch, intent) {
        return abandon(batch, intent, "intent_too_old");
    }
    crypto_add(batch, intent)?;
    begin_and_submit(batch, intent, true)
}

/// The gate before the sequence (DEC-160 (8)), on arrival and on release alike: the verdict is
/// read without journaling it, an allowed exit's sequence starts, and only then is `GateDecided`
/// journaled, so the protective cancels precede it on the journal; an allowed intent is then
/// submitted. With `always` false a verdict that does not allow is not journaled.
fn begin_and_submit(
    batch: &mut Batch<'_, '_>,
    intent: &IntentId,
    always: bool,
) -> Result<(), ExecutorError> {
    if decide(batch, intent)?.0.verdict_name() == ALLOW {
        begin_exit(batch, intent)?;
    }
    match gate(batch, intent, always)? {
        ALLOW => submit(batch, intent)?,
        HOLD if WAITS.contains(&decide(batch, intent)?.0.reason_code()) => {
            reprotect_unpriced(batch, intent)?;
        }
        _ => {}
    }
    Ok(())
}

const ALLOW: &str = "allow";
const HOLD: &str = "hold";
/// The holds that may outlast a running sequence's interval, so its protection is re-placed
/// rather than left cancelled while the exit waits (DEC-160 (12), DEC-260 (13)).
const WAITS: [&str; 3] = [UNPRICED, SESSION_CLOSED, SESSION_UNKNOWN];

/// The gate's verdict on `intent` against the current state, journaling nothing: only an exit
/// the gate allows starts §5.4's sequence, so a held or denied one leaves protection resting.
fn decide(
    batch: &Batch<'_, '_>,
    intent: &IntentId,
) -> Result<(PartialGateDecision, Purpose), ExecutorError> {
    let (agent, body) = intent_of(batch, intent)?;
    let IntentBody::Order {
        instrument,
        side,
        qty,
        limit,
        purpose,
        protection,
    } = &body
    else {
        return Err(ExecutorError::NotInterpreted {
            what: "a flatten plan at the gate".to_owned(),
            story: "E7-4",
        });
    };
    let decision = account_stream_checks(
        &batch.view,
        &Proposal {
            agent: &agent,
            instrument,
            side: *side,
            qty: *qty,
            purpose: *purpose,
            bracketed: protection.is_some(),
        },
        batch.ports,
    )?;
    let allowed = decision.verdict == GateVerdict::Allow;
    let closed = closed_hold(batch.ports, instrument, batch.at())
        .filter(|_| allowed && !purpose.adds_risk());
    let unpriced = *purpose == Purpose::DiscretionaryExit
        && allowed
        && exit_limit(batch, intent, instrument, *purpose, *limit) == ExitPrice::Held;
    let decision = match closed {
        Some(reason) => decision.closed(reason),
        None if unpriced => decision.unpriced(),
        None => decision.crowded_out(),
    };
    Ok((decision, *purpose))
}

/// Runs the binding gate on an intent, journals the decision, and answers its verdict name:
/// `allow`, `hold`, or `deny`. With `always` false, a decision that does not allow is not
/// journaled again: a held intent re-checked at a tick records only the moment it is released,
/// or a discretionary exit's terminal denial beside a plan (DEC-410 item 3).
fn gate(
    batch: &mut Batch<'_, '_>,
    intent: &IntentId,
    always: bool,
) -> Result<&'static str, ExecutorError> {
    let (decision, purpose) = decide(batch, intent)?;
    let verdict = decision.verdict_name();
    if verdict == ALLOW || always || decision.crowded_denial() {
        let decided = journal_decision(batch, intent, &decision, purpose, Vec::new())?;
        if let reason @ (UNPRICED | SESSION_UNKNOWN) = decision.reason_code() {
            batch.notify(
                decided,
                if reason == UNPRICED {
                    UNPRICED
                } else {
                    SESSION_UNKNOWN
                },
            );
        }
    }
    Ok(verdict)
}

fn journal_decision(
    batch: &mut Batch<'_, '_>,
    intent: &IntentId,
    decision: &PartialGateDecision,
    purpose: Purpose,
    mut extra: Vec<(&'static str, Value)>,
) -> Result<EventId, ExecutorError> {
    let mut pairs = vec![
        ("intent_id", text(intent.0.0.clone())),
        ("verdict", text(decision.verdict_name())),
        ("reason_code", text(decision.reason_code())),
        ("purpose", text(purpose_name(purpose))),
        ("checks", decision.checks_value()?),
        ("evaluation", text("account_stream_only")),
    ];
    if let Some(sized) = decision.sized() {
        pairs.push(("sized_qty", text(sized.to_string())));
    }
    pairs.append(&mut extra);
    batch.journal("GateDecided", None, pairs)
}

/// The coordinator's ruling D2: an exit still held past `max_intent_age_s` is journaled once more,
/// `held_long`, with its hold's reason, and the owner alerted once, so a long hold is seen.
fn held_long(batch: &mut Batch<'_, '_>, intent: &IntentId) -> Result<(), ExecutorError> {
    if !exit(batch, intent) || !aged(batch, intent) || batch.view.held_long.contains(intent) {
        return Ok(());
    }
    let (decision, purpose) = decide(batch, intent)?;
    if decision.verdict_name() != HOLD {
        return Ok(());
    }
    let extra = vec![("held_long", Value::Bool(true))];
    let decided = journal_decision(batch, intent, &decision, purpose, extra)?;
    batch.notify(decided, "exit_held_long");
    Ok(())
}

pub(crate) fn intent_of(
    batch: &Batch<'_, '_>,
    intent: &IntentId,
) -> Result<(AgentId, IntentBody), ExecutorError> {
    let unknown = || ExecutorError::NotInterpreted {
        what: format!("intent {}", intent.0.0),
        story: "E7-2",
    };
    let record = batch.view.intents.get(intent).ok_or_else(unknown)?;
    let body = batch.view.bodies.get(intent).ok_or_else(unknown)?;
    Ok((record.agent.clone(), body.clone()))
}

/// An order's time in force: GTC for crypto, DAY otherwise; an exit's later rungs take the same.
pub(crate) fn order_tif(batch: &Batch<'_, '_>, instrument: &InstrumentId) -> TimeInForce {
    match batch.ports.instruments.asset_class(instrument) {
        Some(AssetClass::Crypto) => TimeInForce::Gtc,
        _ => TimeInForce::Day,
    }
}

/// The first submission of an intent: journaled as `OrderSubmitted` naming the id the request
/// carries, and only then described (journal spec §5.2, `AGENTS.md` rule 5). An exit with nothing
/// to price from goes at its own limit, journaled and alerted (DEC-160 (12)); a discretionary one
/// is held before the gate allows it ([`decide`]), so one found unpriced after its allow goes the
/// same way rather than being dropped (rule 13).
fn submit(batch: &mut Batch<'_, '_>, intent: &IntentId) -> Result<(), ExecutorError> {
    let (agent, body) = intent_of(batch, intent)?;
    let IntentBody::Order {
        instrument,
        side,
        qty,
        limit,
        purpose,
        protection,
    } = body
    else {
        return Ok(());
    };
    if awaits_cancel(&batch.view, &agent, &instrument, purpose)
        || passive_exit(batch, intent, &instrument, qty, limit)?
    {
        return Ok(());
    }
    let bracket = bracket_of(batch, &instrument, purpose, protection)?;
    let lone = !batch.view.exiting.contains_key(&instrument)
        && !rests(&batch.view, &instrument)
        && alone(batch, &instrument, purpose, limit);
    let limit = match exit_limit(batch, intent, &instrument, purpose, limit) {
        ExitPrice::Own(limit) | ExitPrice::Laddered(limit) => limit,
        ExitPrice::Fallback(limit) => {
            fallback(batch, intent, &instrument, limit)?;
            limit
        }
        ExitPrice::Held => {
            fallback(batch, intent, &instrument, limit)?;
            limit
        }
    };
    let tif = match bracket {
        Some(_) => TimeInForce::Gtc,
        None => order_tif(batch, &instrument),
    };
    let extended_hours =
        bracket.is_none() && extended_hours(batch.ports, &instrument, batch.at(), purpose);
    let request = SubmitOrder {
        client_order_id: ClientOrderId::for_intent(intent)?,
        instrument,
        side,
        qty,
        order_type: OrderType::Limit,
        tif,
        limit_price: Some(limit),
        stop_price: None,
        bracket,
        oco: None,
        extended_hours,
        purpose,
    };
    if !lone {
        return send(batch, request, Some(intent), &agent, 1);
    }
    let first = vec![("rung", int(0)?), ("at_floor", Value::Bool(false))];
    journal_rung(batch, &request, Some(intent), &agent, 1, first)?;
    batch.broker(BrokerRequest::Submit(request));
    Ok(())
}

/// §5.4's tranche model: an equity entry that carries its protective prices goes as one GTC
/// bracket at exactly those prices, an add where protection already rests included, which is a new
/// bracket beside it and never a replacement of it (DEC-346 item 1). An entry with no prices goes
/// plain, and an exit never carries a bracket. A crypto entry is never a bracket (§5.4: simple
/// orders only); its stop-limit is slice 5's. An equity entry with a stop and no take-profit has no
/// bracket to go as, and answers slice 5's stub rather than going unprotected: an opening may
/// always be refused (`AGENTS.md` rule 2; DEC-346 item 2).
pub(crate) fn bracket_of(
    batch: &Batch<'_, '_>,
    instrument: &InstrumentId,
    purpose: Purpose,
    protection: Option<ProtectionPrices>,
) -> Result<Option<BracketLegs>, ExecutorError> {
    let crypto = batch.ports.instruments.asset_class(instrument) == Some(AssetClass::Crypto);
    let Some(prices) = protection.filter(|_| purpose.adds_risk() && !crypto) else {
        return Ok(None);
    };
    match prices.take_profit {
        Some(take_profit) => Ok(Some(BracketLegs {
            take_profit,
            stop: prices.stop,
        })),
        None => Err(ExecutorError::Unimplemented { story: "E7-4" }),
    }
}

/// Journals `OrderSubmitted` with every field of the request, then describes the request. The
/// draft is the effect immediately before the request in the list, so the shell cannot send
/// what the journal has not named.
pub(crate) fn send(
    batch: &mut Batch<'_, '_>,
    request: SubmitOrder,
    intent: Option<&IntentId>,
    agent: &AgentId,
    attempt: u32,
) -> Result<(), ExecutorError> {
    journal_submission(batch, &request, intent, agent, attempt)?;
    batch.broker(BrokerRequest::Submit(request));
    Ok(())
}

/// `OrderSubmitted` for `request`, before a sequence records more and then sends it.
pub(crate) fn journal_submission(
    batch: &mut Batch<'_, '_>,
    request: &SubmitOrder,
    intent: Option<&IntentId>,
    agent: &AgentId,
    attempt: u32,
) -> Result<(), ExecutorError> {
    journal_rung(batch, request, intent, agent, attempt, Vec::new())
}

/// [`journal_submission`] with `extra` fields: a ladder rung's `rung` and `at_floor` (§5.6).
///
/// One batch writes two events (§9.5, DEC-446 item 3): the `OrderRequestRecorded` companion
/// carrying the executor-only members the fold rebuilds the exact request and the order's owner
/// from, immediately followed by the version-2 `OrderSubmitted` that names it as its
/// `causation_id` — the request the broker sees, its eight members with `limit_price` null when
/// the order has none, and the batch's `risk_clock`. A crash cannot leave one without the other.
pub(crate) fn journal_rung(
    batch: &mut Batch<'_, '_>,
    request: &SubmitOrder,
    intent: Option<&IntentId>,
    agent: &AgentId,
    attempt: u32,
    extra: Vec<(&'static str, Value)>,
) -> Result<(), ExecutorError> {
    let rung = extra
        .iter()
        .find(|(name, _)| *name == "rung")
        .and_then(|(_, value)| value.as_int());
    let at_floor = rung.is_some()
        && extra
            .iter()
            .any(|(name, value)| *name == "at_floor" && *value == Value::Bool(true));
    let (order_class, take_profit, stop) = match (&request.bracket, &request.oco) {
        (Some(legs), _) => (Some("bracket"), Some(legs.take_profit), Some(legs.stop)),
        (_, Some(legs)) => (Some("oco"), Some(legs.take_profit), Some(legs.stop)),
        _ => (None, None, None),
    };
    let rung_value = match rung {
        Some(rung) => int(rung)?,
        None => Value::Null,
    };
    let companion = vec![
        ("agent_id", text(agent.0.clone())),
        (
            "intent_id",
            intent.map_or(Value::Null, |intent| text(intent.0.0.clone())),
        ),
        ("purpose", text(purpose_name(request.purpose))),
        ("extended_hours", Value::Bool(request.extended_hours)),
        (
            "stop_price",
            request
                .stop_price
                .map_or(Value::Null, |price| text(price.to_string())),
        ),
        ("order_class", order_class.map_or(Value::Null, text)),
        (
            "take_profit",
            take_profit.map_or(Value::Null, |price| text(price.to_string())),
        ),
        (
            "stop",
            stop.map_or(Value::Null, |price| text(price.to_string())),
        ),
        ("rung", rung_value),
        ("at_floor", Value::Bool(at_floor)),
    ];
    let companion_id = batch.next_id();
    batch.journal("OrderRequestRecorded", None, companion)?;
    let mut submission = vec![
        ("client_order_id", text(request.client_order_id.as_str())),
        ("attempt", int(u64::from(attempt))?),
        ("instrument_id", text(request.instrument.as_str())),
        ("side", text(side_name(request.side))),
        ("type", text(order_type_name(request.order_type))),
        ("tif", text(tif_name(request.tif))),
        ("qty", text(request.qty.to_string())),
    ];
    submission.extend(order_submitted_optional_fields(request)?);
    batch.journal("OrderSubmitted", Some(companion_id), submission)?;
    Ok(())
}

/// `IntentReceived`'s payload: exactly the nine members journal spec §9.1 closes and
/// `mandate-journal` registers (`INTENT_RECEIVED_V1`), and nothing else. Seven are
/// `IntentProposed`'s, copied from what the agent proposed: `instrument_id`, `side`, `type`,
/// `tif`, `qty`, `limit_price`, and `purpose`. The other two are the ones §9.1 says the executor
/// adds: `intent_id` and `agent_id`. Never `agent`, `kind`, `instrument`, or `limit` (DEC-174
/// item 5), and never the protective prices: `received` writes `stop` and `take_profit` beside
/// the intent today, and the registered schema refuses that payload at `payload.stop` (DEC-307
/// item 1). Where they are journaled instead is DEC-360's question.
///
/// `tif` is the one the agent proposed, never the one the submission will carry: §9.1 has
/// `IntentReceived` copy `IntentProposed`, whose action members repeat its `DecisionMade`
/// exactly, and the two differ in practice, since a crypto add is submitted limit `ioc` where the
/// proposal said `day` (`protection::crypto_add`). `type` is `limit`: an intent's body carries a
/// limit and nothing else, since opening orders are limit orders (`AGENTS.md` rule 12).
///
/// `received` still journals its own pairs: `IntentHandoff` carries no proposed `tif`, and the
/// fold reads the protective prices and the old member names from this record, so the writer is
/// wired in once DEC-360 is ruled and a tests change brings the proposed `tif` (DEC-389 item 3).
///
/// # Errors
/// [`ExecutorError::NotInterpreted`] for a flatten plan, which is no proposed order (E7-4).
#[allow(
    dead_code,
    reason = "`received` writes this form once DEC-360 is ruled and `IntentHandoff` carries the proposed tif (DEC-389 item 3)"
)]
pub(crate) fn intent_received_fields(
    intent_id: &IntentId,
    agent: &AgentId,
    body: &IntentBody,
    proposed_tif: TimeInForce,
) -> Result<Vec<(&'static str, Value)>, ExecutorError> {
    let IntentBody::Order {
        instrument,
        side,
        qty,
        limit,
        purpose,
        ..
    } = body
    else {
        return Err(ExecutorError::NotInterpreted {
            what: "a flatten plan as IntentReceived".to_owned(),
            story: "E7-4",
        });
    };
    Ok(vec![
        ("intent_id", text(intent_id.0.0.clone())),
        ("agent_id", text(agent.0.clone())),
        ("instrument_id", text(instrument.as_str())),
        ("side", text(side_name(*side))),
        ("type", text(order_type_name(OrderType::Limit))),
        ("tif", text(tif_name(proposed_tif))),
        ("qty", text(qty.to_string())),
        ("limit_price", text(limit.to_string())),
        ("purpose", text(purpose_name(*purpose))),
    ])
}

/// `OrderSubmitted`'s nullable members, present as `null` when they are empty rather than omitted
/// (§4.2, DEC-174 item 5). The schema `mandate-journal` registers (`ORDER_SUBMITTED_V1`, the
/// vectors' seq 4) declares one: `limit_price`, the limit a market order never has. That is the
/// whole list: a draft that omits a schema-declared member is refused at that member, and one
/// that writes a member the schema does not declare is refused at that member too, `null` or
/// not, so this writes `limit_price` and nothing else (DEC-307 item 2).
///
/// `journal_rung` still writes `limit` (and the members DEC-360 holds open) beside the request, so
/// this is wired in with DEC-360's ruling (DEC-389 item 3).
///
/// # Errors
/// None today; the result keeps the writers' one signature.
#[allow(
    dead_code,
    reason = "`journal_rung` writes this form once DEC-360 is ruled (DEC-389 item 3)"
)]
pub(crate) fn order_submitted_optional_fields(
    request: &SubmitOrder,
) -> Result<Vec<(&'static str, Value)>, ExecutorError> {
    let limit_price = request
        .limit_price
        .map_or(Value::Null, |limit| text(limit.to_string()));
    Ok(vec![("limit_price", limit_price)])
}

/// `OrderAbandoned`: one of §5.7's two cases — a gate re-check that does not allow, or an intent
/// older than `max_intent_age` — or §5.4's bound cancelling an exit that has no order yet
/// ([`crate::protection::bound`]). Terminal, so the reservation is released and a later re-hand of
/// the intent hits the fold lookup and produces nothing.
pub(crate) fn abandon(
    batch: &mut Batch<'_, '_>,
    intent: &IntentId,
    reason: &str,
) -> Result<(), ExecutorError> {
    batch.journal(
        "OrderAbandoned",
        None,
        vec![
            (
                "client_order_id",
                text(ClientOrderId::for_intent(intent)?.as_str()),
            ),
            ("intent_id", text(intent.0.0.clone())),
            ("reason", text(reason)),
        ],
    )?;
    Ok(())
}

/// `Unknown → Intent` after a confirmed absence: the gate re-runs and the order is resubmitted
/// with the **same** client order id and the next attempt number, or abandoned (§5.7). Only a
/// re-check that **denies** abandons; one that holds leaves the order in `Intent`, held, for the
/// first tick its hold clears — an exit is held, never denied (`AGENTS.md` rule 13).
pub(crate) fn resubmit(batch: &mut Batch<'_, '_>, id: &ClientOrderId) -> Result<(), ExecutorError> {
    let Some(intent) = batch
        .view
        .orders
        .get(id)
        .and_then(|order| order.intent_id.clone())
    else {
        return send_again(batch, id);
    };
    if too_old(batch, &intent) {
        return abandon(batch, &intent, "intent_too_old");
    }
    match gate(batch, &intent, true)? {
        ALLOW => send_again(batch, id),
        HOLD => Ok(()),
        _ => abandon(batch, &intent, "gate_recheck"),
    }
}

/// Sends the request an `OrderSubmitted` already named again, under the same id with the next
/// attempt number.
fn send_again(batch: &mut Batch<'_, '_>, id: &ClientOrderId) -> Result<(), ExecutorError> {
    let unknown = || ExecutorError::UnknownOrder {
        client_order_id: id.as_str().to_owned(),
    };
    let order = batch.view.orders.get(id).cloned().ok_or_else(unknown)?;
    let request = batch
        .view
        .details
        .get(id)
        .and_then(|detail| detail.request.clone())
        .ok_or_else(unknown)?;
    let attempt = order.attempt.saturating_add(1);
    send(
        batch,
        request,
        order.intent_id.as_ref(),
        order.agent.as_ref().ok_or_else(unknown)?,
        attempt,
    )
}

/// Received intents with no order yet. With `stale_only`, the ones past their age are abandoned
/// and the rest are left for later; otherwise every one is gated now. `Input::Started` abandons
/// the stale at once and resumes the rest only once the startup reconciliation has run, so a
/// restart never submits a waiting intent on state it has not reconciled.
///
/// A new opening received before that run is not resumed here: the gate holds it until an account
/// has been journaled and a reconciliation has run since the start, and the first tick after
/// releases it or abandons it as too old (`release_held`).
pub(crate) fn resume(batch: &mut Batch<'_, '_>, stale_only: bool) -> Result<(), ExecutorError> {
    let waiting: Vec<IntentId> = batch
        .view
        .intents
        .values()
        .filter(|record| record.outcome == IntentOutcome::Received)
        .map(|record| record.intent_id.clone())
        .filter(|intent| {
            ClientOrderId::for_intent(intent).is_ok_and(|id| !batch.view.orders.contains_key(&id))
        })
        .collect();
    for intent in waiting {
        if too_old(batch, &intent) {
            abandon(batch, &intent, "intent_too_old")?;
        } else if !stale_only {
            gate_and_submit(batch, &intent)?;
        }
    }
    Ok(())
}

/// A held intent is released at the first tick its hold has cleared — as its first submission,
/// its sequence's cancels journaled before its `GateDecided` as on arrival (#286 round 1, minor
/// 1), or as the resubmission of an order a confirmed absence returned to `Intent` — abandoned
/// once it is too old, and otherwise left waiting without a journal entry per tick.
pub(crate) fn release_held(batch: &mut Batch<'_, '_>) -> Result<(), ExecutorError> {
    let held: Vec<IntentId> = batch.view.held.iter().cloned().collect();
    for intent in held {
        let id = ClientOrderId::for_intent(&intent)?;
        held_long(batch, &intent)?;
        if too_old(batch, &intent) {
            abandon(batch, &intent, "intent_too_old")?;
        } else if !batch.view.orders.contains_key(&id) {
            begin_and_submit(batch, &intent, false)?;
        } else if gate(batch, &intent, false)? == ALLOW {
            send_again(batch, &id)?;
        }
    }
    Ok(())
}

/// The writer/fold round-trip for an OCO submission (§9.5, rule 45): `journal_rung` journals the
/// companion carrying the OCO's class and legs beside the executor-only members, then the
/// causation-naming version-2 submission; the fold rebuilds the exact request from the pair — the
/// OCO's quantity from the submission's — and the order's owner and intent from the companion.
#[cfg(test)]
mod oco_round_trip_tests {
    use mandate_accounting::{InstrumentId, Side};
    use mandate_canon::Value;
    use mandate_num::{Price, Qty};

    use super::journal_rung;
    use crate::batch::Batch;
    use crate::error::ExecutorError;
    use crate::fold::fold;
    use crate::ids::{ClientOrderId, IntentId};
    use crate::payload::text;
    use crate::ports::Ports;
    use crate::reconcile::tests::{Everything, Executor, Ids, executor_config, fees};
    use crate::state::ExecutorState;
    use crate::types::{
        AccountRef, AccountScope, AgentId, EventId, FoldedEvent, OcoLegs, Purpose, Seq,
        SubmitOrder, TimeInForce, WorkspaceId,
    };

    fn refused(what: &str) -> ExecutorError {
        crate::error::ExecutorError::UnknownOrder {
            client_order_id: what.to_owned(),
        }
    }

    #[test]
    fn an_oco_submission_journals_its_companion_and_rebuilds_the_request()
    -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let executor = Executor::opened(&ports)?;
        let mut batch = Batch::new(&executor.state, &ports)?;
        let intent = IntentId(EventId("01JABCDEFGHJKMNPQRSTVWXYZ1".to_owned()));
        let agent = AgentId("agent-a".to_owned());
        let request = SubmitOrder {
            client_order_id: ClientOrderId::for_intent(&intent)?,
            instrument: InstrumentId::new("AAPL")?,
            side: Side::Sell,
            qty: Qty::parse("4")?,
            order_type: crate::types::OrderType::Limit,
            tif: TimeInForce::Gtc,
            limit_price: Some(Price::parse("149")?),
            stop_price: None,
            bracket: None,
            oco: Some(OcoLegs {
                take_profit: Price::parse("170")?,
                stop: Price::parse("140")?,
                qty: Qty::parse("4")?,
            }),
            extended_hours: false,
            purpose: Purpose::RiskExit,
        };
        journal_rung(&mut batch, &request, Some(&intent), &agent, 1, Vec::new())?;
        let drafts: Vec<_> = batch
            .effects
            .iter()
            .filter_map(|effect| match effect {
                crate::types::Effect::Journal(draft) => Some(draft.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            drafts.len(),
            2,
            "one batch, two events: the companion then the order"
        );
        let companion_draft = drafts.first().ok_or_else(|| refused("the companion"))?;
        let submission = drafts.get(1).ok_or_else(|| refused("the submission"))?;
        assert_eq!(companion_draft.event_type, "OrderRequestRecorded");
        assert_eq!(submission.event_type, "OrderSubmitted");
        assert_eq!(
            submission.causation_id.as_ref(),
            Some(&companion_draft.event_id),
            "the submission names its companion as its causation_id"
        );
        let companion = &companion_draft.payload;
        assert_eq!(companion.get("order_class"), Some(&text("oco")));
        assert_eq!(companion.get("take_profit"), Some(&text("170")));
        assert_eq!(companion.get("stop"), Some(&text("140")));
        assert_eq!(companion.get("agent_id"), Some(&text("agent-a")));
        assert_eq!(
            companion.get("intent_id"),
            Some(&text("01JABCDEFGHJKMNPQRSTVWXYZ1"))
        );
        assert_eq!(companion.get("purpose"), Some(&text("risk_exit")));
        assert_eq!(companion.get("rung"), Some(&Value::Null));
        assert_eq!(companion.get("at_floor"), Some(&Value::Bool(false)));

        let mut state = ExecutorState::new(AccountScope {
            account: AccountRef("acct-1".to_owned()),
            workspace: WorkspaceId("ws1".to_owned()),
        });
        let opened = FoldedEvent {
            stream: state.account_stream(),
            seq: Seq(1),
            event_id: EventId("01JABCDEFGHJKMNPQRSTVWXYZ0".to_owned()),
            event_type: "StreamOpened".to_owned(),
            causation_id: None,
            payload: crate::payload::object(vec![("environment", text("paper"))])?,
        };
        fold(&mut state, &opened)?;
        let stream = state.account_stream();
        for (index, draft) in drafts.iter().enumerate() {
            fold(
                &mut state,
                &FoldedEvent {
                    stream: stream.clone(),
                    seq: Seq(u64::try_from(index).unwrap_or(0) + 2),
                    event_id: draft.event_id.clone(),
                    event_type: draft.event_type.clone(),
                    causation_id: draft.causation_id.clone(),
                    payload: draft.payload.clone(),
                },
            )?;
        }
        let rebuilt = state
            .request_of(&request.client_order_id)
            .ok_or_else(|| crate::error::ExecutorError::UnknownOrder {
                client_order_id: request.client_order_id.as_str().to_owned(),
            })?
            .clone();
        assert_eq!(
            rebuilt.oco.as_ref().map(|legs| legs.qty),
            Some(Qty::parse("4")?),
            "the OCO's quantity is the submission's"
        );
        assert_eq!(
            rebuilt.oco.as_ref().map(|legs| legs.stop.to_string()),
            Some("140".to_owned())
        );
        assert_eq!(rebuilt.purpose, Purpose::RiskExit);
        assert!(!rebuilt.extended_hours);
        assert_eq!(rebuilt.stop_price, None);
        let order = state.order(&request.client_order_id).ok_or_else(|| {
            crate::error::ExecutorError::UnknownOrder {
                client_order_id: request.client_order_id.as_str().to_owned(),
            }
        })?;
        assert_eq!(
            order.agent.as_ref().map(|agent| agent.0.clone()),
            Some("agent-a".to_owned())
        );
        assert_eq!(order.intent_id, Some(intent));
        Ok(())
    }
}

#[cfg(test)]
mod bracket_call_tests {
    use mandate_accounting::{AssetClass, InstrumentId, Side};
    use mandate_canon::Value;
    use mandate_num::{Price, Qty, ShareIncrement};

    use super::bracket_of;
    use crate::batch::Batch;
    use crate::error::ExecutorError;
    use crate::ids::IntentId;
    use crate::ports::{InstrumentSnapshot, Ports};
    use crate::reconcile::tests::{
        Everything, Executor, Ids, executor_config, fees, protected_by_an_oco, submitted,
    };
    use crate::types::{
        AgentId, BracketLegs, BrokerRequest, Effect, EventId, ExitTier, Input, IntentBody,
        IntentHandoff, ProtectionPrices, Purpose, SubmitOrder, TimeInForce,
    };

    /// §5.4: only a risk-adding equity order carrying both prices is a bracket. An exit or a
    /// protective order never is, nor is a crypto entry (simple orders only); an equity entry with
    /// a stop and no take-profit answers the stub rather than going bare (DEC-346 items 1, 2).
    #[test]
    fn only_a_risk_adding_equity_entry_with_both_prices_is_a_bracket() -> Result<(), ExecutorError>
    {
        let (config, fees) = (executor_config(), fees()?);
        let both = ProtectionPrices {
            stop: Price::parse("140")?,
            take_profit: Some(Price::parse("170")?),
        };
        let stop_only = ProtectionPrices {
            take_profit: None,
            ..both
        };
        let legs = BracketLegs {
            take_profit: Price::parse("170")?,
            stop: Price::parse("140")?,
        };
        let aapl = InstrumentId::new("AAPL")?;
        let snapshots: [(&dyn InstrumentSnapshot, bool); 2] = [(&Everything, false), (&Coin, true)];
        for (instruments, crypto) in snapshots {
            let ports = Ports {
                ids: &Ids,
                mandates: &Everything,
                instruments,
                config: &config,
                fees: &fees,
            };
            let executor = Executor::opened(&ports)?;
            let batch = Batch::new(&executor.state, &ports)?;
            for purpose in PURPOSES {
                let case = format!("{purpose:?} crypto {crypto}");
                let bracket = purpose.adds_risk() && !crypto;
                assert_eq!(
                    bracket_of(&batch, &aapl, purpose, Some(both)),
                    Ok(bracket.then(|| legs.clone())),
                    "{case}"
                );
                assert_eq!(bracket_of(&batch, &aapl, purpose, None), Ok(None), "{case}");
                let expected = if bracket {
                    Err(ExecutorError::Unimplemented { story: "E7-4" })
                } else {
                    Ok(None)
                };
                assert_eq!(
                    bracket_of(&batch, &aapl, purpose, Some(stop_only)),
                    expected,
                    "{case}"
                );
            }
        }
        Ok(())
    }

    const PURPOSES: [Purpose; 7] = [
        Purpose::Open,
        Purpose::Increase,
        Purpose::RiskExit,
        Purpose::OwnerExit,
        Purpose::DiscretionaryExit,
        Purpose::Protective,
        Purpose::Flatten,
    ];

    /// A snapshot that classifies every instrument as crypto.
    struct Coin;

    impl InstrumentSnapshot for Coin {
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

    fn add(intent: &str, protection: Option<ProtectionPrices>) -> Result<Input, ExecutorError> {
        Ok(Input::Intent(IntentHandoff {
            intent_id: IntentId(EventId(intent.to_owned())),
            agent: AgentId("agent-a".to_owned()),
            tif: TimeInForce::Day,
            body: IntentBody::Order {
                instrument: InstrumentId::new("AAPL")?,
                side: Side::Buy,
                qty: Qty::parse("5")?,
                limit: Price::parse("150")?,
                purpose: Purpose::Increase,
                protection,
            },
        }))
    }

    /// #174 ruling (b), §5.4: where protection rests, the gate denies a plain add
    /// (`add_blocked_by_protective_order`), and a bracketed one goes as a new bracket beside the
    /// resting legs, cancelling nothing (DEC-346 item 1).
    #[test]
    fn an_add_where_protection_rests_is_denied_or_goes_as_a_new_bracket()
    -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = protected_by_an_oco(&ports)?;
        let plain = executor.run(add("01JABCDEFGHJKMNPQRSTVWXYZ1", None)?, &ports)?;
        assert!(
            plain.iter().any(|effect| matches!(
                effect,
                Effect::Journal(draft) if draft.event_type == "GateDecided"
                    && draft.payload.get("verdict") == Some(&Value::Str("deny".to_owned()))
                    && draft.payload.get("reason_code")
                        == Some(&Value::Str("add_blocked_by_protective_order".to_owned()))
            )),
            "{plain:?}"
        );
        assert_eq!(submitted(&plain), 0);
        let prices = ProtectionPrices {
            stop: Price::parse("140")?,
            take_profit: Some(Price::parse("170")?),
        };
        let bracketed = executor.run(add("01JABCDEFGHJKMNPQRSTVWXYZ2", Some(prices))?, &ports)?;
        let sent: Vec<&SubmitOrder> = bracketed
            .iter()
            .filter_map(|effect| match effect {
                Effect::Broker(BrokerRequest::Submit(order)) => Some(order),
                _ => None,
            })
            .collect();
        assert_eq!(sent.len(), 1, "{bracketed:?}");
        assert_eq!(
            sent.first().and_then(|order| order.bracket.clone()),
            Some(BracketLegs {
                take_profit: Price::parse("170")?,
                stop: Price::parse("140")?,
            }),
            "a bracketed add is a new GTC bracket at its own prices (§5.4)"
        );
        assert_eq!(sent.first().map(|order| order.tif), Some(TimeInForce::Gtc));
        assert!(
            !bracketed
                .iter()
                .any(|effect| matches!(effect, Effect::Broker(BrokerRequest::Cancel { .. }))),
            "and never a replacement of the resting protection"
        );
        Ok(())
    }
}

/// The account-stream writers' member pins (DEC-174 item 5, DEC-307, journal spec §9.1): each
/// reads its expectations from the committed vectors and hands what the writer wrote to
/// `mandate-journal`'s own `Draft::parse`, so every pinned payload is one the journal accepts.
#[cfg(test)]
mod draft_member_tests {
    use mandate_accounting::{InstrumentId, Side};
    use mandate_canon::{Value, to_canonical};
    use mandate_journal::Draft;
    use mandate_num::{Price, Qty};
    use serde_json::{Map, Value as Json};

    use super::{intent_received_fields, order_submitted_optional_fields};
    use crate::error::ExecutorError;
    use crate::ids::{ClientOrderId, IntentId};
    use crate::types::{
        AgentId, EventId, IntentBody, OcoLegs, OrderType, ProtectionPrices, Purpose, SubmitOrder,
        TimeInForce,
    };

    fn non_canonical() -> ExecutorError {
        ExecutorError::NonCanonicalPayload {
            field: "journal fixture".to_owned(),
        }
    }

    /// One account-stream chain event of the journal vectors (`fixtures/refcases/journal.json`,
    /// generated from `docs/specs/reference-cases/journal.yaml`) as a draft: its body less the
    /// members only a sealed row carries, checked to be `event_type`.
    fn vector_draft(index: usize, event_type: &str) -> Result<Map<String, Json>, ExecutorError> {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/refcases/journal.json"
        );
        let text = std::fs::read_to_string(path).map_err(|_| non_canonical())?;
        let fixture: Json = serde_json::from_str(&text).map_err(|_| non_canonical())?;
        let mut body = fixture
            .pointer(&format!("/chain/{index}/body"))
            .and_then(Json::as_object)
            .cloned()
            .ok_or_else(non_canonical)?;
        assert_eq!(
            body.get("event_type").and_then(Json::as_str),
            Some(event_type),
            "the vectors' chain event {index} is the {event_type}"
        );
        for sealed in ["seq", "recorded_at", "prev_hash"] {
            body.remove(sealed);
        }
        Ok(body)
    }

    fn payload_of(draft: &Map<String, Json>) -> Result<Map<String, Json>, ExecutorError> {
        draft
            .get("payload")
            .and_then(Json::as_object)
            .cloned()
            .ok_or_else(non_canonical)
    }

    fn sorted_names(payload: &Map<String, Json>) -> Vec<String> {
        let mut names: Vec<String> = payload.keys().cloned().collect();
        names.sort_unstable();
        names
    }

    /// What a writer wrote, as the journal reads it: its member names in order, duplicates kept,
    /// and each member's canonical JSON.
    fn written(
        pairs: Vec<(&'static str, Value)>,
    ) -> Result<(Vec<String>, Map<String, Json>), ExecutorError> {
        let mut names: Vec<String> = pairs.iter().map(|(name, _)| (*name).to_owned()).collect();
        names.sort_unstable();
        let mut members = Map::new();
        for (name, value) in pairs {
            let carried: Json =
                serde_json::from_slice(&to_canonical(&value)).map_err(|_| non_canonical())?;
            members.insert(name.to_owned(), carried);
        }
        Ok((names, members))
    }

    /// `draft` carrying `payload`, through `mandate-journal`'s validator: `None` when it accepts.
    fn journal_refuses(
        mut draft: Map<String, Json>,
        payload: Map<String, Json>,
    ) -> Result<Option<String>, ExecutorError> {
        draft.insert("payload".to_owned(), Json::Object(payload));
        let bytes = serde_json::to_vec(&Json::Object(draft)).map_err(|_| non_canonical())?;
        Ok(Draft::parse(&bytes)
            .err()
            .map(|refusal| format!("{refusal:?}")))
    }

    fn vector_text<'v>(
        payload: &'v Map<String, Json>,
        name: &str,
    ) -> Result<&'v str, ExecutorError> {
        payload
            .get(name)
            .and_then(Json::as_str)
            .ok_or_else(non_canonical)
    }

    /// The vectors' intent as the executor holds it, with `protection` as given.
    fn vector_intent(
        payload: &Map<String, Json>,
        protection: Option<ProtectionPrices>,
    ) -> Result<(IntentId, AgentId, IntentBody), ExecutorError> {
        Ok((
            IntentId(EventId(vector_text(payload, "intent_id")?.to_owned())),
            AgentId(vector_text(payload, "agent_id")?.to_owned()),
            IntentBody::Order {
                instrument: InstrumentId::new(vector_text(payload, "instrument_id")?)?,
                side: Side::Buy,
                qty: Qty::parse(vector_text(payload, "qty")?)?,
                limit: Price::parse(vector_text(payload, "limit_price")?)?,
                purpose: Purpose::Open,
                protection,
            },
        ))
    }

    /// `IntentReceived` is exactly the nine members §9.1 closes, each once, in the vector's own
    /// types and values, whatever protection the intent carries: `agent_id`, `instrument_id`,
    /// `type`, `tif`, and `limit_price` beside `intent_id`, `side`, `qty`, and `purpose`. Never
    /// `agent`, `kind`, `instrument`, or `limit` (DEC-174 item 5), and never the protective prices,
    /// which the registered schema refuses at `payload.stop` (DEC-307 item 1). Each payload is
    /// accepted by `mandate-journal`.
    #[test]
    fn intent_received_names_the_members_the_vectors_intent_carries() -> Result<(), ExecutorError> {
        let draft = vector_draft(1, "IntentReceived")?;
        let vector = payload_of(&draft)?;
        let closed = sorted_names(&vector);
        assert_eq!(closed.len(), 9, "§9.1 closes nine members: {closed:?}");
        let (stop, take_profit) = (Price::parse("140")?, Price::parse("160")?);
        for protection in [
            None,
            Some(ProtectionPrices {
                stop,
                take_profit: Some(take_profit),
            }),
            Some(ProtectionPrices {
                stop,
                take_profit: None,
            }),
        ] {
            let (intent, agent, body) = vector_intent(&vector, protection)?;
            let (names, members) = written(intent_received_fields(
                &intent,
                &agent,
                &body,
                TimeInForce::Day,
            )?)?;
            assert_eq!(
                names, closed,
                "exactly §9.1's nine members, each once, with protection {protection:?}"
            );
            assert_eq!(
                members, vector,
                "the vector's intent, member for member and type for type"
            );
            assert_eq!(
                journal_refuses(draft.clone(), members)?,
                None,
                "and mandate-journal accepts it"
            );
        }
        Ok(())
    }

    /// `IntentReceived`'s `tif` is the one the agent proposed, whatever it is: §9.1 has it copy
    /// `IntentProposed`, never the TIF the submission will carry or one inferred from the body
    /// (DEC-307 item 1).
    #[test]
    fn intent_received_carries_the_tif_the_agent_proposed() -> Result<(), ExecutorError> {
        let draft = vector_draft(1, "IntentReceived")?;
        let vector = payload_of(&draft)?;
        for (proposed, name) in [
            (TimeInForce::Day, "day"),
            (TimeInForce::Gtc, "gtc"),
            (TimeInForce::Ioc, "ioc"),
        ] {
            let (intent, agent, body) = vector_intent(&vector, None)?;
            let (_, members) = written(intent_received_fields(&intent, &agent, &body, proposed)?)?;
            let mut expected = vector.clone();
            expected.insert("tif".to_owned(), Json::String(name.to_owned()));
            assert_eq!(members, expected, "the proposed {name} is the recorded tif");
            assert_eq!(
                journal_refuses(draft.clone(), members)?,
                None,
                "and mandate-journal accepts it"
            );
        }
        Ok(())
    }

    fn vector_order(
        order_type: OrderType,
        limit_price: Option<Price>,
        stop_price: Option<Price>,
        oco: Option<OcoLegs>,
    ) -> Result<SubmitOrder, ExecutorError> {
        Ok(SubmitOrder {
            client_order_id: ClientOrderId::seeded_for_tests("c-1"),
            instrument: InstrumentId::new("b0b6dd9d-8b9b-48a9-ba46-b9d54906e415")?,
            side: Side::Buy,
            qty: Qty::parse("10")?,
            order_type,
            tif: TimeInForce::Day,
            limit_price,
            stop_price,
            bracket: None,
            oco,
            extended_hours: false,
            purpose: Purpose::Open,
        })
    }

    /// A market order writes `limit_price` present as `null`, never omitted (§4.2, DEC-174 item 5),
    /// and that is the only member this writes: `limit_price` is the one nullable member the
    /// registered `OrderSubmitted` declares, and any other member is refused, `null` or not
    /// (DEC-307 item 2). The vectors' `OrderSubmitted` as a market order, with it, is accepted.
    #[test]
    fn order_submitted_writes_null_for_its_empty_members() -> Result<(), ExecutorError> {
        let market = vector_order(OrderType::Market, None, None, None)?;
        let (names, members) = written(order_submitted_optional_fields(&market)?)?;
        assert_eq!(
            names,
            vec!["limit_price".to_owned()],
            "the schema's one nullable member, once"
        );
        assert_eq!(
            members.get("limit_price"),
            Some(&Json::Null),
            "a market order's limit is present as null"
        );
        let draft = vector_draft(3, "OrderSubmitted")?;
        let mut payload = payload_of(&draft)?;
        payload.insert("type".to_owned(), Json::String("market".to_owned()));
        payload.extend(members);
        assert_eq!(
            journal_refuses(draft, payload)?,
            None,
            "and mandate-journal accepts the market order"
        );
        Ok(())
    }

    /// An order with a limit writes it as `limit_price`, the vector's value in the vector's type,
    /// and still writes nothing else: a stop price and OCO legs are not members of the registered
    /// `OrderSubmitted`, so they are never written here, with or without a value (DEC-307 item 2).
    #[test]
    fn order_submitted_carries_its_optional_members_when_they_have_values()
    -> Result<(), ExecutorError> {
        let draft = vector_draft(3, "OrderSubmitted")?;
        let vector = payload_of(&draft)?;
        let limit = Price::parse(vector_text(&vector, "limit_price")?)?;
        let (stop, take_profit) = (Price::parse("140")?, Price::parse("160")?);
        for order in [
            vector_order(OrderType::Limit, Some(limit), None, None)?,
            vector_order(OrderType::StopLimit, Some(limit), Some(stop), None)?,
            vector_order(
                OrderType::Limit,
                Some(limit),
                None,
                Some(OcoLegs {
                    take_profit,
                    stop,
                    qty: Qty::parse("10")?,
                }),
            )?,
        ] {
            let (names, members) = written(order_submitted_optional_fields(&order)?)?;
            assert_eq!(
                names,
                vec!["limit_price".to_owned()],
                "only the schema's one nullable member, for {:?}",
                order.order_type
            );
            assert_eq!(
                members.get("limit_price"),
                vector.get("limit_price"),
                "the limit is the vector's, as decimal text"
            );
            let mut payload = vector.clone();
            payload.extend(members);
            assert_eq!(
                journal_refuses(draft.clone(), payload)?,
                None,
                "and mandate-journal accepts it"
            );
        }
        Ok(())
    }
}
