//! The §5.7 order state machine over what the broker says: acknowledgments, query answers, and
//! status updates.

use mandate_canon::Value;
use mandate_num::Qty;

use crate::batch::Batch;
use crate::codec::state_name;
use crate::error::ExecutorError;
use crate::ids::ClientOrderId;
use crate::payload::text;
use crate::types::{BrokerOrder, EventId, OrderState, StatusMapping};

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
        PendingCancel => matches!(to, Canceled | Filled | Accepted | PartiallyFilled),
        PendingReplace => matches!(
            to,
            Replaced | Filled | Canceled | Expired | Accepted | PartiallyFilled
        ),
        Filled | Canceled | Rejected | Expired | Replaced | Abandoned => false,
    }
}

/// The order a broker id names, if it is one this executor derived and the fold carries.
fn known(batch: &Batch<'_, '_>, raw: Option<&str>) -> Option<ClientOrderId> {
    let id = ClientOrderId::parse(raw?).ok()?;
    batch.view.orders.contains_key(&id).then_some(id)
}

/// Journals one transition, or — when §5.7 has no such edge — the attempt, marked ignored, with
/// the order left where it was (§5.7: "journaled and ignored").
pub(crate) fn transition(
    batch: &mut Batch<'_, '_>,
    id: &ClientOrderId,
    to: OrderState,
    mut extra: Vec<(&'static str, Value)>,
) -> Result<EventId, ExecutorError> {
    let from = batch
        .view
        .orders
        .get(id)
        .map_or(OrderState::Unknown, |order| order.state);
    let mut pairs = vec![("client_order_id", text(id.as_str()))];
    if legal(from, to) {
        pairs.push(("state", text(state_name(to))));
    } else {
        pairs.push(("state", text(state_name(from))));
        pairs.push(("attempted", text(state_name(to))));
        pairs.push(("ignored", Value::Bool(true)));
    }
    pairs.append(&mut extra);
    batch.journal("OrderStateChanged", None, pairs)
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
    let status = vec![
        ("broker_status", text(order.status.clone())),
        ("filled_qty", text(order.filled_qty.to_string())),
    ];
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
        Ok(StatusMapping::Becomes(to)) => {
            let mut extra = status;
            if let Some(code) = &order.reject_code {
                extra.push(("reject_code", text(code.clone())));
            }
            transition(batch, &id, to, extra)?;
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
    Ok(())
}

/// §5.7's last row: a status outside the table pauses the agent and alerts the owner. The order
/// is left in its state, because a status that cannot be mapped is a state that cannot be derived.
fn unmapped(batch: &mut Batch<'_, '_>, id: &ClientOrderId) -> Result<(), ExecutorError> {
    let agent = batch
        .view
        .orders
        .get(id)
        .map(|order| order.agent.0.clone())
        .unwrap_or_default();
    let applied = batch.journal(
        "AgentModeApplied",
        None,
        vec![
            ("agent", text(agent)),
            ("to", text("paused")),
            ("restriction", text("broker_status_unmapped")),
            ("originated", Value::Bool(true)),
        ],
    )?;
    batch.notify(applied, "broker_status_unmapped");
    Ok(())
}

/// `replaced`: the old order becomes `Replaced` and the new one, linked to it under an id derived
/// from the event that records the replacement, becomes `Accepted` holding the old reservation
/// (§5.7, interpretation 26).
fn replaced(
    batch: &mut Batch<'_, '_>,
    id: &ClientOrderId,
    order: &BrokerOrder,
    mut status: Vec<(&'static str, Value)>,
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
    status.push(("replaced_by", text(linked.as_str())));
    if let Some(broker) = &order.replaced_by_broker_order_id {
        status.push(("replaced_by_broker_order_id", text(broker.clone())));
    }
    transition(batch, id, OrderState::Replaced, status)?;
    batch.journal(
        "OrderStateChanged",
        None,
        vec![
            ("client_order_id", text(linked.as_str())),
            ("state", text(state_name(OrderState::Accepted))),
            ("replaces", text(id.as_str())),
        ],
    )?;
    Ok(())
}

/// A cancel the broker confirmed: the order is `Canceled` and its reservation released.
pub(crate) fn cancelled(batch: &mut Batch<'_, '_>, raw: &str) -> Result<(), ExecutorError> {
    if let Some(id) = known(batch, Some(raw)) {
        transition(batch, &id, OrderState::Canceled, Vec::new())?;
    }
    Ok(())
}
