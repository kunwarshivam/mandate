//! The replay: one journaled event into the state, effect-free (journal spec §8).

use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::{InstrumentId, Side};
use mandate_canon::Value;
use mandate_num::{Price, Qty, SignedQty, Usd};
use mandate_time::Date;

use crate::codec::{mode_of, order_type_of, purpose_of, side_of, state_of, tif_of};
use crate::error::ExecutorError;
use crate::ids::{ClientOrderId, IntentId};
use crate::kill::{close_named, closes_with};
use crate::payload::{
    clock_of, flag, optional_int, optional_price, optional_qty, optional_text, optional_usd, qty,
    required_text, usd,
};
use crate::protection::climbs;
use crate::state::{
    Adoption, ExecutorState, ExitSequence, IntentOutcome, IntentRecord, Ladder, LoneLadder,
    ObservedAccount, OrderDetail, PendingRequest, Replacement, Switch,
};
use crate::types::{
    AccountState, ActivityCursor, AgentId, BracketLegs, EventId, FillId, FoldedEvent, IntentBody,
    Mode, OcoLegs, Order, OrderState, OrderType, Protection, ProtectionPrices, Purpose, RiskClock,
    Seq, SubmitOrder, TimeInForce, UnprotectedInterval,
};

/// The copied cross-stream facts of journal spec §2 this crate interprets. Each carries a
/// `causation_id` naming its origin, unless the executor originated it itself and says so with
/// `originated`. `OwnerAcknowledged` and `TradingDayStarted` join them with the slices that
/// interpret them, and until then answer those slices' stubs like every other event not reached.
const COPIED: [&str; 4] = [
    "AgentModeApplied",
    "ClockAdvanced",
    "TradingDayStarted",
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
    if let (true, Some(origin)) = (COPIED.contains(&kind), &event.causation_id) {
        state.copied.insert(event.event_id.clone(), origin.clone());
    }
    state.risk_clock = Some(at);
    let interpreted = match kind {
        "IntentReceived" => intent_received(state, payload, at),
        "GateDecided" => gate_decided(state, payload, at),
        "OrderRequestRecorded" => order_request_recorded(state, event),
        "OrderSubmitted" => {
            let companion = take_companion(state, event);
            rung_submitted(state, payload, at, companion.as_ref())?;
            order_submitted(state, event, companion)
        }
        "OrderStateChanged" => {
            adoption(state, event)?;
            rung_stepped(state, payload)?;
            rung_ended(state, payload)?;
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
        "KillSwitchActivated" => kill_switch_activated(state, event),
        "ClockAdvanced" | "MarkUpdated" | "ConductBreachDetected" => Ok(()),
        "TradingDayStarted" => {
            let date = Date::parse(required_text(payload, "date")?)?;
            state.trading_day = Some(state.trading_day.map_or(date, |current| current.max(date)));
            Ok(())
        }
        "FillApplied" | "LateFillApplied" => fill_applied(state, payload, event.seq),
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
        "ProtectionChanged" => protection_changed(state, payload, at),
        "AccountStateObserved" => account_observed(state, payload),
        "AccountSnapshotRecorded" => {
            account_observed(state, payload)?;
            cash_compared(state, payload);
            Ok(())
        }
        "RejectObserved" => {
            state.consecutive_403s = if optional_int(payload, "http_status") == Some(403) {
                state.consecutive_403s.saturating_add(1)
            } else {
                0
            };
            Ok(())
        }
        _ => later_slice(),
    };
    interpreted?;
    net_after(state, payload)
}

/// DEC-421 item 5's netting runs after every account event, for the instrument of the order the
/// event names or, when it names none the fold knows, the instrument it names. Every way an order
/// becomes terminal (`OrderStateChanged`, `OrderAbandoned`) and every sell fill that names no
/// order (`FillApplied`) passes through here, so a state that ends an order cannot skip the
/// netting and leave held shares bare. The netting is idempotent, so an event that ends nothing
/// and pools nothing changes nothing.
fn net_after(state: &mut ExecutorState, payload: &Value) -> Result<(), ExecutorError> {
    let ordered = optional_text(payload, "client_order_id")
        .and_then(|raw| ClientOrderId::parse(raw).ok())
        .and_then(|id| state.orders.get(&id))
        .map(|order| order.instrument.clone());
    match ordered.or_else(|| instrument(payload).ok()) {
        Some(instrument) => net_unattributed(state, &instrument),
        None => Ok(()),
    }
}

/// Corporate actions, reconciliation's records and snapshot, recorded broker exchanges, the owner
/// acknowledgment, the trading and risk days, and the kill switch (trading-domain spec §5.5 to §5.7,
/// §6, §10, §11): the later slices of this stack; the trading day is slice 5's trading-day part,
/// which owes what a new day starts (GTC re-placement, the copy, the harness's start) together.
/// An `ExternalActivityIngested` and a `ConductBreachDetected` fold as records only: the
/// restriction either causes is its own `AgentModeApplied`, and a conduct control never holds an
/// exit (`AGENTS.md` rule 13).
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
        | "OrderRequestRecorded"
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

/// `StreamOpened` fixes the stream's `environment` for good (ADR-0001 ES-23). The member is the
/// older payload shape's; §9's registered schema carries the environment in the envelope instead,
/// so a payload without it opens the stream and leaves the environment to the envelope's own
/// field, and a mismatch is still refused the moment any record names one.
fn stream_opened(state: &mut ExecutorState, payload: &Value) -> Result<(), ExecutorError> {
    let Some(found) = optional_text(payload, "environment") else {
        return Ok(());
    };
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
    let at =
        clock_of(&event.payload, "risk_clock").ok_or_else(|| ExecutorError::RiskClockMissing {
            event_type: event.event_type.clone(),
        })?;
    if let Some(last) = state.risk_clock
        && at < last
    {
        return Err(ExecutorError::RiskClockWentBackwards {
            last: last.secs(),
            found: at.secs(),
        });
    }
    Ok(at)
}

/// The instrument a payload names, either spelling the account stream has written: the closed
/// form's `instrument_id` (§9.5) beside the legacy `instrument` the golden journal carries
/// (DEC-446 item 7: the fold's legacy reads stay).
fn instrument(payload: &Value) -> Result<InstrumentId, ExecutorError> {
    let name = optional_text(payload, "instrument_id")
        .or_else(|| optional_text(payload, "instrument"))
        .ok_or_else(|| refused("instrument_id"))?;
    Ok(InstrumentId::new(name)?)
}

/// The agent a payload names, either spelling: the closed form's `agent_id` beside the legacy
/// `agent` (DEC-446 item 7).
fn agent_of(payload: &Value) -> Result<AgentId, ExecutorError> {
    let name = optional_text(payload, "agent_id")
        .or_else(|| optional_text(payload, "agent"))
        .ok_or_else(|| refused("agent_id"))?;
    Ok(AgentId(name.to_owned()))
}

/// The limit price a payload names, either spelling: the closed form's `limit_price` beside the
/// legacy `limit` (DEC-446 item 7).
fn limit_of(payload: &Value) -> Result<Option<Price>, ExecutorError> {
    optional_price(payload, "limit_price")?
        .or(optional_price(payload, "limit")?)
        .map(Ok)
        .transpose()
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
        limit: limit_of(payload)?.ok_or_else(|| refused("limit_price"))?,
        purpose: purpose_of(required_text(payload, "purpose")?)?,
        protection: prices_of(payload)?,
    };
    let agent = agent_of(payload)?;
    if let (IntentBody::Order { instrument, .. }, Some((switch, ordinal))) =
        (&body, close_named(&id))
        && let Some(switch) = state.switches.get_mut(&switch)
        && closes_with(switch, ordinal, &agent, instrument)
    {
        switch.pending.remove(instrument);
    }
    state.intents.insert(
        id.clone(),
        IntentRecord {
            intent_id: id.clone(),
            agent,
            received_at: at,
            outcome: IntentOutcome::Received,
            allowed_at: None,
        },
    );
    state.bodies.insert(id, body);
    Ok(())
}

/// One agent-scoped `KillSwitchActivated` (§5.5, DEC-485 item 3): the agent, the purpose its
/// sells carry, the owner's confirmed floor if any, and the closes it named, now or deferred,
/// every one pending until its flatten's `IntentReceived` folds. Any other scope, the account and
/// workspace ones included, answers the next slice's stub.
fn kill_switch_activated(
    state: &mut ExecutorState,
    event: &FoldedEvent,
) -> Result<(), ExecutorError> {
    let payload = &event.payload;
    if optional_text(payload, "scope") != Some("agent") {
        return Err(ExecutorError::Unimplemented { story: "E7-4" });
    }
    let named = |field: &str| -> Result<Vec<InstrumentId>, ExecutorError> {
        match payload.get(field) {
            Some(Value::Array(items)) => items
                .iter()
                .map(|item| {
                    Ok(InstrumentId::new(
                        item.as_str().ok_or_else(|| refused(field))?,
                    )?)
                })
                .collect(),
            _ => Err(refused(field)),
        }
    };
    let mut instruments = named("closes")?;
    instruments.extend(named("deferred")?);
    let switch = Switch {
        agent: AgentId(required_text(payload, "subject")?.to_owned()),
        purpose: purpose_of(required_text(payload, "purpose")?)?,
        floor: optional_price(payload, "floor")?,
        pending: instruments.iter().cloned().collect(),
        instruments,
    };
    state.switches.insert(event.event_id.clone(), switch);
    Ok(())
}

fn gate_decided(
    state: &mut ExecutorState,
    payload: &Value,
    at: RiskClock,
) -> Result<(), ExecutorError> {
    let id = intent_id(payload)?;
    match required_text(payload, "verdict")? {
        "allow" => {
            if let Some(record) = state.intents.get_mut(&id) {
                record.allowed_at = Some(at);
            }
            if let (Some(sized), Some(IntentBody::Order { qty, .. })) = (
                optional_qty(payload, "sized_qty")?,
                state.bodies.get_mut(&id),
            ) {
                *qty = sized;
            }
        }
        "deny" => {
            if let Some(record) = state.intents.get_mut(&id) {
                record.outcome = IntentOutcome::Denied;
            }
        }
        "hold" if flag(payload, "parked") => {
            if flag(payload, "held_long") {
                state.held_long.insert(id.clone());
            }
            let ladders = state
                .exiting
                .values_mut()
                .map(|sequence| (&sequence.intent, &mut sequence.ladder))
                .chain(
                    state
                        .ladders
                        .values_mut()
                        .map(|lone| (&lone.intent, &mut lone.ladder)),
                );
            for (intent, ladder) in ladders {
                if intent == &id {
                    ladder.parked = true;
                }
            }
            return Ok(());
        }
        "hold" | "defer" => {
            if flag(payload, "held_long") {
                state.held_long.insert(id.clone());
            }
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
        bracket: match (
            optional_text(payload, "order_class"),
            optional_price(payload, "take_profit")?,
            optional_price(payload, "stop")?,
        ) {
            (Some("bracket"), Some(take_profit), Some(stop)) => {
                Some(BracketLegs { take_profit, stop })
            }
            (Some("bracket"), _, _) => return Err(refused("order_class")),
            _ => None,
        },
        oco: match (
            optional_text(payload, "order_class"),
            optional_price(payload, "take_profit")?,
            optional_price(payload, "stop")?,
        ) {
            (Some("oco"), Some(take_profit), Some(stop)) => Some(OcoLegs {
                take_profit,
                stop,
                qty: qty(payload, "qty")?,
            }),
            (Some("oco"), _, _) => return Err(refused("order_class")),
            _ => None,
        },
        extended_hours: flag(payload, "extended_hours"),
        purpose: optional_text(payload, "purpose").map_or(Ok(Purpose::Open), purpose_of)?,
    })
}

/// §5.6: the sequence's exit, or a later rung of it, was submitted, and the ladder's clock
/// restarts from it. A submission's rung comes from its `OrderRequestRecorded` companion at
/// version 2 — `rung` non-null says a ladder rung, and a lone ladder's first rung writes rung 0
/// (DEC-446 item 2) — and from the legacy members beside it on an older record.
fn rung_submitted(
    state: &mut ExecutorState,
    payload: &Value,
    at: RiskClock,
    companion: Option<&PendingRequest>,
) -> Result<(), ExecutorError> {
    let intent = companion
        .and_then(|pending| pending.intent.clone())
        .or_else(|| {
            optional_text(payload, "intent_id").map(|raw| IntentId(EventId(raw.to_owned())))
        });
    let Some(intent) = intent else {
        return Ok(());
    };
    let instrument = instrument(payload)?;
    let (rung, floored, laddered) = match companion {
        Some(pending) => (
            pending.rung.unwrap_or(0),
            pending.at_floor && pending.rung.is_some(),
            pending.rung.is_some(),
        ),
        None => (
            optional_int(payload, "rung")
                .and_then(|rung| u32::try_from(rung).ok())
                .unwrap_or(0),
            flag(payload, "at_floor"),
            flag(payload, "laddered"),
        ),
    };
    let ladder = Ladder {
        rung,
        since: Some(at),
        floored,
        stepping: false,
        parked: false,
    };
    if let Some(sequence) = state
        .exiting
        .get_mut(&instrument)
        .filter(|sequence| sequence.intent == intent && !sequence.passive)
    {
        sequence.ladder = ladder;
    } else if laddered {
        let lone = LoneLadder {
            intent: intent.clone(),
            agent: match companion {
                Some(pending) => pending.agent.clone(),
                None => agent_of(payload)?,
            },
            ladder,
        };
        state
            .ladders
            .insert((instrument, lone.intent.clone()), lone);
    }
    Ok(())
}

/// §5.6 step 2: the current rung's cancel was asked for a step, so its confirmation submits the
/// next rung rather than ending the sequence.
fn rung_stepped(state: &mut ExecutorState, payload: &Value) -> Result<(), ExecutorError> {
    if !flag(payload, "ladder_step") {
        return Ok(());
    }
    let id = client_order_id(payload)?;
    let ladders = state
        .exiting
        .values_mut()
        .map(|sequence| (&sequence.intent, &mut sequence.ladder))
        .chain(
            state
                .ladders
                .values_mut()
                .map(|lone| (&lone.intent, &mut lone.ladder)),
        );
    for (intent, ladder) in ladders {
        if ClientOrderId::for_intent(intent)?.rung(ladder.rung)? == id {
            ladder.stepping = true;
        }
    }
    Ok(())
}

/// DEC-409: a sequence's stepped rung whose cancel is confirmed while its ladder no longer climbs
/// (its agent paused or stopped, or its interval past its bound) ends the ladder there. The
/// remainder is never sent as a next rung, so a later resumption cannot send it beside an exit
/// allowed meanwhile, and protection returns for what is left (§5.4). The mode and the bound are
/// both folded, so a restart ends the same ladders.
fn rung_ended(state: &mut ExecutorState, payload: &Value) -> Result<(), ExecutorError> {
    if optional_text(payload, "state") != Some("canceled") {
        return Ok(());
    }
    let id = client_order_id(payload)?;
    let mut ended = Vec::new();
    for (instrument, sequence) in &state.exiting {
        let rung = ClientOrderId::for_intent(&sequence.intent)?.rung(sequence.ladder.rung)?;
        if rung == id && sequence.ladder.stepping && !climbs(state, sequence) {
            ended.push(instrument.clone());
        }
    }
    for instrument in ended {
        if let Some(sequence) = state.exiting.get_mut(&instrument) {
            sequence.ladder.stepping = false;
        }
    }
    Ok(())
}

/// The companion a submission names, taken from the pending map (rule 45): its
/// `OrderRequestRecorded` folded immediately before it in the same batch. A submission naming
/// nothing pending keeps the legacy rebuild, which is what a version-1 record replays through.
fn take_companion(state: &mut ExecutorState, event: &FoldedEvent) -> Option<PendingRequest> {
    event
        .causation_id
        .as_ref()
        .and_then(|named| state.pending_requests.remove(named))
}

/// Folds one `OrderRequestRecorded` (§9.5): the executor-only members of the order that follows
/// it, held against the event id the submission names as its `causation_id`.
fn order_request_recorded(
    state: &mut ExecutorState,
    event: &FoldedEvent,
) -> Result<(), ExecutorError> {
    let payload = &event.payload;
    let legs = |name: &str| -> Result<Option<(Price, Price)>, ExecutorError> {
        match (
            optional_text(payload, "order_class"),
            optional_price(payload, "take_profit")?,
            optional_price(payload, "stop")?,
        ) {
            (Some(kind), Some(take_profit), Some(stop)) if kind == name => {
                Ok(Some((take_profit, stop)))
            }
            (Some(kind), _, _) if kind == name => Err(refused("order_class")),
            _ => Ok(None),
        }
    };
    let pending = PendingRequest {
        agent: agent_of(payload)?,
        intent: optional_text(payload, "intent_id").map(|raw| IntentId(EventId(raw.to_owned()))),
        purpose: purpose_of(required_text(payload, "purpose")?)?,
        extended_hours: flag(payload, "extended_hours"),
        stop_price: optional_price(payload, "stop_price")?,
        bracket: legs("bracket")?.map(|(take_profit, stop)| BracketLegs { take_profit, stop }),
        oco: legs("oco")?.map(|(take_profit, stop)| OcoLegs {
            take_profit,
            stop,
            qty: Qty::ZERO,
        }),
        rung: optional_int(payload, "rung").and_then(|rung| u32::try_from(rung).ok()),
        at_floor: flag(payload, "at_floor"),
    };
    state
        .pending_requests
        .insert(event.event_id.clone(), pending);
    Ok(())
}

fn order_submitted(
    state: &mut ExecutorState,
    event: &FoldedEvent,
    companion: Option<PendingRequest>,
) -> Result<(), ExecutorError> {
    let payload = &event.payload;
    let id = client_order_id(payload)?;
    let intent = companion
        .as_ref()
        .and_then(|pending| pending.intent.clone())
        .or_else(|| {
            optional_text(payload, "intent_id").map(|raw| IntentId(EventId(raw.to_owned())))
        });
    let request = match &companion {
        Some(pending) => SubmitOrder {
            client_order_id: id.clone(),
            instrument: instrument(payload)?,
            side: side_of(required_text(payload, "side")?)?,
            qty: qty(payload, "qty")?,
            order_type: order_type_of(required_text(payload, "type")?)?,
            tif: tif_of(required_text(payload, "tif")?)?,
            limit_price: optional_price(payload, "limit_price")?,
            stop_price: pending.stop_price,
            bracket: pending.bracket.clone(),
            oco: match pending.oco.clone() {
                Some(pending_legs) => Some(OcoLegs {
                    qty: qty(payload, "qty")?,
                    ..pending_legs
                }),
                None => None,
            },
            extended_hours: pending.extended_hours,
            purpose: pending.purpose,
        },
        None => request_of(payload, id.clone())?,
    };
    let attempt = optional_int(payload, "attempt")
        .and_then(|attempt| u32::try_from(attempt).ok())
        .unwrap_or(1);
    let reserved = match request.side {
        Side::Buy => request
            .limit_price
            .map_or(Ok(Usd::ZERO), |limit| request.qty.notional(limit))?,
        Side::Sell => Usd::ZERO,
    };
    let agent = Some(match &companion {
        Some(pending) => pending.agent.clone(),
        None => agent_of(payload)?,
    });
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
    if let Some(reported) = optional_qty(payload, "filled_qty")? {
        detail.reported_filled = detail.reported_filled.max(Some(reported));
    }
    if flag(payload, "cancel_overdue") {
        detail.cancel_overdue = true;
    }
    if flag(payload, "ignored") {
        if flag(payload, "cancel_requested") {
            order.cancel_unconfirmed = true;
        }
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
    if flag(payload, "cancel_requested") {
        order.cancel_unconfirmed = true;
    }
    if next.is_terminal() || flag(payload, "cancel_confirmed") {
        order.cancel_unconfirmed = false;
    }
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
    let ended = (next.is_terminal() && order.purpose == Purpose::Protective)
        .then(|| (order.instrument.clone(), order.qty));
    if let Some((instrument, qty)) = ended {
        leaves_protection(state, &instrument, &id, qty);
    }
    Ok(())
}

/// A protective order gone by any path — confirmed cancel, fill, expiry, reject — leaves its
/// instrument's protection with the quantity it covered, and the protection goes with its last
/// order (§5.4), so nothing waits on, or counts, an order the broker no longer holds.
fn leaves_protection(
    state: &mut ExecutorState,
    instrument: &InstrumentId,
    id: &ClientOrderId,
    qty: Qty,
) {
    let Some(protection) = state.protection.get_mut(instrument) else {
        return;
    };
    if protection.resting.contains(id) {
        protection.resting.retain(|resting| resting != id);
        protection.covered_qty = protection.covered_qty.checked_sub(qty).unwrap_or(Qty::ZERO);
    }
    if protection.resting.is_empty() {
        state.protection.remove(instrument);
    }
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
            agent: Some(record.agent.clone()),
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
///
/// A sell fill that names no order has taken shares off the position that an ended sell's
/// unapplied report may also name, so it joins the instrument's unattributed sells and is netted
/// against those reports ([`net_unattributed`], DEC-421 item 5).
fn fill_applied(state: &mut ExecutorState, payload: &Value, seq: Seq) -> Result<(), ExecutorError> {
    let fill = FillId(required_text(payload, "fill_id")?.to_owned());
    if state.fills.contains(&fill) {
        return Ok(());
    }
    let instrument = instrument(payload)?;
    let quantity = qty(payload, "qty_gross")?;
    let notional =
        quantity.notional(optional_price(payload, "price")?.ok_or_else(|| refused("price"))?)?;
    let side = side_of(required_text(payload, "side")?)?;
    let (signed, cash) = match side {
        Side::Buy => (SignedQty::from(quantity), notional.negated()),
        Side::Sell => (SignedQty::from(quantity).negated(), notional),
    };
    let position_of = instrument.clone();
    let position = state.positions.entry(instrument).or_insert(SignedQty::ZERO);
    *position = position.checked_add(signed)?;
    state.cash_flow = state.cash_flow.checked_add(cash)?;
    state.fill_notional = state.fill_notional.checked_add(notional)?;
    let named = optional_text(payload, "client_order_id");
    if let Some(order) = named
        .and_then(|raw| ClientOrderId::parse(raw).ok())
        .and_then(|id| state.orders.get_mut(&id))
    {
        order.filled_qty = order.filled_qty.checked_add(quantity)?;
    }
    if named.is_none() && side == Side::Sell {
        state
            .unattributed
            .entry(position_of.clone())
            .or_default()
            .push((seq, quantity));
    }
    state.fills.insert(fill);
    reattribute(state, &position_of);
    Ok(())
}

/// DEC-421 item 5: an ended sell's reported fill that no update has applied, and a sell fill
/// that named no order, may be the same shares, so they come off the held position once. Each
/// ended sell in `instrument`, in id order, nets its report's unapplied part against the
/// unattributed sells applied after it was submitted, oldest first: one applied before an order
/// was submitted cannot be that order's fill. What a later update applies to the order caps what
/// it has netted, and the excess is not given back: that unattributed sell was then a different
/// sale, which must never net another order's report and leave protection over the position.
fn net_unattributed(
    state: &mut ExecutorState,
    instrument: &InstrumentId,
) -> Result<(), ExecutorError> {
    let ended: Vec<(ClientOrderId, Qty)> = state
        .orders
        .iter()
        .filter(|(_, order)| {
            &order.instrument == instrument && order.side == Side::Sell && order.state.is_terminal()
        })
        .map(|(id, order)| (id.clone(), order.filled_qty))
        .collect();
    let pool = state.unattributed.entry(instrument.clone()).or_default();
    for (id, applied) in ended {
        let Some(detail) = state.details.get_mut(&id) else {
            continue;
        };
        let unapplied = detail
            .reported_filled
            .map_or(Ok(Qty::ZERO), |reported| reported.checked_sub(applied))
            .unwrap_or(Qty::ZERO);
        let mut netted = detail.netted.unwrap_or(Qty::ZERO).min(unapplied);
        for (applied_at, left) in pool.iter_mut() {
            if detail
                .submitted_seq
                .is_some_and(|submitted| *applied_at <= submitted)
            {
                continue;
            }
            let take = (*left).min(unapplied.checked_sub(netted).unwrap_or(Qty::ZERO));
            *left = left.checked_sub(take)?;
            netted = netted.checked_add(take)?;
        }
        detail.netted = Some(netted);
    }
    pool.retain(|(_, left)| *left != Qty::ZERO);
    if pool.is_empty() {
        state.unattributed.remove(instrument);
    }
    Ok(())
}

/// One restriction on one agent (or on every agent, as `*`). A mode is the strictest of an
/// agent's active restrictions, and `normal` lifts the restriction it names (mandate spec §5.9).
/// Lifting the `kill_switch` restriction also lifts whatever closes the agent's kill switches had
/// not yet raised (DEC-485 item 7): the agent was resumed, so nothing sells its lots behind its
/// back later.
fn agent_mode_applied(state: &mut ExecutorState, payload: &Value) -> Result<(), ExecutorError> {
    let agent = AgentId(required_text(payload, "agent")?.to_owned());
    let mode = mode_of(required_text(payload, "to")?)?;
    let restriction = optional_text(payload, "restriction")
        .unwrap_or_default()
        .to_owned();
    if mode == Mode::Normal {
        for switch in state.switches.values_mut() {
            if switch.agent == agent && restriction == "kill_switch" {
                switch.pending.clear();
            }
        }
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

/// A reconciliation's recorded snapshot says whether the broker's cash fell inside the band. The
/// consecutive runs outside it are counted here, from the journal, so the count survives the
/// re-anchor the snapshot makes and a restart (§11, DEC-146); a snapshot that compared no cash
/// leaves it alone.
fn cash_compared(state: &mut ExecutorState, payload: &Value) {
    match payload.get("cash_in_band") {
        Some(Value::Bool(true)) => state.cash_out_of_band = 0,
        Some(Value::Bool(false)) => {
            state.cash_out_of_band = state.cash_out_of_band.saturating_add(1);
        }
        _ => {}
    }
}

/// §5.4's protective orders and unprotected intervals, as `ProtectionChanged` journals them
/// (journal spec §9: instrument, action, orders, the interval's start or end). `placed` adds the
/// resting protective orders it names and the quantity they cover, as a tranche beside any already
/// resting; `cancelled` takes the named orders off, and the quantity they covered with them.
/// `unprotected_start` opens an interval for the instrument and `unprotected_end` closes that
/// instrument's open one, so an interval can only end by being journaled as ended. Any other
/// action is refused rather than skipped.
fn protection_changed(
    state: &mut ExecutorState,
    payload: &Value,
    at: RiskClock,
) -> Result<(), ExecutorError> {
    let instrument = instrument(payload)?;
    let action = required_text(payload, "action")?;
    match action {
        "intended" => {
            let intent = intent_id(payload)?;
            let prices = prices_of(payload)?;
            let body =
                state
                    .bodies
                    .get_mut(&intent)
                    .ok_or_else(|| ExecutorError::UnknownOrder {
                        client_order_id: intent.0.0.clone(),
                    })?;
            if let IntentBody::Order { protection, .. } = body {
                *protection = prices;
            }
        }
        "placed" => {
            if let Some(entry) = optional_text(payload, "bracket") {
                let entry = ClientOrderId::parse(entry)?;
                state.details.entry(entry).or_default().bracket_placed = true;
            }
            let orders = protective_orders(payload)?;
            let covered = qty(payload, "qty")?;
            let prices = prices_of(payload)?;
            let created = created_on(payload)?;
            legs(state, &instrument, &orders, covered, created);
            for id in &orders {
                if let Some(order) = state.orders.get_mut(id) {
                    order.created_on = order.created_on.or(created);
                }
            }
            if state
                .exiting
                .get(&instrument)
                .is_some_and(|sequence| sequence.passive)
            {
                state.exiting.remove(&instrument);
            }
            let protection = state
                .protection
                .entry(instrument.clone())
                .or_insert_with(|| Protection {
                    instrument,
                    resting: Vec::new(),
                    covered_qty: Qty::ZERO,
                    prices: None,
                });
            protection.resting.extend(orders);
            protection.prices = prices.or(protection.prices);
            protection.covered_qty = protection.covered_qty.checked_add(covered)?;
        }
        "cancelled" => {
            let orders = protective_orders(payload)?;
            let uncovered = optional_qty(payload, "qty")?;
            if let Some(protection) = state.protection.get_mut(&instrument) {
                protection.resting.retain(|id| !orders.contains(id));
                if let Some(uncovered) = uncovered {
                    protection.covered_qty = protection
                        .covered_qty
                        .checked_sub(uncovered)
                        .unwrap_or(Qty::ZERO);
                }
                if protection.resting.is_empty() {
                    state.protection.remove(&instrument);
                }
            }
        }
        "unprotected_start" | "passive_start" => {
            let passive = action == "passive_start";
            let bracket = optional_text(payload, "bracket")
                .map(ClientOrderId::parse)
                .transpose()?;
            if let Some(entry) = &bracket {
                let detail = state.details.entry(entry.clone()).or_default();
                detail.bracket_since = detail.bracket_since.or(Some(at));
            }
            if flag(payload, "replacing")
                && let (Some(entry), Some(agent)) = (
                    optional_text(payload, "entry"),
                    optional_text(payload, "agent_id").or(optional_text(payload, "agent")),
                )
            {
                state.replacing.insert(
                    instrument.clone(),
                    Replacement {
                        entry: ClientOrderId::parse(entry)?,
                        agent: AgentId(agent.to_owned()),
                        prices: prices_of(payload)?,
                    },
                );
            }
            if let (Some(intent), Some(entry), Some(agent)) = (
                optional_text(payload, "intent_id"),
                optional_text(payload, "entry"),
                optional_text(payload, "agent_id").or(optional_text(payload, "agent")),
            ) {
                let prices = prices_of(payload)?;
                let intent = IntentId(EventId(intent.to_owned()));
                let ladder = state
                    .ladders
                    .remove(&(instrument.clone(), intent.clone()))
                    .map_or_else(Ladder::default, |lone| lone.ladder);
                state.exiting.insert(
                    instrument.clone(),
                    ExitSequence {
                        intent,
                        entry: ClientOrderId::parse(entry)?,
                        agent: AgentId(agent.to_owned()),
                        prices,
                        passive,
                        ladder,
                    },
                );
            }
            if !passive {
                let awaited: Option<BTreeSet<ClientOrderId>> =
                    state.awaiting.remove(&instrument).map(|ids| {
                        ids.into_iter()
                            .filter_map(|id| id.protected_entry())
                            .collect()
                    });
                if let Some(waiting) = state.unprotected.iter_mut().find(|interval| {
                    interval.instrument == instrument
                        && interval.ended_at.is_none()
                        && (interval.uncovered
                            || awaited.as_ref().is_some_and(|entries| {
                                interval
                                    .bracket
                                    .as_ref()
                                    .is_none_or(|entry| entries.contains(entry))
                            }))
                }) {
                    waiting.ended_at = Some(at);
                }
                state.unprotected.push(UnprotectedInterval {
                    instrument,
                    started_at: at,
                    ended_at: None,
                    alerted: false,
                    uncovered: false,
                    bracket,
                });
            }
        }
        "watchdog" => {
            state.watchdogged.insert(instrument.clone(), at);
        }
        "exit_unpriced" | "ladder_floor" | "expiry_unreplaceable" => {}
        "rung_short" if optional_qty(payload, "sent")? == Some(Qty::ZERO) => {
            let intent = required_text(payload, "intent_id")?;
            if let Some(sequence) = state
                .exiting
                .get_mut(&instrument)
                .filter(|sequence| sequence.intent.0.0 == intent)
            {
                sequence.ladder.stepping = false;
            }
            state
                .ladders
                .remove(&(instrument.clone(), IntentId(EventId(intent.to_owned()))));
        }
        "rung_short" => {}
        "unprotected_end" if flag(payload, "acknowledged") => {
            let entries: BTreeSet<ClientOrderId> = state
                .awaiting
                .remove(&instrument)
                .into_iter()
                .flatten()
                .filter_map(|id| id.protected_entry())
                .collect();
            if let Some(open) = state.unprotected.iter_mut().find(|interval| {
                interval.instrument == instrument
                    && interval.ended_at.is_none()
                    && interval
                        .bracket
                        .as_ref()
                        .is_none_or(|entry| entries.contains(entry))
            }) {
                open.ended_at = Some(at);
            }
        }
        "interval_limit" | "unprotected_end" => {
            let awaiting = protective_orders_named(payload, "awaiting")?;
            let uncovered = flag(payload, "uncovered");
            let ends = action == "unprotected_end" && awaiting.is_empty() && !uncovered;
            if !awaiting.is_empty() {
                state
                    .awaiting
                    .insert(instrument.clone(), awaiting.into_iter().collect());
            }
            let finished = action == "unprotected_end";
            if let Some(entry) = optional_text(payload, "bracket").filter(|_| finished) {
                let entry = ClientOrderId::parse(entry)?;
                state.details.entry(entry).or_default().bracket_placed = true;
            }
            if finished {
                state.replacing.remove(&instrument);
            }
            let bounded = state.unprotected.iter().any(|interval| {
                interval.instrument == instrument && interval.ended_at.is_none() && interval.alerted
            });
            if finished
                && let Some(sequence) = state.exiting.remove(&instrument)
                && sequence.ladder.parked
                && !bounded
            {
                let lone = LoneLadder {
                    intent: sequence.intent,
                    agent: sequence.agent,
                    ladder: sequence.ladder,
                };
                state
                    .ladders
                    .insert((instrument.clone(), lone.intent.clone()), lone);
            }
            let named = optional_text(payload, "bracket")
                .map(ClientOrderId::parse)
                .transpose()?;
            if let Some(open) = state.unprotected.iter_mut().find(|interval| {
                interval.instrument == instrument
                    && interval.ended_at.is_none()
                    && (named.is_none() || interval.bracket == named)
            }) {
                if ends {
                    open.ended_at = Some(at);
                } else if uncovered {
                    open.uncovered = true;
                } else if !finished {
                    open.alerted = true;
                }
            }
        }
        _ => return Err(refused("action")),
    }
    Ok(())
}

/// The prices a `ProtectionChanged` names: its stop and, unless crypto's, its take-profit.
fn prices_of(payload: &Value) -> Result<Option<ProtectionPrices>, ExecutorError> {
    optional_price(payload, "stop")?
        .map(|stop| {
            let take_profit = optional_price(payload, "take_profit")?;
            Ok(ProtectionPrices { stop, take_profit })
        })
        .transpose()
}

/// DEC-160's leg-agent rule (trading-domain spec §2.3, §5.4): a protective order the broker
/// created, which no `OrderSubmitted` recorded, joins the order set as a live sell of the covered
/// quantity with a zero reservation, owned by the entry's agent when its id names the entry, else by
/// the position's single holder. With neither it joins with no agent, by its own id: its
/// instrument holds openings until it is attributed or done, and nothing guesses an owner. Its
/// lifecycle — fills, expiry, the confirmed cancel — folds through the order set like any leg's.
fn legs(
    state: &mut ExecutorState,
    instrument: &InstrumentId,
    orders: &[ClientOrderId],
    covered: Qty,
    created_on: Option<Date>,
) {
    for id in orders {
        if state.orders.contains_key(id) {
            continue;
        }
        let agent = leg_agent(state, instrument, id);
        state.orders.insert(
            id.clone(),
            Order {
                client_order_id: id.clone(),
                intent_id: None,
                agent,
                instrument: instrument.clone(),
                side: Side::Sell,
                qty: covered,
                filled_qty: Qty::ZERO,
                state: OrderState::Accepted,
                attempt: 1,
                purpose: Purpose::Protective,
                absent_lookups: 0,
                first_absence_at: None,
                cancel_unconfirmed: false,
                replaced_by: None,
                created_on,
            },
        );
        state.reservations.insert(id.clone(), Usd::ZERO);
    }
}

/// Retries DEC-160's rule for the ownerless legs in one instrument, after the fold changed who
/// holds it: a leg that can now be named takes that agent and stops holding openings.
fn reattribute(state: &mut ExecutorState, instrument: &InstrumentId) {
    let waiting: Vec<ClientOrderId> = state
        .orders
        .values()
        .filter(|order| order.agent.is_none() && &order.instrument == instrument)
        .map(|order| order.client_order_id.clone())
        .collect();
    for id in waiting {
        let agent = leg_agent(state, instrument, &id);
        if let Some(order) = state.orders.get_mut(&id) {
            order.agent = agent;
        }
    }
}

/// The agent a broker-created leg belongs to, or `None` when DEC-160's rule cannot name one: the
/// agent of the entry the leg's id names (everything before its last `-p`, §2.3), else the one
/// agent whose attributed lots — its own orders' fills, buys net of sells — make up the whole open
/// quantity (§8.1). An agent that went flat holds nothing, and quantity no order of ours accounts
/// for leaves no holder at all.
fn leg_agent(
    state: &ExecutorState,
    instrument: &InstrumentId,
    leg: &ClientOrderId,
) -> Option<AgentId> {
    let entry = leg
        .protected_entry()
        .and_then(|entry| state.orders.get(&entry))
        .and_then(|entry| entry.agent.clone());
    if entry.is_some() {
        return entry;
    }
    single_holder(state, instrument)
}

/// The position's single holder (§5.4's leg-agent rule, DEC-160 (3)(b)): the one agent whose
/// attributed lots make up the whole open quantity, else none.
pub(crate) fn single_holder(state: &ExecutorState, instrument: &InstrumentId) -> Option<AgentId> {
    let mut lots: BTreeMap<&AgentId, SignedQty> = BTreeMap::new();
    for order in state
        .orders
        .values()
        .filter(|order| &order.instrument == instrument)
    {
        let filled = SignedQty::from(order.filled_qty);
        let signed = match order.side {
            Side::Buy => filled,
            Side::Sell => filled.negated(),
        };
        let Some(agent) = &order.agent else {
            continue;
        };
        let held = lots.entry(agent).or_insert(SignedQty::ZERO);
        *held = held.checked_add(signed).ok()?;
    }
    let open = state.positions.get(instrument).copied()?;
    let holders: Vec<(&AgentId, SignedQty)> = lots
        .into_iter()
        .filter(|(_, held)| *held != SignedQty::ZERO)
        .collect();
    match holders.as_slice() {
        [(agent, held)] if *held == open => Some((*agent).clone()),
        _ => None,
    }
}

fn created_on(payload: &Value) -> Result<Option<Date>, ExecutorError> {
    optional_text(payload, "created_on")
        .map(|raw| Date::parse(raw).map_err(ExecutorError::from))
        .transpose()
}

/// The protective orders a `ProtectionChanged` names, either form: the closed schema's list
/// beside the space- or comma-joined text the legacy records carry (DEC-446 item 7).
fn protective_orders(payload: &Value) -> Result<Vec<ClientOrderId>, ExecutorError> {
    match payload.get("orders") {
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| ClientOrderId::parse(item.as_str().ok_or_else(|| refused("orders"))?))
            .collect(),
        _ => ids(required_text(payload, "orders")?),
    }
}

/// The orders a record names under `key`, either form, none where it names none.
fn protective_orders_named(
    payload: &Value,
    key: &str,
) -> Result<Vec<ClientOrderId>, ExecutorError> {
    match payload.get(key) {
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| ClientOrderId::parse(item.as_str().ok_or_else(|| refused(key))?))
            .collect(),
        _ => optional_text(payload, key).map_or(Ok(Vec::new()), ids),
    }
}

fn ids(raw: &str) -> Result<Vec<ClientOrderId>, ExecutorError> {
    raw.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|raw| !raw.is_empty())
        .map(ClientOrderId::parse)
        .collect()
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
        posted(
            state,
            usd(payload, "accrued")?,
            optional_usd(payload, "charged")?,
        )?;
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

/// The broker's own fee, §8.3's `FeesCharged` row: the accrual grows by `accrued`, and a posted
/// `charged` leaves settled cash (the model's cash flow) and clears that much of the accrual. The
/// accrual never goes below zero: a charge above what was accrued clears it and still takes the
/// whole charge from cash, so neither the cash band nor buying power ever reads a negative accrual
/// as spare cash (§11 "exact after posting", `AGENTS.md` rule 3; #205 review, round 2).
fn posted(
    state: &mut ExecutorState,
    accrued: Usd,
    charged: Option<Usd>,
) -> Result<(), ExecutorError> {
    let charged = charged.unwrap_or(Usd::ZERO);
    state.cash_flow = state.cash_flow.checked_sub(charged)?;
    state.unposted_fees = state
        .unposted_fees
        .checked_add(accrued)?
        .checked_sub(charged)?
        .max(Usd::ZERO);
    Ok(())
}

/// Only an owner acknowledgment carrying step-up evidence clears a reconciliation mismatch and the
/// pause it caused (trading-domain spec §11, interpretation 14).
///
/// The owner is who authorizes it: the event reaches the account stream only as a copy from the
/// control stream, which only the owner-facing surface writes, and it names its `user` (required,
/// opaque) and carries step-up evidence. Inside this crate nothing else tells an owner from an
/// agent (journal spec §9, #205 review, round 1, finding 6).
fn owner_acknowledged(state: &mut ExecutorState, payload: &Value) -> Result<(), ExecutorError> {
    let subject = required_text(payload, "subject")?;
    required_text(payload, "user")?;
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
mod companion_tests {
    use mandate_accounting::InstrumentId;
    use mandate_canon::Value;

    use super::{fold, order_request_recorded};
    const STREAM: &str = "acct:ws1:acct-1";
    use crate::error::ExecutorError;
    use crate::payload::{object, text};
    use crate::state::{ExecutorState, PendingRequest};
    use crate::types::{
        AccountRef, AccountScope, EventId, FoldedEvent, RiskClock, Seq, WorkspaceId,
    };

    fn folded(payload: Vec<(&'static str, Value)>) -> Result<FoldedEvent, ExecutorError> {
        Ok(FoldedEvent {
            stream: STREAM.to_owned(),
            seq: Seq(2),
            event_id: EventId("e-2".to_owned()),
            event_type: "OrderRequestRecorded".to_owned(),
            causation_id: None,
            payload: object(payload)?,
        })
    }

    fn companion(payload: Vec<(&'static str, Value)>) -> Result<PendingRequest, ExecutorError> {
        let mut state = ExecutorState::new(AccountScope {
            account: AccountRef("acct-1".to_owned()),
            workspace: WorkspaceId("ws1".to_owned()),
        });
        state.risk_clock = Some(RiskClock::from_secs(1));
        order_request_recorded(&mut state, &folded(payload)?)?;
        let held = state
            .pending_requests
            .get(&EventId("e-2".to_owned()))
            .cloned();
        match held {
            Some(pending) => Ok(pending),
            None => Err(refused("the companion is held against its own event id")),
        }
    }

    fn base() -> Vec<(&'static str, Value)> {
        vec![
            ("agent_id", text("agent-a")),
            ("intent_id", text("01JABCDEFGHJKMNPQRSTVWXYZ1")),
            ("purpose", text("open")),
            ("extended_hours", Value::Bool(false)),
            ("stop_price", Value::Null),
            ("order_class", text("bracket")),
            ("take_profit", text("159")),
            ("stop", text("139")),
            ("rung", Value::Null),
            ("at_floor", Value::Null),
            ("risk_clock", text("2026-09-21T14:00:00.000000000Z")),
        ]
    }

    /// §9.5: a well-formed companion carries its legs, and one whose class names a price it does
    /// not carry is refused at `order_class`, never folded legless (DEC-85); a class the legs do
    /// not name takes no legs from them.
    #[test]
    fn a_companion_without_its_class_legs_is_refused_never_legless() -> Result<(), ExecutorError> {
        let whole = companion(base())?;
        let legs = whole.bracket.as_ref().ok_or_else(|| refused("bracket"))?;
        assert_eq!(legs.stop.to_string(), "139");
        assert_eq!(legs.take_profit.to_string(), "159");
        assert!(whole.oco.is_none(), "the bracket is not an oco");
        for missing in ["take_profit", "stop"] {
            let mut payload = base();
            for (name, value) in payload.iter_mut() {
                if *name == missing {
                    *value = Value::Null;
                }
            }
            let answer = companion(payload);
            assert_eq!(
                answer.as_ref().err().map(ExecutorError::code),
                Some("non_canonical_payload"),
                "a bracket without its {missing} is refused: {answer:?}"
            );
        }
        let mut other_class = base();
        for (name, value) in other_class.iter_mut() {
            if *name == "order_class" {
                *value = text("oco");
            }
        }
        let other = companion(other_class)?;
        assert!(
            other.bracket.is_none() && other.oco.is_some(),
            "the oco's legs build for the oco"
        );
        Ok(())
    }

    fn refused(field: &str) -> ExecutorError {
        ExecutorError::NonCanonicalPayload {
            field: field.to_owned(),
        }
    }

    /// §9.5: an interval's bound names the orders its end awaits, either form the fold reads —
    /// the closed schema's list beside the legacy joined text — and the names land in the state's
    /// awaiting set, so the interval cannot end unwatched (§5.4, DEC-348 item 2).
    #[test]
    fn an_interval_awaits_the_orders_its_record_names_in_either_form() -> Result<(), ExecutorError>
    {
        let fold_awaiting = |awaiting: Value| -> Result<Vec<String>, ExecutorError> {
            let mut state = ExecutorState::new(AccountScope {
                account: AccountRef("acct-1".to_owned()),
                workspace: WorkspaceId("ws1".to_owned()),
            });
            fold(
                &mut state,
                &FoldedEvent {
                    stream: STREAM.to_owned(),
                    seq: Seq(1),
                    event_id: EventId("e-1".to_owned()),
                    event_type: "StreamOpened".to_owned(),
                    causation_id: None,
                    payload: object(vec![("environment", text("paper"))])?,
                },
            )?;
            let event = FoldedEvent {
                stream: STREAM.to_owned(),
                seq: Seq(2),
                event_id: EventId("e-2".to_owned()),
                event_type: "ProtectionChanged".to_owned(),
                causation_id: None,
                payload: object(vec![
                    ("instrument_id", text("AAPL")),
                    ("action", text("interval_limit")),
                    ("orders", Value::Array(Vec::new())),
                    ("awaiting", awaiting),
                    ("qty", Value::Null),
                    ("stop", Value::Null),
                    ("take_profit", Value::Null),
                    ("intent_id", Value::Null),
                    ("bracket", Value::Null),
                    ("entry", Value::Null),
                    ("agent_id", Value::Null),
                    ("replacing", Value::Null),
                    ("created_on", Value::Null),
                    ("sent", Value::Null),
                    ("uncovered", Value::Bool(true)),
                    ("acknowledged", Value::Null),
                    ("risk_clock", text("2026-09-21T14:00:00.000000000Z")),
                ])?,
            };
            fold(&mut state, &event)?;
            Ok(state
                .awaiting
                .get(&InstrumentId::new("AAPL")?)
                .into_iter()
                .flatten()
                .map(|id| id.as_str().to_owned())
                .collect())
        };
        let listed = fold_awaiting(Value::Array(vec![text("md-1-p1"), text("md-1-p2")]))?;
        assert_eq!(listed, vec!["md-1-p1".to_owned(), "md-1-p2".to_owned()]);
        let joined = fold_awaiting(text("md-1-p1 md-1-p2"))?;
        assert_eq!(joined, vec!["md-1-p1".to_owned(), "md-1-p2".to_owned()]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use mandate_accounting::{InstrumentId, Side};
    use mandate_num::Qty;

    use super::{fold, net_unattributed};
    use crate::error::ExecutorError;
    use crate::ids::{ClientOrderId, IntentId};
    use crate::payload::{clock, object, text};
    use crate::state::{ExecutorState, OrderDetail};
    use crate::types::{
        AccountRef, AccountScope, AgentId, EventId, FoldedEvent, Order, OrderState, Purpose,
        RiskClock, Seq, WorkspaceId,
    };

    /// DEC-421 item 5: an unattributed sell of 5 AAPL nets only against an ended AAPL sell's
    /// unapplied report, here 3 of `md-3-ended`'s. A live AAPL sell, an ended AAPL buy and an
    /// ended MSFT sell, each with an unapplied report and sorted ahead of it, net nothing, and the
    /// 2 left wait in the pool for the next ended sell. One submitted after the sell applied
    /// cannot net it.
    #[test]
    fn an_unattributed_sell_nets_only_ended_sells_in_its_instrument() -> Result<(), ExecutorError> {
        let mut state = ExecutorState::new(AccountScope {
            account: AccountRef("acct-1".to_owned()),
            workspace: WorkspaceId("ws1".to_owned()),
        });
        let (aapl, msft) = (InstrumentId::new("AAPL")?, InstrumentId::new("MSFT")?);
        let rows = [
            (
                "md-0-msft",
                msft.clone(),
                Side::Sell,
                OrderState::Canceled,
                1,
            ),
            (
                "md-1-live",
                aapl.clone(),
                Side::Sell,
                OrderState::Accepted,
                1,
            ),
            ("md-2-buy", aapl.clone(), Side::Buy, OrderState::Canceled, 1),
            (
                "md-3-ended",
                aapl.clone(),
                Side::Sell,
                OrderState::Canceled,
                1,
            ),
            (
                "md-4-later",
                aapl.clone(),
                Side::Sell,
                OrderState::Canceled,
                9,
            ),
        ];
        for (id, instrument, side, now, submitted) in rows {
            let client_order_id = ClientOrderId::parse(id)?;
            state.orders.insert(
                client_order_id.clone(),
                Order {
                    client_order_id: client_order_id.clone(),
                    intent_id: None,
                    agent: Some(AgentId("agent-a".to_owned())),
                    instrument,
                    side,
                    qty: Qty::parse("4")?,
                    filled_qty: Qty::ZERO,
                    state: now,
                    attempt: 1,
                    purpose: Purpose::RiskExit,
                    absent_lookups: 0,
                    first_absence_at: None,
                    cancel_unconfirmed: false,
                    replaced_by: None,
                    created_on: None,
                },
            );
            state.details.insert(
                client_order_id,
                OrderDetail {
                    submitted_seq: Some(Seq(submitted)),
                    reported_filled: Some(Qty::parse("3")?),
                    ..OrderDetail::default()
                },
            );
        }
        state
            .unattributed
            .insert(aapl.clone(), vec![(Seq(5), Qty::parse("5")?)]);
        net_unattributed(&mut state, &aapl)?;
        let netted = |id: &str| -> Result<Option<Qty>, ExecutorError> {
            Ok(state
                .details
                .get(&ClientOrderId::parse(id)?)
                .and_then(|detail| detail.netted))
        };
        assert_eq!(
            (
                netted("md-0-msft")?,
                netted("md-1-live")?,
                netted("md-2-buy")?,
                netted("md-3-ended")?,
                netted("md-4-later")?
            ),
            (None, None, None, Some(Qty::parse("3")?), Some(Qty::ZERO))
        );
        assert_eq!(
            state.unattributed.get(&aapl),
            Some(&vec![(Seq(5), Qty::parse("2")?)]),
            "the 2 left wait for the next ended sell"
        );
        Ok(())
    }

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
                agent: Some(AgentId("agent-a".to_owned())),
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
    use mandate_num::{Bps, FeeCap, FeePerShare, FeeRate, Price, Qty, SignedQty, Usd};
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

    /// §8.3's `FeesCharged` row: a posting takes the charge from settled cash as it clears the
    /// accrual, and a charge above the accrual clears it to zero and still takes the whole charge.
    /// Buying power is right between the posting and the next account read (#205 review, round 2).
    #[test]
    fn a_posted_fee_leaves_cash_and_clears_the_accrual_never_below_zero()
    -> Result<(), ExecutorError> {
        let broker = |accrued: &str, charged: &str| {
            vec![
                ("family", text("equities")),
                ("day", text("2026-09-22")),
                ("accrued", text(accrued)),
                ("charged", text(charged)),
                ("simulated", Value::Bool(false)),
            ]
        };
        let mut stream = Stream::opened()?;
        stream.fold("AccountStateObserved", account("1000", "5000"))?;
        stream.fold("FeesCharged", broker("5", "0"))?;
        assert_eq!(
            stream.state.buying_power(),
            Some(usd("995")?),
            "the accrual is reserved"
        );
        stream.fold("FeesCharged", broker("0", "5"))?;
        assert_eq!(
            (stream.state.unposted_fees, stream.state.cash_flow),
            (Usd::ZERO, usd("-5")?),
            "the posting clears the accrual and leaves cash"
        );
        assert_eq!(
            stream.state.buying_power(),
            Some(usd("995")?),
            "and buying power is right before the next account read"
        );
        stream.fold("FeesCharged", broker("2", "5"))?;
        assert_eq!(
            (stream.state.unposted_fees, stream.state.cash_flow),
            (Usd::ZERO, usd("-10")?),
            "a charge above the accrual floors it at zero and takes the whole charge"
        );
        assert_eq!(stream.state.buying_power(), Some(usd("990")?));
        Ok(())
    }

    /// §6.3 and §11's crypto row, as DEC-147 records: an asset-denominated crypto fee is not an
    /// accrued USD liability. It leaves the position net of the fee, is held per instrument in
    /// `asset_fees` until it posts, and never reaches the USD `unposted_fees` the cash band and the
    /// fee comparison read.
    #[test]
    fn a_crypto_asset_fee_is_held_in_the_asset_never_as_usd() -> Result<(), ExecutorError> {
        let mut stream = Stream::opened()?;
        stream.fold(
            "FillApplied",
            vec![
                ("fill_id", text("f-btc")),
                ("instrument", text("BTC/USD")),
                ("side", text("buy")),
                ("qty_gross", text("0.5")),
                ("price", text("60000")),
            ],
        )?;
        stream.fold(
            "FeesCharged",
            vec![
                ("family", text("crypto_asset")),
                ("accrued", text("0.00125")),
                ("charged", text("0")),
                ("instrument", text("BTC/USD")),
            ],
        )?;
        let btc = InstrumentId::new("BTC/USD")?;
        assert_eq!(
            stream.state.asset_fees.get(&btc).copied(),
            Some(Qty::parse("0.00125")?),
            "held in the asset"
        );
        assert_eq!(stream.state.unposted_fees, Usd::ZERO, "never as USD");
        assert_eq!(
            stream.state.positions.get(&btc).copied(),
            Some(SignedQty::parse("0.49875")?),
            "the position is net of the fee"
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

    /// The bucket is keyed by family as well as day: two families' fees on one day are rounded up
    /// once each, never merged and rounded once, which would read a cent richer (§7.2, DEC-104;
    /// the backlog's family-key row, #198 review, round 2, finding 2).
    #[test]
    fn two_families_on_one_day_are_two_buckets() -> Result<(), ExecutorError> {
        let mut stream = Stream::opened()?;
        stream.fold("AccountStateObserved", account("1000", "5000"))?;
        stream.fold("FeesCharged", fee("2026-09-22", "0.001", true))?;
        let mut crypto = fee("2026-09-22", "0.001", true);
        for pair in &mut crypto {
            if pair.0 == "family" {
                pair.1 = text("crypto");
            }
        }
        stream.fold("FeesCharged", crypto)?;
        assert_eq!(
            stream.state.buying_power(),
            Some(usd("999.98")?),
            "0.01 for each family's 0.001, not 0.01 for a merged 0.002"
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

#[cfg(test)]
mod protection_tests {
    use mandate_accounting::{InstrumentId, Side};
    use mandate_canon::Value;
    use mandate_num::{Price, Qty, Usd};
    use mandate_time::Date;

    use super::fold;
    use crate::error::ExecutorError;
    use crate::ids::ClientOrderId;
    use crate::payload::{clock, object, text};
    use crate::state::ExecutorState;
    use crate::types::{
        AccountRef, AccountScope, AgentId, EventId, FoldedEvent, OrderState, ProtectionPrices,
        Purpose, RiskClock, Seq, WorkspaceId,
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
            stream.fold("StreamOpened", vec![("environment", text("paper"))], None)?;
            Ok(stream)
        }

        fn fold(
            &mut self,
            event_type: &str,
            mut pairs: Vec<(&str, Value)>,
            causation: Option<&str>,
        ) -> Result<(), ExecutorError> {
            self.seq = self.seq.saturating_add(1);
            if event_type != "StreamOpened" {
                pairs.push(("risk_clock", clock(RiskClock::from_secs(10))?));
            }
            let event = FoldedEvent {
                stream: self.state.account_stream(),
                seq: Seq(self.seq),
                event_id: EventId(format!("e-{}", self.seq)),
                event_type: event_type.to_owned(),
                causation_id: causation.map(|origin| EventId(origin.to_owned())),
                payload: object(pairs)?,
            };
            fold(&mut self.state, &event)
        }

        /// `agent`'s own buy of `qty` AAPL, submitted and filled at 150.
        fn bought(&mut self, agent: &str, id: &str, qty: &str) -> Result<(), ExecutorError> {
            self.fold(
                "OrderSubmitted",
                vec![
                    ("client_order_id", text(id)),
                    ("agent", text(agent)),
                    ("instrument", text("AAPL")),
                    ("side", text("buy")),
                    ("qty", text(qty)),
                    ("limit", text("150")),
                ],
                None,
            )?;
            self.fold(
                "FillApplied",
                vec![
                    ("fill_id", text(format!("f-{id}"))),
                    ("client_order_id", text(id)),
                    ("instrument", text("AAPL")),
                    ("side", text("buy")),
                    ("qty_gross", text(qty)),
                    ("price", text("150")),
                ],
                None,
            )
        }

        /// `agent`'s own sell of `qty` AAPL, submitted and filled at 160.
        fn sold(&mut self, agent: &str, id: &str, qty: &str) -> Result<(), ExecutorError> {
            self.fold(
                "OrderSubmitted",
                vec![
                    ("client_order_id", text(id)),
                    ("agent", text(agent)),
                    ("instrument", text("AAPL")),
                    ("side", text("sell")),
                    ("qty", text(qty)),
                    ("limit", text("160")),
                ],
                None,
            )?;
            self.fold(
                "FillApplied",
                vec![
                    ("fill_id", text(format!("f-{id}"))),
                    ("client_order_id", text(id)),
                    ("instrument", text("AAPL")),
                    ("side", text("sell")),
                    ("qty_gross", text(qty)),
                    ("price", text("160")),
                ],
                None,
            )
        }

        fn protection(
            &mut self,
            action: &str,
            orders: &str,
            qty: &str,
        ) -> Result<(), ExecutorError> {
            self.fold(
                "ProtectionChanged",
                vec![
                    ("instrument", text("AAPL")),
                    ("action", text(action)),
                    ("orders", text(orders)),
                    ("qty", text(qty)),
                    ("created_on", text("2026-09-22")),
                ],
                None,
            )
        }
    }

    fn aapl() -> Result<InstrumentId, ExecutorError> {
        Ok(InstrumentId::new("AAPL")?)
    }

    fn id(raw: &str) -> Result<ClientOrderId, ExecutorError> {
        ClientOrderId::parse(raw)
    }

    fn qty(raw: &str) -> Result<Qty, ExecutorError> {
        Ok(Qty::parse(raw)?)
    }

    /// What a test expected and did not find. Never `Unimplemented`, whose story `ci pending`
    /// reads as a stub (DEC-137).
    fn missing(what: &str) -> ExecutorError {
        ExecutorError::NonCanonicalPayload {
            field: format!("expected {what}"),
        }
    }

    /// The legs in the order set that no agent owns.
    fn ownerless(stream: &Stream) -> Vec<ClientOrderId> {
        stream
            .state
            .orders
            .values()
            .filter(|order| order.agent.is_none())
            .map(|order| order.client_order_id.clone())
            .collect()
    }

    #[test]
    fn a_placed_oco_is_the_instruments_protection_and_its_legs_join_the_order_set()
    -> Result<(), ExecutorError> {
        let mut stream = Stream::opened()?;
        let aapl = aapl()?;
        assert_eq!(stream.state.protection(&aapl)?, None);
        assert_eq!(stream.state.protective_sell_qty(&aapl)?, Qty::ZERO);
        stream.bought("agent-a", "md-buy-1", "10")?;
        stream.protection("placed", "md-oco-1 md-oco-2", "10")?;
        let protection = stream
            .state
            .protection(&aapl)?
            .ok_or_else(|| missing("the placed protection"))?;
        assert_eq!(protection.resting, vec![id("md-oco-1")?, id("md-oco-2")?]);
        assert_eq!(stream.state.protective_sell_qty(&aapl)?, qty("10")?);
        for leg in ["md-oco-1", "md-oco-2"] {
            let order = stream
                .state
                .orders
                .get(&id(leg)?)
                .ok_or_else(|| missing("the leg in the order set"))?;
            assert_eq!(
                order.agent,
                Some(AgentId("agent-a".to_owned())),
                "{leg} has the single holder"
            );
            assert_eq!(order.side, Side::Sell);
            assert_eq!(order.purpose, Purpose::Protective);
            assert_eq!(order.state, OrderState::Accepted);
            assert_eq!(order.qty, qty("10")?);
            assert_eq!(order.created_on, Some(Date::parse("2026-09-22")?));
            assert_eq!(stream.state.reservations.get(&id(leg)?), Some(&Usd::ZERO));
        }
        assert!(ownerless(&stream).is_empty());
        Ok(())
    }

    #[test]
    fn a_placement_folds_its_prices_and_a_later_one_without_prices_keeps_them()
    -> Result<(), ExecutorError> {
        let mut stream = Stream::opened()?;
        let aapl = aapl()?;
        stream.bought("agent-a", "md-buy-1", "10")?;
        stream.fold(
            "ProtectionChanged",
            vec![
                ("instrument", text("AAPL")),
                ("action", text("placed")),
                ("orders", text("md-oco-1")),
                ("qty", text("6")),
                ("take_profit", text("170")),
                ("stop", text("140")),
            ],
            None,
        )?;
        let placed = Some(ProtectionPrices {
            stop: Price::parse("140")?,
            take_profit: Some(Price::parse("170")?),
        });
        let prices = |stream: &Stream| {
            stream
                .state
                .protection
                .get(&aapl)
                .and_then(|protection| protection.prices)
        };
        assert_eq!(prices(&stream), placed);
        stream.protection("placed", "md-oco-2", "4")?;
        assert_eq!(
            prices(&stream),
            placed,
            "a placement that names no stop keeps the prices it cannot replace"
        );
        stream.fold(
            "ProtectionChanged",
            vec![
                ("instrument", text("AAPL")),
                ("action", text("placed")),
                ("orders", text("md-sl-3")),
                ("qty", text("1")),
                ("stop", text("135")),
            ],
            None,
        )?;
        assert_eq!(
            prices(&stream),
            Some(ProtectionPrices {
                stop: Price::parse("135")?,
                take_profit: None,
            }),
            "the latest placement's own prices, a stop without a take-profit included"
        );
        Ok(())
    }

    #[test]
    fn a_leg_named_for_its_entry_takes_the_agent_of_that_entry_whoever_else_holds()
    -> Result<(), ExecutorError> {
        let mut stream = Stream::opened()?;
        stream.bought("agent-a", "md-buy-1", "4")?;
        stream.bought("agent-b", "md-buy-2", "6")?;
        stream.protection("placed", "md-buy-2-p1", "6")?;
        let order = stream
            .state
            .orders
            .get(&id("md-buy-2-p1")?)
            .ok_or_else(|| missing("the named leg"))?;
        assert_eq!(order.agent, Some(AgentId("agent-b".to_owned())));
        assert!(ownerless(&stream).is_empty());
        Ok(())
    }

    #[test]
    fn a_leg_with_no_holder_or_several_joins_the_order_set_with_no_agent()
    -> Result<(), ExecutorError> {
        for holders in [&[][..], &["agent-a", "agent-b"][..]] {
            let mut stream = Stream::opened()?;
            for (n, agent) in holders.iter().enumerate() {
                stream.bought(agent, &format!("md-buy-{n}"), "5")?;
            }
            stream.protection("placed", "md-oco-1", "10")?;
            let leg = stream
                .state
                .orders
                .get(&id("md-oco-1")?)
                .ok_or_else(|| missing("the ownerless leg in the order set"))?;
            assert_eq!(
                (
                    leg.agent.clone(),
                    leg.instrument.clone(),
                    leg.side,
                    leg.qty,
                    leg.purpose,
                    leg.state
                ),
                (
                    None,
                    aapl()?,
                    Side::Sell,
                    qty("10")?,
                    Purpose::Protective,
                    OrderState::Accepted
                ),
                "{holders:?}: in the order set by its own id, never guessed onto an agent"
            );
            assert_eq!(
                stream.state.reservations.get(&id("md-oco-1")?),
                Some(&Usd::ZERO),
                "{holders:?}"
            );
            assert_eq!(
                stream.state.protective_sell_qty(&aapl()?)?,
                qty("10")?,
                "{holders:?}: the protection itself still counts"
            );
        }
        Ok(())
    }

    /// #258 round 1 (b), (c), (d): an ownerless leg's lifecycle folds through the order set like
    /// any leg's — the broker's expiry, a fill and the confirmed cancel — so a journal carrying
    /// them replays; and its fill counts toward no agent's lots.
    #[test]
    fn an_ownerless_legs_expiry_fill_and_confirmed_cancel_fold() -> Result<(), ExecutorError> {
        for (event, pairs, state, filled) in [
            (
                "OrderStateChanged",
                vec![("state", text("expired"))],
                OrderState::Expired,
                "0",
            ),
            (
                "OrderStateChanged",
                vec![
                    ("state", text("canceled")),
                    ("cancel_confirmed", Value::Bool(true)),
                ],
                OrderState::Canceled,
                "0",
            ),
            (
                "FillApplied",
                vec![
                    ("fill_id", text("f-leg")),
                    ("instrument", text("AAPL")),
                    ("side", text("sell")),
                    ("qty_gross", text("5")),
                    ("price", text("140")),
                ],
                OrderState::Accepted,
                "5",
            ),
        ] {
            let mut stream = Stream::opened()?;
            stream.bought("agent-a", "md-buy-1", "5")?;
            stream.bought("agent-b", "md-buy-2", "5")?;
            stream.protection("placed", "md-oco-1", "10")?;
            let mut pairs = pairs;
            pairs.push(("client_order_id", text("md-oco-1")));
            stream.fold(event, pairs, None)?;
            let leg = stream
                .state
                .orders
                .get(&id("md-oco-1")?)
                .ok_or_else(|| missing("the ownerless leg"))?;
            assert_eq!(
                (leg.state, leg.filled_qty, leg.agent.clone()),
                (state, qty(filled)?, None),
                "{event} {state:?}: folded on the leg, which stays no one's"
            );
        }
        Ok(())
    }

    /// #258 round 2, minor 1: a holder change in one instrument re-attributes that instrument's
    /// ownerless legs only; another instrument's ownerless leg keeps no owner.
    #[test]
    fn a_holder_change_re_attributes_only_its_own_instruments_legs() -> Result<(), ExecutorError> {
        let mut stream = Stream::opened()?;
        stream.fold(
            "ProtectionChanged",
            vec![
                ("instrument", text("MSFT")),
                ("action", text("placed")),
                ("orders", text("md-oco-9")),
                ("qty", text("3")),
            ],
            None,
        )?;
        stream.bought("agent-a", "md-buy-1", "5")?;
        stream.bought("agent-b", "md-buy-2", "5")?;
        stream.protection("placed", "md-oco-1", "10")?;
        stream.sold("agent-b", "md-sell-1", "5")?;
        let owner = |raw: &str| -> Result<Option<AgentId>, ExecutorError> {
            Ok(stream
                .state
                .orders
                .get(&id(raw)?)
                .and_then(|order| order.agent.clone()))
        };
        assert_eq!(owner("md-oco-1")?, Some(AgentId("agent-a".to_owned())));
        assert_eq!(
            owner("md-oco-9")?,
            None,
            "MSFT's leg is not AAPL's to re-attribute"
        );
        Ok(())
    }

    #[test]
    fn a_leg_is_attributed_once_one_agents_lots_make_up_the_open_quantity()
    -> Result<(), ExecutorError> {
        let mut stream = Stream::opened()?;
        stream.bought("agent-a", "md-buy-1", "5")?;
        stream.bought("agent-b", "md-buy-2", "5")?;
        stream.protection("placed", "md-oco-1", "10")?;
        assert_eq!(ownerless(&stream), vec![id("md-oco-1")?]);
        stream.sold("agent-b", "md-sell-1", "5")?;
        assert!(
            ownerless(&stream).is_empty(),
            "agent-b went flat, so agent-a's five are the whole position"
        );
        let order = stream
            .state
            .orders
            .get(&id("md-oco-1")?)
            .ok_or_else(|| missing("the attributed leg"))?;
        assert_eq!(order.agent, Some(AgentId("agent-a".to_owned())));
        assert_eq!(
            stream.state.reservations.get(&id("md-oco-1")?),
            Some(&Usd::ZERO)
        );
        Ok(())
    }

    #[test]
    fn quantity_no_order_of_ours_accounts_for_leaves_no_holder() -> Result<(), ExecutorError> {
        let mut stream = Stream::opened()?;
        stream.bought("agent-a", "md-buy-1", "5")?;
        stream.fold(
            "FillApplied",
            vec![
                ("fill_id", text("f-outside")),
                ("instrument", text("AAPL")),
                ("side", text("buy")),
                ("qty_gross", text("3")),
                ("price", text("150")),
            ],
            None,
        )?;
        stream.protection("placed", "md-oco-1", "8")?;
        assert_eq!(
            ownerless(&stream),
            vec![id("md-oco-1")?],
            "agent-a's five are not the whole eight"
        );
        Ok(())
    }

    #[test]
    fn a_leg_already_in_the_order_set_keeps_its_own_record() -> Result<(), ExecutorError> {
        let mut stream = Stream::opened()?;
        stream.bought("agent-a", "md-buy-1", "10")?;
        stream.fold(
            "OrderSubmitted",
            vec![
                ("client_order_id", text("md-stop-1")),
                ("agent", text("agent-a")),
                ("instrument", text("AAPL")),
                ("side", text("sell")),
                ("qty", text("4")),
                ("limit", text("140")),
                ("purpose", text("protective")),
            ],
            None,
        )?;
        stream.protection("placed", "md-stop-1", "4")?;
        let order = stream
            .state
            .orders
            .get(&id("md-stop-1")?)
            .ok_or_else(|| missing("the submitted leg"))?;
        assert_eq!(
            order.state,
            OrderState::Submitting,
            "the submission's record stands"
        );
        assert_eq!(order.qty, qty("4")?);
        Ok(())
    }

    #[test]
    fn a_cancelled_leg_uncovers_its_quantity_and_the_last_one_clears_the_protection()
    -> Result<(), ExecutorError> {
        let mut stream = Stream::opened()?;
        let aapl = aapl()?;
        stream.bought("agent-a", "md-buy-1", "10")?;
        stream.protection("placed", "md-oco-1", "6")?;
        stream.protection("placed", "md-oco-2", "4")?;
        assert_eq!(stream.state.protective_sell_qty(&aapl)?, qty("10")?);
        stream.protection("cancelled", "md-oco-1", "6")?;
        assert_eq!(stream.state.protective_sell_qty(&aapl)?, qty("4")?);
        stream.protection("cancelled", "md-oco-2", "4")?;
        assert_eq!(stream.state.protection(&aapl)?, None);
        assert_eq!(stream.state.protective_sell_qty(&aapl)?, Qty::ZERO);
        Ok(())
    }

    #[test]
    fn a_cancel_names_its_own_legs_and_uncovers_only_what_it_says() -> Result<(), ExecutorError> {
        let mut stream = Stream::opened()?;
        let aapl = aapl()?;
        stream.bought("agent-a", "md-buy-1", "10")?;
        stream.protection("placed", "md-oco-1 md-oco-2 md-oco-3", "10")?;
        stream.fold(
            "ProtectionChanged",
            vec![
                ("instrument", text("AAPL")),
                ("action", text("cancelled")),
                ("orders", text("md-oco-1")),
            ],
            None,
        )?;
        let protection = stream
            .state
            .protection(&aapl)?
            .ok_or_else(|| missing("the protection the other legs keep"))?;
        assert_eq!(
            protection.resting,
            vec![id("md-oco-2")?, id("md-oco-3")?],
            "only the named leg leaves"
        );
        assert_eq!(
            protection.covered_qty,
            qty("10")?,
            "a cancel that uncovers no quantity leaves the cover alone"
        );
        stream.protection("cancelled", "md-oco-2", "3")?;
        assert_eq!(stream.state.protective_sell_qty(&aapl)?, qty("7")?);
        stream.protection("cancelled", "md-oco-3", "20")?;
        assert_eq!(
            stream.state.protection(&aapl)?,
            None,
            "the last leg clears it"
        );
        Ok(())
    }

    /// #258 round 1, minor 3: an action the fold does not interpret is refused, never skipped.
    #[test]
    fn an_unknown_protection_action_is_refused() -> Result<(), ExecutorError> {
        let mut stream = Stream::opened()?;
        stream.bought("agent-a", "md-buy-1", "10")?;
        let before = stream.state.clone();
        assert_eq!(
            stream.protection("moved", "md-oco-1", "10"),
            Err(ExecutorError::NonCanonicalPayload {
                field: "action".to_owned()
            })
        );
        assert_eq!(stream.state, before, "and nothing is folded");
        Ok(())
    }

    /// §5.4's bound alerts one instrument's open interval, never another's.
    #[test]
    fn an_interval_limit_marks_only_its_own_open_interval() -> Result<(), ExecutorError> {
        let mut stream = Stream::opened()?;
        for (instrument, action) in [
            ("MSFT", "unprotected_start"),
            ("AAPL", "unprotected_start"),
            ("AAPL", "interval_limit"),
        ] {
            stream.fold(
                "ProtectionChanged",
                vec![
                    ("instrument", text(instrument)),
                    ("action", text(action)),
                    ("orders", text("")),
                ],
                None,
            )?;
        }
        let alerted: Vec<(&str, bool)> = stream
            .state
            .unprotected
            .iter()
            .map(|interval| (interval.instrument.as_str(), interval.alerted))
            .collect();
        assert_eq!(alerted, vec![("MSFT", false), ("AAPL", true)]);
        Ok(())
    }

    #[test]
    fn an_unprotected_interval_ends_only_its_own_open_one() -> Result<(), ExecutorError> {
        let mut stream = Stream::opened()?;
        let interval = |instrument: &str, action: &str| {
            vec![
                ("instrument", text(instrument)),
                ("action", text(action)),
                ("orders", text("")),
            ]
        };
        stream.fold(
            "ProtectionChanged",
            interval("MSFT", "unprotected_start"),
            None,
        )?;
        stream.fold(
            "ProtectionChanged",
            interval("AAPL", "unprotected_start"),
            None,
        )?;
        stream.fold(
            "ProtectionChanged",
            interval("AAPL", "unprotected_end"),
            None,
        )?;
        stream.fold(
            "ProtectionChanged",
            interval("AAPL", "unprotected_start"),
            None,
        )?;
        stream.fold(
            "ProtectionChanged",
            interval("AAPL", "unprotected_end"),
            None,
        )?;
        let ended: Vec<(&str, bool)> = stream
            .state
            .unprotected
            .iter()
            .map(|interval| (interval.instrument.as_str(), interval.ended_at.is_some()))
            .collect();
        assert_eq!(
            ended,
            vec![("MSFT", false), ("AAPL", true), ("AAPL", true)],
            "each end closes its own instrument's open interval, never another's or a closed one"
        );
        Ok(())
    }

    #[test]
    fn only_a_filled_buy_in_the_instrument_makes_a_holder() -> Result<(), ExecutorError> {
        let mut stream = Stream::opened()?;
        stream.bought("agent-a", "md-buy-1", "10")?;
        stream.fold(
            "OrderSubmitted",
            vec![
                ("client_order_id", text("md-buy-2")),
                ("agent", text("agent-b")),
                ("instrument", text("AAPL")),
                ("side", text("buy")),
                ("qty", text("5")),
                ("limit", text("150")),
            ],
            None,
        )?;
        stream.fold(
            "OrderSubmitted",
            vec![
                ("client_order_id", text("md-sell-1")),
                ("agent", text("agent-a")),
                ("instrument", text("AAPL")),
                ("side", text("sell")),
                ("qty", text("2")),
                ("limit", text("160")),
            ],
            None,
        )?;
        stream.fold(
            "FillApplied",
            vec![
                ("fill_id", text("f-sell-1")),
                ("client_order_id", text("md-sell-1")),
                ("instrument", text("AAPL")),
                ("side", text("sell")),
                ("qty_gross", text("2")),
                ("price", text("160")),
            ],
            None,
        )?;
        stream.fold(
            "OrderSubmitted",
            vec![
                ("client_order_id", text("md-msft-1")),
                ("agent", text("agent-d")),
                ("instrument", text("MSFT")),
                ("side", text("buy")),
                ("qty", text("1")),
                ("limit", text("400")),
            ],
            None,
        )?;
        stream.fold(
            "FillApplied",
            vec![
                ("fill_id", text("f-msft-1")),
                ("client_order_id", text("md-msft-1")),
                ("instrument", text("MSFT")),
                ("side", text("buy")),
                ("qty_gross", text("1")),
                ("price", text("400")),
            ],
            None,
        )?;
        stream.protection("placed", "md-oco-1", "8")?;
        let order = stream
            .state
            .orders
            .get(&id("md-oco-1")?)
            .ok_or_else(|| missing("the leg of the single holder"))?;
        assert_eq!(
            order.agent,
            Some(AgentId("agent-a".to_owned())),
            "an unfilled buy and another instrument's buyer hold nothing here, and the holder's own \
             sell counts against its lots"
        );
        Ok(())
    }

    #[test]
    fn a_requested_cancel_is_unconfirmed_until_a_confirmation_or_a_terminal_state()
    -> Result<(), ExecutorError> {
        for (then, confirmed) in [("accepted", true), ("filled", false)] {
            let mut stream = Stream::opened()?;
            stream.bought("agent-a", "md-buy-1", "10")?;
            stream.fold(
                "OrderStateChanged",
                vec![
                    ("client_order_id", text("md-buy-1")),
                    ("state", text("accepted")),
                    ("cancel_requested", Value::Bool(true)),
                ],
                None,
            )?;
            let unconfirmed = |stream: &Stream| {
                stream
                    .state
                    .orders
                    .get(&ClientOrderId::parse("md-buy-1").ok()?)
                    .map(|order| order.cancel_unconfirmed)
            };
            assert_eq!(unconfirmed(&stream), Some(true), "{then}: requested");
            stream.fold(
                "OrderStateChanged",
                vec![
                    ("client_order_id", text("md-buy-1")),
                    ("state", text(then)),
                    ("cancel_confirmed", Value::Bool(confirmed)),
                ],
                None,
            )?;
            assert_eq!(unconfirmed(&stream), Some(false), "{then}: cleared");
        }
        Ok(())
    }

    #[test]
    fn a_copied_fact_records_the_origin_it_cites() -> Result<(), ExecutorError> {
        let mut stream = Stream::opened()?;
        stream.fold("ClockAdvanced", vec![], Some("origin-1"))?;
        stream.fold(
            "ClockAdvanced",
            vec![("originated", Value::Bool(true))],
            None,
        )?;
        assert_eq!(
            stream.state.copied_origin(&EventId("e-2".to_owned()))?,
            Some(&EventId("origin-1".to_owned()))
        );
        assert_eq!(
            stream.state.copied_origin(&EventId("e-3".to_owned()))?,
            None
        );
        Ok(())
    }
}

#[cfg(test)]
mod interval_tests {
    use mandate_canon::Value;

    use crate::error::ExecutorError;
    use crate::payload::{clock, object};
    use crate::ports::Ports;
    use crate::reconcile::tests::{Everything, Executor, Ids, executor_config, fees};
    use crate::types::RiskClock;

    fn protection_changed(
        executor: &mut Executor,
        at: i64,
        mut pairs: Vec<(&str, Value)>,
    ) -> Result<(), ExecutorError> {
        pairs.push(("instrument", Value::Str("AAPL".to_owned())));
        pairs.push(("risk_clock", clock(RiskClock::from_secs(at))?));
        executor.commit_one("ProtectionChanged", object(pairs)?)
    }

    fn action(name: &str) -> (&'static str, Value) {
        ("action", Value::Str(name.to_owned()))
    }

    /// DEC-367 item 4 (#468's round-4 review, m2): a sequence that ends with no prices to place
    /// protection at leaves its interval open, bounded and alerted, since nothing covers the
    /// position; the next start in the instrument ends it there, as it ends one waiting on an
    /// acknowledgment, so it never stays open beside a new interval.
    #[test]
    fn an_uncovered_end_leaves_the_interval_open_until_the_next_start() -> Result<(), ExecutorError>
    {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = Executor::opened(&ports)?;
        protection_changed(&mut executor, 1, vec![action("unprotected_start")])?;
        let uncovered = ("uncovered", Value::Bool(true));
        protection_changed(&mut executor, 2, vec![action("unprotected_end"), uncovered])?;
        let spans = |executor: &Executor| -> Vec<(i64, Option<i64>, bool)> {
            executor
                .state
                .unprotected
                .iter()
                .map(|interval| {
                    (
                        interval.started_at.secs(),
                        interval.ended_at.map(RiskClock::secs),
                        interval.uncovered,
                    )
                })
                .collect()
        };
        assert_eq!(spans(&executor), vec![(1, None, true)]);
        protection_changed(&mut executor, 3, vec![action("unprotected_start")])?;
        assert_eq!(spans(&executor), vec![(1, Some(3), true), (3, None, false)]);
        Ok(())
    }

    /// DEC-348 item 2: an interval waiting on the broker's acknowledgment of new protection is
    /// ended by the next start in its instrument, at that instant, and a new interval opens; an
    /// interval that already ended is never touched again.
    #[test]
    fn a_start_ends_the_interval_waiting_on_an_acknowledgment() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = Executor::opened(&ports)?;
        protection_changed(&mut executor, 1, vec![action("unprotected_start")])?;
        protection_changed(&mut executor, 2, vec![action("unprotected_end")])?;
        protection_changed(&mut executor, 3, vec![action("unprotected_start")])?;
        let awaiting = ("awaiting", Value::Str("md-a-p1".to_owned()));
        protection_changed(&mut executor, 4, vec![action("unprotected_end"), awaiting])?;
        let spans = |executor: &Executor| -> Vec<(i64, Option<i64>)> {
            executor
                .state
                .unprotected
                .iter()
                .map(|interval| {
                    (
                        interval.started_at.secs(),
                        interval.ended_at.map(RiskClock::secs),
                    )
                })
                .collect()
        };
        assert_eq!(spans(&executor), vec![(1, Some(2)), (3, None)]);
        assert_eq!(executor.state.awaiting.len(), 1);
        protection_changed(&mut executor, 5, vec![action("unprotected_start")])?;
        assert_eq!(
            spans(&executor),
            vec![(1, Some(2)), (3, Some(5)), (5, None)]
        );
        assert!(executor.state.awaiting.is_empty());
        Ok(())
    }
}
