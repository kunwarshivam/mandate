//! The fixture paper broker (DEC-2007 item 1): Alpaca's paper trading host answered in process,
//! below `mandate-alpaca`'s client, with the bodies that host sends, so every rule the client holds
//! applies to it unchanged. It names no host, holds no credential, and opens no socket: the
//! executor's broker in the dev and CI cells is this fixture, never a broker host (E21-30).
//!
//! What it answers, and nothing more:
//! - `GET /v2/account`: an active paper account with the fixture's cash, and no account number;
//! - `GET /v2/positions` and `GET /v2/account/activities`: what its orders filled, which is nothing
//!   in this slice, since no order fills here (fills come with the order slice, X1b);
//! - `POST /v2/orders`: an order accepted and resting, or `422` for a `client_order_id` it already
//!   holds, as Alpaca answers;
//! - `GET /v2/orders` (open orders, legs nested), `GET /v2/orders:by_client_order_id` (`404` for
//!   one it never took), and `DELETE /v2/orders/{id}` (`204`, the order then `canceled`).
//!
//! Every other request is `404`. Ids are a counter in UUID form, so a run is deterministic.

use mandate_alpaca::{HttpRequest, Response, TradingTransport, TransportError};
use mandate_time::UtcNanos;

/// The fixture account's cash, equity and buying power: a paper account of 100,000 dollars.
pub const CASH: &str = "100000";

/// The fixture broker. Clones share one book, as a broker shared by every request is.
#[derive(Debug, Clone)]
pub struct FixturePaper {
    at: String,
}

impl FixturePaper {
    /// A broker with an empty book whose records are stamped `at`.
    pub fn new(at: UtcNanos) -> Self {
        Self {
            at: at.to_string(),
        }
    }
}

impl TradingTransport for FixturePaper {
    /// The stub the tests-first commit runs against: every request fails until the
    /// implementation commit answers it (DEC-2007 item 1).
    async fn send(&self, _request: &HttpRequest) -> Result<Response, TransportError> {
        let _ = &self.at;
        Err(TransportError::Request)
    }
}
