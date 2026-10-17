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

use std::fs;
use std::path::Path;
use std::rc::Rc;

use mandate_accounting::InstrumentId;
use mandate_alpaca::{
    BarsRequest, DataTransport, HttpRequest, Pause, QuoteRequest, Response, TokioPause,
    TradingTransport, TransportError,
};
use mandate_canon::{Key, Object, Value, to_canonical};
use mandate_num::{Price, Qty};
use mandate_time::UtcNanos;

/// The variable the rehearsal build reads the fixture directory from (DEC-961 item 1).
pub const MARKET_FIXTURE_VAR: &str = "MANDATE_RH_SIM_MARKET";

/// The fixture's one file in that directory.
pub const MARKET_FILE: &str = "market.json";

/// Why a fixture directory gives no market source; each is a refusal before anything is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FixtureRefusal {
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
    /// The market `dir`'s [`MARKET_FILE`] scripts, its quote stamped by `clock`: a strict JSON
    /// object whose members are each text, `symbol` an instrument id, `asset_id` and `exchange`
    /// not empty, `bid` and `ask` prices and `bid_size`, `ask_size` and `minute_volume`
    /// quantities, in canonical decimal text.
    ///
    /// # Errors
    /// [`FixtureRefusal::Unreadable`] for a file that is missing or cannot be read so.
    pub fn load(dir: &Path, clock: C) -> Result<Self, FixtureRefusal> {
        let bytes = fs::read(dir.join(MARKET_FILE)).map_err(|_| FixtureRefusal::Unreadable)?;
        let fixture = mandate_canon::parse(&bytes).map_err(|_| FixtureRefusal::Unreadable)?;
        let text = |name: &str| {
            fixture
                .get(name)
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
                .ok_or(FixtureRefusal::Unreadable)
        };
        let price = |name: &str| Price::parse(text(name)?).map_err(|_| FixtureRefusal::Unreadable);
        let qty = |name: &str| Qty::parse(text(name)?).map_err(|_| FixtureRefusal::Unreadable);
        let symbol = InstrumentId::new(text("symbol")?).map_err(|_| FixtureRefusal::Unreadable)?;
        let asset = asset_body(&symbol, text("asset_id")?, text("exchange")?);
        Ok(Self {
            asset: Rc::new(asset),
            bid: price("bid")?,
            ask: price("ask")?,
            bid_size: qty("bid_size")?,
            ask_size: qty("ask_size")?,
            minute_volume: qty("minute_volume")?,
            symbol,
            clock,
        })
    }

    /// The latest-quote body Alpaca's data host sends for the fixture's symbol, stamped `at`.
    fn quote_body(&self, at: UtcNanos) -> Vec<u8> {
        let Self {
            symbol,
            bid,
            ask,
            bid_size,
            ask_size,
            ..
        } = self;
        format!(
            r#"{{"quote":{{"ap":{ask},"as":{ask_size},"ax":"V","bp":{bid},"bs":{bid_size},"bx":"V","c":["R"],"t":"{at}","z":"C"}},"symbol":"{symbol}"}}"#,
            symbol = symbol.as_str(),
        )
        .into_bytes()
    }

    /// The bars body Alpaca's data host sends for `request`: one bar for every minute from its
    /// first start to its last, each at the bid with the fixture's volume, on one page.
    fn bars_body(&self, request: &BarsRequest) -> Result<Vec<u8>, TransportError> {
        let (first, last) = (request.first_start().secs(), request.last_start().secs());
        let starts = (first..=last).step_by(MINUTE_S);
        let bars: Vec<String> = starts
            .map(|secs| {
                let start = UtcNanos::from_parts(secs, 0).map_err(|_| TransportError::Request)?;
                let stamp = whole_second(start)?;
                Ok(format!(
                    r#"{{"c":{p},"h":{p},"l":{p},"n":100,"o":{p},"t":"{stamp}","v":{v},"vw":{p}}}"#,
                    p = self.bid,
                    v = self.minute_volume,
                ))
            })
            .collect::<Result<_, TransportError>>()?;
        Ok(format!(
            r#"{{"bars":[{}],"next_page_token":null,"symbol":"{}"}}"#,
            bars.join(","),
            self.symbol.as_str()
        )
        .into_bytes())
    }
}

/// One bar's span in seconds, the grid every bar starts on.
const MINUTE_S: usize = 60;

/// A whole-second instant as Alpaca writes a bar's start, `2026-10-19T13:55:00Z`.
fn whole_second(at: UtcNanos) -> Result<String, TransportError> {
    at.to_string()
        .strip_suffix(".000000000Z")
        .map(|seconds| format!("{seconds}Z"))
        .ok_or(TransportError::Request)
}

/// The asset record Alpaca's trading host sends for an active, tradable, whole-share US equity
/// listed on `exchange`, with no attribute (so neither `ipo` nor `ptp_no_exception`).
fn asset_body(symbol: &InstrumentId, asset_id: &str, exchange: &str) -> Vec<u8> {
    let symbol = symbol.as_str();
    let fields = [
        ("class", "us_equity"),
        ("exchange", exchange),
        ("id", asset_id),
        ("status", "active"),
        ("symbol", symbol),
    ];
    let mut object = Object::new();
    for (name, value) in fields {
        if let Ok(key) = Key::new(name) {
            object.insert(key, Value::Str(value.to_owned()));
        }
    }
    for (name, value) in [("fractionable", false), ("tradable", true)] {
        if let Ok(key) = Key::new(name) {
            object.insert(key, Value::Bool(value));
        }
    }
    if let Ok(key) = Key::new("attributes") {
        object.insert(key, Value::Array(Vec::new()));
    }
    to_canonical(&Value::Object(object))
}

/// A `200` answer carrying `body`.
fn answered(body: Vec<u8>) -> Response {
    Response { status: 200, body }
}

impl<C: Pause> TradingTransport for SimMarket<C> {
    /// The fixture's asset record for a `GET /v2/assets/{symbol}` of its own symbol; every other
    /// request is refused with nothing answered.
    async fn send(&self, request: &HttpRequest) -> Result<Response, TransportError> {
        if *request != HttpRequest::asset(&self.symbol)? {
            return Err(TransportError::RefusedPath);
        }
        Ok(answered(self.asset.as_ref().clone()))
    }
}

impl<C: Pause> DataTransport for SimMarket<C> {
    /// The fixture's quote of its own symbol, stamped at the clock's now.
    async fn send(&self, request: &QuoteRequest) -> Result<Response, TransportError> {
        if *request != QuoteRequest::latest(&self.symbol)? {
            return Err(TransportError::RefusedPath);
        }
        Ok(answered(self.quote_body(self.clock.now())))
    }

    /// One bar for every minute the request's window holds, of the fixture's own symbol.
    async fn send_bars(&self, request: &BarsRequest) -> Result<Response, TransportError> {
        if *request.instrument() != self.symbol {
            return Err(TransportError::RefusedPath);
        }
        Ok(answered(self.bars_body(request)?))
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
