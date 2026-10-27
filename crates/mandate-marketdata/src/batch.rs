//! Alpaca's multi-symbol requests for the data process's generations (DEC-2283 item 5,
//! data-plane spec §3.8): one request carries every instrument a generation stores, the US-equity
//! `iex` bars of a span or window, unadjusted, or the corporate actions of a span, and each answer
//! is split by symbol on receipt. An answer naming a symbol not asked for, naming one twice on a
//! page, or holding a record outside the asked span refuses the page, so a generation is never
//! written from it.

use std::collections::{BTreeMap, BTreeSet};

use mandate_time::UtcNanos;
use serde_json::value::RawValue;

use crate::alpaca::{
    Entries, Envelope, Window, WireError, bars, json, parse_actions_for, percent_encode, shift_days,
};
use crate::alpaca::{PROCESS_DATE_DAYS_AFTER, PROCESS_DATE_DAYS_BEFORE};
use crate::model::{Bar, CorporateActions, DayRange, Symbol, Timeframe};
use crate::timestamp::TimestampError;

/// The most symbols one batch asks for: the data process's cap on a workspace's instruments
/// (`BARS_INSTRUMENTS`, data-plane spec §3.8), which also keeps the request line short.
pub const MAX_BATCH_SYMBOLS: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BatchError {
    #[error("a batch asks for at least one symbol")]
    NoSymbols,
    #[error("a batch asks for at most {MAX_BATCH_SYMBOLS} symbols")]
    TooManySymbols,
    #[error("a batch's span ends after it starts")]
    EmptySpan,
    #[error("a batch's span starts and ends on whole seconds")]
    NotWholeSecond,
}

impl BatchError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(self) -> &'static str {
        match self {
            Self::NoSymbols => "no_symbols",
            Self::TooManySymbols => "too_many_symbols",
            Self::EmptySpan => "empty_span",
            Self::NotWholeSecond => "not_whole_second",
        }
    }
}

/// The US-equity `iex` bars of `timeframe` that start from `start` up to, not including, `end`,
/// for every symbol in `symbols`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarsBatch {
    symbols: BTreeSet<Symbol>,
    timeframe: Timeframe,
    start: UtcNanos,
    end: UtcNanos,
}

impl BarsBatch {
    /// # Errors
    /// [`BatchError`] for no symbol, more than [`MAX_BATCH_SYMBOLS`], a span that does not end
    /// after it starts, or a bound that is not a whole second.
    pub fn new(
        symbols: BTreeSet<Symbol>,
        timeframe: Timeframe,
        start: UtcNanos,
        end: UtcNanos,
    ) -> Result<Self, BatchError> {
        if symbols.is_empty() {
            return Err(BatchError::NoSymbols);
        }
        if symbols.len() > MAX_BATCH_SYMBOLS {
            return Err(BatchError::TooManySymbols);
        }
        if start.nanos() != 0 || end.nanos() != 0 {
            return Err(BatchError::NotWholeSecond);
        }
        if end <= start {
            return Err(BatchError::EmptySpan);
        }
        Ok(Self {
            symbols,
            timeframe,
            start,
            end,
        })
    }

    pub fn symbols(&self) -> &BTreeSet<Symbol> {
        &self.symbols
    }

    pub fn timeframe(&self) -> Timeframe {
        self.timeframe
    }

    pub fn start(&self) -> UtcNanos {
        self.start
    }

    pub fn end(&self) -> UtcNanos {
        self.end
    }
}

/// The symbols, percent-encoded and comma-joined in symbol order.
fn joined(symbols: &BTreeSet<Symbol>) -> String {
    symbols
        .iter()
        .map(|symbol| percent_encode(symbol.as_str()))
        .collect::<Vec<_>>()
        .join(",")
}

/// `time` to the second, as Alpaca reads a span's bound: `YYYY-MM-DDTHH:MM:SSZ`.
fn whole_second(time: UtcNanos) -> String {
    let text = time.to_string();
    let seconds = text.split_once('.').map_or(text.as_str(), |(head, _)| head);
    format!("{seconds}Z")
}

/// The path and query of one page of `batch`, sorted ascending.
pub fn bars_path(batch: &BarsBatch, limit: u32, page_token: Option<&str>) -> String {
    let mut path = format!(
        "/v2/stocks/bars?symbols={}&timeframe={}&start={}&end={}&limit={limit}\
         &feed=iex&adjustment=raw&sort=asc",
        joined(&batch.symbols),
        batch.timeframe,
        whole_second(batch.start),
        whole_second(batch.end),
    );
    if let Some(token) = page_token {
        path.push_str(&format!("&page_token={}", percent_encode(token)));
    }
    path
}

/// One page of a batch: each symbol's bars on it, and the token of the next page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarsPage {
    pub bars: BTreeMap<Symbol, Vec<Bar>>,
    pub next_page_token: Option<String>,
}

/// Parses one page of `batch`. A record stamped at the span's end belongs to the next span and is
/// dropped; any other record outside the span refuses the page.
///
/// # Errors
/// [`WireError`] for a malformed page, a symbol not asked for or named twice, or a record outside
/// the span.
pub fn parse_bars_page(batch: &BarsBatch, body: &[u8]) -> Result<BarsPage, WireError> {
    let envelope: Envelope<'_> = serde_json::from_slice(body).map_err(json)?;
    let raw_map = envelope.bars.ok_or(WireError::MissingMember("bars"))?;
    let raw_token = envelope
        .next_page_token
        .ok_or(WireError::MissingMember("next_page_token"))?;
    let next_page_token: Option<String> = serde_json::from_str(raw_token.get()).map_err(json)?;
    let window = Window::span(batch.start, batch.end);
    let entries: Vec<(String, &RawValue)> = if raw_map.get() == "null" {
        Vec::new()
    } else {
        serde_json::from_str::<Entries<'_>>(raw_map.get())
            .map_err(json)?
            .0
    };
    let mut page = BTreeMap::new();
    for (name, raw) in entries {
        let symbol = batch
            .symbols
            .iter()
            .find(|symbol| symbol.as_str() == name)
            .ok_or_else(|| WireError::UnexpectedSymbol(name.clone()))?;
        if page.contains_key(symbol) {
            return Err(WireError::DuplicateSymbol(name));
        }
        page.insert(symbol.clone(), bars(raw, &window)?);
    }
    Ok(BarsPage {
        bars: page,
        next_page_token,
    })
}

/// The path and query of one page of the corporate actions of `symbols` that may have an ex-date
/// in `ex_dates`, over the same process-date window as one symbol's request
/// ([`crate::alpaca::corporate_actions_path`]).
///
/// # Errors
/// [`TimestampError`] when the window leaves the calendar.
pub fn actions_path(
    symbols: &BTreeSet<Symbol>,
    ex_dates: DayRange,
    limit: u32,
    page_token: Option<&str>,
) -> Result<String, TimestampError> {
    let start = shift_days(ex_dates.first(), -i64::from(PROCESS_DATE_DAYS_BEFORE))?;
    let end = shift_days(ex_dates.last(), i64::from(PROCESS_DATE_DAYS_AFTER))?;
    let mut path = format!(
        "/v1/corporate-actions?symbols={}&start={start}&end={end}&limit={limit}&sort=asc",
        joined(symbols),
    );
    if let Some(token) = page_token {
        path.push_str(&format!("&page_token={}", percent_encode(token)));
    }
    Ok(path)
}

/// One page of corporate actions for several symbols: each asked symbol's actions on it, and the
/// token of the next page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionsPage {
    pub actions: BTreeMap<Symbol, CorporateActions>,
    pub next_page_token: Option<String>,
}

/// Parses one page of corporate actions asked for `symbols`: a split or cash dividend must name an
/// asked symbol, and any other action goes to each asked symbol it names, or to all of them when it
/// names none.
///
/// # Errors
/// [`WireError`] for a malformed page, a split or dividend of a symbol not asked for, or an action
/// listed twice.
pub fn parse_actions_page(
    symbols: &BTreeSet<Symbol>,
    body: &[u8],
) -> Result<ActionsPage, WireError> {
    let (actions, next_page_token) = parse_actions_for(symbols, body)?;
    Ok(ActionsPage {
        actions,
        next_page_token,
    })
}
