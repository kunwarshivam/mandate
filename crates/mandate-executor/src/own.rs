//! The own-fill reconciliation (DEC-955): the fills of the orders this account stream already
//! owns that the broker filled and the stream never booked, journaled before anything else reads
//! the account. Read-only at the broker and journal-only here: it drafts fill records and nothing
//! else, so it can run where no order may be sent.

use mandate_accounting::InstrumentId;

use crate::error::ExecutorError;
use crate::ids::ClientOrderId;
use crate::ports::Ports;
use crate::state::ExecutorState;
use crate::types::{BrokerFill, BrokerOrder, Effect, RiskClock, WriterEpoch};

/// The orders this stream owns whose fills it may not have booked: every order it journaled
/// `OrderSubmitted` for whose booked quantity is below its own, in client order id order. Only
/// these are asked about, and only by their own id (DEC-955 item 2).
pub fn owed_orders(state: &ExecutorState) -> Vec<ClientOrderId> {
    let _ = state;
    Vec::new()
}

/// The fill records the broker's word on `orders` (each the answer to a lookup of one of
/// [`owed_orders`] by its own id) and its `fills` call for, drafted at `epoch` and at `at` or the
/// folded clock, whichever is later (DEC-955 items 2 to 4).
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
    let _ = (
        state,
        epoch,
        at,
        orders,
        fills,
        ports,
        InstrumentId::new("x"),
    );
    Err(ExecutorError::Unimplemented { story: "E2" })
}
