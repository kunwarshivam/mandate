//! The market-data stream's protocol on Alpaca's stream host (backlog E2-5, data-plane spec v0.3
//! §3.2, DEC-1174): the frames sent to authenticate and subscribe, and each received text frame
//! read into typed messages.
//!
//! The module is pure: it never opens a connection, and the connection states and the reconnect
//! are its caller's (DEC-1174 item 3). A stream frame is not an [`crate::HttpRequest`], so nothing
//! here can reach the trading host, and the one host it names is [`STREAM_HOST`], a market-data
//! host beside [`crate::DATA_HOST`] (ES-23, DEC-1174 item 4).
//!
//! Every price and size is read from the JSON number's own text through `mandate-num`, never a
//! float, and every time as UTC nanoseconds (ES-23). A zero-priced quote side is absent, not a
//! price of zero (DEC-116). A message that cannot be read exactly, or of a type this module does
//! not know, is refused and counted with its place in the frame, never completed (spec §3.1); the
//! rest of the frame is still read.

use std::collections::BTreeSet;

use mandate_accounting::InstrumentId;
use mandate_num::{Price, Qty};
use mandate_time::UtcNanos;
use secrecy::SecretString;

use crate::error::WireError;

/// The market-data stream host. With [`crate::PAPER_HOST`] and [`crate::DATA_HOST`], one of the
/// hosts compiled in (DEC-1174 item 4).
pub const STREAM_HOST: &str = "wss://stream.data.alpaca.markets";

const STORY: &str = "E2-5 slice 1";

/// Which equity feed a stream reads (DEC-35): `iex` for paper, `sip` for live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamFeed {
    Iex,
    Sip,
}

impl StreamFeed {
    /// The feed's stream address on [`STREAM_HOST`].
    pub fn url(self) -> String {
        String::new()
    }
}

/// The instruments each channel is subscribed to. Corrections and cancel errors come with a
/// trades subscription and have no channel of their own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Channels {
    pub quotes: BTreeSet<InstrumentId>,
    pub trades: BTreeSet<InstrumentId>,
    pub bars: BTreeSet<InstrumentId>,
    pub updated_bars: BTreeSet<InstrumentId>,
    pub daily_bars: BTreeSet<InstrumentId>,
    pub statuses: BTreeSet<InstrumentId>,
    pub lulds: BTreeSet<InstrumentId>,
}

/// The authentication frame. It carries the credential, so it stays secret until the transport
/// writes it, and is never logged.
pub fn auth_frame(key_id: &SecretString, secret: &SecretString) -> SecretString {
    let _ = (key_id, secret);
    SecretString::from(String::new())
}

/// The subscription frame for `channels`, every symbol checked against the equity alphabet.
///
/// # Errors
/// [`WireError::WrongType`] for a channel set that names nothing, or a symbol that is not an
/// equity's.
pub fn subscribe_frame(channels: &Channels) -> Result<String, WireError> {
    let _ = channels;
    Err(WireError::Unimplemented { story: STORY })
}

/// What the stream's error message asks of the connection (spec §3.2, DEC-1174 item 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// A rejected credential or plan (402, 409): the connection pauses and alerts, never retried
    /// in a loop.
    Pause,
    /// Too many symbols (405): the overflow is dropped by priority and reported to the owner.
    Overflow,
    /// Anything else: the connection goes `down` and reconnects with backoff.
    Down,
}

/// The stream's error message: its code and what it asks of the connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fault {
    pub code: u16,
    pub disposition: Disposition,
}

impl Fault {
    /// The fault for the vendor's `code`.
    pub fn of(code: u16) -> Self {
        Self {
            code,
            disposition: Disposition::Down,
        }
    }
}

/// One side of a quote: its price, size and exchange.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteSide {
    pub price: Price,
    pub size: Qty,
    pub exchange: String,
}

/// A quote (`q`). A side Alpaca sent at a zero price is `None` (DEC-116).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamQuote {
    pub instrument: InstrumentId,
    pub vendor_time: UtcNanos,
    pub bid: Option<QuoteSide>,
    pub ask: Option<QuoteSide>,
    pub conditions: Vec<String>,
    pub tape: String,
}

/// A trade (`t`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamTrade {
    pub instrument: InstrumentId,
    pub vendor_time: UtcNanos,
    pub id: u64,
    pub exchange: String,
    pub price: Price,
    pub size: Qty,
    pub conditions: Vec<String>,
    pub tape: String,
}

/// Which bar channel a bar came on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarKind {
    /// A minute bar (`b`).
    Minute,
    /// A minute bar revised by a late trade (`u`): a new version of the bar (spec §3.3).
    Updated,
    /// The day's bar (`d`).
    Daily,
}

/// A bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamBar {
    pub instrument: InstrumentId,
    pub start: UtcNanos,
    pub open: Price,
    pub high: Price,
    pub low: Price,
    pub close: Price,
    pub volume: Qty,
    pub vwap: Price,
    pub trades: u64,
}

/// A trading status (`s`), with the vendor's codes as sent; spec §3.5 interprets them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamStatus {
    pub instrument: InstrumentId,
    pub vendor_time: UtcNanos,
    pub status_code: String,
    pub status_message: String,
    pub reason_code: String,
    pub reason_message: String,
    pub tape: String,
}

/// A limit-up limit-down band (`l`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamLuld {
    pub instrument: InstrumentId,
    pub vendor_time: UtcNanos,
    pub up: Price,
    pub down: Price,
    pub indicator: String,
    pub tape: String,
}

/// One trade as a correction names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeTerms {
    pub id: u64,
    pub price: Price,
    pub size: Qty,
    pub conditions: Vec<String>,
}

/// A trade correction (`c`): a new record naming the original (DP-2, spec §3.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamCorrection {
    pub instrument: InstrumentId,
    pub vendor_time: UtcNanos,
    pub exchange: String,
    pub original: TradeTerms,
    pub corrected: TradeTerms,
    pub tape: String,
}

/// Whether a cancel error (`x`) cancels its trade or marks it an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelAction {
    Cancel,
    Error,
}

/// A trade cancel or error (`x`), naming the trade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamCancel {
    pub instrument: InstrumentId,
    pub vendor_time: UtcNanos,
    pub id: u64,
    pub exchange: String,
    pub price: Price,
    pub size: Qty,
    pub action: CancelAction,
    pub tape: String,
}

/// One message read from a frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamMessage {
    /// `success` `connected`: the socket is open and awaits authentication.
    Connected,
    /// `success` `authenticated`.
    Authenticated,
    /// `subscription`: the channels now subscribed, as the vendor lists them.
    Subscribed(Channels),
    Error(Fault),
    Quote(StreamQuote),
    Trade(StreamTrade),
    Bar(BarKind, StreamBar),
    Status(StreamStatus),
    Luld(StreamLuld),
    Correction(StreamCorrection),
    Cancel(StreamCancel),
}

/// A message of a frame that could not be read, by its place in the frame, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct Refused {
    pub index: usize,
    pub reason: WireError,
}

/// One received frame: the messages read, in order, and those refused.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frame {
    pub messages: Vec<StreamMessage>,
    pub refused: Vec<Refused>,
}

/// One received text frame, read message by message.
///
/// # Errors
/// [`WireError::NotJson`] for a frame that is not JSON, and [`WireError::WrongType`] for one that
/// is not an array: nothing of it is read.
pub fn read_frame(text: &str) -> Result<Frame, WireError> {
    let _ = text;
    Err(WireError::Unimplemented { story: STORY })
}
