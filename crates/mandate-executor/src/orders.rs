//! The §5.7 order state machine over what the broker says: acknowledgments, query answers,
//! status updates, fills, and silence.

use mandate_accounting::{
    Account, AccountType, AssetClass, Execution, FeeKind, Input as AccountingInput, InstrumentId,
    Record,
};
use mandate_canon::{Object, Value};
use mandate_notify::OwnerAlertKind;
use mandate_num::{Qty, Usd};
use mandate_time::{NewYorkTime, new_york_instant};

use crate::batch::{Batch, config_refs};
use crate::codec::{side_name, state_name};
use crate::error::ExecutorError;
use crate::ids::ClientOrderId;
use crate::intent::resubmit;
use crate::listing::{lookup_target, query_unknown, queryable};
use crate::payload::{broker_text, int, object, text};
use crate::protection::{overdue, protection_cancelled};
use crate::state::{EVERY_AGENT, ExecutorState, restriction_for};
use crate::types::{
    AccountState, AgentId, BrokerAccount, BrokerFill, BrokerOrder, BrokerReject, BrokerRequest,
    ConnectionHealth, ConnectionTrouble, EventId, Mode, OrderState, Purpose, RiskClock, Seq,
    StatusMapping,
};

/// Trading-domain spec §5.7's broker status table. It is **total**: every value the table names
/// maps, and any other value is [`ExecutorError::UnmappedBrokerStatus`], which the step turns into
/// a pause and an alert — the table's own last row — never a silent no-op.
pub(crate) fn status_mapping(raw: &str) -> Result<StatusMapping, ExecutorError> {
    Ok(match raw {
        "new" | "accepted" | "pending_new" | "accepted_for_bidding" | "held" => {
            StatusMapping::Becomes(OrderState::Accepted)
        }
        "partially_filled" => StatusMapping::Becomes(OrderState::PartiallyFilled),
        "filled" => StatusMapping::Becomes(OrderState::Filled),
        "done_for_day" | "stopped" | "calculated" => StatusMapping::Unchanged,
        "pending_cancel" => StatusMapping::Becomes(OrderState::PendingCancel),
        "canceled" => StatusMapping::Becomes(OrderState::Canceled),
        "expired" => StatusMapping::Becomes(OrderState::Expired),
        "rejected" => StatusMapping::Becomes(OrderState::Rejected),
        "suspended" => StatusMapping::AcceptedFlaggedRestricted,
        "pending_replace" => StatusMapping::Becomes(OrderState::PendingReplace),
        "replaced" => StatusMapping::ReplacedPair,
        other => {
            return Err(ExecutorError::UnmappedBrokerStatus {
                status: other.to_owned(),
            });
        }
    })
}

/// Whether §5.7's diagram has an edge from `from` to `to`. Terminal states have none. An update
/// can be missed, so a state the broker reports is reachable from any earlier live state that
/// leads to it: an acknowledgment may already report a fill or a cancel, and a `replaced` may
/// arrive without the `pending_replace` before it.
pub(crate) fn legal(from: OrderState, to: OrderState) -> bool {
    use OrderState::{
        Abandoned, Accepted, Canceled, Expired, Filled, Intent, PartiallyFilled, PendingCancel,
        PendingReplace, Rejected, Replaced, Submitting, Unknown,
    };
    match from {
        Intent => matches!(to, Submitting | Abandoned),
        Submitting => matches!(
            to,
            Accepted | PartiallyFilled | Filled | Canceled | Expired | Rejected | Unknown
        ),
        Unknown => matches!(
            to,
            Accepted | PartiallyFilled | Filled | Canceled | Expired | Rejected | Intent
        ),
        Accepted => matches!(
            to,
            PartiallyFilled
                | Filled
                | Rejected
                | PendingCancel
                | PendingReplace
                | Replaced
                | Canceled
                | Expired
        ),
        PartiallyFilled => matches!(
            to,
            PartiallyFilled
                | Filled
                | PendingCancel
                | PendingReplace
                | Replaced
                | Canceled
                | Expired
        ),
        PendingCancel => matches!(
            to,
            Canceled | Filled | Accepted | PartiallyFilled | Expired | Rejected
        ),
        PendingReplace => matches!(
            to,
            Replaced | Filled | Canceled | Expired | Accepted | PartiallyFilled
        ),
        Filled | Canceled | Rejected | Expired | Replaced | Abandoned => false,
    }
}

/// The order a broker id names, if it is one this executor derived and the fold carries.
pub(crate) fn known(batch: &Batch<'_, '_>, raw: Option<&str>) -> Option<ClientOrderId> {
    let id = ClientOrderId::parse(raw?).ok()?;
    batch.view.orders.contains_key(&id).then_some(id)
}

/// The one order whose journaled records carry `broker` as their only broker order id (journal
/// spec v0.80 §9.16, DEC-2353): a fill that names no `client_order_id` is that order's. An id on
/// two orders, or an order whose records carry two ids, names none.
fn by_broker_id(batch: &Batch<'_, '_>, broker: Option<&str>) -> Option<ClientOrderId> {
    let broker = broker?;
    let mut carrying =
        batch.view.details.iter().filter(|(_, detail)| {
            detail.broker_ids.len() == 1 && detail.broker_ids.contains(broker)
        });
    let (id, _) = carrying.next()?;
    carrying.next().is_none().then(|| id.clone())
}

/// The complete nullable and boolean evidence carried by `OrderStateChanged` (DEC-459), with
/// version 2's `broker_order_id` (journal spec §9.16, DEC-869): the broker's own id for the order,
/// set only from a broker order record that carries it and `None` on every other record.
#[derive(Default)]
pub(crate) struct StateEvidence {
    pub(crate) attempted: Option<OrderState>,
    pub(crate) broker_status: Option<String>,
    pub(crate) filled_qty: Option<Qty>,
    pub(crate) reject_code: Option<String>,
    pub(crate) replaces: Option<ClientOrderId>,
    pub(crate) replaced_by: Option<ClientOrderId>,
    pub(crate) replaced_by_broker_order_id: Option<String>,
    pub(crate) lookup_absent: bool,
    pub(crate) ignored: bool,
    pub(crate) cancel_requested: bool,
    pub(crate) cancel_confirmed: bool,
    pub(crate) cancel_overdue: bool,
    pub(crate) adopted: bool,
    pub(crate) ladder_step: bool,
    pub(crate) broker_order_id: Option<String>,
}

/// The broker's id as a broker order record gives it, for `broker_order_id` (DEC-869 item 3).
/// Empty text is no id (§9.2's `text`), so it is written `null`, never guessed at.
pub(crate) fn broker_id(raw: Option<&str>) -> Option<String> {
    raw.filter(|id| !id.is_empty()).map(str::to_owned)
}

fn nullable(value: Option<Value>) -> Value {
    value.unwrap_or(Value::Null)
}

/// The one constructor for every newly journaled `OrderStateChanged`, at `schema_version` 2
/// (journal spec §9.16); [`Batch::journal`] appends its `risk_clock`.
fn state_change_fields(
    id: &ClientOrderId,
    state: OrderState,
    evidence: StateEvidence,
) -> Vec<(&'static str, Value)> {
    vec![
        ("client_order_id", text(id.as_str())),
        ("state", text(state_name(state))),
        (
            "attempted",
            nullable(evidence.attempted.map(|state| text(state_name(state)))),
        ),
        ("broker_status", nullable(evidence.broker_status.map(text))),
        (
            "filled_qty",
            nullable(evidence.filled_qty.map(|qty| text(qty.to_string()))),
        ),
        ("reject_code", nullable(evidence.reject_code.map(text))),
        (
            "replaces",
            nullable(evidence.replaces.map(|order| text(order.as_str()))),
        ),
        (
            "replaced_by",
            nullable(evidence.replaced_by.map(|order| text(order.as_str()))),
        ),
        (
            "replaced_by_broker_order_id",
            nullable(evidence.replaced_by_broker_order_id.map(text)),
        ),
        (
            "lookup",
            if evidence.lookup_absent {
                text("absent")
            } else {
                Value::Null
            },
        ),
        ("ignored", Value::Bool(evidence.ignored)),
        ("cancel_requested", Value::Bool(evidence.cancel_requested)),
        ("cancel_confirmed", Value::Bool(evidence.cancel_confirmed)),
        ("cancel_overdue", Value::Bool(evidence.cancel_overdue)),
        ("adopted", Value::Bool(evidence.adopted)),
        ("ladder_step", Value::Bool(evidence.ladder_step)),
        (
            "broker_order_id",
            nullable(evidence.broker_order_id.map(text)),
        ),
    ]
}

pub(crate) fn record_state_change(
    batch: &mut Batch<'_, '_>,
    id: &ClientOrderId,
    state: OrderState,
    evidence: StateEvidence,
) -> Result<EventId, ExecutorError> {
    let ended = batch.journal(
        "OrderStateChanged",
        None,
        state_change_fields(id, state, evidence),
    )?;
    if matches!(
        state,
        OrderState::Canceled | OrderState::Rejected | OrderState::Expired
    ) {
        crate::kill::certain_part_ended(batch, id, state, &ended)?;
    }
    Ok(ended)
}

/// Journals one transition, or — when §5.7 has no such edge — the attempt, marked ignored, with
/// the order left where it was (§5.7: "journaled and ignored").
pub(crate) fn transition(
    batch: &mut Batch<'_, '_>,
    id: &ClientOrderId,
    to: OrderState,
    mut evidence: StateEvidence,
) -> Result<EventId, ExecutorError> {
    let from = batch
        .view
        .orders
        .get(id)
        .map_or(OrderState::Unknown, |order| order.state);
    let state = if legal(from, to) {
        to
    } else {
        evidence.attempted = Some(to);
        evidence.ignored = true;
        from
    };
    record_state_change(batch, id, state, evidence)
}

/// An order the broker described — an acknowledgment, a query answer, or a pushed update — folded
/// through the status table. The broker's word on the order is adopted; a status outside the table
/// pauses the agent and alerts.
pub(crate) fn described(
    batch: &mut Batch<'_, '_>,
    order: &BrokerOrder,
) -> Result<(), ExecutorError> {
    let Some(id) = known(batch, order.client_order_id.as_deref()) else {
        batch.request_reconciliation();
        return Ok(());
    };
    let status = StateEvidence {
        broker_status: Some(order.status.clone()),
        filled_qty: Some(order.filled_qty),
        broker_order_id: broker_id(Some(&order.broker_order_id)),
        ..StateEvidence::default()
    };
    let current = batch
        .view
        .orders
        .get(&id)
        .map_or(OrderState::Unknown, |known| known.state);
    match status_mapping(&order.status) {
        Err(_) => unmapped(batch, &id)?,
        Ok(StatusMapping::Unchanged) => {
            transition(batch, &id, current, status)?;
        }
        Ok(StatusMapping::AcceptedFlaggedRestricted) => {
            transition(batch, &id, OrderState::Accepted, status)?;
            batch.request_reconciliation();
        }
        Ok(StatusMapping::ReplacedPair) => replaced(batch, &id, order, status)?,
        Ok(StatusMapping::Becomes(OrderState::Accepted | OrderState::PartiallyFilled))
            if cancelling(batch, &id) =>
        {
            transition(batch, &id, OrderState::PendingCancel, status)?;
        }
        Ok(StatusMapping::Becomes(to)) => {
            transition(
                batch,
                &id,
                to,
                StateEvidence {
                    reject_code: order.reject_code.clone(),
                    ..status
                },
            )?;
        }
    }
    let applied = batch
        .view
        .orders
        .get(&id)
        .map_or(Qty::ZERO, |known| known.filled_qty);
    if order.filled_qty > applied {
        batch.request_reconciliation();
    }
    crate::reconcile::settle_doubted_placements(batch, &id, order)
}

/// Whether `id` is `PendingCancel` with a cancel outstanding that a working status does not end.
/// A working status the broker reports (`new`, `accepted`, `partially_filled`, ...) says the
/// broker has not cancelled the order yet, so it stays `PendingCancel` and the next start sends
/// its cancel again (DEC-2302, §5.7). Only a refusal of the cancel ([`reject`], which marks the
/// wait overdue) is §5.7's "cancel rejected" edge back to the prior state, and never for a kill
/// switch's cancel, which must not lapse while the order works (§5.5, rule 13).
fn cancelling(batch: &Batch<'_, '_>, id: &ClientOrderId) -> bool {
    let refused = batch
        .view
        .details
        .get(id)
        .is_some_and(|detail| detail.cancel_overdue);
    (!refused || batch.view.kill_switch_cancels(id))
        && batch.view.orders.get(id).is_some_and(|order| {
            order.state == OrderState::PendingCancel && order.cancel_unconfirmed
        })
}

/// §5.7's last row: a status outside the table pauses the agent and alerts the owner. The order
/// is left in its state, because a status that cannot be mapped is a state that cannot be derived.
fn unmapped(batch: &mut Batch<'_, '_>, id: &ClientOrderId) -> Result<(), ExecutorError> {
    let (agent, to) = match batch.view.orders.get(id).map(|order| order.agent.clone()) {
        Some(Some(agent)) => (agent.0, "paused"),
        Some(None) => (EVERY_AGENT.to_owned(), "exits_only"),
        None => (String::new(), "paused"),
    };
    let applied = batch.journal(
        "AgentModeApplied",
        None,
        vec![
            ("agent", text(agent)),
            ("to", text(to)),
            ("restriction", text("broker_status_unmapped")),
            ("originated", Value::Bool(true)),
        ],
    )?;
    batch.notify(applied, OwnerAlertKind::AgentHeld);
    Ok(())
}

/// A transport failure or an ambiguous answer: every order whose request was in flight is
/// `Unknown`, its reservation held, and queried by client order id — never treated as rejected
/// and never resent (interpretation 10).
pub(crate) fn silence(batch: &mut Batch<'_, '_>) -> Result<(), ExecutorError> {
    let in_flight: Vec<ClientOrderId> = batch
        .view
        .orders
        .values()
        .filter(|order| order.state == OrderState::Submitting)
        .map(|order| order.client_order_id.clone())
        .collect();
    for id in in_flight {
        transition(batch, &id, OrderState::Unknown, StateEvidence::default())?;
        query_unknown(batch, id)?;
    }
    Ok(())
}

/// The broker refusing our own `client_order_id` means the order is already there: it is queried,
/// never failed (E7-2 step 6).
pub(crate) fn duplicate(batch: &mut Batch<'_, '_>, raw: &str) -> Result<(), ExecutorError> {
    let Some(id) = known(batch, Some(raw)) else {
        return Ok(());
    };
    transition(batch, &id, OrderState::Unknown, StateEvidence::default())?;
    query_unknown(batch, id)
}

/// One answer that the broker does not have the order. It is counted, never acted on alone: only
/// `unknown_absent_lookups` absences spanning `unknown_absent_window_s` confirm it, and only then
/// does the order return to `Intent` for the gate to re-run (§5.7, interpretation 9).
pub(crate) fn absent(batch: &mut Batch<'_, '_>, raw: &str) -> Result<(), ExecutorError> {
    let Some(id) = known(batch, Some(raw)) else {
        return Ok(());
    };
    if batch.view.orders.get(&id).map(|order| order.state) != Some(OrderState::Unknown) {
        return Ok(());
    }
    record_state_change(
        batch,
        &id,
        OrderState::Unknown,
        StateEvidence {
            lookup_absent: true,
            ..StateEvidence::default()
        },
    )?;
    let config = batch.ports.config;
    let confirmed = batch.view.orders.get(&id).is_some_and(|order| {
        order.absent_lookups >= config.unknown_absent_lookups
            && order.first_absence_at.is_some_and(|first| {
                batch.at().secs().saturating_sub(first.secs()) >= config.unknown_absent_window_s
            })
    });
    if confirmed {
        transition(batch, &id, OrderState::Intent, StateEvidence::default())?;
        resubmit(batch, &id)?;
    }
    Ok(())
}

/// A cancel the broker confirmed (§5.7): the order it names becomes `Canceled`, which releases its
/// reservation and clears the unconfirmed cancel that held its instrument (§5.4). An id this
/// executor did not derive, or does not carry, asks for a reconciliation rather than being
/// dropped. The exits the cancel held back are released after the step, as after any other.
pub(crate) fn cancelled(batch: &mut Batch<'_, '_>, raw: &str) -> Result<(), ExecutorError> {
    let Some(id) = known(batch, Some(raw)) else {
        batch.request_reconciliation();
        return Ok(());
    };
    transition(
        batch,
        &id,
        OrderState::Canceled,
        StateEvidence {
            cancel_confirmed: true,
            ..StateEvidence::default()
        },
    )?;
    match batch.view.orders.get(&id) {
        Some(order) if order.purpose == Purpose::Protective => {
            let instrument = order.instrument.clone();
            protection_cancelled(batch, &instrument, &id)
        }
        _ => Ok(()),
    }
}

/// An `Unknown` order is queried again once `unknown_absent_window_s ÷ (N − 1)` seconds (rounded
/// up) have passed since it went `Unknown` or since its last absence, so N lookups span the whole
/// window and no faster. A bracket placement's doubt is asked after by the entry its id names,
/// never by the handle no broker order carries, and only while the entry's record is current in
/// the ledger ([`lookup_target`], DEC-878 item 1, #1292's contract item 3).
pub(crate) fn lookups_due(batch: &mut Batch<'_, '_>) {
    if !queryable(&batch.view) {
        return;
    }
    let config = batch.ports.config;
    let gaps = i64::from(config.unknown_absent_lookups.saturating_sub(1).max(1));
    let spacing = config
        .unknown_absent_window_s
        .saturating_add(gaps.saturating_sub(1))
        .checked_div(gaps)
        .unwrap_or(config.unknown_absent_window_s);
    let now = batch.at().secs();
    let due: Vec<ClientOrderId> = batch
        .view
        .orders
        .values()
        .filter(|order| order.state == OrderState::Unknown)
        .filter(|order| {
            batch
                .view
                .details
                .get(&order.client_order_id)
                .and_then(|detail| detail.last_absence.or(detail.unknown_since))
                .is_some_and(|since| now.saturating_sub(since.secs()) >= spacing)
        })
        .map(|order| order.client_order_id.clone())
        .collect();
    for id in due {
        if let Some(target) = lookup_target(&batch.view, &id) {
            batch.broker(BrokerRequest::GetOrderByClientId(target));
        }
    }
}

/// DEC-2302 item 7, on a tick: a kill switch's cancel still unconfirmed, on a `PendingCancel`
/// order or on one an ignored ask left `Accepted` or `PartiallyFilled`, is asked again once
/// `unknown_absent_window_s` has passed by the risk clock since the journal last asked it, at most
/// `unknown_absent_lookups` times in each run: §5.7's "N lookups over T seconds" pair, which
/// carries this meaning too. Each re-send is journaled before it is sent (rule 5). A run counts
/// the re-sends journaled since its start: the switch's own ask, journaled in its run, is not one,
/// and the start's re-send (item 3) is journaled nowhere (item 4), so a run that began with the
/// cancel already asked sends it at most `unknown_absent_lookups` + 1 times. When the window after
/// the run's last re-send passes with the cancel still unconfirmed, the alert sounds on the tick it
/// first passes, judged against the tick before it, at most once per run. No other cancel is
/// retried here: protection keeps its own bound (§5.4).
pub(crate) fn kill_cancels_due(batch: &mut Batch<'_, '_>) -> Result<(), ExecutorError> {
    let config = batch.ports.config;
    let now = batch.at().secs();
    let view = &batch.view;
    let window = config.unknown_absent_window_s;
    let expired = |at: i64, asked: RiskClock| at.saturating_sub(asked.secs()) >= window;
    let mut due = Vec::new();
    let mut exhausted = Vec::new();
    for order in view.orders.values().filter(|order| {
        order.cancel_unconfirmed
            && matches!(
                order.state,
                OrderState::PendingCancel | OrderState::Accepted | OrderState::PartiallyFilled
            )
            && view.kill_switch_cancels(&order.client_order_id)
    }) {
        let Some(detail) = view.details.get(&order.client_order_id) else {
            continue;
        };
        let Some(asked) = detail.cancel_asked_at else {
            continue;
        };
        let before_run = |seq: &&Seq| view.started_at.is_some_and(|start| **seq <= start);
        let asked_before = detail.cancel_asked_seqs.iter().any(|seq| before_run(&seq));
        let in_run = detail
            .cancel_asked_seqs
            .iter()
            .filter(|seq| !before_run(seq))
            .count();
        let resent = in_run.saturating_sub(usize::from(!asked_before));
        let budget = usize::try_from(config.unknown_absent_lookups).unwrap_or(usize::MAX);
        if !expired(now, asked) {
            continue;
        }
        if resent < budget {
            due.push(order.client_order_id.clone());
        } else if !view
            .previous_now
            .is_some_and(|previous| expired(previous.secs(), asked))
            && let Some(last) = detail.cancel_asked_event.clone()
        {
            exhausted.push(last);
        }
    }
    for id in due {
        record_state_change(
            batch,
            &id,
            OrderState::PendingCancel,
            StateEvidence {
                cancel_requested: true,
                ..StateEvidence::default()
            },
        )?;
        batch.broker(BrokerRequest::Cancel {
            client_order_id: id,
        });
    }
    for last in exhausted {
        batch.notify(last, OwnerAlertKind::ExitStalled);
    }
    Ok(())
}

/// `replaced`: the old order becomes `Replaced` and the new one, linked to it under an id derived
/// from the event that records the replacement, becomes `Accepted` holding the old reservation
/// (§5.7, interpretation 26).
fn replaced(
    batch: &mut Batch<'_, '_>,
    id: &ClientOrderId,
    order: &BrokerOrder,
    status: StateEvidence,
) -> Result<(), ExecutorError> {
    let from = batch
        .view
        .orders
        .get(id)
        .map_or(OrderState::Unknown, |known| known.state);
    if !legal(from, OrderState::Replaced) {
        transition(batch, id, OrderState::Replaced, status)?;
        return Ok(());
    }
    let linked = ClientOrderId::for_replacement(&batch.next_id())?;
    transition(
        batch,
        id,
        OrderState::Replaced,
        StateEvidence {
            replaced_by: Some(linked.clone()),
            replaced_by_broker_order_id: order.replaced_by_broker_order_id.clone(),
            ..status
        },
    )?;
    record_state_change(
        batch,
        &linked,
        OrderState::Accepted,
        StateEvidence {
            replaces: Some(id.clone()),
            broker_order_id: broker_id(order.replaced_by_broker_order_id.as_deref()),
            ..StateEvidence::default()
        },
    )?;
    Ok(())
}

/// One broker fill, applied by its fill id, once (§5.7). A fill for a terminal order is a
/// `LateFillApplied` that triggers a reconciliation. A fill that cannot be one of our orders' —
/// not our id, another instrument or side, or more than the order has left — is still applied to
/// accounting, unattributed, as external activity: every agent goes `exits_only` and the owner is
/// alerted (§7.1), so filled quantity never exceeds an order's quantity. Its `FillApplied` is
/// version 2's, naming no order (`null`), and moves the account's position and cash, no agent's
/// (journal spec §9.5, DEC-2812).
///
/// `before` is the state a reconciliation began from, when the fill is one it found missing: an
/// order the same reconciliation adopted as terminal was not terminal when the fill happened, so
/// the fill is an ordinary one, not a late one. A late fill outside a reconciliation asks for one
/// here; one a reconciliation found leaves that to the reconciliation, which asks once its own
/// `ReconciliationRun` has moved the activity checkpoint past the fill (DEC-938 item 5), so the
/// next one reads on from there. Answers whether the fill was late.
pub(crate) fn fill(
    batch: &mut Batch<'_, '_>,
    fill: &BrokerFill,
    before: Option<&ExecutorState>,
) -> Result<bool, ExecutorError> {
    if batch.view.fills.contains(&fill.fill_id) {
        return Ok(false);
    }
    let named = match &fill.client_order_id {
        Some(raw) => known(batch, Some(raw)),
        None => by_broker_id(batch, fill.broker_order_id.as_deref()),
    };
    let ours = named.filter(|id| {
        batch.view.orders.get(id).is_some_and(|order| {
            order.instrument == fill.instrument
                && order.side == fill.side
                && order
                    .qty
                    .checked_sub(order.filled_qty)
                    .is_ok_and(|left| fill.qty <= left)
        })
    });
    let terminal = ours
        .as_ref()
        .and_then(|id| before.unwrap_or(&batch.view).orders.get(id))
        .is_some_and(|order| order.state.is_terminal());
    let refs = fill_config(batch, &fill.instrument)?;
    let mut pairs = vec![
        ("fill_id", text(fill.fill_id.0.clone())),
        ("instrument_id", text(fill.instrument.as_str())),
        ("side", text(side_name(fill.side))),
        ("qty_gross", text(fill.qty.to_string())),
        ("price", text(fill.price.to_string())),
        ("fees", broker_fees(fill.fees)?),
        ("trade_date", text(fill.trade_date.to_string())),
    ];
    let prior = ours
        .as_ref()
        .and_then(|id| batch.view.orders.get(id))
        .map_or(Qty::ZERO, |order| order.filled_qty);
    let Some(id) = ours else {
        let ingested = batch.journal(
            "ExternalActivityIngested",
            None,
            vec![
                ("source", text("fill")),
                ("broker_order_id", Value::Null),
                ("fill_id", broker_text(Some(fill.fill_id.0.as_str()))),
                ("client_order_id", Value::Null),
                ("instrument", Value::Null),
                ("side", Value::Null),
                ("qty", Value::Null),
            ],
        )?;
        pairs.push(("client_order_id", Value::Null));
        let refs = if refs.is_empty() {
            account_fill_config(batch)?
        } else {
            refs
        };
        batch.journal_with_refs("FillApplied", None, refs, pairs)?;
        every_agent_alerted(
            batch,
            Mode::ExitsOnly,
            &restriction_for(EXTERNAL),
            ingested,
            OwnerAlertKind::ExternalActivity,
        )?;
        return Ok(false);
    };
    pairs.push(("client_order_id", text(id.as_str())));
    let kind = if terminal {
        "LateFillApplied"
    } else {
        "FillApplied"
    };
    batch.journal_with_refs(kind, None, refs, pairs)?;
    simulated_fee(batch, fill, &id, prior)?;
    if terminal {
        if before.is_none() {
            batch.request_reconciliation();
        }
        return Ok(true);
    }
    if let Some(order) = batch.view.orders.get(&id) {
        let pending = matches!(
            order.state,
            OrderState::PendingCancel | OrderState::PendingReplace
        );
        let to = if order.filled_qty >= order.qty {
            OrderState::Filled
        } else if pending {
            order.state
        } else {
            OrderState::PartiallyFilled
        };
        if to != order.state {
            transition(batch, &id, to, StateEvidence::default())?;
        }
    }
    Ok(false)
}

/// Every agent of the account to `mode` under `restriction`, and the owner alerted about
/// `subject` as `kind`, in one call: the `*` restriction reaches an agent the executor has not
/// yet seen (DEC-133 item 39), and the alert cannot be split from it (§7.1, §7.3). Originated here
/// rather than copied. The alert carries the subject event's id and a message key only
/// (`AGENTS.md` rule 6).
pub(crate) fn every_agent_alerted(
    batch: &mut Batch<'_, '_>,
    mode: Mode,
    restriction: &str,
    subject: EventId,
    kind: OwnerAlertKind,
) -> Result<(), ExecutorError> {
    batch.journal(
        "AgentModeApplied",
        None,
        vec![
            ("agent", text(EVERY_AGENT)),
            ("to", text(crate::codec::mode_name(mode))),
            ("restriction", text(restriction)),
            ("originated", Value::Bool(true)),
        ],
    )?;
    batch.notify(subject, kind);
    Ok(())
}

/// The subject external activity is recorded and acknowledged under (§7.1).
pub(crate) const EXTERNAL: &str = "external_activity";

/// The subject a run or batch that could not be recorded whole is held and acknowledged under (§11,
/// DEC-140's slice-5 amendment, DEC-2805 item 1).
pub(crate) const INCOMPLETE: &str = "incomplete";

/// DEC-2805 item 1: the journal refused the last batch. Every agent's openings are held under
/// [`INCOMPLETE`] and the owner alerted, once while the hold is in force, so a batch refused again
/// adds nothing. Exits, protection and the kill switch go on (`AGENTS.md` rule 13).
pub(crate) fn held_on_refusal(batch: &mut Batch<'_, '_>) -> Result<(), ExecutorError> {
    let restriction = restriction_for(INCOMPLETE);
    let held = (AgentId(EVERY_AGENT.to_owned()), restriction.clone());
    if batch.view.restrictions.contains_key(&held) {
        return Ok(());
    }
    let applied = batch.journal(
        "AgentModeApplied",
        None,
        vec![
            ("agent", text(EVERY_AGENT)),
            ("to", text(crate::codec::mode_name(Mode::ExitsOnly))),
            ("restriction", text(restriction)),
            ("originated", Value::Bool(true)),
        ],
    )?;
    batch.notify(applied, OwnerAlertKind::Reconciliation);
    Ok(())
}

/// The `config_refs` a fill of `instrument` is journaled with (journal spec §9: `fee`, `cal`,
/// `set`, `ins`), from the instrument snapshot's port; none when the port knows none, which the
/// journal refuses (DEC-938 item 2).
fn fill_config(batch: &Batch<'_, '_>, instrument: &InstrumentId) -> Result<Object, ExecutorError> {
    let Some(refs) = batch.ports.instruments.fill_refs(instrument) else {
        return Ok(Object::new());
    };
    config_refs(&[
        ("fee_config", Some(refs.fee_config.as_str())),
        (
            "instrument_snapshot",
            Some(refs.instrument_snapshot.as_str()),
        ),
        (
            "settlement_calendar",
            Some(refs.settlement_calendar.as_str()),
        ),
        ("trading_calendar", Some(refs.trading_calendar.as_str())),
    ])
}

/// The `config_refs` an unattributed fill in an instrument with no snapshot is journaled with: the
/// account's fee configuration and both calendars, and no instrument snapshot (journal spec §9.5
/// rule 300, DEC-2817); none when the port knows none, which the journal refuses as before.
fn account_fill_config(batch: &Batch<'_, '_>) -> Result<Object, ExecutorError> {
    let Some(refs) = batch.ports.instruments.account_fill_refs() else {
        return Ok(Object::new());
    };
    config_refs(&[
        ("fee_config", Some(refs.fee_config.as_str())),
        (
            "settlement_calendar",
            Some(refs.settlement_calendar.as_str()),
        ),
        ("trading_calendar", Some(refs.trading_calendar.as_str())),
    ])
}

/// The fee a broker reported on its fill, as the `fees` list a `FillApplied` carries (journal
/// spec §9, trading spec §6.1): none when it reported none, as Alpaca's `FILL` never does, and
/// otherwise its one USD commission (trading spec §6.2's only per-fill broker charge), charged
/// (DEC-938 item 2).
fn broker_fees(fees: Usd) -> Result<Value, ExecutorError> {
    if fees.is_zero() {
        return Ok(Value::Array(Vec::new()));
    }
    Ok(Value::Array(vec![object(vec![
        ("kind", text("commission")),
        ("amount", text(fees.to_string())),
        ("asset", text("USD")),
        ("status", text("charged")),
    ])?]))
}

/// Paper's regulatory fees (trading-domain spec §10, R-22). Alpaca paper charges no SEC, TAF, or
/// CAT fee, so on a paper stream the fee live would charge is computed with `mandate-accounting`'s
/// own rules from the effective fee configuration and booked `simulated = true`: in P&L and buying
/// power, and out of the cash comparison. Crypto fees are the broker's own on paper too, so only
/// equity fills are simulated.
///
/// Only a fill attributed to one of our orders books one. §10 does not say whether an
/// unattributed paper fill (external activity, §7.1) does; the backlog carries that question, and
/// until it is answered an unattributed fill books no simulated fee.
///
/// The fee is keyed as the journal keys every fee (journal spec §6): `family` `equities`, `day` the
/// fill's New York trade date as the account's calendar derives it, `accrued` at full precision,
/// and `charged` zero, because a simulated fee is never charged by the broker. The three equity
/// components travel beside them, each always present and `null` where the side owes none, as
/// `FeesCharged` version 1 registers them, and the record names the fee configuration its fill
/// was accounted under; a snapshot that names none leaves it unbound, and the journal refuses it
/// (DEC-948 item 3).
///
/// DEC-87's per-order TAF cap is `max(0, cap − TAF already charged on the fill's client order)`.
/// The order's `prior` filled quantity is replayed first as one earlier execution under the same
/// client order id, so the account's own TAF rule sees what the order has already been charged:
/// under `per_order` its cumulative TAF is `min(cap, rate × quantity)` however it was split, and
/// under `per_execution` the earlier execution changes nothing.
fn simulated_fee(
    batch: &mut Batch<'_, '_>,
    fill: &BrokerFill,
    id: &ClientOrderId,
    prior: Qty,
) -> Result<(), ExecutorError> {
    let equity =
        batch.ports.instruments.asset_class(&fill.instrument) == Some(AssetClass::UsEquity);
    if batch.view.environment() != Some("paper") || !equity {
        return Ok(());
    }
    let executed_at = new_york_instant(fill.trade_date, NewYorkTime::new(12, 0)?)?;
    let execution = |fill_id: String, qty_gross: Qty| Execution {
        fill_id,
        client_order_id: Some(id.as_str().to_owned()),
        instrument: fill.instrument.clone(),
        asset_class: AssetClass::UsEquity,
        side: fill.side,
        qty_gross,
        price: fill.price,
        liquidity: None,
        executed_at,
    };
    let fees = batch.ports.fees;
    let mut account = Account::opening(AccountType::Margin, Usd::ZERO, Vec::new());
    if !prior.is_zero() {
        let earlier = execution(format!("{}:earlier", fill.fill_id.0), prior);
        account = account
            .apply(&AccountingInput::Fill(earlier), fees)?
            .account;
    }
    let applied = account.apply(
        &AccountingInput::Fill(execution(fill.fill_id.0.clone(), fill.qty)),
        fees,
    )?;
    let Record::Fill {
        trade_date: Some(day),
        fees: components,
        ..
    } = applied.record
    else {
        return Ok(());
    };
    let accrued = components
        .iter()
        .try_fold(Usd::ZERO, |total, fee| total.checked_add(fee.usd))?;
    let mut pairs = vec![
        ("family", text("equities")),
        ("day", text(day.to_string())),
        ("accrued", text(accrued.to_string())),
        ("charged", text("0")),
        ("simulated", Value::Bool(true)),
        ("client_order_id", text(id.as_str())),
    ];
    for kind in [FeeKind::Sec, FeeKind::Taf, FeeKind::Cat] {
        let owed = components
            .iter()
            .filter(|fee| fee.kind == kind)
            .try_fold(None, |total: Option<Usd>, fee| {
                total.unwrap_or(Usd::ZERO).checked_add(fee.usd).map(Some)
            })?;
        let value = match owed {
            Some(usd) => text(usd.to_string()),
            None => Value::Null,
        };
        pairs.push((fee_name(kind), value));
    }
    let refs = match batch.ports.instruments.fill_refs(&fill.instrument) {
        Some(refs) => config_refs(&[("fee_config", Some(refs.fee_config.as_str()))])?,
        None => Object::new(),
    };
    batch.journal_with_refs("FeesCharged", None, refs, pairs)?;
    Ok(())
}

/// An account snapshot the broker pushed: journaled without the account number or id, which the
/// type has nowhere to hold (journal spec §6.4), and read against §7.3's first row.
pub(crate) fn account(
    batch: &mut Batch<'_, '_>,
    account: &BrokerAccount,
) -> Result<(), ExecutorError> {
    batch.journal("AccountStateObserved", None, account_fields(account)?)?;
    account_restriction(batch, account)
}

/// §7.3's first row, read on any account the broker reports, pushed or read by a reconciliation:
/// a status other than `ACTIVE`, or any blocking flag, restricts the account as blocked and pauses
/// every agent, once.
pub(crate) fn account_restriction(
    batch: &mut Batch<'_, '_>,
    account: &BrokerAccount,
) -> Result<(), ExecutorError> {
    let blocked = account.status != "ACTIVE"
        || account.trading_blocked
        || account.account_blocked
        || account.trade_suspended_by_user;
    if blocked {
        restrict(
            batch,
            Restriction::Blocked,
            Cause::BrokerNotice,
            "account_trading_blocked",
        )?;
    }
    Ok(())
}

/// Every account field §7.2 and §7.3 name, as journaled: never the account number or id, which
/// [`BrokerAccount`] has nowhere to hold (journal spec §6.4).
pub(crate) fn account_fields(
    account: &BrokerAccount,
) -> Result<Vec<(&'static str, Value)>, ExecutorError> {
    Ok(vec![
        ("status", text(account.status.clone())),
        ("crypto_status", text(account.crypto_status.clone())),
        ("trading_blocked", Value::Bool(account.trading_blocked)),
        ("account_blocked", Value::Bool(account.account_blocked)),
        (
            "trade_suspended_by_user",
            Value::Bool(account.trade_suspended_by_user),
        ),
        ("multiplier", int(u64::from(account.multiplier))?),
        ("equity", text(account.equity.to_string())),
        ("cash", text(account.cash.to_string())),
        ("buying_power", text(account.buying_power.to_string())),
        (
            "non_marginable_buying_power",
            text(account.non_marginable_buying_power.to_string()),
        ),
        ("accrued_fees", text(account.accrued_fees.to_string())),
    ])
}

/// The two restrictions §7.3 detects. There is no variant for an active account, so a restriction
/// can never be written as `active`, which the fold refuses.
#[derive(Debug, Clone, Copy)]
enum Restriction {
    ClosingOnly,
    Blocked,
}

impl Restriction {
    fn state(self) -> AccountState {
        match self {
            Self::ClosingOnly => AccountState::ClosingOnly,
            Self::Blocked => AccountState::Blocked,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::ClosingOnly => "closing_only",
            Self::Blocked => "blocked",
        }
    }

    /// §7.3's agent effect: closing only restricts every agent to exits, and a blocked account
    /// pauses them.
    fn mode(self) -> Mode {
        match self {
            Self::ClosingOnly => Mode::ExitsOnly,
            Self::Blocked => Mode::Paused,
        }
    }
}

/// What detected a restriction, journaled as its closed `cause` (journal spec §9.5, DEC-441 item
/// 23, DEC-2137): the broker's account report (§7.3 row 1), its rejects (rows 2 and 3), or a
/// connection the platform cannot reach or use (row 5, DEC-2826).
#[derive(Debug, Clone, Copy)]
enum Cause {
    BrokerNotice,
    BrokerReject,
    ConnectionUnavailable,
}

impl Cause {
    fn name(self) -> &'static str {
        match self {
            Self::BrokerNotice => "broker_notice",
            Self::BrokerReject => "broker_reject",
            Self::ConnectionUnavailable => "connection_unavailable",
        }
    }

    /// The owner's alert for a record of this cause: the broker's restriction, or the distinct one
    /// that says the broker has not restricted the account (notifications spec §3.2, DEC-2520).
    fn alert(self) -> OwnerAlertKind {
        match self {
            Self::BrokerNotice | Self::BrokerReject => OwnerAlertKind::AccountRestriction,
            Self::ConnectionUnavailable => OwnerAlertKind::ConnectionUnavailable,
        }
    }
}

/// Journals a detected restriction's cause when it is not already in force at that restriction or
/// a stricter one (journal spec v0.96 rule 309, DEC-2816), alerting the owner. Only a restriction
/// that makes the account state stricter applies its agent effect to every agent (§7.3); one
/// beside a stricter cause moves no agent, which already holds the stricter mode. The answer says
/// whether a cause was journaled.
fn restrict(
    batch: &mut Batch<'_, '_>,
    restriction: Restriction,
    cause: Cause,
    reason: &'static str,
) -> Result<bool, ExecutorError> {
    let held = batch.view.causes.get(cause.name()).copied();
    if held.is_some_and(|held| held >= restriction.state()) {
        return Ok(false);
    }
    let tightens = batch.view.account_state < restriction.state();
    let changed = batch.journal(
        "AccountRestrictionChanged",
        None,
        vec![
            ("restriction", text(restriction.name())),
            ("cause", text(cause.name())),
            ("reason_code", text(reason)),
        ],
    )?;
    if tightens {
        every_agent_alerted(batch, restriction.mode(), reason, changed, cause.alert())?;
    } else {
        batch.notify(changed, cause.alert());
    }
    Ok(true)
}

/// A connection the platform cannot reach or use (trading spec §7.3 row 5, connections spec CN-6
/// and §9.1, DEC-2826), reported at every step while it holds. While the cause
/// `connection_unavailable` is not in force, the step journals only the restriction, every agent
/// `exits_only` and the owner's alert, so the halt is never behind the state change (journal spec
/// §9.8). Once it is in force, a later step records the state change: into the reported state, or
/// a new reason while staying in it, each once per stay. A degrading cause while `suspended` is
/// held and journals nothing (DEC-687 item 1, rule 60), and so is every state change before the
/// stream's copy of the establishment (rule 68).
pub(crate) fn connection(
    batch: &mut Batch<'_, '_>,
    trouble: ConnectionTrouble,
) -> Result<(), ExecutorError> {
    if restrict(
        batch,
        Restriction::ClosingOnly,
        Cause::ConnectionUnavailable,
        "account_restricted",
    )? {
        return Ok(());
    }
    let Some(view) = batch.view.connection.clone() else {
        return Ok(());
    };
    let to = trouble.state();
    let held = view.health == ConnectionHealth::Suspended && to == ConnectionHealth::Degraded;
    let recorded = view.health == to && view.reasons.contains(trouble.reason());
    if held || recorded {
        return Ok(());
    }
    batch.journal(
        "ConnectionStateChanged",
        None,
        vec![
            ("connection_id", text(&view.id)),
            ("from", text(view.health.name())),
            ("to", text(to.name())),
            ("reason", text(trouble.reason())),
        ],
    )?;
    Ok(())
}

/// A reject the broker answered with. Journaled with its status, code and message; an order of
/// ours it names is `Rejected`, which releases its reservation — unless a cancel of the order is
/// outstanding, in any live state: then the broker refused the cancel, not the order, and the
/// order is queried as overdue ([`overdue`], DEC-160 (7), (13)). The account is restricted to
/// closing only (§7.3) by DEC-143's reading: a message that says `closing` or `restricted`, since
/// no connector reject table maps a `code` yet, or the configured run of consecutive 403s, every
/// 403 counting, a named order's included. A reject naming an id we do not know counts toward
/// that run and is nothing more. The account is asked for again once, when the run journals the
/// reject as a cause in force, even beside a stricter one (DEC-2816), not at every 403 after.
pub(crate) fn reject(
    batch: &mut Batch<'_, '_>,
    reject: &BrokerReject,
) -> Result<(), ExecutorError> {
    let pairs = vec![
        ("http_status", int(u64::from(reject.http_status))?),
        ("code", broker_text(reject.code.as_deref())),
        ("message", broker_text(Some(reject.message.as_str()))),
        (
            "client_order_id",
            broker_text(reject.client_order_id.as_deref()),
        ),
    ];
    batch.journal("RejectObserved", None, pairs)?;
    let cancelling = known(batch, reject.client_order_id.as_deref()).filter(|id| {
        batch
            .view
            .orders
            .get(id)
            .is_some_and(|order| order.cancel_unconfirmed)
    });
    if let Some(id) = cancelling {
        overdue(batch, &id)?;
    } else if let Some(id) = known(batch, reject.client_order_id.as_deref()) {
        transition(
            batch,
            &id,
            OrderState::Rejected,
            StateEvidence {
                reject_code: reject.code.clone(),
                ..StateEvidence::default()
            },
        )?;
    }
    let message = reject.message.to_ascii_lowercase();
    let closing_only = message.contains("closing") || message.contains("restricted");
    let threshold = batch.view.consecutive_403s >= batch.ports.config.restriction_403_threshold;
    if (closing_only || threshold)
        && restrict(
            batch,
            Restriction::ClosingOnly,
            Cause::BrokerReject,
            "account_restricted",
        )?
        && threshold
    {
        batch.broker(BrokerRequest::GetAccount);
    }
    Ok(())
}

/// The payload name of one fee component.
fn fee_name(kind: FeeKind) -> &'static str {
    match kind {
        FeeKind::Sec => "sec",
        FeeKind::Taf => "taf",
        FeeKind::Cat => "cat",
        FeeKind::CryptoAsset => "crypto_asset",
        FeeKind::CryptoUsd => "crypto_usd",
    }
}

#[cfg(test)]
mod state_change_tests {
    use mandate_canon::{Value, to_canonical};
    use mandate_journal::Draft;
    use mandate_num::Qty;

    use super::{StateEvidence, state_change_fields};
    use crate::batch::schema_version;
    use crate::error::ExecutorError;
    use crate::ids::ClientOrderId;
    use crate::payload::{object, text};
    use crate::types::OrderState;

    fn accepted(evidence: StateEvidence) -> Result<(), ExecutorError> {
        let id = ClientOrderId::parse("md-order-1")?;
        let mut fields = state_change_fields(&id, OrderState::Accepted, evidence);
        fields.push(("risk_clock", text("2026-09-21T14:00:00.000000000Z")));
        let payload = object(fields)?;
        let body = format!(
            r#"{{"envelope_version":1,"environment":"paper",
            "event_id":"01J8Z3M4000000000000000001","stream_id":"acct:ws_1:ACCT1",
            "event_type":"OrderStateChanged","schema_version":{},
            "event_time":"2026-09-21T14:00:00.000000000Z","clock_source":"local",
            "causation_id":null,"correlation_id":null,
            "actor":{{"kind":"system","id":"executor","version":"0.1.0",
            "build":"sha256:{}"}},"config_refs":{{}},"payload":{},
            "artifact_refs":[],"pii_refs":[]}}"#,
            schema_version("OrderStateChanged"),
            "3".repeat(64),
            String::from_utf8_lossy(&to_canonical(&payload))
        );
        assert_eq!(
            Draft::parse(body.as_bytes()).map(|_| ()),
            Ok(()),
            "{payload:?}"
        );
        Ok(())
    }

    #[test]
    fn every_reachable_writer_evidence_shape_passes_the_journal_schema() -> Result<(), ExecutorError>
    {
        let cases = [
            StateEvidence::default(),
            StateEvidence {
                attempted: Some(OrderState::Filled),
                ignored: true,
                ..StateEvidence::default()
            },
            StateEvidence {
                broker_status: Some("partially_filled".to_owned()),
                filled_qty: Some(Qty::parse("1.25")?),
                reject_code: Some("insufficient_buying_power".to_owned()),
                ..StateEvidence::default()
            },
            StateEvidence {
                lookup_absent: true,
                ..StateEvidence::default()
            },
            StateEvidence {
                cancel_requested: true,
                ..StateEvidence::default()
            },
            StateEvidence {
                cancel_confirmed: true,
                ..StateEvidence::default()
            },
            StateEvidence {
                cancel_overdue: true,
                ..StateEvidence::default()
            },
            StateEvidence {
                adopted: true,
                ..StateEvidence::default()
            },
            StateEvidence {
                ladder_step: true,
                cancel_requested: true,
                ..StateEvidence::default()
            },
            StateEvidence {
                replaces: Some(ClientOrderId::parse("md-order-0")?),
                ..StateEvidence::default()
            },
            StateEvidence {
                replaced_by: Some(ClientOrderId::parse("md-order-2")?),
                replaced_by_broker_order_id: Some("broker-order-2".to_owned()),
                ..StateEvidence::default()
            },
        ];
        for evidence in cases {
            accepted(evidence)?;
        }
        Ok(())
    }

    #[test]
    fn constructor_emits_every_boolean_even_when_false() -> Result<(), ExecutorError> {
        let id = ClientOrderId::parse("md-order-1")?;
        let payload = object(state_change_fields(
            &id,
            OrderState::Accepted,
            StateEvidence::default(),
        ))?;
        for field in [
            "ignored",
            "cancel_requested",
            "cancel_confirmed",
            "cancel_overdue",
            "adopted",
            "ladder_step",
        ] {
            assert_eq!(payload.get(field), Some(&Value::Bool(false)), "{field}");
        }
        Ok(())
    }
}
