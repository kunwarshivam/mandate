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

use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::InstrumentId;
use mandate_num::{Price, Qty};
use mandate_time::UtcNanos;
use secrecy::{ExposeSecret, SecretString};
use serde_json::value::RawValue;

use crate::error::WireError;
use crate::http::is_symbol_like;
use crate::read::{raw_object, token};

/// The market-data stream host. With [`crate::PAPER_HOST`] and [`crate::DATA_HOST`], one of the
/// hosts compiled in (DEC-1174 item 4).
pub const STREAM_HOST: &str = "wss://stream.data.alpaca.markets";

/// Which equity feed a stream reads (DEC-35): `iex` for paper, `sip` for live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamFeed {
    Iex,
    Sip,
}

impl StreamFeed {
    /// The feed's stream address on [`STREAM_HOST`].
    pub fn url(self) -> String {
        let feed = match self {
            Self::Iex => "iex",
            Self::Sip => "sip",
        };
        format!("{STREAM_HOST}/v2/{feed}")
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
    let frame = serde_json::json!({
        "action": "auth",
        "key": key_id.expose_secret(),
        "secret": secret.expose_secret(),
    });
    SecretString::from(frame.to_string())
}

/// The subscription frame for `channels`, every symbol checked against the equity alphabet.
///
/// # Errors
/// [`WireError::WrongType`] for a channel set that names nothing, or a symbol that is not an
/// equity's.
pub fn subscribe_frame(channels: &Channels) -> Result<String, WireError> {
    let named = channels.named();
    if named.iter().all(|(_, set)| set.is_empty()) {
        return Err(WireError::WrongType { field: "channels" });
    }
    let mut frame = String::from(r#"{"action":"subscribe""#);
    for (key, set) in named {
        let symbols = set
            .iter()
            .map(|instrument| {
                let symbol = instrument.as_str();
                if is_symbol_like(symbol) {
                    Ok(symbol)
                } else {
                    Err(WireError::WrongType { field: key })
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        let list =
            serde_json::to_string(&symbols).map_err(|_| WireError::WrongType { field: key })?;
        frame.push_str(&format!(r#","{key}":{list}"#));
    }
    frame.push('}');
    Ok(frame)
}

impl Channels {
    /// These channels without the statuses and LULD bands, the two a feed may not carry
    /// (DEC-1174 item 2): what is sent again after a 410. An instrument then has no status
    /// channel, which only paper may open without (DEC-2605 row 22).
    pub fn narrowed(&self) -> Self {
        Self {
            statuses: BTreeSet::new(),
            lulds: BTreeSet::new(),
            ..self.clone()
        }
    }

    /// Each channel with the key the vendor names it by, in the subscribe frame's order.
    fn named(&self) -> [(&'static str, &BTreeSet<InstrumentId>); 7] {
        [
            ("trades", &self.trades),
            ("quotes", &self.quotes),
            ("bars", &self.bars),
            ("updatedBars", &self.updated_bars),
            ("dailyBars", &self.daily_bars),
            ("statuses", &self.statuses),
            ("lulds", &self.lulds),
        ]
    }
}

/// What the stream's error message asks of the connection (spec §3.2, DEC-1174 item 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// A rejected credential or plan (402, 409): the connection pauses and alerts, never retried
    /// in a loop.
    Pause,
    /// Too many symbols (405): the overflow is dropped by priority and reported to the owner.
    Overflow,
    /// A channel the feed does not carry (410): the subscription is narrowed to the channels every
    /// feed carries ([`Channels::narrowed`]) and sent again, never a reconnect loop that would take
    /// the quotes down with it (DEC-1174 item 5).
    Narrow,
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
        let disposition = match code {
            402 | 409 => Disposition::Pause,
            405 => Disposition::Overflow,
            410 => Disposition::Narrow,
            _ => Disposition::Down,
        };
        Self { code, disposition }
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
    /// The message's `T`, when it could be read, so its reader can tell a missed quote from a
    /// missed status.
    pub kind: Option<String>,
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
    let raw: &RawValue = serde_json::from_str(text).map_err(|_| WireError::NotJson)?;
    let items: Vec<&RawValue> =
        serde_json::from_str(raw.get()).map_err(|_| WireError::WrongType { field: "frame" })?;
    let mut frame = Frame::default();
    for (index, item) in items.into_iter().enumerate() {
        match message(item) {
            Ok(message) => frame.messages.push(message),
            Err(reason) => frame.refused.push(Refused {
                index,
                kind: kind_of(item),
                reason,
            }),
        }
    }
    Ok(frame)
}

type Fields<'a> = BTreeMap<String, &'a RawValue>;

/// A message's `T`, when the message is an object whose `T` is text.
fn kind_of(raw: &RawValue) -> Option<String> {
    let f = raw_object(raw, "message").ok()?;
    text(&f, "T").ok()
}

fn message(raw: &RawValue) -> Result<StreamMessage, WireError> {
    let f = raw_object(raw, "message")?;
    Ok(match text(&f, "T")?.as_str() {
        "success" => match text(&f, "msg")?.as_str() {
            "connected" => StreamMessage::Connected,
            "authenticated" => StreamMessage::Authenticated,
            _ => return Err(WireError::WrongType { field: "msg" }),
        },
        "subscription" => StreamMessage::Subscribed(subscribed(&f)?),
        "error" => {
            let code = u16::try_from(integer(&f, "code")?)
                .map_err(|_| WireError::WrongType { field: "code" })?;
            StreamMessage::Error(Fault::of(code))
        }
        "q" => StreamMessage::Quote(StreamQuote {
            instrument: instrument(&f)?,
            vendor_time: time(&f, "t")?,
            bid: side(&f, "bp", "bs", "bx")?,
            ask: side(&f, "ap", "as", "ax")?,
            conditions: texts(&f, "c")?,
            tape: text(&f, "z")?,
        }),
        "t" => StreamMessage::Trade(StreamTrade {
            instrument: instrument(&f)?,
            vendor_time: time(&f, "t")?,
            id: integer(&f, "i")?,
            exchange: text(&f, "x")?,
            price: price(&f, "p")?,
            size: qty(&f, "s")?,
            conditions: texts(&f, "c")?,
            tape: text(&f, "z")?,
        }),
        "b" => StreamMessage::Bar(BarKind::Minute, bar(&f)?),
        "u" => StreamMessage::Bar(BarKind::Updated, bar(&f)?),
        "d" => StreamMessage::Bar(BarKind::Daily, bar(&f)?),
        "s" => StreamMessage::Status(StreamStatus {
            instrument: instrument(&f)?,
            vendor_time: time(&f, "t")?,
            status_code: text(&f, "sc")?,
            status_message: text(&f, "sm")?,
            reason_code: text(&f, "rc")?,
            reason_message: text(&f, "rm")?,
            tape: text(&f, "z")?,
        }),
        "l" => StreamMessage::Luld(StreamLuld {
            instrument: instrument(&f)?,
            vendor_time: time(&f, "t")?,
            up: price(&f, "u")?,
            down: price(&f, "d")?,
            indicator: text(&f, "i")?,
            tape: text(&f, "z")?,
        }),
        "c" => StreamMessage::Correction(StreamCorrection {
            instrument: instrument(&f)?,
            vendor_time: time(&f, "t")?,
            exchange: text(&f, "x")?,
            original: TradeTerms {
                id: integer(&f, "oi")?,
                price: price(&f, "op")?,
                size: qty(&f, "os")?,
                conditions: texts(&f, "oc")?,
            },
            corrected: TradeTerms {
                id: integer(&f, "ci")?,
                price: price(&f, "cp")?,
                size: qty(&f, "cs")?,
                conditions: texts(&f, "cc")?,
            },
            tape: text(&f, "z")?,
        }),
        "x" => StreamMessage::Cancel(StreamCancel {
            instrument: instrument(&f)?,
            vendor_time: time(&f, "t")?,
            id: integer(&f, "i")?,
            exchange: text(&f, "x")?,
            price: price(&f, "p")?,
            size: qty(&f, "s")?,
            action: match text(&f, "a")?.as_str() {
                "C" => CancelAction::Cancel,
                "E" => CancelAction::Error,
                _ => return Err(WireError::WrongType { field: "a" }),
            },
            tape: text(&f, "z")?,
        }),
        _ => return Err(WireError::WrongType { field: "T" }),
    })
}

/// The channels a `subscription` message lists. A channel it leaves out is empty, and the
/// corrections and cancel errors it mirrors from trades are not channels of their own.
fn subscribed(f: &Fields<'_>) -> Result<Channels, WireError> {
    let read = |key: &'static str| -> Result<BTreeSet<InstrumentId>, WireError> {
        let symbols = match f.get(key) {
            Some(_) => texts(f, key)?,
            None => Vec::new(),
        };
        symbols
            .iter()
            .map(|symbol| symbol_id(symbol, key))
            .collect()
    };
    Ok(Channels {
        quotes: read("quotes")?,
        trades: read("trades")?,
        bars: read("bars")?,
        updated_bars: read("updatedBars")?,
        daily_bars: read("dailyBars")?,
        statuses: read("statuses")?,
        lulds: read("lulds")?,
    })
}

fn bar(f: &Fields<'_>) -> Result<StreamBar, WireError> {
    Ok(StreamBar {
        instrument: instrument(f)?,
        start: time(f, "t")?,
        open: price(f, "o")?,
        high: price(f, "h")?,
        low: price(f, "l")?,
        close: price(f, "c")?,
        volume: qty(f, "v")?,
        vwap: price(f, "vw")?,
        trades: integer(f, "n")?,
    })
}

/// A quote side, or `None` for the zero price Alpaca sends for an empty side (DEC-116).
fn side(
    f: &Fields<'_>,
    price_field: &'static str,
    size_field: &'static str,
    exchange_field: &'static str,
) -> Result<Option<QuoteSide>, WireError> {
    if matches!(token(f, price_field)?.as_str(), "0" | "-0") {
        return Ok(None);
    }
    Ok(Some(QuoteSide {
        price: price(f, price_field)?,
        size: qty(f, size_field)?,
        exchange: text(f, exchange_field)?,
    }))
}

fn raw<'a>(f: &Fields<'a>, field: &'static str) -> Result<&'a RawValue, WireError> {
    f.get(field)
        .copied()
        .ok_or(WireError::MissingField { field })
}

fn text(f: &Fields<'_>, field: &'static str) -> Result<String, WireError> {
    serde_json::from_str(raw(f, field)?.get()).map_err(|_| WireError::WrongType { field })
}

fn texts(f: &Fields<'_>, field: &'static str) -> Result<Vec<String>, WireError> {
    serde_json::from_str(raw(f, field)?.get()).map_err(|_| WireError::WrongType { field })
}

fn integer(f: &Fields<'_>, field: &'static str) -> Result<u64, WireError> {
    token(f, field)?
        .parse()
        .map_err(|_| WireError::WrongType { field })
}

fn price(f: &Fields<'_>, field: &'static str) -> Result<Price, WireError> {
    Ok(Price::parse(&token(f, field)?)?)
}

fn qty(f: &Fields<'_>, field: &'static str) -> Result<Qty, WireError> {
    Ok(Qty::parse(&token(f, field)?)?)
}

fn time(f: &Fields<'_>, field: &'static str) -> Result<UtcNanos, WireError> {
    Ok(UtcNanos::parse_rfc3339(&text(f, field)?)?)
}

fn instrument(f: &Fields<'_>) -> Result<InstrumentId, WireError> {
    symbol_id(&text(f, "S")?, "S")
}

/// An equity's symbol as an instrument: the stream names each by its symbol alone.
fn symbol_id(symbol: &str, field: &'static str) -> Result<InstrumentId, WireError> {
    if !is_symbol_like(symbol) {
        return Err(WireError::WrongType { field });
    }
    InstrumentId::new(symbol).map_err(|_| WireError::WrongType { field })
}
