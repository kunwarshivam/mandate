//! Alpaca's historical-data endpoints: the request path for one page of one UTC day, and the
//! response page, parsed with every number read as raw text (ADR-0001 ES-23).

use std::fmt;

use mandate_canon::DecStr;
use mandate_time::{Date, UtcNanos};
use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::value::RawValue;

use crate::model::{AssetClass, Bar, DatasetId, Kind, Records, Trade};
use crate::number::{NumberError, decimal_from_json, unsigned_from_json};
use crate::timestamp::{TimestampError, day_start, parse_rfc3339_utc};

/// Largest page Alpaca serves for bars and trades.
pub const MAX_PAGE_LIMIT: u32 = 10_000;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WireError {
    #[error("malformed response: {0}")]
    Json(String),
    #[error("the response has no `{0}` member")]
    MissingMember(&'static str),
    #[error("the response lists symbol {0} twice")]
    DuplicateSymbol(String),
    #[error("the response holds data for {0}, which was not requested")]
    UnexpectedSymbol(String),
    #[error("field `{field}` = {raw}: {source}")]
    Number {
        field: &'static str,
        raw: String,
        source: NumberError,
    },
    #[error("field `{field}` is negative: {raw}")]
    Negative { field: &'static str, raw: String },
    #[error("timestamp {raw}: {source}")]
    Timestamp { raw: String, source: TimestampError },
    #[error("record at {time} is outside the requested day {day}")]
    OutsideDay { time: UtcNanos, day: Date },
}

impl WireError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Json(_) => "json",
            Self::MissingMember(_) => "missing_member",
            Self::DuplicateSymbol(_) => "duplicate_symbol",
            Self::UnexpectedSymbol(_) => "unexpected_symbol",
            Self::Number { .. } => "number",
            Self::Negative { .. } => "negative",
            Self::Timestamp { .. } => "timestamp",
            Self::OutsideDay { .. } => "outside_day",
        }
    }
}

/// One response page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub records: Records,
    pub next_page_token: Option<String>,
}

/// The path and query of one page of `dataset` for the UTC day `day`, sorted ascending.
pub fn page_path(
    dataset: &DatasetId,
    day: Date,
    limit: u32,
    page_token: Option<&str>,
) -> Result<String, TimestampError> {
    let start = day;
    let end = day.next().map_err(TimestampError::Time)?;
    let endpoint = match (dataset.asset_class(), dataset.kind()) {
        (AssetClass::UsEquity, Kind::Bars(_)) => "/v2/stocks/bars",
        (AssetClass::UsEquity, Kind::Trades) => "/v2/stocks/trades",
        (AssetClass::Crypto, Kind::Bars(_)) => "/v1beta3/crypto/us/bars",
        (AssetClass::Crypto, Kind::Trades) => "/v1beta3/crypto/us/trades",
    };
    let mut path = format!(
        "{endpoint}?symbols={}",
        percent_encode(dataset.symbol().as_str())
    );
    if let Kind::Bars(timeframe) = dataset.kind() {
        path.push_str(&format!("&timeframe={timeframe}"));
    }
    path.push_str(&format!(
        "&start={start}T00:00:00Z&end={end}T00:00:00Z&limit={limit}"
    ));
    match (dataset.asset_class(), dataset.kind()) {
        (AssetClass::UsEquity, Kind::Bars(_)) => {
            path.push_str(&format!("&feed={}&adjustment=raw", dataset.feed()));
        }
        (AssetClass::UsEquity, Kind::Trades) => {
            path.push_str(&format!("&feed={}", dataset.feed()));
        }
        (AssetClass::Crypto, _) => {}
    }
    path.push_str("&sort=asc");
    if let Some(token) = page_token {
        path.push_str(&format!("&page_token={}", percent_encode(token)));
    }
    Ok(path)
}

/// Parses one page of `dataset` for `day`. Records stamped at the next midnight belong to the
/// next day and are dropped; any other record outside the day is an error.
pub fn parse_page(dataset: &DatasetId, day: Date, body: &[u8]) -> Result<Page, WireError> {
    let envelope: Envelope<'_> = serde_json::from_slice(body).map_err(json)?;
    let (member, raw_map) = match dataset.kind() {
        Kind::Bars(_) => ("bars", envelope.bars),
        Kind::Trades => ("trades", envelope.trades),
    };
    let raw_map = raw_map.ok_or(WireError::MissingMember(member))?;
    let raw_token = envelope
        .next_page_token
        .ok_or(WireError::MissingMember("next_page_token"))?;
    let next_page_token: Option<String> = serde_json::from_str(raw_token.get()).map_err(json)?;
    let window = Window::of(day)?;
    let entries = if raw_map.get() == "null" {
        Vec::new()
    } else {
        serde_json::from_str::<Entries<'_>>(raw_map.get())
            .map_err(json)?
            .0
    };
    let wanted = dataset.symbol().as_str();
    let mut records = None;
    for (symbol, raw) in entries {
        if symbol != wanted {
            return Err(WireError::UnexpectedSymbol(symbol));
        }
        if records.is_some() {
            return Err(WireError::DuplicateSymbol(symbol));
        }
        records = Some(raw);
    }
    let records = match records {
        None => Records::empty(dataset.kind()),
        Some(raw) => match dataset.kind() {
            Kind::Bars(_) => Records::Bars(bars(raw, &window)?),
            Kind::Trades => Records::Trades(trades(raw, &window)?),
        },
    };
    Ok(Page {
        records,
        next_page_token,
    })
}

fn json(e: serde_json::Error) -> WireError {
    WireError::Json(e.to_string())
}

fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[derive(Deserialize)]
struct Envelope<'a> {
    #[serde(borrow, default, deserialize_with = "present")]
    bars: Option<&'a RawValue>,
    #[serde(borrow, default, deserialize_with = "present")]
    trades: Option<&'a RawValue>,
    #[serde(borrow, default, deserialize_with = "present")]
    next_page_token: Option<&'a RawValue>,
}

fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<&'de RawValue>, D::Error> {
    <&RawValue>::deserialize(d).map(Some)
}

struct Entries<'a>(Vec<(String, &'a RawValue)>);

impl<'de> Deserialize<'de> for Entries<'de> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_map(EntriesVisitor)
    }
}

struct EntriesVisitor;

impl<'de> Visitor<'de> for EntriesVisitor {
    type Value = Entries<'de>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an object keyed by symbol")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut entries = Vec::new();
        while let Some(entry) = map.next_entry::<String, &'de RawValue>()? {
            entries.push(entry);
        }
        Ok(Entries(entries))
    }
}

struct Window {
    day: Date,
    start: UtcNanos,
    end: UtcNanos,
}

impl Window {
    fn of(day: Date) -> Result<Self, WireError> {
        let bad_day = |source| WireError::Timestamp {
            raw: day.to_string(),
            source,
        };
        let start = day_start(day).map_err(bad_day)?;
        let end = day
            .next()
            .map_err(TimestampError::Time)
            .and_then(day_start)
            .map_err(bad_day)?;
        Ok(Self { day, start, end })
    }

    /// The record's time, and whether it belongs to this day rather than starting the next.
    fn place(&self, raw: &str) -> Result<(UtcNanos, bool), WireError> {
        let time = parse_rfc3339_utc(raw).map_err(|source| WireError::Timestamp {
            raw: raw.to_owned(),
            source,
        })?;
        if time < self.start || time > self.end {
            return Err(WireError::OutsideDay {
                time,
                day: self.day,
            });
        }
        Ok((time, time < self.end))
    }
}

#[derive(Deserialize)]
struct WireBar<'a> {
    t: String,
    #[serde(borrow)]
    o: &'a RawValue,
    #[serde(borrow)]
    h: &'a RawValue,
    #[serde(borrow)]
    l: &'a RawValue,
    #[serde(borrow)]
    c: &'a RawValue,
    #[serde(borrow)]
    v: &'a RawValue,
    #[serde(borrow)]
    vw: &'a RawValue,
    #[serde(borrow)]
    n: &'a RawValue,
}

#[derive(Deserialize)]
struct WireTrade<'a> {
    t: String,
    #[serde(borrow)]
    p: &'a RawValue,
    #[serde(borrow)]
    s: &'a RawValue,
    #[serde(borrow)]
    i: &'a RawValue,
    x: Option<String>,
    c: Option<Vec<String>>,
    z: Option<String>,
    tks: Option<String>,
}

fn bars(raw: &RawValue, window: &Window) -> Result<Vec<Bar>, WireError> {
    let wire: Vec<WireBar<'_>> = serde_json::from_str(raw.get()).map_err(json)?;
    let mut out = Vec::with_capacity(wire.len());
    for w in wire {
        let (start, in_day) = window.place(&w.t)?;
        let bar = Bar {
            start,
            open: amount("o", w.o)?,
            high: amount("h", w.h)?,
            low: amount("l", w.l)?,
            close: amount("c", w.c)?,
            volume: amount("v", w.v)?,
            vwap: amount("vw", w.vw)?,
            trade_count: count("n", w.n)?,
        };
        if in_day {
            out.push(bar);
        }
    }
    Ok(out)
}

fn trades(raw: &RawValue, window: &Window) -> Result<Vec<Trade>, WireError> {
    let wire: Vec<WireTrade<'_>> = serde_json::from_str(raw.get()).map_err(json)?;
    let mut out = Vec::with_capacity(wire.len());
    for w in wire {
        let (time, in_day) = window.place(&w.t)?;
        let trade = Trade {
            time,
            price: amount("p", w.p)?,
            size: amount("s", w.s)?,
            trade_id: count("i", w.i)?,
            exchange: w.x,
            conditions: w.c,
            tape: w.z,
            taker_side: w.tks,
        };
        if in_day {
            out.push(trade);
        }
    }
    Ok(out)
}

fn amount(field: &'static str, raw: &RawValue) -> Result<DecStr, WireError> {
    let text = raw.get();
    let value = decimal_from_json(text).map_err(|source| WireError::Number {
        field,
        raw: text.to_owned(),
        source,
    })?;
    if value.as_str().starts_with('-') {
        return Err(WireError::Negative {
            field,
            raw: text.to_owned(),
        });
    }
    Ok(value)
}

fn count(field: &'static str, raw: &RawValue) -> Result<u64, WireError> {
    let text = raw.get();
    unsigned_from_json(text).map_err(|source| WireError::Number {
        field,
        raw: text.to_owned(),
        source,
    })
}
