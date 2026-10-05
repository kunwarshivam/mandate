//! The instrument snapshot and the latest quote as exact values (backlog E7-8, DEC-168).
//!
//! Both come from the broker: the instrument from the trading host's asset record
//! (trading-domain spec §3.1) and the quote from the data host (§4.1, [`crate::data`]). Each is a
//! fact or a [`ReadError`], never a guess: a field that is missing or cannot be read, an answer
//! about another instrument, and an answer older than the caller's bound are refused
//! (`AGENTS.md` rule 3). Numbers never pass through a float (ADR-0001 ES-23).

use std::collections::BTreeMap;
use std::time::Duration;

use mandate_accounting::{AssetClass, InstrumentId};
use mandate_num::{Price, Qty};
use mandate_time::UtcNanos;
use serde_json::value::RawValue;
use serde_json::{Map, Value};

use crate::data::BarsRequest;
use crate::error::{ReadError, WireError};
use crate::wire;

/// The listing exchange, as trading-domain spec §3.1 names Alpaca's codes. `CRYPTO` is the code
/// Alpaca gives every pair; any code the spec does not name is [`Exchange::Other`]. Only
/// [`Exchange::Nasdaq`], [`Exchange::Nyse`], [`Exchange::Arca`], [`Exchange::Amex`] and
/// [`Exchange::Bats`] are eligible for an equity opening (§3.2 item 2): [`Exchange::Otc`] and
/// [`Exchange::Other`] are both ineligible, so a floor reading this must admit the five by name
/// rather than refuse `Other` alone, and an unknown code fails closed there rather than here
/// (#277 review, minor 2).
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
        judge_age(self.loaded_at, now, max_age)?;
        Ok(&self.asset)
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

/// One complete one-minute bar's start and volume, the two fields the trailing volume reads. The
/// volume is the JSON number's own digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MinuteBar {
    pub start: UtcNanos,
    pub volume: Qty,
}

/// One equity's complete one-minute IEX bars inside a [`BarsRequest`]'s window, in start
/// order, at least one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinuteBars {
    pub instrument: InstrumentId,
    pub bars: Vec<MinuteBar>,
}

/// Parses the body of a bars read for `request`: `{"bars": [...], "next_page_token": null,
/// "symbol": ...}`.
///
/// Refused: another symbol ([`ReadError::OtherInstrument`]); a `next_page_token` that is not `null`
/// ([`ReadError::Paginated`]), since a second page would be bars this read never judged; no bar, or
/// `null` ([`ReadError::Absent`]); a bar off the minute grid, outside the request's first and last
/// starts, or not after the bar before it ([`ReadError::OutOfWindow`]); and a stamp or volume that
/// is not exact text (`WireError`). Prices are not read.
pub fn minute_bars(request: &BarsRequest, body: &[u8]) -> Result<MinuteBars, ReadError> {
    let top = raw_object(raw_json(body)?, "bars")?;
    let symbol = top
        .get("symbol")
        .ok_or(WireError::MissingField { field: "symbol" })?;
    let symbol: String =
        serde_json::from_str(symbol.get()).map_err(|_| WireError::WrongType { field: "symbol" })?;
    if symbol != request.instrument().as_str() {
        return Err(ReadError::OtherInstrument);
    }
    let page = top.get("next_page_token").ok_or(WireError::MissingField {
        field: "next_page_token",
    })?;
    if page.get() != "null" {
        return Err(ReadError::Paginated);
    }
    let listed = top
        .get("bars")
        .ok_or(WireError::MissingField { field: "bars" })?;
    let listed: Option<Vec<&RawValue>> =
        serde_json::from_str(listed.get()).map_err(|_| WireError::WrongType { field: "bars" })?;
    let listed = listed
        .filter(|bars| !bars.is_empty())
        .ok_or(ReadError::Absent)?;
    let mut bars: Vec<MinuteBar> = Vec::with_capacity(listed.len());
    for bar in listed {
        let fields = raw_object(bar, "bars")?;
        let start = stamp(&fields, "t")?;
        let on_grid = start.nanos() == 0 && start.secs().checked_rem(MINUTE_S) == Some(0);
        let after_previous = bars.last().is_none_or(|previous| previous.start < start);
        if !on_grid
            || !after_previous
            || start < request.first_start()
            || start > request.last_start()
        {
            return Err(ReadError::OutOfWindow);
        }
        bars.push(MinuteBar {
            start,
            volume: Qty::parse(&token(&fields, "v")?).map_err(WireError::from)?,
        });
    }
    Ok(MinuteBars {
        instrument: request.instrument().clone(),
        bars,
    })
}

/// A bar's span in seconds, and the grid its start sits on.
const MINUTE_S: i64 = 60;

/// One RFC 3339 stamp field.
fn stamp(fields: &BTreeMap<String, &RawValue>, field: &'static str) -> Result<UtcNanos, WireError> {
    let text = fields.get(field).ok_or(WireError::MissingField { field })?;
    let text: String =
        serde_json::from_str(text.get()).map_err(|_| WireError::WrongType { field })?;
    Ok(UtcNanos::parse_rfc3339(&text)?)
}

/// Parses the body of `GET /v2/assets/{symbol}` for `instrument`.
///
/// The record must name `instrument` itself; an equity's `attributes` array is required, because
/// its absence cannot say "not an IPO"; and a pair's three order constraints are required, as
/// strings under `wire`'s decimal rules (DEC-168 item 6).
pub fn asset(instrument: &InstrumentId, body: &[u8]) -> Result<Asset, ReadError> {
    let value = wire::json(body)?;
    let fields = wire::object(&value, "asset")?;
    if &wire::instrument(fields)? != instrument {
        return Err(ReadError::OtherInstrument);
    }
    let asset_id = wire::broker_id(wire::text(fields, "id")?, "id")?;
    let class = match wire::text(fields, "class")? {
        "us_equity" => AssetClass::UsEquity,
        "crypto" => AssetClass::Crypto,
        _ => return Err(WireError::WrongType { field: "class" }.into()),
    };
    let active = match wire::text(fields, "status")? {
        "active" => true,
        "inactive" => false,
        _ => return Err(WireError::WrongType { field: "status" }.into()),
    };
    let attributes = attributes(fields, class)?;
    let has = |name: &str| attributes.contains(&name);
    let (min_order_size, min_trade_increment, price_increment) = match class {
        AssetClass::Crypto => (
            Some(wire::qty(fields, "min_order_size")?),
            Some(wire::qty(fields, "min_trade_increment")?),
            Some(wire::price(fields, "price_increment")?),
        ),
        AssetClass::UsEquity => (
            optional(fields, "min_order_size", wire::qty)?,
            optional(fields, "min_trade_increment", wire::qty)?,
            optional(fields, "price_increment", wire::price)?,
        ),
    };
    Ok(Asset {
        asset_id,
        instrument: instrument.clone(),
        class,
        exchange: exchange(wire::text(fields, "exchange")?),
        active,
        tradable: wire::flag(fields, "tradable")?,
        fractionable: wire::flag(fields, "fractionable")?,
        ipo: has("ipo"),
        ptp_no_exception: has("ptp_no_exception"),
        min_order_size,
        min_trade_increment,
        price_increment,
    })
}

/// Spec §3.1's exchange codes, matched exactly as Alpaca writes them.
fn exchange(code: &str) -> Exchange {
    match code {
        "NASDAQ" => Exchange::Nasdaq,
        "NYSE" => Exchange::Nyse,
        "ARCA" => Exchange::Arca,
        "AMEX" => Exchange::Amex,
        "BATS" => Exchange::Bats,
        "OTC" => Exchange::Otc,
        "CRYPTO" => Exchange::Crypto,
        _ => Exchange::Other,
    }
}

/// The record's attribute names. An equity must carry the array; a pair may omit it.
fn attributes(fields: &Map<String, Value>, class: AssetClass) -> Result<Vec<&str>, WireError> {
    let field = "attributes";
    match fields.get(field) {
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| item.as_str().ok_or(WireError::WrongType { field }))
            .collect(),
        None | Some(Value::Null) if class == AssetClass::Crypto => Ok(Vec::new()),
        None | Some(Value::Null) => Err(WireError::MissingField { field }),
        Some(_) => Err(WireError::WrongType { field }),
    }
}

/// A decimal field that may be absent or `null`, read by `parse` when it is present.
fn optional<T>(
    fields: &Map<String, Value>,
    field: &'static str,
    parse: fn(&Map<String, Value>, &'static str) -> Result<T, WireError>,
) -> Result<Option<T>, WireError> {
    match fields.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => parse(fields, field).map(Some),
    }
}

/// Parses the body of a latest-quote read for `instrument`. Its age is not judged here:
/// [`crate::DataClient::latest_quote`] judges it against the clock.
///
/// An equity's answer is `{"quote": {...}, "symbol": ...}` and must name `instrument`; a pair's is
/// `{"quotes": {"<pair>": {...}}}` and is found by its key. No quote, or a `null` one, is
/// [`ReadError::Absent`]. Each price and size is the JSON number's own text (DEC-168 item 3), and
/// a zero price on either side is [`ReadError::OneSided`].
pub fn latest_quote(instrument: &InstrumentId, body: &[u8]) -> Result<LatestQuote, ReadError> {
    let top = raw_object(raw_json(body)?, "quote")?;
    let is_pair = instrument.as_str().contains('/');
    let (quote, feed) = if is_pair {
        let quotes = top
            .get("quotes")
            .ok_or(WireError::MissingField { field: "quotes" })?;
        let quotes: Option<BTreeMap<String, &RawValue>> = serde_json::from_str(quotes.get())
            .map_err(|_| WireError::WrongType { field: "quotes" })?;
        let quote = quotes
            .and_then(|quotes| quotes.get(instrument.as_str()).copied())
            .ok_or(ReadError::Absent)?;
        (quote, Feed::Crypto)
    } else {
        let symbol = top
            .get("symbol")
            .ok_or(WireError::MissingField { field: "symbol" })?;
        let symbol: String = serde_json::from_str(symbol.get())
            .map_err(|_| WireError::WrongType { field: "symbol" })?;
        if symbol != instrument.as_str() {
            return Err(ReadError::OtherInstrument);
        }
        let quote = top.get("quote").copied().ok_or(ReadError::Absent)?;
        (quote, Feed::Iex)
    };
    if quote.get() == "null" {
        return Err(ReadError::Absent);
    }
    let fields = raw_object(quote, "quote")?;
    let stamped = fields
        .get("t")
        .ok_or(WireError::MissingField { field: "t" })?;
    let stamped: String =
        serde_json::from_str(stamped.get()).map_err(|_| WireError::WrongType { field: "t" })?;
    Ok(LatestQuote {
        instrument: instrument.clone(),
        at: UtcNanos::parse_rfc3339(&stamped).map_err(WireError::from)?,
        bid: side_price(&fields, "bp")?,
        bid_size: Qty::parse(&token(&fields, "bs")?).map_err(WireError::from)?,
        ask: side_price(&fields, "ap")?,
        ask_size: Qty::parse(&token(&fields, "as")?).map_err(WireError::from)?,
        feed,
    })
}

/// The body as unparsed JSON, or [`WireError::NotJson`].
fn raw_json(body: &[u8]) -> Result<&RawValue, WireError> {
    serde_json::from_slice(body).map_err(|_| WireError::NotJson)
}

/// A JSON object whose values stay unparsed, so each number keeps its own text.
fn raw_object<'a>(
    raw: &'a RawValue,
    field: &'static str,
) -> Result<BTreeMap<String, &'a RawValue>, WireError> {
    serde_json::from_str(raw.get()).map_err(|_| WireError::WrongType { field })
}

/// A quote side's price. A zero price is Alpaca's way of saying the side is empty, which is
/// [`ReadError::OneSided`], not a price; any other value `Price` cannot hold is unreadable.
fn side_price(
    fields: &BTreeMap<String, &RawValue>,
    field: &'static str,
) -> Result<Price, ReadError> {
    let text = token(fields, field)?;
    if matches!(text.as_str(), "0" | "-0") {
        return Err(ReadError::OneSided);
    }
    Ok(Price::parse(&text).map_err(WireError::from)?)
}

/// One number field's canonical text, taken from the JSON number token itself, never from a
/// float: a string, a `null` or any other value is [`WireError::WrongType`], an exponent is
/// [`WireError::ExponentForm`], and more than 9 places is [`WireError::TooManyPlaces`].
fn token(fields: &BTreeMap<String, &RawValue>, field: &'static str) -> Result<String, WireError> {
    let text = fields
        .get(field)
        .ok_or(WireError::MissingField { field })?
        .get();
    let numeric = text
        .bytes()
        .next()
        .is_some_and(|b| b == b'-' || b.is_ascii_digit());
    if !numeric {
        return Err(WireError::WrongType { field });
    }
    if text.contains(['e', 'E']) {
        return Err(WireError::ExponentForm { field });
    }
    let places = text.split_once('.').map_or("", |(_, places)| places);
    if places.len() > wire::MAX_PLACES {
        return Err(WireError::TooManyPlaces { field });
    }
    Ok(wire::canonical(text))
}

/// Whether an answer stamped `at` is current at `now` under `max_age`: stamped after `now` is
/// [`ReadError::AheadOfClock`], older than `max_age` is [`ReadError::Stale`], and an age of
/// exactly `max_age` is current. The age is counted in whole nanoseconds.
pub(crate) fn judge_age(at: UtcNanos, now: UtcNanos, max_age: Duration) -> Result<(), ReadError> {
    if at > now {
        return Err(ReadError::AheadOfClock);
    }
    let age = nanos(now)
        .zip(nanos(at))
        .and_then(|(now, at)| now.checked_sub(at))
        .and_then(|age| u128::try_from(age).ok())
        .ok_or(ReadError::Stale)?;
    if age > max_age.as_nanos() {
        return Err(ReadError::Stale);
    }
    Ok(())
}

/// Whole nanoseconds since the epoch.
fn nanos(at: UtcNanos) -> Option<i128> {
    i128::from(at.secs())
        .checked_mul(1_000_000_000)?
        .checked_add(i128::from(at.nanos()))
}

#[cfg(test)]
mod tests {
    use mandate_accounting::InstrumentId;
    use mandate_num::{Price, Qty};

    use super::{asset, latest_quote};

    fn instrument(symbol: &str) -> Result<InstrumentId, String> {
        InstrumentId::new(symbol).map_err(|e| format!("{e:?}"))
    }

    const EQUITY: &str = r#"{"attributes":[],"class":"us_equity","exchange":"NYSE","fractionable":false,"id":"11111111-2222-4333-8444-555555555555","status":"active","symbol":"KO","tradable":true"#;
    const PAIR: &str = r#"{"class":"crypto","exchange":"CRYPTO","fractionable":true,"id":"276e2673-764b-4ab6-a611-caf665ca6340","min_order_size":"0.000018","min_trade_increment":"0.000000001","price_increment":"1","status":"active","symbol":"BTC/USD","tradable":true"#;

    /// An equity record that does carry order constraints has them read, and a `null` one is
    /// none (the E7-8 implementation's mutation gate: `optional` answering `None` regardless).
    #[test]
    fn an_equity_record_with_order_constraints_reads_them() -> Result<(), String> {
        let ko = instrument("KO")?;
        let with = format!(
            r#"{EQUITY},"min_order_size":"1","min_trade_increment":"1","price_increment":"0.01"}}"#
        );
        let read = asset(&ko, with.as_bytes()).map_err(|e| e.to_string())?;
        let one = Qty::parse("1").map_err(|e| e.to_string())?;
        assert_eq!(read.min_order_size, Some(one));
        assert_eq!(read.min_trade_increment, Some(one));
        assert_eq!(
            read.price_increment,
            Some(Price::parse("0.01").map_err(|e| e.to_string())?)
        );
        let null = format!(r#"{EQUITY},"min_order_size":null}}"#);
        let read = asset(&ko, null.as_bytes()).map_err(|e| e.to_string())?;
        assert_eq!(read.min_order_size, None);
        let bad = format!(r#"{EQUITY},"price_increment":"1e-2"}}"#);
        assert_eq!(
            asset(&ko, bad.as_bytes()).map_err(|e| e.code()),
            Err("exponent_form"),
            "a constraint an equity sends is read under the same rules as a pair's"
        );
        Ok(())
    }

    /// #288 review, minor 1: a size or price is the token's own digits, which an `f64` round trip
    /// would not keep. `0.000000001` would come back `1e-09`, and `322390037.75749118` would come
    /// back as `…4912` (DEC-168 item 3).
    #[test]
    fn a_quote_keeps_digits_a_float_would_lose() -> Result<(), String> {
        let body = br#"{"quote":{"ap":322390037.75749118,"as":322390037.75749118,"bp":0.000000001,"bs":0.000000001,"t":"2026-09-25T19:59:59.826431742Z"},"symbol":"AAPL"}"#;
        let quote = latest_quote(&instrument("AAPL")?, body).map_err(|e| e.to_string())?;
        let exact = |text: &str| Qty::parse(text).map_err(|e| e.to_string());
        assert_eq!(quote.bid_size, exact("0.000000001")?);
        assert_eq!(quote.ask_size, exact("322390037.75749118")?);
        assert_eq!(
            quote.bid,
            Price::parse("0.000000001").map_err(|e| e.to_string())?
        );
        assert_eq!(
            quote.ask,
            Price::parse("322390037.75749118").map_err(|e| e.to_string())?
        );
        Ok(())
    }

    /// A pair may omit `attributes`, or send it `null`: no pair is an IPO or a partnership
    /// (DEC-168 item 6). An equity may not.
    #[test]
    fn a_pair_without_attributes_reads_and_an_equity_without_them_does_not() -> Result<(), String> {
        let pair = instrument("BTC/USD")?;
        for body in [
            format!("{PAIR}}}"),
            format!(r#"{PAIR},"attributes":null}}"#),
        ] {
            let read = asset(&pair, body.as_bytes()).map_err(|e| e.to_string())?;
            assert_eq!((read.ipo, read.ptp_no_exception), (false, false), "{body}");
        }
        let without = EQUITY.replacen(r#""attributes":[],"#, "", 1);
        assert_eq!(
            asset(&instrument("KO")?, format!("{without}}}").as_bytes()).map_err(|e| e.code()),
            Err("missing_field")
        );
        Ok(())
    }
}
