//! The replay: one journaled event into the state, effect-free (journal spec §8).

use mandate_accounting::{InstrumentId, Side};
use mandate_canon::Value;
use mandate_num::{Qty, SignedQty, Usd};

use crate::codec::{mode_of, order_type_of, purpose_of, side_of, state_of, tif_of};
use crate::error::ExecutorError;
use crate::ids::{ClientOrderId, IntentId};
use crate::payload::{
    flag, optional_int, optional_price, optional_qty, optional_text, optional_usd, qty,
    required_text, usd,
};
use crate::state::{
    Adoption, ExecutorState, IntentOutcome, IntentRecord, ObservedAccount, OrderDetail,
};
use crate::types::{
    AccountState, ActivityCursor, AgentId, EventId, FillId, FoldedEvent, IntentBody, Mode, Order,
    OrderState, OrderType, Purpose, RiskClock, SubmitOrder, TimeInForce,
};

/// The copied cross-stream facts of journal spec §2 this crate interprets. Each carries a
/// `causation_id` naming its origin, unless the executor originated it itself and says so with
/// `originated`. `OwnerAcknowledged` and `TradingDayStarted` join them with the slices that
/// interpret them, and until then answer those slices' stubs like every other event not reached.
const COPIED: [&str; 3] = ["AgentModeApplied", "ClockAdvanced", "UniverseChanged"];

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
        "OrderStateChanged" => {
            adoption(state, event)?;
            order_state_changed(state, payload, at)
        }
        "CompensatingEvent" => {
            if let Some(Value::Array(corrected)) = payload.get("corrected_event_ids") {
                for id in corrected.iter().filter_map(Value::as_str) {
                    state.uncompensated.remove(&EventId(id.to_owned()));
                }
            }
            Ok(())
        }
        "BrokerPositionObserved" => {
            if flag(payload, "mismatch") {
                state.mismatched.insert(instrument(payload)?);
            }
            Ok(())
        }
        "OwnerAcknowledged" => owner_acknowledged(state, payload),
        "ReconciliationRun" => {
            state.reconciled_through = Some(event.seq);
            if let Some(cursor) = optional_text(payload, "checkpoint") {
                state.checkpoint = Some(ActivityCursor(cursor.to_owned()));
            }
            Ok(())
        }
        "OrderAbandoned" => order_abandoned(state, payload),
        "AgentModeApplied" => agent_mode_applied(state, payload),
        "ClockAdvanced" | "MarkUpdated" => Ok(()),
        "FillApplied" | "LateFillApplied" => fill_applied(state, payload),
        "FeesCharged" => fees_charged(state, payload),
        "ExternalActivityIngested" => Ok(()),
        "AccountRestrictionChanged" => {
            state.account_state = match required_text(payload, "restriction")? {
                "closing_only" => AccountState::ClosingOnly,
                "blocked" => AccountState::Blocked,
                _ => return Err(refused("restriction")),
            };
            state.consecutive_403s = 0;
            Ok(())
        }
        "AccountStateObserved" | "AccountSnapshotRecorded" => account_observed(state, payload),
        "RejectObserved" => {
            state.consecutive_403s = if optional_int(payload, "http_status") == Some(403) {
                state.consecutive_403s.saturating_add(1)
            } else {
                0
            };
            Ok(())
        }
        _ => later_slice(),
    }
}

/// Corporate actions, reconciliation's records and snapshot, conduct breaches, recorded broker
/// exchanges, the owner acknowledgment, the trading and risk days, protection and the kill switch
/// (trading-domain spec §5.4 to §5.7, §6, §10, §11): the later slices of this stack. An
/// `ExternalActivityIngested` folds as a record only: the restriction it causes is its own
/// `AgentModeApplied`.
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
            submitted_seq: Some(event.seq),
            ..OrderDetail::default()
        },
    );
    if let Some(record) = intent.and_then(|intent| state.intents.get_mut(&intent)) {
        record.outcome = IntentOutcome::Submitted;
    }
    state.last_submission = Some(event.seq);
    Ok(())
}

/// Records an adoption as owed a `CompensatingEvent` until one names it (§11).
fn adoption(state: &mut ExecutorState, event: &FoldedEvent) -> Result<(), ExecutorError> {
    if !flag(&event.payload, "adopted") {
        return Ok(());
    }
    let subject = client_order_id(&event.payload)?;
    let from = state
        .orders
        .get(&subject)
        .map(|order| order.state)
        .ok_or_else(|| ExecutorError::UnknownOrder {
            client_order_id: subject.as_str().to_owned(),
        })?;
    let to = state_of(required_text(&event.payload, "state")?)?;
    state
        .uncompensated
        .insert(event.event_id.clone(), Adoption { subject, from, to });
    Ok(())
}

fn order_state_changed(
    state: &mut ExecutorState,
    payload: &Value,
    at: RiskClock,
) -> Result<(), ExecutorError> {
    let id = client_order_id(payload)?;
    let next = state_of(required_text(payload, "state")?)?;
    if let Some(old) = optional_text(payload, "replaces") {
        replacement(state, &id, &ClientOrderId::parse(old)?)?;
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
        let amount = state.reservations.remove(&id);
        if let Some(new) = optional_text(payload, "replaced_by") {
            let new = ClientOrderId::parse(new)?;
            order.replaced_by = Some(new.clone());
            if let Some(amount) = amount {
                state.reservations.insert(new, amount);
            }
        }
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
    let notional =
        quantity.notional(optional_price(payload, "price")?.ok_or_else(|| refused("price"))?)?;
    let (signed, cash) = match side_of(required_text(payload, "side")?)? {
        Side::Buy => (SignedQty::from(quantity), notional.negated()),
        Side::Sell => (SignedQty::from(quantity).negated(), notional),
    };
    let position = state.positions.entry(instrument).or_insert(SignedQty::ZERO);
    *position = position.checked_add(signed)?;
    state.cash_flow = state.cash_flow.checked_add(cash)?;
    state.fill_notional = state.fill_notional.checked_add(notional)?;
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

/// The order a broker-initiated replacement created, linked to the one it replaced (trading-domain
/// spec §5.7, interpretation 26). Its quantity is what the original had left, `qty − filled_qty`,
/// and it has filled nothing, so the pair never absorbs more than the gate approved: a later fill
/// beyond that remainder cannot be the new order's and is ingested as external activity (§7.1).
///
/// The reservation the original held passes over unchanged when the original ends. It was sized
/// for the whole original order, more than is now left, and over-reserving is the safe side
/// (`AGENTS.md` rule 3); an original that held none passes none.
fn replacement(
    state: &mut ExecutorState,
    id: &ClientOrderId,
    old: &ClientOrderId,
) -> Result<(), ExecutorError> {
    let original = state
        .orders
        .get(old)
        .ok_or_else(|| ExecutorError::UnknownOrder {
            client_order_id: old.as_str().to_owned(),
        })?;
    let linked = Order {
        client_order_id: id.clone(),
        qty: original.qty.checked_sub(original.filled_qty)?,
        filled_qty: Qty::ZERO,
        replaced_by: None,
        ..original.clone()
    };
    state.orders.insert(id.clone(), linked);
    Ok(())
}

/// The broker's account as last reported (trading-domain spec §7.2, §7.3). The model's cash is
/// moved from this base by every fill since, which is what buying power and the cash comparison
/// start from (§11). Its state reads §7.3's first row: any blocking flag, or a status other than
/// `ACTIVE`, is `blocked`. `crypto_status` is journaled and not folded until the crypto row of §7.3
/// is (the backlog carries it). An account missing `non_marginable_buying_power` or
/// `accrued_fees` is incomplete, and buying power is `None` for it rather than guessed.
fn account_observed(state: &mut ExecutorState, payload: &Value) -> Result<(), ExecutorError> {
    let buying_power = usd(payload, "buying_power")?;
    let non_marginable = optional_usd(payload, "non_marginable_buying_power")?;
    let accrued = optional_usd(payload, "accrued_fees")?;
    let blocked = required_text(payload, "status")? != "ACTIVE"
        || flag(payload, "trading_blocked")
        || flag(payload, "account_blocked")
        || flag(payload, "trade_suspended_by_user");
    state.observed = Some(ObservedAccount {
        state: if blocked {
            AccountState::Blocked
        } else {
            AccountState::Active
        },
        multiplier: optional_int(payload, "multiplier")
            .and_then(|multiplier| u32::try_from(multiplier).ok())
            .unwrap_or(1),
        equity: usd(payload, "equity")?,
        cash: usd(payload, "cash")?,
        buying_power,
        non_marginable_buying_power: non_marginable.unwrap_or(buying_power),
        accrued_fees: accrued.unwrap_or(Usd::ZERO),
        complete: non_marginable.is_some() && accrued.is_some(),
    });
    state.cash_flow = Usd::ZERO;
    state.fill_notional = Usd::ZERO;
    Ok(())
}

/// Paper's simulated fee accrues in its `(family, day)` bucket, which buying power charges at
/// `round(bucket, 2, ceiling)` as `mandate-accounting` charges it (trading-domain spec §6.2, §7.2,
/// §10, DEC-104), so paper never looks richer than live. The broker's own fees, posted or
/// unposted, are the cash slice's; until then they fold as records only.
fn fees_charged(state: &mut ExecutorState, payload: &Value) -> Result<(), ExecutorError> {
    if required_text(payload, "family")? == "crypto_asset" {
        let instrument = instrument(payload)?;
        let accrued = qty(payload, "accrued")?;
        let charged = optional_qty(payload, "charged")?.unwrap_or(Qty::ZERO);
        let position = state
            .positions
            .entry(instrument.clone())
            .or_insert(SignedQty::ZERO);
        *position = position.checked_add(SignedQty::from(accrued).negated())?;
        let unposted = state.asset_fees.entry(instrument).or_insert(Qty::ZERO);
        *unposted = unposted
            .checked_add(accrued)?
            .checked_sub(charged)
            .unwrap_or(Qty::ZERO);
        return Ok(());
    }
    if !flag(payload, "simulated") {
        let charged = optional_usd(payload, "charged")?.unwrap_or(Usd::ZERO);
        state.unposted_fees = state
            .unposted_fees
            .checked_add(usd(payload, "accrued")?)?
            .checked_sub(charged)?;
        return Ok(());
    }
    {
        let bucket = (
            required_text(payload, "family")?.to_owned(),
            required_text(payload, "day")?.to_owned(),
        );
        let accrued = state.simulated_fees.entry(bucket).or_insert(Usd::ZERO);
        *accrued = accrued.checked_add(usd(payload, "accrued")?)?;
    }
    Ok(())
}

/// Only an owner acknowledgment carrying step-up evidence clears a reconciliation mismatch and the
/// pause it caused (trading-domain spec §11, interpretation 14).
fn owner_acknowledged(state: &mut ExecutorState, payload: &Value) -> Result<(), ExecutorError> {
    let subject = required_text(payload, "subject")?;
    if required_text(payload, "step_up")?.is_empty() {
        return Ok(());
    }
    if let Ok(instrument) = InstrumentId::new(subject) {
        state.mismatched.remove(&instrument);
    }
    let lifted = crate::state::restriction_for(subject);
    let agents: Vec<AgentId> = state
        .restrictions
        .keys()
        .filter(|(_, name)| *name == lifted)
        .map(|(agent, _)| agent.clone())
        .collect();
    for agent in agents {
        state.restrictions.remove(&(agent.clone(), lifted.clone()));
        recompute(state, &agent);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use mandate_accounting::{InstrumentId, Side};
    use mandate_num::Qty;

    use super::fold;
    use crate::error::ExecutorError;
    use crate::ids::{ClientOrderId, IntentId};
    use crate::payload::{clock, object, text};
    use crate::state::ExecutorState;
    use crate::types::{
        AccountRef, AccountScope, AgentId, EventId, FoldedEvent, Order, OrderState, Purpose,
        RiskClock, Seq, WorkspaceId,
    };

    /// A stream opened on paper, then an `OrderStateChanged` for an order the fold has never seen,
    /// carrying one extra field that names `md-r-e-1`. Without that field the fold answers
    /// `UnknownOrder` for the order itself; `replaces` is followed to the order it names first.
    fn fold_state_change(extra: &str) -> Result<(), ExecutorError> {
        let mut state = ExecutorState::new(AccountScope {
            account: AccountRef("acct-1".to_owned()),
            workspace: WorkspaceId("ws1".to_owned()),
        });
        let stream = state.account_stream();
        let event = |seq: u64, event_type: &str, payload| FoldedEvent {
            stream: stream.clone(),
            seq: Seq(seq),
            event_id: EventId(format!("e-{seq}")),
            event_type: event_type.to_owned(),
            causation_id: None,
            payload,
        };
        fold(
            &mut state,
            &event(
                1,
                "StreamOpened",
                object(vec![("environment", text("paper"))])?,
            ),
        )?;
        let mut pairs = vec![
            ("client_order_id", text("md-01JABCDEFGHJKMNPQRSTVWXYZ0")),
            ("state", text("accepted")),
            ("risk_clock", clock(RiskClock::from_secs(10))?),
        ];
        if !extra.is_empty() {
            pairs.push((extra, text("md-r-e-1")));
        }
        fold(&mut state, &event(2, "OrderStateChanged", object(pairs)?))
    }

    #[test]
    fn a_replacement_field_is_followed_to_the_order_it_names() {
        let named = |answer: Result<(), ExecutorError>| match answer {
            Err(ExecutorError::UnknownOrder { client_order_id }) => Some(client_order_id),
            _ => None,
        };
        assert_eq!(
            named(fold_state_change("replaces")).as_deref(),
            Some("md-r-e-1"),
            "`replaces` links the new order to the one it replaced, so an unseen original is \
             refused by its own id, never folded without it"
        );
        for extra in ["replaced_by", ""] {
            assert_eq!(
                named(fold_state_change(extra)).as_deref(),
                Some("md-01JABCDEFGHJKMNPQRSTVWXYZ0"),
                "without `replaces` the event reaches the lookup of its own order ({extra:?})"
            );
        }
    }

    #[test]
    fn a_replacement_passes_on_only_a_reservation_that_was_held() -> Result<(), ExecutorError> {
        let mut state = ExecutorState::new(AccountScope {
            account: AccountRef("acct-1".to_owned()),
            workspace: WorkspaceId("ws1".to_owned()),
        });
        let stream = state.account_stream();
        let event = |seq: u64, event_type: &str, payload| FoldedEvent {
            stream: stream.clone(),
            seq: Seq(seq),
            event_id: EventId(format!("e-{seq}")),
            event_type: event_type.to_owned(),
            causation_id: None,
            payload,
        };
        fold(
            &mut state,
            &event(
                1,
                "StreamOpened",
                object(vec![("environment", text("paper"))])?,
            ),
        )?;
        let original = ClientOrderId::parse("md-01JABCDEFGHJKMNPQRSTVWXYZ0")?;
        state.orders.insert(
            original.clone(),
            Order {
                client_order_id: original.clone(),
                intent_id: Some(IntentId(EventId("01JABCDEFGHJKMNPQRSTVWXYZ0".to_owned()))),
                agent: AgentId("agent-a".to_owned()),
                instrument: InstrumentId::new("AAPL")?,
                side: Side::Buy,
                qty: Qty::parse("10")?,
                filled_qty: Qty::ZERO,
                state: OrderState::Accepted,
                attempt: 1,
                purpose: Purpose::Open,
                absent_lookups: 0,
                first_absence_at: None,
                cancel_unconfirmed: false,
                replaced_by: None,
                created_on: None,
            },
        );
        fold(
            &mut state,
            &event(
                2,
                "OrderStateChanged",
                object(vec![
                    ("client_order_id", text(original.as_str())),
                    ("state", text("replaced")),
                    ("replaced_by", text("md-r-e-1")),
                    ("risk_clock", clock(RiskClock::from_secs(10))?),
                ])?,
            ),
        )?;
        assert!(
            state.reservations.is_empty(),
            "no reservation was held, so none, not a zero, is passed on: {:?}",
            state.reservations
        );
        Ok(())
    }
}

/// Buying power over the journal (trading-domain spec §6.2, §7.2, §10, DEC-104), driven by the
/// events the executor writes and read back, with `mandate-accounting`'s own `Account` as the
/// independent oracle for the arithmetic.
#[cfg(test)]
mod buying_power_tests {
    use mandate_accounting::{
        Account, AccountType, AssetClass, Config, CryptoFees, EquityFees, Execution,
        Input as AccountingInput, InstrumentId, Record, Reservations, Side, TafCapBasis,
    };
    use mandate_canon::Value;
    use mandate_num::{Bps, FeeCap, FeePerShare, FeeRate, Price, Qty, Usd};
    use mandate_time::{Date, NewYorkTime, TradingCalendar, new_york_instant};

    use super::fold;
    use crate::error::ExecutorError;
    use crate::payload::{clock, object, text};
    use crate::state::ExecutorState;
    use crate::types::{
        AccountRef, AccountScope, AccountState, EventId, FoldedEvent, RiskClock, Seq, WorkspaceId,
    };

    /// One paper account stream, folded event by event.
    struct Stream {
        state: ExecutorState,
        seq: u64,
    }

    impl Stream {
        fn opened() -> Result<Self, ExecutorError> {
            let mut stream = Self {
                state: ExecutorState::new(AccountScope {
                    account: AccountRef("acct-1".to_owned()),
                    workspace: WorkspaceId("ws1".to_owned()),
                }),
                seq: 0,
            };
            stream.seq = 1;
            let event = stream.event(
                "StreamOpened",
                object(vec![("environment", text("paper"))])?,
            );
            fold(&mut stream.state, &event)?;
            Ok(stream)
        }

        fn event(&self, event_type: &str, payload: Value) -> FoldedEvent {
            FoldedEvent {
                stream: self.state.account_stream(),
                seq: Seq(self.seq),
                event_id: EventId(format!("e-{}", self.seq)),
                event_type: event_type.to_owned(),
                causation_id: None,
                payload,
            }
        }

        fn fold(
            &mut self,
            event_type: &str,
            mut pairs: Vec<(&str, Value)>,
        ) -> Result<(), ExecutorError> {
            self.seq = self.seq.saturating_add(1);
            pairs.push(("risk_clock", clock(RiskClock::from_secs(10))?));
            let event = self.event(event_type, object(pairs)?);
            fold(&mut self.state, &event)
        }
    }

    fn usd(raw: &str) -> Result<Usd, ExecutorError> {
        Ok(Usd::parse(raw)?)
    }

    fn account(cash: &str, buying_power: &str) -> Vec<(&'static str, Value)> {
        vec![
            ("status", text("ACTIVE")),
            ("crypto_status", text("ACTIVE")),
            ("trading_blocked", Value::Bool(false)),
            ("account_blocked", Value::Bool(false)),
            ("trade_suspended_by_user", Value::Bool(false)),
            ("equity", text(cash)),
            ("cash", text(cash)),
            ("buying_power", text(buying_power)),
            ("non_marginable_buying_power", text(buying_power)),
            ("accrued_fees", text("0")),
        ]
    }

    fn fill(id: &str, side: &str, qty: &str, price: &str) -> Vec<(&'static str, Value)> {
        vec![
            ("fill_id", text(id)),
            ("instrument", text("AAPL")),
            ("side", text(side)),
            ("qty_gross", text(qty)),
            ("price", text(price)),
        ]
    }

    fn fee(day: &str, accrued: &str, simulated: bool) -> Vec<(&'static str, Value)> {
        vec![
            ("family", text("equities")),
            ("day", text(day)),
            ("accrued", text(accrued)),
            ("charged", text("0")),
            ("simulated", Value::Bool(simulated)),
        ]
    }

    #[test]
    fn buying_power_moves_by_quantity_times_price_and_restarts_at_each_report()
    -> Result<(), ExecutorError> {
        let mut stream = Stream::opened()?;
        assert_eq!(
            stream.state.buying_power(),
            None,
            "before the broker reports an account there is no buying power, never a guess"
        );
        stream.fold("AccountStateObserved", account("1000", "5000"))?;
        assert_eq!(stream.state.buying_power(), Some(usd("1000")?));
        stream.fold("FillApplied", fill("f-1", "buy", "2", "100"))?;
        assert_eq!(
            stream.state.buying_power(),
            Some(usd("800")?),
            "a buy of 2 at 100 lowers it by 200, never raises it (rule 3)"
        );
        stream.fold("FillApplied", fill("f-2", "sell", "1", "110"))?;
        assert_eq!(
            stream.state.buying_power(),
            Some(usd("910")?),
            "an Alpaca account is a margin account (§7.2), so a sell's unsettled proceeds count"
        );
        stream.fold("AccountStateObserved", account("950", "5000"))?;
        assert_eq!(
            stream.state.buying_power(),
            Some(usd("950")?),
            "a later report is the new base: the fills before it are in the broker's cash"
        );
        Ok(())
    }

    /// §11's cash band is 0.01 × the fills since the last broker cash snapshot: a snapshot,
    /// observed or recorded by a reconciliation, starts the count again, so an old fill never
    /// widens a later band.
    #[test]
    fn each_cash_snapshot_restarts_the_fill_notional() -> Result<(), ExecutorError> {
        for snapshot in ["AccountStateObserved", "AccountSnapshotRecorded"] {
            let mut stream = Stream::opened()?;
            stream.fold("AccountStateObserved", account("1000", "5000"))?;
            stream.fold("FillApplied", fill("f-1", "buy", "2", "100"))?;
            assert_eq!(stream.state.fill_notional, usd("200")?);
            stream.fold(snapshot, account("800", "5000"))?;
            assert_eq!(
                stream.state.fill_notional,
                Usd::ZERO,
                "{snapshot} restarts it"
            );
            stream.fold("FillApplied", fill("f-2", "sell", "1", "110"))?;
            assert_eq!(stream.state.fill_notional, usd("110")?);
        }
        Ok(())
    }

    #[test]
    fn a_fill_with_no_price_is_refused_never_priced_at_zero() -> Result<(), ExecutorError> {
        let mut stream = Stream::opened()?;
        stream.fold("AccountStateObserved", account("1000", "5000"))?;
        let mut unpriced = fill("f-1", "buy", "2", "100");
        unpriced.retain(|(key, _)| *key != "price");
        assert!(
            stream.fold("FillApplied", unpriced).is_err(),
            "a fill without a price has no cash effect the fold could know"
        );
        assert_eq!(stream.state.buying_power(), Some(usd("1000")?));
        Ok(())
    }

    #[test]
    fn an_incomplete_report_or_an_overflow_gives_no_buying_power() -> Result<(), ExecutorError> {
        for missing in ["non_marginable_buying_power", "accrued_fees"] {
            let mut stream = Stream::opened()?;
            let mut report = account("1000", "5000");
            report.retain(|(key, _)| *key != missing);
            stream.fold("AccountStateObserved", report)?;
            assert_eq!(
                stream.state.buying_power(),
                None,
                "a report without `{missing}` fails closed, never defaulting to the richer value"
            );
        }
        let mut stream = Stream::opened()?;
        stream.fold(
            "AccountStateObserved",
            account(
                "50000000000000000000000000000",
                "50000000000000000000000000000",
            ),
        )?;
        stream.state.cash_flow = usd("50000000000000000000000000000")?;
        assert_eq!(
            stream.state.buying_power(),
            None,
            "an overflow is no buying power, never zero or a wrapped figure"
        );
        Ok(())
    }

    #[test]
    fn simulated_fees_are_charged_per_family_and_day_at_the_ceiling_cent()
    -> Result<(), ExecutorError> {
        let mut stream = Stream::opened()?;
        stream.fold("AccountStateObserved", account("1000", "5000"))?;
        stream.fold("FeesCharged", fee("2026-09-22", "0.0471", true))?;
        assert_eq!(
            stream.state.buying_power(),
            Some(usd("999.95")?),
            "round(0.0471, 2, ceiling) is 0.05 (§7.2, DEC-104)"
        );
        stream.fold("FeesCharged", fee("2026-09-22", "0.0001", true))?;
        stream.fold("FeesCharged", fee("2026-09-23", "0.001", true))?;
        assert_eq!(
            stream.state.buying_power(),
            Some(usd("999.94")?),
            "each (family, day) bucket is rounded on its own: 0.05 for 0.0472, and 0.01 for 0.001"
        );
        stream.fold("FeesCharged", fee("2026-09-23", "5.001", false))?;
        assert_eq!(
            stream.state.buying_power(),
            Some(usd("994.93")?),
            "and the broker's own unposted fee, 5.001, is charged at its ceiling cent, 5.01 (§7.2)"
        );
        Ok(())
    }

    #[test]
    fn a_crypto_order_reads_no_more_than_the_non_marginable_figure() -> Result<(), ExecutorError> {
        let mut stream = Stream::opened()?;
        let mut report = account("1000", "5000");
        for pair in &mut report {
            if pair.0 == "non_marginable_buying_power" {
                pair.1 = text("600");
            }
        }
        stream.fold("AccountStateObserved", report)?;
        assert_eq!(stream.state.buying_power(), Some(usd("1000")?));
        assert_eq!(
            stream.state.crypto_buying_power(),
            Some(usd("600")?),
            "§7.2's crypto row: the equities figure, capped by the broker's non-marginable one"
        );
        stream.fold("AccountStateObserved", account("400", "5000"))?;
        assert_eq!(
            stream.state.crypto_buying_power(),
            Some(usd("400")?),
            "and never more than the equities figure"
        );
        Ok(())
    }

    #[test]
    fn a_blocking_flag_folds_the_observed_account_as_blocked() -> Result<(), ExecutorError> {
        for flag in [
            "trading_blocked",
            "account_blocked",
            "trade_suspended_by_user",
        ] {
            let mut stream = Stream::opened()?;
            let mut report = account("1000", "5000");
            for pair in &mut report {
                if pair.0 == flag {
                    pair.1 = Value::Bool(true);
                }
            }
            stream.fold("AccountStateObserved", report)?;
            assert_eq!(
                stream
                    .state
                    .observed_account()
                    .map(|observed| observed.state),
                Some(AccountState::Blocked),
                "§7.3 row 1: `{flag}` alone is a blocked account, whatever the status says"
            );
        }
        Ok(())
    }

    /// The fee configuration of the trading-domain reference cases (`test_default`).
    fn config() -> Result<Config, ExecutorError> {
        let date = |raw: &str| Date::parse(raw).map_err(ExecutorError::from);
        Ok(Config {
            equities: EquityFees {
                sec_rate: FeeRate::parse("0.00003")?,
                taf_per_share: FeePerShare::parse("0.0002")?,
                taf_cap: FeeCap::parse("9.79")?,
                taf_cap_basis: TafCapBasis::PerExecution,
                cat_per_share: FeePerShare::parse("0.00001")?,
            },
            crypto: CryptoFees {
                maker: Bps::parse("15")?,
                taker: Bps::parse("25")?,
            },
            calendar: TradingCalendar::new(
                date("2026-09-01")?,
                date("2026-12-31")?,
                [date("2026-11-26")?, date("2026-12-25")?],
                [date("2026-10-12")?, date("2026-11-11")?],
            )?,
        })
    }

    #[test]
    fn the_executors_buying_power_agrees_with_the_accounts_own() -> Result<(), ExecutorError> {
        let config = config()?;
        let mut ledger = Account::opening(AccountType::Margin, usd("1000")?, Vec::new());
        let mut stream = Stream::opened()?;
        stream.fold("AccountStateObserved", account("1000", "1000000"))?;
        for (id, side, qty, price, day) in [
            ("f-1", Side::Buy, "2", "100", "2026-09-22"),
            ("f-2", Side::Sell, "1", "110", "2026-09-22"),
            ("f-3", Side::Buy, "1", "50", "2026-09-23"),
        ] {
            let trade_date = Date::parse(day)?;
            let applied = ledger.apply(
                &AccountingInput::Fill(Execution {
                    fill_id: id.to_owned(),
                    client_order_id: None,
                    instrument: InstrumentId::new("AAPL")?,
                    asset_class: AssetClass::UsEquity,
                    side,
                    qty_gross: Qty::parse(qty)?,
                    price: Price::parse(price)?,
                    liquidity: None,
                    executed_at: new_york_instant(trade_date, NewYorkTime::new(12, 0)?)?,
                }),
                &config,
            )?;
            let Record::Fill { fees, .. } = &applied.record else {
                return Err(ExecutorError::Unimplemented { story: "E7-3" });
            };
            let accrued = fees
                .iter()
                .try_fold(Usd::ZERO, |total, fee| total.checked_add(fee.usd))?;
            ledger = applied.account;
            let named = match side {
                Side::Buy => "buy",
                Side::Sell => "sell",
            };
            stream.fold("FillApplied", fill(id, named, qty, price))?;
            stream.fold("FeesCharged", fee(day, &accrued.to_string(), true))?;
        }
        assert_eq!(
            stream.state.buying_power(),
            Some(ledger.buying_power(Reservations::NONE)?),
            "the same fills and fees give the executor the figure `mandate-accounting` gives, one \
             ceiling cent per (family, day) bucket included"
        );
        Ok(())
    }
}
