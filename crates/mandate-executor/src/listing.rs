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

/// The id a lookup of `id` asks the broker for, when it asks at all (DEC-878 item 1, §2.3): a
/// bracket placement's own id is the platform's handle `{entry}-p{record}`, which no broker order
/// carries and whose every answer is the broker's 404, so a doubt about it — in doubt, or
/// awaiting the confirmation of its cancel — is asked after by the **entry's** own id, whose
/// nested legs decide the placement. Every other order asks by its own id: a sent OCO or a
/// re-placed stop-limit carries its own `OrderSubmitted` of the very id and is found by it or
/// not at all (DEC-878 item 5); and a protective id that is genuinely ours but names no entry
/// keeps its own lookup.
///
/// The entry is asked only while its own record is current in the ledger
/// ([`ledger_current`]): an answer read back for an entry the broker has already reported more
/// filled of than the journal has applied would itself ask for a reconciliation — §11 step 2's
/// ingest of the missing fill is what resolves it — so while the entry is ahead of its journal
/// the doubt is asked after by no raw query at all; the next reconciliation's gathered snapshot
/// ingests the fill and then the doubt asks, still never by the handle (#1292's contract item 3,
/// bounded to the reads that are folded; the shell drives a reconciliation's own queries
/// outside the gather that answers everything else, so a query whose answer is known to need a
/// gather waits for one rather than reaching it).
pub(crate) fn lookup_target(view: &ExecutorState, id: &ClientOrderId) -> Option<ClientOrderId> {
    let unsent = view
        .details
        .get(id)
        .is_none_or(|detail| detail.request.is_none());
    match (unsent, id.protected_entry()) {
        (true, Some(entry)) if ledger_current(view, &entry) => Some(entry),
        (true, Some(_)) => None,
        _ => Some(id.clone()),
    }
}

/// Whether `entry`'s record is current in the ledger: no broker-reported fill of it that the
/// journal has not applied remains, so the fold of an answer read back for it asks for no
/// reconciliation (§11's missing-fill rule: the ingest is the reconciliation's own).
fn ledger_current(view: &ExecutorState, entry: &ClientOrderId) -> bool {
    view.details.get(entry).is_none_or(|detail| {
        detail.reported_filled.is_none_or(|reported| {
            view.orders
                .get(entry)
                .is_none_or(|known| reported <= known.filled_qty)
        })
    })
}

/// Asks what became of an order in doubt, by the id [`lookup_target`] names for it. Where the
/// profile can query by client order id, by that id. Otherwise an `Unknown` order on an account
/// dedicated to its agent is listed once (DEC-862 items 1 to 3); on a shared account nothing is
/// asked and the order stays `Unknown`. An order in any other state, which the broker has already
/// described to us, is listed the same way on any account, and only the record naming it by our
/// own id is ever adopted for it ([`listed`]), so no query the connector refuses unsent is asked
/// (DEC-1150 item 1).
pub(crate) fn query_unknown(
    batch: &mut Batch<'_, '_>,
    id: ClientOrderId,
) -> Result<(), ExecutorError> {
    let Some(order) = batch.view.orders.get(&id) else {
        return Ok(());
    };
    if queryable(&batch.view) {
        if let Some(target) = lookup_target(&batch.view, &id) {
            batch.broker(BrokerRequest::GetOrderByClientId(target));
        }
        return Ok(());
    }
    if order.state != OrderState::Unknown {
        if let Some(target) = lookup_target(&batch.view, &id) {
            list(batch, target);
        }
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
    let (true, Some(_)) = (listable, submitted) else {
        return Ok(());
    };
    list(batch, id);
    Ok(())
}

/// One listing of `id`'s instrument from DEC-862 item 2's margin before its `OrderSubmitted`, or
/// nothing when the order or its submission is not in the fold.
fn list(batch: &mut Batch<'_, '_>, id: ClientOrderId) {
    if let Some(listing) = batch.view.listing(&id) {
        batch.broker(BrokerRequest::ListOrders(listing));
    }
}

impl ExecutorState {
    /// The listing of `id`'s instrument the core asks for when it lists `id` (DEC-862 item 2):
    /// orders an agent placed, from the margin before its `OrderSubmitted`. `None` when the order
    /// or its submission is not in the fold.
    pub fn listing(&self, id: &ClientOrderId) -> Option<OrderListing> {
        let order = self.orders.get(id)?;
        let at = self
            .details
            .get(id)
            .and_then(|detail| detail.submitted_at)?;
        Some(OrderListing {
            client_order_id: id.clone(),
            instrument: order.instrument.clone(),
            origin: OrderOrigin::Agentic,
            created_since: RiskClock::from_secs(at.secs().saturating_sub(LISTING_MARGIN_S)),
        })
    }
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
/// or ours, and, carrying none, looks like no other order of ours. Its stop price is ours too
/// ([DEC-2395](../../../docs/project/decisions/DEC-2395.md) item 2): an order edited in the app
/// to another stop is not the order we placed, so its filled quantity never reaches the journal
/// and a sell is never booked at a limit it may no longer carry (DEC-2300 item 1).
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
        && record.order.stop_price == ours.stop_price
        && record.order_type == ours.order_type
        && record.tif == ours.tif;
    let named = match record.order.client_order_id.as_deref() {
        Some(raw) => raw == id.as_str(),
        None => !lookalike(view, id, ours),
    };
    same && named
}

/// The answer to a listing (DEC-862 items 3 to 5). Only an order this executor would have listed
/// is considered; exactly one matching record is folded as the broker's description of it,
/// through §5.7's table, and zero or several leave it as it was with nothing sent. For an
/// `Unknown` order a record carrying no client order id may match (DEC-863); for one in any other
/// state only a record naming it by our own id does, never a look-alike, so a working order is
/// never taken as filled or gone on a guess (DEC-1150 item 2).
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
    let working = order.state != OrderState::Unknown;
    let asked = !queryable(&batch.view)
        && (working
            || order
                .agent
                .as_ref()
                .is_some_and(|agent| dedicated(&batch.view, agent)));
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
        .filter(|record| !working || record.order.client_order_id.as_deref() == Some(id.as_str()))
        .collect();
    let [one] = found.as_slice() else {
        return Ok(());
    };
    let mut adopted = one.order.clone();
    adopted.client_order_id = Some(id.as_str().to_owned());
    described(batch, &adopted)
}
