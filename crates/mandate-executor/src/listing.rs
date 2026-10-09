//! What became of an `Unknown` order on a profile that cannot query by client order id: one
//! listing of the account's orders, and the adoption of exactly one matching record
//! ([DEC-529](../../../docs/project/decisions/DEC-529.md) item 4, connections spec §6.2,
//! [DEC-862](../../../docs/project/decisions/DEC-862.md),
//! [DEC-863](../../../docs/project/decisions/DEC-863.md)).

use crate::batch::Batch;
use crate::error::ExecutorError;
use crate::ids::ClientOrderId;
use crate::orders::{described, known};
use crate::state::ExecutorState;
use crate::types::{
    AgentId, BrokerRequest, ListedOrder, OrderListing, OrderOrigin, OrderState, RiskClock,
    SubmitOrder,
};

/// DEC-862 item 2: the listing starts this long before the order's `OrderSubmitted`.
const LISTING_MARGIN_S: i64 = 300;

/// Whether the broker's profile can look an order up by its client order id. A state with no
/// profile keeps the lookup it always had.
pub(crate) fn queryable(view: &ExecutorState) -> bool {
    view.profile
        .as_ref()
        .is_none_or(|profile| profile.idempotency().query_by_client_order_id)
}

/// DEC-862 item 3: every intent and every order the account stream holds names `agent`.
fn dedicated(view: &ExecutorState, agent: &AgentId) -> bool {
    view.orders
        .values()
        .all(|order| order.agent.as_ref() == Some(agent))
        && view.intents.values().all(|intent| &intent.agent == agent)
}

/// Asks what became of an order in doubt. Where the profile can query by client order id, by
/// that id. Otherwise an `Unknown` order on an account dedicated to its agent is listed once
/// (DEC-862 items 1 to 3); on a shared account nothing is asked and the order stays `Unknown`.
/// An order in any other state keeps the lookup by id, which such a connector refuses unsent.
pub(crate) fn query_unknown(
    batch: &mut Batch<'_, '_>,
    id: ClientOrderId,
) -> Result<(), ExecutorError> {
    let Some(order) = batch.view.orders.get(&id) else {
        return Ok(());
    };
    if queryable(&batch.view) || order.state != OrderState::Unknown {
        batch.broker(BrokerRequest::GetOrderByClientId(id));
        return Ok(());
    }
    let listable = order
        .agent
        .as_ref()
        .is_some_and(|agent| dedicated(&batch.view, agent));
    let submitted = batch
        .view
        .details
        .get(&id)
        .and_then(|detail| detail.submitted_at);
    let (true, Some(at)) = (listable, submitted) else {
        return Ok(());
    };
    let listing = OrderListing {
        client_order_id: id,
        instrument: order.instrument.clone(),
        origin: OrderOrigin::Agentic,
        created_since: RiskClock::from_secs(at.secs().saturating_sub(LISTING_MARGIN_S)),
    };
    batch.broker(BrokerRequest::ListOrders(listing));
    Ok(())
}

/// DEC-863 item 3: another order the account stream holds that a record with no client order id
/// could equally be. An order whose request was not folded counts when its instrument, side and
/// quantity agree.
fn lookalike(view: &ExecutorState, id: &ClientOrderId, ours: &SubmitOrder) -> bool {
    view.orders
        .values()
        .filter(|order| &order.client_order_id != id)
        .filter(|order| {
            order.instrument == ours.instrument && order.side == ours.side && order.qty == ours.qty
        })
        .any(|order| {
            view.details
                .get(&order.client_order_id)
                .and_then(|detail| detail.request.as_ref())
                .is_none_or(|other| {
                    other.limit_price == ours.limit_price
                        && other.order_type == ours.order_type
                        && other.tif == ours.tif
                })
        })
}

/// DEC-862 item 4 and DEC-863: the record is the order as journaled, carries no client order id
/// or ours, and, carrying none, looks like no other order of ours.
fn matches(
    view: &ExecutorState,
    id: &ClientOrderId,
    ours: &SubmitOrder,
    record: &ListedOrder,
) -> bool {
    let same = record.order.instrument == ours.instrument
        && record.order.side == ours.side
        && record.order.qty == ours.qty
        && record.order.limit_price == ours.limit_price
        && record.order_type == ours.order_type
        && record.tif == ours.tif;
    let named = match record.order.client_order_id.as_deref() {
        Some(raw) => raw == id.as_str(),
        None => !lookalike(view, id, ours),
    };
    same && named
}

/// The answer to a listing (DEC-862 items 3 to 5). Only an `Unknown` order this executor would
/// have listed is considered; exactly one matching record is folded as the broker's description
/// of it, through §5.7's table, and zero or several leave it `Unknown` with nothing sent.
pub(crate) fn listed(
    batch: &mut Batch<'_, '_>,
    raw: &str,
    records: &[ListedOrder],
) -> Result<(), ExecutorError> {
    let Some(id) = known(batch, Some(raw)) else {
        return Ok(());
    };
    let Some(order) = batch.view.orders.get(&id) else {
        return Ok(());
    };
    let asked = order.state == OrderState::Unknown
        && !queryable(&batch.view)
        && order
            .agent
            .as_ref()
            .is_some_and(|agent| dedicated(&batch.view, agent));
    let request = batch
        .view
        .details
        .get(&id)
        .and_then(|detail| detail.request.as_ref());
    let (true, Some(ours)) = (asked, request) else {
        return Ok(());
    };
    let found: Vec<&ListedOrder> = records
        .iter()
        .filter(|record| matches(&batch.view, &id, ours, record))
        .collect();
    let [one] = found.as_slice() else {
        return Ok(());
    };
    let mut adopted = one.order.clone();
    adopted.client_order_id = Some(id.as_str().to_owned());
    described(batch, &adopted)
}
