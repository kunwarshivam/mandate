//! The intent protocol (E7-2): received, gated, submitted under a derived id, and — after a
//! confirmed absence — resubmitted under the same id or abandoned.

use mandate_accounting::{AssetClass, InstrumentId};
use mandate_canon::Value;

use crate::batch::Batch;
use crate::codec::{order_type_name, purpose_name, side_name, tif_name};
use crate::error::ExecutorError;
use crate::gate::{PartialGateDecision, Proposal, UNPRICED, account_stream_checks};
use crate::ids::{ClientOrderId, IntentId};
use crate::payload::{int, text};
use crate::protection::{
    ExitPrice, awaits_cancel, begin_exit, bracketed_add, crypto_add, exit_limit, fallback,
    passive_exit, reprotect_unpriced,
};
use crate::state::IntentOutcome;
use crate::types::{
    AgentId, BrokerRequest, GateVerdict, IntentBody, IntentHandoff, OrderType, Purpose,
    SubmitOrder, TimeInForce,
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
        protection,
    } = &handoff.body
    else {
        return flatten_plan();
    };
    let mut pairs = vec![
        ("intent_id", text(handoff.intent_id.0.0.clone())),
        ("agent", text(handoff.agent.0.clone())),
        ("kind", text("order")),
        ("instrument", text(instrument.as_str())),
        ("side", text(side_name(*side))),
        ("qty", text(qty.to_string())),
        ("limit", text(limit.to_string())),
        ("purpose", text(purpose_name(*purpose))),
    ];
    if let Some(prices) = protection {
        pairs.push(("stop", text(prices.stop.to_string())));
        if let Some(take_profit) = prices.take_profit {
            pairs.push(("take_profit", text(take_profit.to_string())));
        }
    }
    batch.journal("IntentReceived", None, pairs)?;
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
        HOLD if decide(batch, intent)?.0.reason_code() == UNPRICED => {
            reprotect_unpriced(batch, intent)?;
        }
        _ => {}
    }
    Ok(())
}

const ALLOW: &str = "allow";
const HOLD: &str = "hold";

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
    let unpriced = *purpose == Purpose::DiscretionaryExit
        && decision.verdict == GateVerdict::Allow
        && exit_limit(batch, intent, instrument, *purpose, *limit) == ExitPrice::Held;
    Ok((
        if unpriced {
            decision.unpriced()
        } else {
            decision
        },
        *purpose,
    ))
}

/// Runs the binding gate on an intent, journals the decision, and answers its verdict name:
/// `allow`, `hold`, or `deny`. With `always` false, a decision that does not allow is not
/// journaled again: a held intent re-checked at a tick records only the moment it is released.
fn gate(
    batch: &mut Batch<'_, '_>,
    intent: &IntentId,
    always: bool,
) -> Result<&'static str, ExecutorError> {
    let (decision, purpose) = decide(batch, intent)?;
    let verdict = decision.verdict_name();
    if verdict == ALLOW || always {
        let decided = batch.journal(
            "GateDecided",
            None,
            vec![
                ("intent_id", text(intent.0.0.clone())),
                ("verdict", text(verdict)),
                ("reason_code", text(decision.reason_code())),
                ("purpose", text(purpose_name(purpose))),
                ("checks", decision.checks_value()?),
                ("evaluation", text("account_stream_only")),
            ],
        )?;
        if decision.reason_code() == UNPRICED {
            batch.notify(decided, UNPRICED);
        }
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
    bracketed_add(&batch.view, &instrument, purpose, protection.is_some())?;
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
    let tif = order_tif(batch, &instrument);
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
pub(crate) fn journal_rung(
    batch: &mut Batch<'_, '_>,
    request: &SubmitOrder,
    intent: Option<&IntentId>,
    agent: &AgentId,
    attempt: u32,
    extra: Vec<(&'static str, Value)>,
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
    pairs.extend(extra);
    batch.journal("OrderSubmitted", None, pairs)?;
    Ok(())
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

#[cfg(test)]
mod bracket_call_tests {
    use mandate_accounting::{InstrumentId, Side};
    use mandate_canon::Value;
    use mandate_num::{Price, Qty};

    use crate::error::ExecutorError;
    use crate::ids::IntentId;
    use crate::ports::Ports;
    use crate::reconcile::tests::{
        Everything, Ids, executor_config, fees, protected_by_an_oco, submitted,
    };
    use crate::types::{
        AgentId, Effect, EventId, Input, IntentBody, IntentHandoff, ProtectionPrices, Purpose,
    };

    fn add(intent: &str, protection: Option<ProtectionPrices>) -> Result<Input, ExecutorError> {
        Ok(Input::Intent(IntentHandoff {
            intent_id: IntentId(EventId(intent.to_owned())),
            agent: AgentId("agent-a".to_owned()),
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
    /// (`add_blocked_by_protective_order`), and `submit` asks `bracketed_add` before a bracketed
    /// one goes out, so it answers slice 2's stub rather than being sent beside the resting legs.
    #[test]
    fn an_add_where_protection_rests_is_denied_or_answers_slice_2s_stub()
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
        assert_eq!(
            executor.run(add("01JABCDEFGHJKMNPQRSTVWXYZ2", Some(prices))?, &ports),
            Err(ExecutorError::Unimplemented { story: "E7-4" })
        );
        Ok(())
    }
}
