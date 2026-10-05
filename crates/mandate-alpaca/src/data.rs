//! The latest-quote read on Alpaca's market-data host (backlog E7-8, DEC-168), and the recent
//! one-minute IEX bars of one equity (E7-7, DEC-471).
//!
//! The trading host serves no market data, so these two GETs are the only reads this crate sends
//! anywhere else, and they are kept apart by type: a [`QuoteRequest`] or a [`BarsRequest`] is not
//! an [`crate::HttpRequest`], a [`DataTransport`] is not a [`crate::TradingTransport`], and neither
//! can be handed the other's request. So no trading call can reach [`DATA_HOST`] and no data read
//! can reach the trading host (ADR-0001 ES-23 compiles in the paper trading host and the data host,
//! and no other).
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
use mandate_time::{TimeError, UtcNanos, new_york_date_and_hour};

use crate::client::{Pause, reference_status};
use crate::error::{ReadError, TransportError};
use crate::http::{Response, is_pair_half, symbol_segment};
use crate::read::{self, LatestQuote, MinuteBars, judge_age};

/// The market-data host. With [`crate::PAPER_HOST`], one of the two hosts compiled in.
pub const DATA_HOST: &str = "https://data.alpaca.markets";

/// One bar's span, and the grid every bar starts on.
const MINUTE_S: i64 = 60;
/// The widest window a bars read may ask for. One page of a whole session is not this read's
/// purpose: it serves only the trailing volume a decision is about to rest on.
pub const MAX_BARS_WINDOW: Duration = Duration::from_secs(900);

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

    /// A request on any path, for the transport's own refusal tests, which must hand it a path
    /// [`Self::latest`] would never build.
    #[cfg(test)]
    pub(crate) fn for_tests(path_and_query: &str) -> Self {
        Self {
            instrument: InstrumentId::new("AAPL").unwrap_or_else(|_| unreachable!()),
            path_and_query: path_and_query.to_owned(),
        }
    }

    /// The whole URL: the data host and this path, and nothing a caller chose.
    pub fn url(&self) -> String {
        format!("{DATA_HOST}{}", self.path_and_query)
    }
}

/// One read of the complete one-minute IEX bars of one equity inside a window that ends at a clock:
/// the instrument, the first and last bar starts it asks for, and the path and query it is sent on.
///
/// The fields are private and the only constructor is [`BarsRequest::recent_minutes`], so a
/// transport is never handed a timeframe, feed, window, page, or symbol a caller chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarsRequest {
    instrument: InstrumentId,
    first_start: UtcNanos,
    last_start: UtcNanos,
    path_and_query: String,
}

impl BarsRequest {
    /// The bars of `instrument` that start no earlier than `window` before `now` and end by `now`,
    /// read as
    /// `/v2/stocks/{symbol}/bars?timeframe=1Min&start={first}&end={last}&limit={n}&adjustment=raw&feed=iex&sort=asc`.
    ///
    /// `first` is the first whole minute at or after `now − window`, and `last` the start of the
    /// last minute that has closed by `now`, both inclusive, as Alpaca reads `start` and `end`.
    /// `limit` is the number of minutes between them, so one page holds every bar the window can.
    /// The feed is IEX, the paper equity profile (trading-domain spec §4.2), and prices are raw.
    ///
    /// Refused, with nothing built ([`TransportError::RefusedPath`]): a crypto pair or a symbol
    /// [`symbol_segment`] cannot write; a window that is not a whole number of minutes from one to
    /// [`MAX_BARS_WINDOW`]; a window holding no closed minute; and a window that starts on an
    /// earlier New York date than `now`, since the read is of today's trading only.
    pub fn recent_minutes(
        instrument: &InstrumentId,
        now: UtcNanos,
        window: Duration,
    ) -> Result<Self, TransportError> {
        if instrument.as_str().contains('/') {
            return Err(TransportError::RefusedPath);
        }
        let symbol = symbol_segment(instrument)?;
        let window_s = i64::try_from(window.as_secs()).map_err(|_| TransportError::RefusedPath)?;
        let whole_minutes = window.subsec_nanos() == 0 && window_s.checked_rem(MINUTE_S) == Some(0);
        if !whole_minutes || window.is_zero() || window > MAX_BARS_WINDOW {
            return Err(TransportError::RefusedPath);
        }
        let refused = |_: TimeError| TransportError::RefusedPath;
        let (first_start, last_start) = window_bounds(now, window_s).map_err(refused)?;
        if first_start > last_start {
            return Err(TransportError::RefusedPath);
        }
        let (first_day, _) = new_york_date_and_hour(first_start).map_err(refused)?;
        let (today, _) = new_york_date_and_hour(now).map_err(refused)?;
        if first_day != today {
            return Err(TransportError::RefusedPath);
        }
        let minutes = last_start
            .secs()
            .checked_sub(first_start.secs())
            .and_then(|span| span.checked_div(MINUTE_S))
            .and_then(|span| span.checked_add(1))
            .ok_or(TransportError::RefusedPath)?;
        let path_and_query = format!(
            "/v2/stocks/{symbol}/bars?timeframe=1Min&start={}&end={}&limit={minutes}&adjustment=raw&feed=iex&sort=asc",
            whole_second(first_start)?,
            whole_second(last_start)?,
        );
        Ok(Self {
            instrument: instrument.clone(),
            first_start,
            last_start,
            path_and_query,
        })
    }

    pub fn instrument(&self) -> &InstrumentId {
        &self.instrument
    }

    /// The earliest bar start the read accepts.
    pub fn first_start(&self) -> UtcNanos {
        self.first_start
    }

    /// The latest bar start the read accepts: that bar closes by the clock the request was built at.
    pub fn last_start(&self) -> UtcNanos {
        self.last_start
    }

    pub fn path_and_query(&self) -> &str {
        &self.path_and_query
    }

    /// A request on any path, for the transport's own refusal tests.
    #[cfg(test)]
    pub(crate) fn for_tests(path_and_query: &str) -> Self {
        Self {
            instrument: InstrumentId::new("AAPL").unwrap_or_else(|_| unreachable!()),
            first_start: UtcNanos::EPOCH,
            last_start: UtcNanos::EPOCH,
            path_and_query: path_and_query.to_owned(),
        }
    }

    /// The whole URL: the data host and this path, and nothing a caller chose.
    pub fn url(&self) -> String {
        format!("{DATA_HOST}{}", self.path_and_query)
    }
}

/// The first whole minute at or after `now − window_s`, and the start of the last minute closed by
/// `now`.
fn window_bounds(now: UtcNanos, window_s: i64) -> Result<(UtcNanos, UtcNanos), TimeError> {
    let back = window_s.checked_neg().ok_or(TimeError::OutOfRange)?;
    let one_back = MINUTE_S.checked_neg().ok_or(TimeError::OutOfRange)?;
    Ok((
        minute_at_or_after(shifted(now, back)?)?,
        shifted(minute_at_or_before(now)?, one_back)?,
    ))
}

/// `at` moved by `secs` whole seconds.
fn shifted(at: UtcNanos, secs: i64) -> Result<UtcNanos, TimeError> {
    let moved = at.secs().checked_add(secs).ok_or(TimeError::OutOfRange)?;
    UtcNanos::from_parts(moved, at.nanos())
}

/// The start of the minute `at` is in.
fn minute_at_or_before(at: UtcNanos) -> Result<UtcNanos, TimeError> {
    let past = at
        .secs()
        .checked_rem(MINUTE_S)
        .ok_or(TimeError::OutOfRange)?;
    let floor = at.secs().checked_sub(past).ok_or(TimeError::OutOfRange)?;
    UtcNanos::from_parts(floor, 0)
}

/// `at` itself when it is a whole minute, and otherwise the next whole minute.
fn minute_at_or_after(at: UtcNanos) -> Result<UtcNanos, TimeError> {
    let floor = minute_at_or_before(at)?;
    if floor == at {
        Ok(floor)
    } else {
        shifted(floor, MINUTE_S)
    }
}

/// A whole-second instant as RFC 3339 without a fraction, `2026-09-28T16:55:00Z`.
fn whole_second(at: UtcNanos) -> Result<String, TransportError> {
    at.to_string()
        .strip_suffix(".000000000Z")
        .map(|seconds| format!("{seconds}Z"))
        .ok_or(TransportError::RefusedPath)
}

/// Sends one market-data read to [`DATA_HOST`]. Every [`QuoteRequest`] is already one of the two
/// latest-quote endpoints and every [`BarsRequest`] the one bars endpoint, so an implementation has
/// nothing left to decide about what may be sent.
pub trait DataTransport {
    fn send(
        &self,
        request: &QuoteRequest,
    ) -> impl Future<Output = Result<Response, TransportError>>;

    fn send_bars(
        &self,
        request: &BarsRequest,
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
        let response = self
            .transport
            .send(&QuoteRequest::latest(instrument)?)
            .await?;
        reference_status(response.status)?;
        let quote = read::latest_quote(instrument, &response.body)?;
        judge_age(quote.at, self.pause.now(), max_age)?;
        Ok(quote)
    }

    /// The equity's complete one-minute IEX bars inside the `window` before the clock's now, as
    /// [`BarsRequest::recent_minutes`] bounds them. The answer must be one page naming the equity,
    /// and every bar must start on the minute grid, inside the window, after the bar before it, so
    /// no bar is older than `window` and none was still open when the request was built. A window
    /// with no bar is [`ReadError::Absent`]: IEX prints no bar for a minute it did not trade, and
    /// no volume is not a volume this read can vouch for.
    pub async fn recent_minute_bars(
        &self,
        instrument: &InstrumentId,
        window: Duration,
    ) -> Result<MinuteBars, ReadError> {
        let request = BarsRequest::recent_minutes(instrument, self.pause.now(), window)?;
        let response = self.transport.send_bars(&request).await?;
        reference_status(response.status)?;
        read::minute_bars(&request, &response.body)
    }
}
