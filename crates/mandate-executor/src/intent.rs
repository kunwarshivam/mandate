//! The intent protocol (E7-2): received, gated, submitted under a derived id, and — after a
//! confirmed absence — resubmitted under the same id or abandoned.

use mandate_accounting::AssetClass;
use mandate_canon::Value;

use crate::batch::Batch;
use crate::codec::{order_type_name, purpose_name, side_name, tif_name};
use crate::error::ExecutorError;
use crate::gate::{Proposal, decide};
use crate::ids::{ClientOrderId, IntentId};
use crate::payload::{int, text};
use crate::state::IntentOutcome;
use crate::types::{
    AgentId, BrokerRequest, IntentBody, IntentHandoff, OrderType, SubmitOrder, TimeInForce,
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
        side,
        qty,
        limit,
        purpose,
    } = &handoff.body
    else {
        return Err(ExecutorError::NotInterpreted {
            what: "a flatten plan handed over as an intent".to_owned(),
            story: "E7-4",
        });
    };
    batch.journal(
        "IntentReceived",
        None,
        vec![
            ("intent_id", text(handoff.intent_id.0.0.clone())),
            ("agent", text(handoff.agent.0.clone())),
            ("kind", text("order")),
            ("instrument", text(instrument.as_str())),
            ("side", text(side_name(*side))),
            ("qty", text(qty.to_string())),
            ("limit", text(limit.to_string())),
            ("purpose", text(purpose_name(*purpose))),
        ],
    )?;
    gate_and_submit(batch, &handoff.intent_id)
}

/// Whether an intent is past `max_intent_age`, measured from its `IntentReceived`. Checked at
/// **every** `Intent → Submitting` transition, the first included (§5.7, interpretation 11).
fn too_old(batch: &Batch<'_, '_>, intent: &IntentId) -> bool {
    batch.view.intents.get(intent).is_none_or(|record| {
        batch.at().secs().saturating_sub(record.received_at.secs())
            > batch.ports.config.max_intent_age_s
    })
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
    if gate(batch, intent, true)? {
        submit(batch, intent)?;
    }
    Ok(())
}

/// Runs the binding gate on an intent and journals the decision. With `always` false, a decision
/// that does not allow is not journaled again: a held intent re-checked at a tick records only the
/// moment it is released.
fn gate(batch: &mut Batch<'_, '_>, intent: &IntentId, always: bool) -> Result<bool, ExecutorError> {
    let (agent, body) = intent_of(batch, intent)?;
    let IntentBody::Order {
        instrument,
        side,
        qty,
        purpose,
        ..
    } = &body
    else {
        return Ok(false);
    };
    let decision = decide(
        &batch.view,
        &Proposal {
            agent: &agent,
            instrument,
            side: *side,
            qty: *qty,
            purpose: *purpose,
        },
        batch.ports,
    )?;
    let allowed = decision.allows();
    if allowed || always {
        batch.journal(
            "GateDecided",
            None,
            vec![
                ("intent_id", text(intent.0.0.clone())),
                ("verdict", text(decision.verdict_name())),
                ("reason_code", text(decision.reason_code())),
                ("purpose", text(purpose_name(*purpose))),
                ("checks", decision.checks_value()?),
            ],
        )?;
    }
    Ok(allowed)
}

fn intent_of(
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

/// The first submission of an intent: journaled as `OrderSubmitted` naming the id the request
/// carries, and only then described (journal spec §5.2, `AGENTS.md` rule 5).
fn submit(batch: &mut Batch<'_, '_>, intent: &IntentId) -> Result<(), ExecutorError> {
    let (agent, body) = intent_of(batch, intent)?;
    let IntentBody::Order {
        instrument,
        side,
        qty,
        limit,
        purpose,
    } = body
    else {
        return Ok(());
    };
    let tif = match batch.ports.instruments.asset_class(&instrument) {
        Some(AssetClass::Crypto) => TimeInForce::Gtc,
        _ => TimeInForce::Day,
    };
    let request = SubmitOrder {
        client_order_id: ClientOrderId::for_intent(intent)?,
        instrument,
        side,
        qty,
        order_type: OrderType::Limit,
        tif,
        limit_price: Some(limit),
        stop_price: None,
        bracket: None,
        oco: None,
        extended_hours: false,
        purpose,
    };
    send(batch, request, Some(intent), &agent, 1)
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
    let mut pairs = vec![
        ("client_order_id", text(request.client_order_id.as_str())),
        ("agent", text(agent.0.clone())),
        ("instrument", text(request.instrument.as_str())),
        ("side", text(side_name(request.side))),
        ("qty", text(request.qty.to_string())),
        ("order_type", text(order_type_name(request.order_type))),
        ("tif", text(tif_name(request.tif))),
        ("purpose", text(purpose_name(request.purpose))),
        ("attempt", int(u64::from(attempt))?),
        ("extended_hours", Value::Bool(request.extended_hours)),
    ];
    if let Some(intent) = intent {
        pairs.push(("intent_id", text(intent.0.0.clone())));
    }
    if let Some(limit) = request.limit_price {
        pairs.push(("limit", text(limit.to_string())));
    }
    if let Some(stop) = request.stop_price {
        pairs.push(("stop_price", text(stop.to_string())));
    }
    batch.journal("OrderSubmitted", None, pairs)?;
    batch.broker(BrokerRequest::Submit(request));
    Ok(())
}

/// `OrderAbandoned`: one of §5.7's two cases — a gate re-check that does not allow, or an intent
/// older than `max_intent_age`. Terminal, so the reservation is released and a later re-hand of
/// the intent hits the fold lookup and produces nothing.
fn abandon(
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
/// with the **same** client order id and the next attempt number, or abandoned (§5.7).
pub(crate) fn resubmit(batch: &mut Batch<'_, '_>, id: &ClientOrderId) -> Result<(), ExecutorError> {
    let Some(order) = batch.view.orders.get(id).cloned() else {
        return Ok(());
    };
    let request = batch
        .view
        .details
        .get(id)
        .and_then(|detail| detail.request.clone())
        .ok_or_else(|| ExecutorError::UnknownOrder {
            client_order_id: id.as_str().to_owned(),
        })?;
    if let Some(intent) = &order.intent_id {
        if too_old(batch, intent) {
            return abandon(batch, intent, "intent_too_old");
        }
        if !gate(batch, intent, true)? {
            return abandon(batch, intent, "gate_recheck");
        }
    }
    let attempt = order.attempt.saturating_add(1);
    send(
        batch,
        request,
        order.intent_id.as_ref(),
        &order.agent,
        attempt,
    )
}

/// Received intents with no order yet. With `stale_only`, the ones past their age are abandoned
/// and the rest are left for later; otherwise every one is gated now. `Input::Started` abandons
/// the stale at once and resumes the rest only once the startup reconciliation has run, so a
/// restart never submits on state it has not reconciled.
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

/// A held intent is released at the first tick its hold has cleared, abandoned once it is too
/// old, and otherwise left waiting without a journal entry per tick.
pub(crate) fn release_held(batch: &mut Batch<'_, '_>) -> Result<(), ExecutorError> {
    let held: Vec<IntentId> = batch.view.held.iter().cloned().collect();
    for intent in held {
        if too_old(batch, &intent) {
            abandon(batch, &intent, "intent_too_old")?;
        } else if gate(batch, &intent, false)? {
            submit(batch, &intent)?;
        }
    }
    Ok(())
}
