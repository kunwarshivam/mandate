//! Alpaca's historical-data endpoints: the request path for one page of one UTC day, the
//! corporate-actions request for one symbol, and their response pages, parsed with every number
//! read as raw text (ADR-0001 ES-23).

use std::collections::BTreeSet;
use std::fmt;

use mandate_canon::DecStr;
use mandate_time::{Date, TimeError, UtcNanos};
use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::value::RawValue;

use crate::model::{
    AdjustmentError, AssetClass, Bar, CashDividend, CorporateActions, DatasetId, DayRange, Kind,
    OtherAction, Records, Split, Symbol, Trade, split_ratio,
};
use crate::number::{NumberError, decimal_from_json, unsigned_from_json};
use crate::timestamp::{TimestampError, day_start, parse_rfc3339_utc};

/// Largest page Alpaca serves for bars and trades.
pub const MAX_PAGE_LIMIT: u32 = 10_000;
/// Largest page Alpaca serves for corporate actions.
pub const MAX_CORPORATE_ACTIONS_LIMIT: u32 = 1_000;
/// How many days before the first ex-date of a request its process-date window starts. Alpaca
/// filters corporate actions only by process date, and nothing it documents keeps the process
/// date from preceding the ex-date.
pub const PROCESS_DATE_DAYS_BEFORE: u32 = 31;
/// How many days after the last ex-date of a request its process-date window ends. A cash
/// dividend is processed on its payable date, weeks after its ex-date (up to 52 days in the
/// recorded GE history).
pub const PROCESS_DATE_DAYS_AFTER: u32 = 366;
const SECS_PER_DAY: i64 = 86_400;

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
    #[error("field `{field}` = {raw}: {source}")]
    Date {
        field: &'static str,
        raw: String,
        source: TimeError,
    },
    #[error("split {id}: {source}")]
    Split { id: String, source: AdjustmentError },
    #[error("the response lists corporate action {0} twice")]
    DuplicateAction(String),
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
            Self::Date { .. } => "date",
            Self::Split { .. } => "split",
            Self::DuplicateAction(_) => "duplicate_action",
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

/// One response page of corporate actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorporateActionsPage {
    pub actions: CorporateActions,
    pub next_page_token: Option<String>,
}

/// The path and query of one page of the corporate actions of `symbol` that may have an ex-date in
/// `ex_dates`. Alpaca filters and sorts on the process date only, so the request covers the process
/// dates from [`PROCESS_DATE_DAYS_BEFORE`] days before the range to [`PROCESS_DATE_DAYS_AFTER`]
/// days after it, and the caller keeps the actions dated in the range
/// ([`CorporateActions::dated_in`]).
pub fn corporate_actions_path(
    symbol: &Symbol,
    ex_dates: DayRange,
    limit: u32,
    page_token: Option<&str>,
) -> Result<String, TimestampError> {
    let start = shift_days(ex_dates.first(), -i64::from(PROCESS_DATE_DAYS_BEFORE))?;
    let end = shift_days(ex_dates.last(), i64::from(PROCESS_DATE_DAYS_AFTER))?;
    let mut path = format!(
        "/v1/corporate-actions?symbols={}&start={start}&end={end}&limit={limit}&sort=asc",
        percent_encode(symbol.as_str()),
    );
    if let Some(token) = page_token {
        path.push_str(&format!("&page_token={}", percent_encode(token)));
    }
    Ok(path)
}

fn shift_days(day: Date, days: i64) -> Result<Date, TimestampError> {
    let offset = days
        .checked_mul(SECS_PER_DAY)
        .ok_or(TimestampError::Overflow)?;
    let secs = day_start(day)?
        .secs()
        .checked_add(offset)
        .ok_or(TimestampError::Overflow)?;
    UtcNanos::from_parts(secs, 0)
        .map(UtcNanos::date)
        .map_err(TimestampError::Time)
}

/// Parses one page of corporate actions requested for `symbol`. Splits and cash dividends must
/// name `symbol`; any other kind is kept by its member name, ID, and process date, whatever role
/// `symbol` plays in it.
pub fn parse_corporate_actions(
    symbol: &Symbol,
    body: &[u8],
) -> Result<CorporateActionsPage, WireError> {
    let envelope: ActionsEnvelope<'_> = serde_json::from_slice(body).map_err(json)?;
    let raw_actions = envelope
        .corporate_actions
        .ok_or(WireError::MissingMember("corporate_actions"))?;
    let raw_token = envelope
        .next_page_token
        .ok_or(WireError::MissingMember("next_page_token"))?;
    let next_page_token: Option<String> = serde_json::from_str(raw_token.get()).map_err(json)?;
    let members = serde_json::from_str::<Entries<'_>>(raw_actions.get())
        .map_err(json)?
        .0;
    let mut actions = CorporateActions::none(symbol.clone());
    for (member, raw) in members {
        match member.as_str() {
            "forward_splits" | "reverse_splits" => {
                let wire: Vec<WireSplit<'_>> = serde_json::from_str(raw.get()).map_err(json)?;
                for w in wire {
                    expect_symbol(symbol, &w.symbol)?;
                    actions.splits.push(split(w)?);
                }
            }
            "cash_dividends" => {
                let wire: Vec<WireDividend<'_>> = serde_json::from_str(raw.get()).map_err(json)?;
                for w in wire {
                    expect_symbol(symbol, &w.symbol)?;
                    actions.cash_dividends.push(CashDividend {
                        id: w.id,
                        ex_date: date("ex_date", &w.ex_date)?,
                        record_date: optional_date("record_date", w.record_date)?,
                        payable_date: optional_date("payable_date", w.payable_date)?,
                        rate: amount("rate", w.rate)?,
                        special: w.special,
                        foreign: w.foreign,
                    });
                }
            }
            _ => {
                let wire: Vec<WireOther> = serde_json::from_str(raw.get()).map_err(json)?;
                for w in wire {
                    actions.other.push(OtherAction {
                        id: w.id,
                        kind: member.clone(),
                        ex_date: optional_date("ex_date", w.ex_date)?,
                        process_date: date("process_date", &w.process_date)?,
                    });
                }
            }
        }
    }
    let mut ids = BTreeSet::new();
    if let Some(repeated) = actions.ids().find(|id| !ids.insert(*id)) {
        return Err(WireError::DuplicateAction(repeated.to_owned()));
    }
    Ok(CorporateActionsPage {
        actions,
        next_page_token,
    })
}

fn expect_symbol(wanted: &Symbol, got: &str) -> Result<(), WireError> {
    if got == wanted.as_str() {
        Ok(())
    } else {
        Err(WireError::UnexpectedSymbol(got.to_owned()))
    }
}

fn split(w: WireSplit<'_>) -> Result<Split, WireError> {
    let new = count("new_rate", w.new_rate)?;
    let old = count("old_rate", w.old_rate)?;
    let ratio = split_ratio(new, old).map_err(|source| WireError::Split {
        id: w.id.clone(),
        source,
    })?;
    Ok(Split {
        ex_date: date("ex_date", &w.ex_date)?,
        id: w.id,
        ratio,
    })
}

fn date(field: &'static str, raw: &str) -> Result<Date, WireError> {
    Date::parse(raw).map_err(|source| WireError::Date {
        field,
        raw: raw.to_owned(),
        source,
    })
}

fn optional_date(field: &'static str, raw: Option<String>) -> Result<Option<Date>, WireError> {
    raw.map(|text| date(field, &text)).transpose()
}

#[derive(Deserialize)]
struct ActionsEnvelope<'a> {
    #[serde(borrow, default, deserialize_with = "present")]
    corporate_actions: Option<&'a RawValue>,
    #[serde(borrow, default, deserialize_with = "present")]
    next_page_token: Option<&'a RawValue>,
}

#[derive(Deserialize)]
struct WireSplit<'a> {
    id: String,
    symbol: String,
    ex_date: String,
    #[serde(borrow)]
    new_rate: &'a RawValue,
    #[serde(borrow)]
    old_rate: &'a RawValue,
}

#[derive(Deserialize)]
struct WireDividend<'a> {
    id: String,
    symbol: String,
    ex_date: String,
    record_date: Option<String>,
    payable_date: Option<String>,
    #[serde(borrow)]
    rate: &'a RawValue,
    special: bool,
    foreign: bool,
}

#[derive(Deserialize)]
struct WireOther {
    id: String,
    ex_date: Option<String>,
    process_date: String,
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
