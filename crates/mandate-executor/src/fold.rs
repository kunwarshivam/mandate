//! The replay: one journaled event into the state, effect-free (journal spec §8).

use mandate_accounting::{InstrumentId, Side};
use mandate_canon::Value;
use mandate_num::{Qty, SignedQty, Usd};

use crate::codec::{mode_of, order_type_of, purpose_of, side_of, state_of, tif_of};
use crate::error::ExecutorError;
use crate::ids::{ClientOrderId, IntentId};
use crate::payload::{
    flag, optional_int, optional_price, optional_qty, optional_text, qty, required_text,
};
use crate::state::{ExecutorState, IntentOutcome, IntentRecord, OrderDetail};
use crate::types::{
    AccountState, AgentId, EventId, FillId, FoldedEvent, IntentBody, Mode, Order, OrderState,
    OrderType, Purpose, RiskClock, SubmitOrder, TimeInForce,
};

/// The copied cross-stream facts of journal spec §2. Each carries a `causation_id` naming its
/// origin, unless the executor originated it itself and says so with `originated`.
const COPIED: [&str; 5] = [
    "AgentModeApplied",
    "TradingDayStarted",
    "ClockAdvanced",
    "OwnerAcknowledged",
    "UniverseChanged",
];

/// Replays one journaled event into the state.
///
/// Total over the account-stream catalogue and **effect-free**: a replay can never re-send
/// anything, which is half of crash safety (the other half is that only [`crate::handle`]
/// produces effects). An event type, payload field, or stream this crate does not interpret is
/// [`ExecutorError::NotInterpreted`] naming the owning story, never a silent no-op (DEC-85).
pub fn fold(state: &mut ExecutorState, event: &FoldedEvent) -> Result<(), ExecutorError> {
    let account = event.stream == state.account_stream();
    if !account && !follows(state, &event.stream) {
        return Err(ExecutorError::ForeignStream {
            stream: event.stream.clone(),
        });
    }
    let expected = state
        .head(&event.stream)
        .map_or(1, |head| head.0.saturating_add(1));
    if event.seq.0 != expected {
        return Err(ExecutorError::SequenceOutOfOrder {
            stream: event.stream.clone(),
            expected,
            found: event.seq.0,
        });
    }
    if account {
        account_event(state, event)?;
    }
    state.heads.insert(event.stream.clone(), event.seq);
    let head = state.account_head().0;
    if state.unresolved.as_ref().is_some_and(|batch| {
        let drafted = u64::try_from(batch.drafts.len()).unwrap_or(u64::MAX);
        head >= batch.head.0.saturating_add(drafted)
    }) {
        state.unresolved = None;
    }
    Ok(())
}

/// The streams an account's executor reads without writing: its workspace's agent streams, the
/// control stream, and the clock stream (journal spec §2).
fn follows(state: &ExecutorState, stream: &str) -> bool {
    let workspace = &state.scope.workspace.0;
    stream.starts_with(&format!("agent:{workspace}:"))
        || stream == format!("ctl:{workspace}")
        || stream == format!("clock:{workspace}")
}

fn account_event(state: &mut ExecutorState, event: &FoldedEvent) -> Result<(), ExecutorError> {
    let payload = &event.payload;
    let kind = event.event_type.as_str();
    if kind == "StreamOpened" {
        return stream_opened(state, payload);
    }
    if let Some(story) = owner_elsewhere(kind) {
        return not_interpreted(kind, story);
    }
    let at = risk_clock(state, event)?;
    if COPIED.contains(&kind) && event.causation_id.is_none() && !flag(payload, "originated") {
        return Err(ExecutorError::CopyWithoutCausation {
            event_type: event.event_type.clone(),
        });
    }
    state.risk_clock = Some(at);
    match kind {
        "IntentReceived" => intent_received(state, payload, at),
        "GateDecided" => gate_decided(state, payload),
        "OrderSubmitted" => order_submitted(state, event),
        "OrderStateChanged" => order_state_changed(state, payload, at),
        "OrderAbandoned" => order_abandoned(state, payload),
        "AgentModeApplied" => agent_mode_applied(state, payload),
        "TradingDayStarted" | "ClockAdvanced" | "RiskDayStarted" | "MarkUpdated" => Ok(()),
        "FillApplied" | "LateFillApplied" => fill_applied(state, payload),
        "AccountRestrictionChanged" => {
            state.account_state = match required_text(payload, "restriction")? {
                "closing_only" => AccountState::ClosingOnly,
                "blocked" => AccountState::Blocked,
                _ => return Err(refused("restriction")),
            };
            Ok(())
        }
        _ => later_slice(),
    }
}

/// Fees, corporate actions, the account snapshot, rejects, reconciliation's records, the owner
/// acknowledgment, protection and the kill switch (trading-domain spec §5.4 to §5.7, §6, §7.3, §10,
/// §11): the later slices of this stack.
fn later_slice() -> Result<(), ExecutorError> {
    Err(ExecutorError::Unimplemented { story: "E7-3" })
}

/// The story that interprets an event this crate does not, or `None` for one it does. Answered
/// before anything else is read, so an uninterpreted event fails as uninterpreted rather than on
/// a field it was never going to be read for (DEC-85).
fn owner_elsewhere(kind: &str) -> Option<&'static str> {
    match kind {
        "IntentReceived"
        | "GateDecided"
        | "OrderSubmitted"
        | "OrderStateChanged"
        | "OrderAbandoned"
        | "FillApplied"
        | "LateFillApplied"
        | "FeesCharged"
        | "CorporateActionPrepared"
        | "CorporateActionApplied"
        | "ProtectionChanged"
        | "BrokerPositionObserved"
        | "ReconciliationRun"
        | "AccountStateObserved"
        | "AccountSnapshotRecorded"
        | "AccountRestrictionChanged"
        | "RejectObserved"
        | "AgentModeApplied"
        | "OwnerAcknowledged"
        | "TradingDayStarted"
        | "ClockAdvanced"
        | "RiskDayStarted"
        | "MarkUpdated"
        | "SettlementPosted"
        | "DividendPaid"
        | "CashInLieuPosted"
        | "CompensatingEvent"
        | "BrokerExchangeRecorded"
        | "ExternalActivityIngested"
        | "ConductBreachDetected"
        | "KillSwitchActivated" => None,
        "RelatedAccountsCoordination" => Some("E7-5"),
        "MandateVersionApplied"
        | "RiskLimitTriggered"
        | "RiskLimitLifted"
        | "HighWaterMarkReset"
        | "PositionReleased"
        | "InstrumentRestrictionChanged"
        | "GoalCompleted" => Some("E6-4"),
        "UniverseChanged" => Some("E17-3"),
        _ => Some("E7-2"),
    }
}

fn not_interpreted(kind: &str, story: &'static str) -> Result<(), ExecutorError> {
    Err(ExecutorError::NotInterpreted {
        what: kind.to_owned(),
        story,
    })
}

fn refused(field: &str) -> ExecutorError {
    ExecutorError::NonCanonicalPayload {
        field: field.to_owned(),
    }
}

/// `StreamOpened` fixes the stream's `environment` for good (ADR-0001 ES-23).
fn stream_opened(state: &mut ExecutorState, payload: &Value) -> Result<(), ExecutorError> {
    let found = required_text(payload, "environment")?;
    if let Some(opened) = &state.environment {
        return Err(ExecutorError::EnvironmentMismatch {
            opened: opened.clone(),
            found: found.to_owned(),
        });
    }
    state.environment = Some(found.to_owned());
    Ok(())
}

/// Every account-stream risk input carries `risk_clock`, and it never decreases (journal spec §2,
/// mandate spec §5.2).
fn risk_clock(state: &ExecutorState, event: &FoldedEvent) -> Result<RiskClock, ExecutorError> {
    let secs = optional_int(&event.payload, "risk_clock")
        .and_then(|secs| i64::try_from(secs).ok())
        .ok_or_else(|| ExecutorError::RiskClockMissing {
            event_type: event.event_type.clone(),
        })?;
    if let Some(last) = state.risk_clock
        && secs < last.secs()
    {
        return Err(ExecutorError::RiskClockWentBackwards {
            last: last.secs(),
            found: secs,
        });
    }
    Ok(RiskClock::from_secs(secs))
}

fn instrument(payload: &Value) -> Result<InstrumentId, ExecutorError> {
    Ok(InstrumentId::new(required_text(payload, "instrument")?)?)
}

fn intent_id(payload: &Value) -> Result<IntentId, ExecutorError> {
    Ok(IntentId(EventId(
        required_text(payload, "intent_id")?.to_owned(),
    )))
}

fn client_order_id(payload: &Value) -> Result<ClientOrderId, ExecutorError> {
    ClientOrderId::parse(required_text(payload, "client_order_id")?)
}

fn intent_received(
    state: &mut ExecutorState,
    payload: &Value,
    at: RiskClock,
) -> Result<(), ExecutorError> {
    let id = intent_id(payload)?;
    if optional_text(payload, "kind").is_some_and(|kind| kind != "order") {
        return not_interpreted("IntentReceived of a flatten plan", "E7-4");
    }
    let body = IntentBody::Order {
        instrument: instrument(payload)?,
        side: side_of(required_text(payload, "side")?)?,
        qty: qty(payload, "qty")?,
        limit: optional_price(payload, "limit")?.ok_or_else(|| refused("limit"))?,
        purpose: purpose_of(required_text(payload, "purpose")?)?,
        protection: None,
    };
    state.intents.insert(
        id.clone(),
        IntentRecord {
            intent_id: id.clone(),
            agent: AgentId(required_text(payload, "agent")?.to_owned()),
            received_at: at,
            outcome: IntentOutcome::Received,
        },
    );
    state.bodies.insert(id, body);
    Ok(())
}

fn gate_decided(state: &mut ExecutorState, payload: &Value) -> Result<(), ExecutorError> {
    let id = intent_id(payload)?;
    match required_text(payload, "verdict")? {
        "allow" => {}
        "deny" => {
            if let Some(record) = state.intents.get_mut(&id) {
                record.outcome = IntentOutcome::Denied;
            }
        }
        "hold" | "defer" => {
            state.held.insert(id);
            return Ok(());
        }
        _ => return Err(refused("verdict")),
    }
    state.held.remove(&id);
    Ok(())
}

/// The exact request an `OrderSubmitted` names, rebuilt from its payload, so a resubmission after
/// a confirmed absence sends the same body with the same id (trading-domain spec §5.7). A field an
/// older draft left out takes the value this crate's own drafts would have written: a buy, a limit
/// order, day, and an opening.
fn request_of(
    payload: &Value,
    client_order_id: ClientOrderId,
) -> Result<SubmitOrder, ExecutorError> {
    Ok(SubmitOrder {
        client_order_id,
        instrument: instrument(payload)?,
        side: optional_text(payload, "side").map_or(Ok(Side::Buy), side_of)?,
        qty: optional_qty(payload, "qty")?.unwrap_or(Qty::ZERO),
        order_type: optional_text(payload, "order_type")
            .map_or(Ok(OrderType::Limit), order_type_of)?,
        tif: optional_text(payload, "tif").map_or(Ok(TimeInForce::Day), tif_of)?,
        limit_price: optional_price(payload, "limit")?,
        stop_price: optional_price(payload, "stop_price")?,
        bracket: None,
        oco: None,
        extended_hours: flag(payload, "extended_hours"),
        purpose: optional_text(payload, "purpose").map_or(Ok(Purpose::Open), purpose_of)?,
    })
}

fn order_submitted(state: &mut ExecutorState, event: &FoldedEvent) -> Result<(), ExecutorError> {
    let payload = &event.payload;
    let id = client_order_id(payload)?;
    let intent = optional_text(payload, "intent_id").map(|raw| IntentId(EventId(raw.to_owned())));
    let request = request_of(payload, id.clone())?;
    let attempt = optional_int(payload, "attempt")
        .and_then(|attempt| u32::try_from(attempt).ok())
        .unwrap_or(1);
    let reserved = match request.side {
        Side::Buy => request
            .limit_price
            .map_or(Ok(Usd::ZERO), |limit| request.qty.notional(limit))?,
        Side::Sell => Usd::ZERO,
    };
    let agent = AgentId(required_text(payload, "agent")?.to_owned());
    let created_on = None;
    let order = state.orders.entry(id.clone()).or_insert_with(|| Order {
        client_order_id: id.clone(),
        intent_id: intent.clone(),
        agent,
        instrument: request.instrument.clone(),
        side: request.side,
        qty: request.qty,
        filled_qty: Qty::ZERO,
        state: OrderState::Submitting,
        attempt,
        purpose: request.purpose,
        absent_lookups: 0,
        first_absence_at: None,
        cancel_unconfirmed: false,
        replaced_by: None,
        created_on,
    });
    order.state = OrderState::Submitting;
    order.attempt = attempt;
    order.absent_lookups = 0;
    order.first_absence_at = None;
    state.reservations.insert(id.clone(), reserved);
    state.details.insert(
        id,
        OrderDetail {
            request: Some(request),
            ..OrderDetail::default()
        },
    );
    if let Some(record) = intent.and_then(|intent| state.intents.get_mut(&intent)) {
        record.outcome = IntentOutcome::Submitted;
    }
    state.last_submission = Some(event.seq);
    Ok(())
}

fn order_state_changed(
    state: &mut ExecutorState,
    payload: &Value,
    at: RiskClock,
) -> Result<(), ExecutorError> {
    let id = client_order_id(payload)?;
    let next = state_of(required_text(payload, "state")?)?;
    if optional_text(payload, "replaces").is_some()
        || optional_text(payload, "replaced_by").is_some()
    {
        return later_slice();
    }
    let order = state
        .orders
        .get_mut(&id)
        .ok_or_else(|| ExecutorError::UnknownOrder {
            client_order_id: id.as_str().to_owned(),
        })?;
    let detail = state.details.entry(id.clone()).or_default();
    if flag(payload, "ignored") {
        return Ok(());
    }
    if optional_text(payload, "lookup") == Some("absent") {
        order.absent_lookups = order.absent_lookups.saturating_add(1);
        order.first_absence_at = order.first_absence_at.or(Some(at));
        detail.last_absence = Some(at);
    } else if next == OrderState::Unknown {
        order.absent_lookups = 0;
        order.first_absence_at = None;
        detail.unknown_since = Some(at);
        detail.last_absence = None;
    }
    order.state = next;
    if next.is_terminal() {
        state.reservations.remove(&id);
    }
    Ok(())
}

/// `OrderAbandoned` ends an intent. An intent abandoned before it was ever submitted has no order
/// yet, so the order is recorded from the intent, in its terminal state.
fn order_abandoned(state: &mut ExecutorState, payload: &Value) -> Result<(), ExecutorError> {
    let id = client_order_id(payload)?;
    let intent = intent_id(payload)?;
    if let Some(order) = state.orders.get_mut(&id) {
        order.state = OrderState::Abandoned;
    } else {
        let (
            Some(IntentBody::Order {
                instrument,
                side,
                qty,
                purpose,
                ..
            }),
            Some(record),
        ) = (state.bodies.get(&intent), state.intents.get(&intent))
        else {
            return Err(ExecutorError::UnknownOrder {
                client_order_id: id.as_str().to_owned(),
            });
        };
        let order = Order {
            client_order_id: id.clone(),
            intent_id: Some(intent.clone()),
            agent: record.agent.clone(),
            instrument: instrument.clone(),
            side: *side,
            qty: *qty,
            filled_qty: Qty::ZERO,
            state: OrderState::Abandoned,
            attempt: 0,
            purpose: *purpose,
            absent_lookups: 0,
            first_absence_at: None,
            cancel_unconfirmed: false,
            replaced_by: None,
            created_on: None,
        };
        state.orders.insert(id.clone(), order);
    }
    state.reservations.remove(&id);
    if let Some(record) = state.intents.get_mut(&intent) {
        record.outcome = IntentOutcome::Abandoned;
    }
    state.held.remove(&intent);
    Ok(())
}

/// A fill is applied by its broker fill id, once: a re-ingested fill changes nothing
/// (trading-domain spec §5.7, journal spec §5.2). It moves the position the gate's
/// `sell_exceeds_available` reads, and the filled quantity of the order it names.
fn fill_applied(state: &mut ExecutorState, payload: &Value) -> Result<(), ExecutorError> {
    let fill = FillId(required_text(payload, "fill_id")?.to_owned());
    if state.fills.contains(&fill) {
        return Ok(());
    }
    let instrument = instrument(payload)?;
    let quantity = qty(payload, "qty_gross")?;
    let signed = match side_of(required_text(payload, "side")?)? {
        Side::Buy => SignedQty::from(quantity),
        Side::Sell => SignedQty::from(quantity).negated(),
    };
    let position = state.positions.entry(instrument).or_insert(SignedQty::ZERO);
    *position = position.checked_add(signed)?;
    if let Some(order) = optional_text(payload, "client_order_id")
        .and_then(|raw| ClientOrderId::parse(raw).ok())
        .and_then(|id| state.orders.get_mut(&id))
    {
        order.filled_qty = order.filled_qty.checked_add(quantity)?;
    }
    state.fills.insert(fill);
    Ok(())
}

/// One restriction on one agent (or on every agent, as `*`). A mode is the strictest of an
/// agent's active restrictions, and `normal` lifts the restriction it names (mandate spec §5.9).
fn agent_mode_applied(state: &mut ExecutorState, payload: &Value) -> Result<(), ExecutorError> {
    let agent = AgentId(required_text(payload, "agent")?.to_owned());
    let mode = mode_of(required_text(payload, "to")?)?;
    let restriction = optional_text(payload, "restriction")
        .unwrap_or_default()
        .to_owned();
    if mode == Mode::Normal {
        state.restrictions.remove(&(agent.clone(), restriction));
    } else {
        state
            .restrictions
            .insert((agent.clone(), restriction), mode);
    }
    recompute(state, &agent);
    Ok(())
}

fn recompute(state: &mut ExecutorState, agent: &AgentId) {
    let mode = state
        .restrictions
        .iter()
        .filter(|((who, _), _)| who == agent)
        .map(|(_, mode)| *mode)
        .max()
        .unwrap_or_default();
    state.modes.insert(agent.clone(), mode);
}
