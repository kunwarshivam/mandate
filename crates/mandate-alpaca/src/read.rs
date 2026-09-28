//! The instrument snapshot and the latest quote as exact values (backlog E7-8, DEC-168).
//!
//! Both come from the broker: the instrument from the trading host's asset record
//! (trading-domain spec §3.1) and the quote from the data host (§4.1, [`crate::data`]). Each is a
//! fact or a [`ReadError`], never a guess: a field that is missing or cannot be read, an answer
//! about another instrument, and an answer older than the caller's bound are refused
//! (`AGENTS.md` rule 3). Numbers never pass through a float (ADR-0001 ES-23).

use std::time::Duration;

use mandate_accounting::{AssetClass, InstrumentId};
use mandate_num::{Price, Qty};
use mandate_time::UtcNanos;

use crate::error::ReadError;

/// The listing exchange, as trading-domain spec §3.1 names Alpaca's codes. `CRYPTO` is the code
/// Alpaca gives every pair; any code the spec does not name is [`Exchange::Other`], which the
/// eligibility floor treats as ineligible (§3.2 item 2), so an unknown code fails closed there
/// rather than here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exchange {
    Nasdaq,
    Nyse,
    Arca,
    Amex,
    Bats,
    Otc,
    Crypto,
    Other,
}

/// One instrument as the broker's asset record describes it: the §3.1 fields the risk gate, the
/// eligibility floor, and the executor read. The fields §3.1 marks "loaded; unused in v1"
/// (`marginable`, `shortable`, `easy_to_borrow`, and the overnight and extended-hours flags) are
/// not read (DEC-168).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    /// The broker's asset id, a UUID.
    pub asset_id: String,
    pub instrument: InstrumentId,
    pub class: AssetClass,
    pub exchange: Exchange,
    /// `status` is `active`.
    pub active: bool,
    pub tradable: bool,
    pub fractionable: bool,
    /// The `ipo` attribute: limit orders only until the first trade (§3.2 item 3).
    pub ipo: bool,
    /// The `ptp_no_exception` attribute (§3.2 item 3).
    pub ptp_no_exception: bool,
    /// The order constraints of §3.1. Alpaca sends them for a crypto pair and not for an equity.
    pub min_order_size: Option<Qty>,
    pub min_trade_increment: Option<Qty>,
    pub price_increment: Option<Price>,
}

/// An [`Asset`] and the instant it was read, which is what its age is measured from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetSnapshot {
    pub asset: Asset,
    pub loaded_at: UtcNanos,
}

impl AssetSnapshot {
    /// The asset, if it was read no more than `max_age` before `now`. An older snapshot is
    /// [`ReadError::Stale`], and one read after `now` is [`ReadError::AheadOfClock`]. `max_age`
    /// is the caller's: spec §3.1's daily refresh is its schedule, not this read's.
    pub fn current(&self, now: UtcNanos, max_age: Duration) -> Result<&Asset, ReadError> {
        let _ = (now, max_age);
        Err(ReadError::Unimplemented { story: "E7-8" })
    }
}

/// The data profile a quote was read on (trading-domain spec §4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feed {
    /// IEX, the paper equity profile.
    Iex,
    /// Alpaca's crypto feed.
    Crypto,
}

/// One instrument's latest quote (trading-domain spec §4.1), exactly as the broker sent it: both
/// sides priced, kept even when locked or crossed, which is its reader's to judge (§8.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatestQuote {
    pub instrument: InstrumentId,
    pub at: UtcNanos,
    pub bid: Price,
    pub bid_size: Qty,
    pub ask: Price,
    pub ask_size: Qty,
    pub feed: Feed,
}

/// Parses the body of `GET /v2/assets/{symbol}` for `instrument`.
pub fn asset(instrument: &InstrumentId, body: &[u8]) -> Result<Asset, ReadError> {
    let _ = (instrument, body);
    Err(ReadError::Unimplemented { story: "E7-8" })
}

/// Parses the body of a latest-quote read for `instrument`. Its age is not judged here:
/// [`crate::DataClient::latest_quote`] judges it against the clock.
pub fn latest_quote(instrument: &InstrumentId, body: &[u8]) -> Result<LatestQuote, ReadError> {
    let _ = (instrument, body);
    Err(ReadError::Unimplemented { story: "E7-8" })
}
