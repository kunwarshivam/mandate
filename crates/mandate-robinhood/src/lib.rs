#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]
//! The Robinhood equity connector (backlog E7-6, the first live trade brief's C1; connections
//! spec §6.2): the `BrokerConnector` `mandate-executor` declares, over the tool calls of
//! Robinhood's published contract ([robinhood-contract.md]).
//!
//! - **Its profile** ([`robinhood_profile`]) is trading spec §5.2's Robinhood table as data
//!   (DEC-531, DEC-630): limit, whole shares and `gfd` for an opening, one resting `gtc`
//!   stop-limit for protection, a `ref_id` the broker deduplicates by, and no query by it.
//! - **`Submit`** is `review_equity_order`, then `place_equity_order` on the recorded agentic
//!   account with the [`ref_id`] derived from the intent's idempotency key, never omitted. A
//!   pre-trade alert refuses an opening or an increase before the place; a sell or a protective
//!   order is placed anyway (`AGENTS.md` rule 13).
//! - **A place answer that is not exactly an order record is `Unknown`** (LT-6): never rejected,
//!   never placed again (DEC-529 item 4, DEC-860 item 4). States read by [`executor_status`].
//! - **`Cancel`** is `cancel_equity_order` by the broker's `order_id` from the place's answer.
//!
//! Every call goes through the [`Tools`] seam (DEC-860 item 2): in production `McpClient`, which
//! holds the allowlist, the pinned contract and the drift halt. Numbers are decimal text read by
//! `mandate-num`, never a float (ES-23).
//!
//! [robinhood-contract.md]: ../../../docs/project/tasks/robinhood-contract.md

mod connector;
mod profile;

pub use connector::{RobinhoodConnector, Tools};
pub use profile::robinhood as robinhood_profile;

use mandate_executor::ClientOrderId;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RobinhoodError {
    /// The stubs of this story's tests PR return it (DEC-77).
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
    #[error("the order state is none of the contract's ten")]
    UnknownState,
}

/// The `ref_id` for the order with this idempotency key (DEC-860 item 1): a UUID version 8
/// (RFC 9562 §5.8) whose 122 free bits are the first 16 bytes of the SHA-256 of the key's text,
/// written in lower case. A restart derives the same value, so a re-sent order carries it.
pub fn ref_id(_key: &ClientOrderId) -> Result<String, RobinhoodError> {
    Err(RobinhoodError::Unimplemented { story: "E7-6" })
}

/// The status text the executor's §5.7 table reads for a contract `state` (connections spec
/// §6.2, DEC-860 item 3): `new`, `queued`, `confirmed` and `unconfirmed` are `accepted`;
/// `partially_filled` and `filled` are themselves; `cancelled` and `voided` are `canceled`;
/// `rejected` and `failed` are `rejected`. Any other value is [`RobinhoodError::UnknownState`].
pub fn executor_status(_state: &str) -> Result<&'static str, RobinhoodError> {
    Err(RobinhoodError::Unimplemented { story: "E7-6" })
}
