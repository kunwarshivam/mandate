//! The own-fill reconciliation (DEC-955): the fills of the orders this account stream already
//! owns that the broker filled and the stream never booked, journaled before anything else reads
//! the account. Read-only at the broker and journal-only here: it drafts fill records and nothing
//! else, so it can run where no order may be sent.

use std::collections::BTreeSet;

use mandate_num::Qty;

use crate::batch::Batch;
use crate::error::ExecutorError;
use crate::ids::ClientOrderId;
use crate::orders::fill;
use crate::ports::Ports;
use crate::state::ExecutorState;
use crate::types::{BrokerFill, BrokerOrder, Effect, FillId, Order, RiskClock, WriterEpoch};

/// The orders this stream owns whose fills it may not have booked: every order it journaled
/// `OrderSubmitted` for whose booked quantity is below its own, in client order id order. Only
/// these are asked about, and only by their own id (DEC-955 item 2).
pub fn owed_orders(state: &ExecutorState) -> Vec<ClientOrderId> {
    state
        .orders
        .values()
        .filter(|order| submitted(state, &order.client_order_id) && order.filled_qty < order.qty)
        .map(|order| order.client_order_id.clone())
        .collect()
}

/// Whether the stream journaled `OrderSubmitted` for `id`.
fn submitted(state: &ExecutorState, id: &ClientOrderId) -> bool {
    state
        .details
        .get(id)
        .is_some_and(|detail| detail.submitted_seq.is_some())
}

/// The fill records the broker's word on `orders` (each the answer to a lookup of one of
/// [`owed_orders`] by its own id) and its `fills` call for, drafted at `epoch` and at `at` or the
/// folded clock, whichever is later (DEC-955 items 2 to 4).
///
/// For each order the broker answered, what it filled beyond what the stream booked is owed, and
/// the fills that name that order — by our client order id, or by the broker's own id for it —
/// and are not yet booked must add up to exactly that. Each is drafted as the executor drafts any
/// fill: `LateFillApplied` for an order the stream already holds terminal, otherwise
/// `FillApplied` and the order's move to `filled` or `partially_filled` (trading spec §5.7). A
/// fill that names no owned order is left alone, never adopted. Nothing but journal records is
/// drafted.
///
/// # Errors
/// [`ExecutorError::OwnFillMismatch`] when the broker and the journal disagree on an order the
/// journal owns; nothing is drafted then.
pub fn book_own_fills(
    state: &ExecutorState,
    epoch: WriterEpoch,
    at: RiskClock,
    orders: &[BrokerOrder],
    fills: &[BrokerFill],
    ports: &Ports<'_>,
) -> Result<Vec<Effect>, ExecutorError> {
    let mut picked: Vec<BrokerFill> = Vec::new();
    let mut answered: BTreeSet<ClientOrderId> = BTreeSet::new();
    for answer in orders {
        let (id, order) = owned(state, answer)?;
        if !answered.insert(id.clone()) {
            return Err(mismatch(&id, "order_twice"));
        }
        picked.extend(owed_fills(state, &id, order, answer, fills)?);
    }
    if picked.is_empty() {
        return Ok(Vec::new());
    }
    let mut view = state.clone();
    view.epoch = Some(epoch);
    view.now = view.now.max(Some(at));
    let before = view.clone();
    let mut batch = Batch::new(&view, ports)?;
    for one in &picked {
        fill(&mut batch, one, Some(&before))?;
    }
    let journal_only = batch
        .effects
        .iter()
        .all(|effect| matches!(effect, Effect::Journal(_)));
    match answered.first() {
        Some(id) if !journal_only => Err(mismatch(id, "not_journal_only")),
        _ => Ok(batch.effects),
    }
}

/// The owned order the broker's `answer` describes, or the mismatch that it describes none: an
/// order this stream submitted, in the same instrument and side, filled no more than its own
/// quantity.
fn owned<'s>(
    state: &'s ExecutorState,
    answer: &BrokerOrder,
) -> Result<(ClientOrderId, &'s Order), ExecutorError> {
    let raw = answer.client_order_id.clone().unwrap_or_default();
    let not_ours = || ExecutorError::OwnFillMismatch {
        client_order_id: raw.clone(),
        reason: "not_ours",
    };
    let id = ClientOrderId::parse(&raw).map_err(|_| not_ours())?;
    let order = state
        .orders
        .get(&id)
        .filter(|_| submitted(state, &id))
        .ok_or_else(not_ours)?;
    if answer.instrument != order.instrument || answer.side != order.side {
        return Err(mismatch(&id, "order_differs"));
    }
    if answer.filled_qty > order.qty {
        return Err(mismatch(&id, "overfilled"));
    }
    Ok((id, order))
}

/// The unbooked fills of `id` that make up exactly what the broker filled beyond what the stream
/// booked, each carrying `id` as its client order id, in the order the broker listed them.
fn owed_fills(
    state: &ExecutorState,
    id: &ClientOrderId,
    order: &Order,
    answer: &BrokerOrder,
    fills: &[BrokerFill],
) -> Result<Vec<BrokerFill>, ExecutorError> {
    let owed = answer
        .filled_qty
        .checked_sub(order.filled_qty)
        .map_err(|_| mismatch(id, "broker_filled_less"))?;
    let mut seen: BTreeSet<&FillId> = BTreeSet::new();
    let mut found = Vec::new();
    let mut total = Qty::ZERO;
    for one in fills {
        if !names(id, answer, one)? || state.fills.contains(&one.fill_id) {
            continue;
        }
        if !seen.insert(&one.fill_id) {
            continue;
        }
        if one.instrument != order.instrument || one.side != order.side {
            return Err(mismatch(id, "fill_differs"));
        }
        total = total.checked_add(one.qty)?;
        found.push(BrokerFill {
            client_order_id: Some(id.as_str().to_owned()),
            ..one.clone()
        });
    }
    if total < owed {
        return Err(mismatch(id, "fills_short"));
    }
    if total > owed {
        return Err(mismatch(id, "fills_exceed"));
    }
    Ok(found)
}

/// Whether `one` names the order `answer` describes: by our client order id, or, where the fill
/// carries none, by the broker's own id for the order. A fill naming it by one and another order
/// by the other is a mismatch, never a guess.
fn names(
    id: &ClientOrderId,
    answer: &BrokerOrder,
    one: &BrokerFill,
) -> Result<bool, ExecutorError> {
    let by_broker = !answer.broker_order_id.is_empty()
        && one.broker_order_id.as_deref() == Some(answer.broker_order_id.as_str());
    match one.client_order_id.as_deref() {
        Some(client) if client == id.as_str() => Ok(true),
        Some(_) if by_broker => Err(mismatch(id, "fill_names_another_order")),
        Some(_) => Ok(false),
        None => Ok(by_broker),
    }
}

fn mismatch(id: &ClientOrderId, reason: &'static str) -> ExecutorError {
    ExecutorError::OwnFillMismatch {
        client_order_id: id.as_str().to_owned(),
        reason,
    }
}
