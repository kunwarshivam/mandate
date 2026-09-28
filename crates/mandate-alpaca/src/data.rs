//! The latest-quote read on Alpaca's market-data host (backlog E7-8, DEC-168).
//!
//! The trading host serves no quotes, so this is the one read this crate sends anywhere else, and
//! it is kept apart by type: a [`QuoteRequest`] is not an [`crate::HttpRequest`], a
//! [`DataTransport`] is not a [`crate::TradingTransport`], and neither can be handed the other's
//! request. So no trading call can reach [`DATA_HOST`] and no quote read can reach the trading
//! host (ADR-0001 ES-23 compiles in the paper trading host and the data host, and no other).
//!
//! A quote is read on the paper data profile, `iex`, for an equity, and on the `crypto` profile
//! for a pair (trading-domain spec §4.2). What comes back is the broker's quote as exact values,
//! or a [`ReadError`]: a quote that is missing, that cannot be read, that has a side with no
//! price, that names another instrument, or that is older than the caller's bound is refused,
//! never completed or carried forward (`AGENTS.md` rule 3). Whether a quote is sane enough to mark
//! or collar with (spec §8.2) stays its reader's decision; this read only refuses what is not a
//! quote at all.

use std::future::Future;
use std::time::Duration;

use mandate_accounting::InstrumentId;

use crate::client::Pause;
use crate::error::{ReadError, TransportError};
use crate::http::{Response, is_pair_half, symbol_segment};
use crate::read::LatestQuote;

/// The market-data host. With [`crate::PAPER_HOST`], one of the two hosts compiled in.
pub const DATA_HOST: &str = "https://data.alpaca.markets";

/// One latest-quote read: the instrument it asks about and the path and query it is sent on.
///
/// The fields are private. The only constructor is [`QuoteRequest::latest`], which writes the
/// path from the instrument itself, so a transport is never handed a path a caller chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteRequest {
    instrument: InstrumentId,
    path_and_query: String,
}

impl QuoteRequest {
    /// The latest-quote read for one instrument.
    ///
    /// An equity is read as `/v2/stocks/{symbol}/quotes/latest?feed=iex`, its symbol written as
    /// [`symbol_segment`] writes it. A crypto pair is read as
    /// `/v1beta3/crypto/us/latest/quotes?symbols={base}%2F{quote}`, the pair in the query with its
    /// slash percent-encoded, because that endpoint keys its answer by the pair. A symbol neither
    /// form can write is [`TransportError::RefusedPath`] and no request exists.
    pub fn latest(instrument: &InstrumentId) -> Result<Self, TransportError> {
        let path_and_query = match instrument.as_str().split_once('/') {
            Some((base, quote)) if is_pair_half(base) && is_pair_half(quote) => {
                format!("/v1beta3/crypto/us/latest/quotes?symbols={base}%2F{quote}")
            }
            Some(_) => return Err(TransportError::RefusedPath),
            None => format!(
                "/v2/stocks/{}/quotes/latest?feed=iex",
                symbol_segment(instrument)?
            ),
        };
        Ok(Self {
            instrument: instrument.clone(),
            path_and_query,
        })
    }

    pub fn instrument(&self) -> &InstrumentId {
        &self.instrument
    }

    pub fn path_and_query(&self) -> &str {
        &self.path_and_query
    }

    /// The whole URL: the data host and this path, and nothing a caller chose.
    pub fn url(&self) -> String {
        format!("{DATA_HOST}{}", self.path_and_query)
    }
}

/// Sends one latest-quote read to [`DATA_HOST`]. Every [`QuoteRequest`] is already one of the two
/// latest-quote endpoints, so an implementation has nothing left to decide about what may be sent.
pub trait DataTransport {
    fn send(
        &self,
        request: &QuoteRequest,
    ) -> impl Future<Output = Result<Response, TransportError>>;
}

/// The latest-quote client, over an injected transport and clock, so no test touches a network
/// and no test waits (ADR-0001 ES-19).
#[derive(Debug)]
pub struct DataClient<D, P> {
    transport: D,
    pause: P,
}

impl<D: DataTransport, P: Pause> DataClient<D, P> {
    pub fn new(transport: D, pause: P) -> Self {
        Self { transport, pause }
    }

    /// The transport this client sends through, for the tests that assert what was sent.
    pub fn transport(&self) -> &D {
        &self.transport
    }

    /// The instrument's latest quote, if the broker has one, it can be read, and it is no older
    /// than `max_age` at the clock's now. `max_age` is the caller's: the data profile's staleness
    /// threshold is configuration (trading-domain spec §4.2), so this read holds none of its own.
    pub async fn latest_quote(
        &self,
        instrument: &InstrumentId,
        max_age: Duration,
    ) -> Result<LatestQuote, ReadError> {
        let _ = (&self.transport, &self.pause, instrument, max_age);
        Err(ReadError::Unimplemented { story: "E7-8" })
    }
}
