//! The intent protocol (E7-2): received, gated, submitted under a derived id, and — after a
//! confirmed absence — resubmitted under the same id or abandoned.

use mandate_accounting::AssetClass;
use mandate_canon::Value;

use crate::batch::Batch;
use crate::codec::{order_type_name, purpose_name, side_name, tif_name};
use crate::error::ExecutorError;
use crate::gate::{Proposal, account_stream_checks};
use crate::ids::{ClientOrderId, IntentId};
use crate::payload::{int, text};
use crate::protection::{awaits_cancel, begin_exit, sequenced};
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
        ..
    } = &handoff.body
    else {
        return flatten_plan();
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

/// A flatten plan handed over as an intent: the agent-scoped flatten's sells are the protective
/// sequence's (E7-4, the coordinator's ruling (d) on #174).
fn flatten_plan() -> Result<(), ExecutorError> {
    Err(ExecutorError::Unimplemented { story: "E7-4" })
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
    begin_exit(batch, intent)?;
    if gate(batch, intent, true)? == ALLOW {
        submit(batch, intent)?;
    }
    Ok(())
}

const ALLOW: &str = "allow";
const HOLD: &str = "hold";

/// Runs the binding gate on an intent, journals the decision, and answers its verdict name:
/// `allow`, `hold`, or `deny`. With `always` false, a decision that does not allow is not
/// journaled again: a held intent re-checked at a tick records only the moment it is released.
fn gate(
    batch: &mut Batch<'_, '_>,
    intent: &IntentId,
    always: bool,
) -> Result<&'static str, ExecutorError> {
    let (agent, body) = intent_of(batch, intent)?;
    let IntentBody::Order {
        instrument,
        side,
        qty,
        purpose,
        ..
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
        },
        batch.ports,
    )?;
    let verdict = decision.verdict_name();
    if verdict == ALLOW || always {
        batch.journal(
            "GateDecided",
            None,
            vec![
                ("intent_id", text(intent.0.0.clone())),
                ("verdict", text(decision.verdict_name())),
                ("reason_code", text(decision.reason_code())),
                ("purpose", text(purpose_name(*purpose))),
                ("checks", decision.checks_value()?),
                ("evaluation", text("account_stream_only")),
            ],
        )?;
    }
    Ok(verdict)
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
        ..
    } = body
    else {
        return Ok(());
    };
    if awaits_cancel(&batch.view, &instrument, purpose) {
        return Ok(());
    }
    sequenced(&batch.view, &instrument, purpose)?;
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
    journal_submission(batch, &request, intent, agent, attempt)?;
    batch.broker(BrokerRequest::Submit(request));
    Ok(())
}

/// `OrderSubmitted` naming every field of `request`, without describing the request yet, for a
/// sequence that records more before it acts (a re-placement's `ProtectionChanged`).
pub(crate) fn journal_submission(
    batch: &mut Batch<'_, '_>,
    request: &SubmitOrder,
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
    if let Some(oco) = &request.oco {
        pairs.push(("order_class", text("oco")));
        pairs.push(("take_profit", text(oco.take_profit.to_string())));
        pairs.push(("stop", text(oco.stop.to_string())));
    }
    batch.journal("OrderSubmitted", None, pairs)?;
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

/// A held intent is released at the first tick its hold has cleared — as its first submission, or
/// as the resubmission of an order a confirmed absence returned to `Intent` — abandoned once it is
/// too old, and otherwise left waiting without a journal entry per tick.
pub(crate) fn release_held(batch: &mut Batch<'_, '_>) -> Result<(), ExecutorError> {
    let held: Vec<IntentId> = batch.view.held.iter().cloned().collect();
    for intent in held {
        if too_old(batch, &intent) {
            abandon(batch, &intent, "intent_too_old")?;
        } else if gate(batch, &intent, false)? == ALLOW {
            let id = ClientOrderId::for_intent(&intent)?;
            if batch.view.orders.contains_key(&id) {
                send_again(batch, &id)?;
            } else {
                submit(batch, &intent)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod sequence_call_tests {
    use mandate_accounting::{InstrumentId, Side};
    use mandate_canon::Value;
    use mandate_num::{Price, Qty};

    use crate::error::ExecutorError;
    use crate::ids::IntentId;
    use crate::payload::object;
    use crate::ports::Ports;
    use crate::reconcile::tests::{
        Everything, Ids, executor_config, fees, protected_by_an_oco, submitted,
    };
    use crate::types::{
        AgentId, EventId, Input, IntentBody, IntentHandoff, MarketObservation, Purpose, RiskClock,
    };

    fn exit(intent: &str) -> Result<Input, ExecutorError> {
        Ok(Input::Intent(IntentHandoff {
            intent_id: IntentId(EventId(intent.to_owned())),
            agent: AgentId("agent-a".to_owned()),
            body: IntentBody::Order {
                instrument: InstrumentId::new("AAPL")?,
                side: Side::Sell,
                qty: Qty::parse("5")?,
                limit: Price::parse("150")?,
                purpose: Purpose::RiskExit,
                protection: None,
            },
        }))
    }

    /// #258 round 1, major 1; DEC-160 4. `submit` asks `sequenced` before an exit in a protected
    /// instrument goes out, so one no sequence runs for — after slice 3a, a passive exit, a sell
    /// limit above the latest bid — is never sent beside the resting legs: it answers the loud stub
    /// slice 3b replaces. That stub is a denied exit once reachable, which is why nothing creates
    /// protection before exits through it work (`AGENTS.md` rule 13, DEC-160 4). Once no
    /// protective order rests, the same exit is submitted.
    #[test]
    fn an_exit_where_protection_rests_answers_the_sequence_stub() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = protected_by_an_oco(&ports)?;
        let bid = Price::parse("149")?;
        executor.run(
            Input::Market(MarketObservation {
                instrument: InstrumentId::new("AAPL")?,
                bid: Some(bid),
                bid_size: Some(Qty::parse("100")?),
                ask: Some(bid),
                last_trade: Some(bid),
                mark: Some(bid),
                sane: true,
                observed_at: RiskClock::from_secs(0),
            }),
            &ports,
        )?;
        assert_eq!(
            executor.run(exit("01JABCDEFGHJKMNPQRSTVWXYZ1")?, &ports),
            Err(ExecutorError::Unimplemented { story: "E7-4" })
        );
        executor.commit_one(
            concat!("Protection", "Changed"),
            object(vec![
                ("instrument", Value::Str("AAPL".to_owned())),
                ("action", Value::Str("cancelled".to_owned())),
                ("orders", Value::Str("md-oco-1".to_owned())),
                ("qty", Value::Str("10".to_owned())),
                (
                    "risk_clock",
                    crate::payload::clock(RiskClock::from_secs(0))?,
                ),
            ])?,
        )?;
        let sent = executor.run(exit("01JABCDEFGHJKMNPQRSTVWXYZ2")?, &ports)?;
        assert_eq!(submitted(&sent), 1, "with nothing resting the exit goes");
        Ok(())
    }
}
