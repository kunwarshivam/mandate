//! The own-fill reconciliation (DEC-955): the fills of the orders this account stream already
//! owns that the broker filled and the stream never booked, journaled before anything else reads
//! the account. Read-only at the broker and journal-only here: it drafts fill records and nothing
//! else, so it can run where no order may be sent.

use std::collections::BTreeSet;

use mandate_accounting::Side;
use mandate_num::{Qty, SignedQty, Usd};
use mandate_time::{UtcNanos, new_york_date_and_hour};

use crate::batch::Batch;
use crate::error::ExecutorError;
use crate::ids::ClientOrderId;
use crate::orders::fill;
use crate::ports::Ports;
use crate::state::ExecutorState;
use crate::types::{
    BrokerFill, BrokerOrder, BrokerPosition, Effect, FillId, Order, RiskClock, TimeInForce,
    WriterEpoch,
};

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
/// fill that names no owned order is left alone, never adopted. With every fill matched to an
/// owned order whose fill fits it, the executor's fill path drafts journal records only.
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
    let mut view = state.clone();
    view.epoch = Some(epoch);
    view.now = view.now.max(Some(at));
    let before = view.clone();
    let mut batch = Batch::new(&view, ports)?;
    for one in &picked {
        fill(&mut batch, one, Some(&before))?;
    }
    Ok(batch.effects)
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

/// The fill records a broker that reports no fill implies (R0 R19, DEC-1151; connections spec
/// U-R8: "DEC-529's order on a flat, dedicated account compares the order's state and filled
/// quantity and the position"). Robinhood publishes no activity feed and no execution price, so
/// the only broker word on a fill is an order record's `filled_quantity` and the position's
/// average buy price. On a ledger flat in the instrument, that average is exactly the price of
/// what the one order filled, so the fill is read from the two, and from nothing else:
///
/// - the order is one of ours, a buy that adds risk, sent `day` in the regular session (so it
///   filled on the New York date of its `OrderSubmitted`, which is the fill's trade date), whose
///   broker record reported a filled quantity above zero and no more than its own quantity, and
///   none of which is booked;
/// - the ledger holds the instrument flat, with no unattributed fill in it, and every other order
///   in the instrument has booked all its broker record reported, so no other fill can be in the
///   average;
/// - the broker lists exactly one position in the instrument, long exactly the reported
///   quantity, at an average no higher than the order's limit, which no fill of it can exceed: an
///   average above it describes something else.
///
/// Anything else derives nothing: no record of a fill means no fill, the position stays
/// unexplained, and the reconciliation pauses the agent as before (`AGENTS.md` rule 3). The fill
/// carries no fee, since Robinhood charges no commission (DEC-958 item 1); its regulatory fees are
/// the fee configuration's, which every fill record names (DEC-958, DEC-960). Its id is
/// `record:<client order id>:<filled quantity>`, the same on every read of the same record, so
/// it is booked once whatever reads it again (journal spec §5.2), and the reconciliation's own
/// fill path books it ([`crate::reconcile`]).
pub fn recorded_fills(state: &ExecutorState, positions: &[BrokerPosition]) -> Vec<BrokerFill> {
    state
        .orders
        .values()
        .filter_map(|order| recorded_fill(state, positions, order))
        .collect()
}

/// The fills a broker that reports no sell price implies for our own risk-reducing sells
/// ([DEC-2300](../../../docs/project/decisions/DEC-2300.md) items 1 to 3,
/// [DEC-2395](../../../docs/project/decisions/DEC-2395.md)): each increment of a `protective`,
/// `risk_exit`, `owner_exit` or `discretionary_exit` sell that its broker record reported filled,
/// booked at the order's journaled limit price, the worst price the order allowed, under the id
/// `limit:<client order id>:<cumulative filled quantity>`, when the ledger holds the instrument long
/// at least the increment with no unattributed fill, no other order in the instrument has an
/// unbooked report, and the broker lists the position reduced by exactly the increment. A `day`
/// sell is dated by its submission's New York date, a GTC sell by the New York date of the
/// executor's clock, never earlier. Anything else derives nothing (`AGENTS.md` rule 3).
///
/// # Errors
/// [`ExecutorError::Unimplemented`] until DEC-2395's implementation lands.
pub fn limit_fills(
    state: &ExecutorState,
    positions: &[BrokerPosition],
) -> Result<Vec<BrokerFill>, ExecutorError> {
    let _ = (state, positions);
    Err(ExecutorError::Unimplemented { story: "E7-6" })
}

/// The one fill [`recorded_fills`] reads for `order`, or `None`.
fn recorded_fill(
    state: &ExecutorState,
    positions: &[BrokerPosition],
    order: &Order,
) -> Option<BrokerFill> {
    let detail = state.details.get(&order.client_order_id)?;
    let reported = detail.reported_filled.filter(|filled| !filled.is_zero())?;
    let request = detail.request.as_ref()?;
    let entry = order.side == Side::Buy
        && order.purpose.adds_risk()
        && request.tif == TimeInForce::Day
        && !request.extended_hours
        && order.filled_qty.is_zero()
        && reported <= order.qty;
    let flat = state
        .positions
        .get(&order.instrument)
        .is_none_or(|held| *held == SignedQty::ZERO)
        && state
            .unattributed
            .get(&order.instrument)
            .is_none_or(Vec::is_empty);
    let alone = state.orders.values().all(|other| {
        other.client_order_id == order.client_order_id
            || other.instrument != order.instrument
            || state
                .details
                .get(&other.client_order_id)
                .and_then(|other_detail| other_detail.reported_filled)
                .is_none_or(|filled| filled <= other.filled_qty)
    });
    if !(entry && flat && alone) {
        return None;
    }
    let mut held = positions
        .iter()
        .filter(|position| position.instrument == order.instrument);
    let (Some(position), None) = (held.next(), held.next()) else {
        return None;
    };
    let within_limit = request
        .limit_price
        .is_some_and(|limit| position.avg_entry_price <= limit);
    if position.qty != SignedQty::from(reported) || !within_limit {
        return None;
    }
    let submitted = UtcNanos::from_parts(detail.submitted_at?.secs(), 0).ok()?;
    let (trade_date, _) = new_york_date_and_hour(submitted).ok()?;
    Some(BrokerFill {
        fill_id: FillId(format!(
            "record:{}:{reported}",
            order.client_order_id.as_str()
        )),
        client_order_id: Some(order.client_order_id.as_str().to_owned()),
        broker_order_id: None,
        instrument: order.instrument.clone(),
        side: Side::Buy,
        qty: reported,
        price: position.avg_entry_price,
        fees: Usd::ZERO,
        trade_date,
    })
}
