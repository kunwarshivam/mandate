//! The rehearsal's market source (DEC-961): the Alpaca market reads a Robinhood preflight takes
//! (DEC-933 item 2: the asset record, the latest IEX quote and the trailing window's IEX minute
//! bars), answered in process from a fixture directory instead of Alpaca's hosts, so the
//! rehearsal against `mandate-rh-sim` runs with no market-data key (`AGENTS.md` rules 7 and 8).
//!
//! Only the `rh-sim` build compiles it (DEC-922 item 1), and its tests; the `live` build without
//! `rh-sim` holds neither this module nor [`MARKET_FIXTURE_VAR`] (`tests/market_fixture.rs`).
//!
//! It answers below `mandate-alpaca`'s clients, with the bodies Alpaca's data and trading hosts
//! send, so every rule the clients and the shell hold a read to applies to it unchanged: the
//! quote's age bound and its clock skew (DEC-936), the symbol, the feed, the bars' window and
//! grid, and the cross-check with Robinhood's ask within the collar (DEC-529 item 12, DEC-945).
//! The quote is stamped with the fixture's clock as it is read, as a live feed stamps it; the
//! bars are the complete minutes the request's window asks for, each with the fixture's volume.

use std::path::Path;
use std::rc::Rc;

use mandate_accounting::InstrumentId;
use mandate_alpaca::{
    BarsRequest, DataTransport, HttpRequest, Pause, QuoteRequest, Response, TokioPause,
    TradingTransport, TransportError,
};
use mandate_num::{Price, Qty};

/// The variable the rehearsal build reads the fixture directory from (DEC-961 item 1).
pub const MARKET_FIXTURE_VAR: &str = "MANDATE_RH_SIM_MARKET";

/// The fixture's one file in that directory.
pub const MARKET_FILE: &str = "market.json";

/// Why a fixture directory gives no market source; each is a refusal before anything is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FixtureRefusal {
    /// No directory was named.
    #[error("{MARKET_FIXTURE_VAR} is not set")]
    Unset,
    /// The file is missing, is not strict JSON, or a member is missing or not what it must be.
    #[error("the market fixture cannot be read")]
    Unreadable,
}

/// One instrument's market, as the fixture scripts it.
#[derive(Debug, Clone)]
pub struct SimMarket<C = TokioPause> {
    symbol: InstrumentId,
    asset: Rc<Vec<u8>>,
    bid: Price,
    ask: Price,
    bid_size: Qty,
    ask_size: Qty,
    minute_volume: Qty,
    clock: C,
}

impl<C: Pause> SimMarket<C> {
    /// The market `dir`'s [`MARKET_FILE`] scripts, its quote stamped by `clock`.
    ///
    /// # Errors
    /// [`FixtureRefusal::Unreadable`] for a file that is missing or cannot be read.
    pub fn load(dir: &Path, clock: C) -> Result<Self, FixtureRefusal> {
        let _ = (dir, clock);
        Err(FixtureRefusal::Unreadable)
    }
}

impl<C: Pause> TradingTransport for SimMarket<C> {
    /// The fixture's asset record for a `GET /v2/assets/{symbol}` of its own symbol; every other
    /// request is refused with nothing answered.
    async fn send(&self, request: &HttpRequest) -> Result<Response, TransportError> {
        let _ = request;
        Err(TransportError::RefusedPath)
    }
}

impl<C: Pause> DataTransport for SimMarket<C> {
    /// The fixture's quote of its own symbol, stamped at the clock's now.
    async fn send(&self, request: &QuoteRequest) -> Result<Response, TransportError> {
        let _ = request;
        Err(TransportError::RefusedPath)
    }

    /// One bar for every minute the request's window holds, of the fixture's own symbol.
    async fn send_bars(&self, request: &BarsRequest) -> Result<Response, TransportError> {
        let _ = request;
        Err(TransportError::RefusedPath)
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test fixtures: a fixture that does not build fails the test"
)]
mod tests;
