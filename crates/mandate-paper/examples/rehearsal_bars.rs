//! The R0 rehearsal's stored daily bars (DEC-961 item 3): the IEX daily-bars dataset `--bars`
//! names, written for a made-up ticker the founder's `mandate download` cannot fetch, so the
//! rehearsal against `mandate-rh-sim` needs no market-data key (`AGENTS.md` rules 7 and 8).
//!
//! ```text
//! cargo run -p mandate-paper --example rehearsal_bars -- <out> <symbol> <last close> [<at>]
//! ```
//!
//! Writes the last [`SESSIONS`] regular sessions up to the last one completed at `<at>`, an RFC
//! 3339 instant, the run clock the rehearsal sets (DEC-962 item 4), or at the system clock without
//! it, one bar each, the closes rising by [`STEP`] to `<last close>` so the moving-average
//! crossover's fast mean is above its slow one, with [`VOLUME`] shares a session, and prints the
//! dataset's directory, which the run's `--bars` takes. An example, built only from the crate's
//! dev-dependencies, so no product binary holds it; the shell judges the dataset exactly as it
//! judges a downloaded one.

use std::process::ExitCode;

use mandate_alpaca::{Pause, TokioPause};
use mandate_canon::DecStr;
use mandate_marketdata::dataset::Store;
use mandate_marketdata::model::{AssetClass, Bar, DatasetId, Feed, Kind, Records, Symbol};
use mandate_num::Usd;
use mandate_time::{Date, ExchangeCalendar, UtcNanos};

/// The sessions written: past the slow window (20) and the liquidity span (20).
const SESSIONS: usize = 30;
/// The rise from one session's close to the next.
const STEP: &str = "0.1";
/// Each session's volume.
const VOLUME: &str = "50000000";
/// How far back the walk over the calendar starts, in days: comfortably more than
/// [`SESSIONS`] sessions.
const LOOKBACK_DAYS: i64 = 60;

fn write(out: &str, symbol: &str, last_close: &str, at: Option<&str>) -> Result<String, String> {
    let now = match at {
        Some(at) => UtcNanos::parse_rfc3339(at).map_err(|_| "<at> is not an RFC 3339 instant")?,
        None => TokioPause.now(),
    };
    let calendar = ExchangeCalendar::us_equities().map_err(|e| e.to_string())?;
    let latest = calendar
        .last_completed_regular_session(now)
        .map_err(|e| e.to_string())?
        .ok_or("the calendar names no completed session")?;
    let back = LOOKBACK_DAYS
        .checked_mul(86_400)
        .and_then(|span| now.secs().checked_sub(span))
        .ok_or("the clock is too early")?;
    let mut day = UtcNanos::from_parts(back, 0)
        .map_err(|e| e.to_string())?
        .date();
    let mut days: Vec<Date> = Vec::new();
    while day <= latest {
        if calendar.is_trading_day(day).map_err(|e| e.to_string())? {
            days.push(day);
        }
        day = day.next().map_err(|e| e.to_string())?;
    }
    let days = days
        .get(days.len().saturating_sub(SESSIONS)..)
        .ok_or("too few sessions")?;
    let step = Usd::parse(STEP).map_err(|e| e.to_string())?;
    let mut close = Usd::parse(last_close).map_err(|e| e.to_string())?;
    let mut closes = Vec::with_capacity(days.len());
    for _ in days {
        closes.push(close);
        close = close.checked_sub(step).map_err(|e| e.to_string())?;
    }
    closes.reverse();
    let kind = Kind::Bars("1Day".parse().map_err(|_| "the daily timeframe")?);
    let symbol = Symbol::parse(symbol).map_err(|e| e.to_string())?;
    let dataset =
        DatasetId::new(AssetClass::UsEquity, Feed::Iex, kind, symbol).map_err(|e| e.to_string())?;
    let store = Store::new(out);
    for (day, close) in days.iter().zip(closes) {
        let price = DecStr::parse(&close.to_string()).map_err(|e| e.to_string())?;
        let start = UtcNanos::parse_rfc3339(&format!("{day}T04:00:00.000000000Z"))
            .map_err(|e| e.to_string())?;
        let bar = Bar {
            start,
            open: price.clone(),
            high: price.clone(),
            low: price.clone(),
            close: price.clone(),
            volume: DecStr::parse(VOLUME).map_err(|e| e.to_string())?,
            vwap: price,
            trade_count: 400_000,
        };
        store
            .put_day(&dataset, *day, &Records::Bars(vec![bar]))
            .map_err(|e| e.to_string())?;
    }
    Ok(store.dataset_dir(&dataset).display().to_string())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (out, symbol, last_close, at) = match args.as_slice() {
        [out, symbol, last_close] => (out, symbol, last_close, None),
        [out, symbol, last_close, at] => (out, symbol, last_close, Some(at.as_str())),
        _ => {
            eprintln!("usage: rehearsal_bars <out> <symbol> <last close> [<at>]");
            return ExitCode::from(2);
        }
    };
    match write(out, symbol, last_close, at) {
        Ok(dir) => {
            println!("{dir}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("rehearsal_bars: {error}");
            ExitCode::FAILURE
        }
    }
}
