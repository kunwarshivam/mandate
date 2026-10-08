use std::fs;
use std::path::{Path, PathBuf};

use mandate_accounting::InstrumentId;
use mandate_alpaca::{MinuteBar, MinuteBars};
use mandate_canon::DecStr;
use mandate_liquidity::LiquidityError;
use mandate_marketdata::dataset::Store;
use mandate_marketdata::model::{
    AssetClass as DataAssetClass, Bar, DatasetId, Feed as DataFeed, Kind, Records, Symbol,
    TimeUnit, Timeframe,
};
use mandate_num::{NumError, Price, Qty, Usd};
use mandate_time::{Date, TimeError, UtcNanos};

use super::{LiquidityFacts, liquidity_facts, refusal};
use crate::Cause;

const NOW: &str = "2026-09-28T17:00:00Z";

fn text<E: std::fmt::Display>(error: E) -> String {
    error.to_string()
}

fn aapl() -> Result<InstrumentId, String> {
    InstrumentId::new("AAPL").map_err(text)
}

fn at(rfc3339: &str) -> Result<UtcNanos, String> {
    UtcNanos::parse_rfc3339(rfc3339).map_err(text)
}

fn qty(value: &str) -> Result<Qty, String> {
    Qty::parse(value).map_err(text)
}

fn dec(value: &str) -> Result<DecStr, String> {
    DecStr::parse(value).map_err(text)
}

fn bar(start: UtcNanos, close: &str, volume: &str) -> Result<Bar, String> {
    Ok(Bar {
        start,
        open: dec(close)?,
        high: dec(close)?,
        low: dec(close)?,
        close: dec(close)?,
        volume: dec(volume)?,
        vwap: dec(close)?,
        trade_count: 1,
    })
}

/// Daily AAPL bars, one per weekday from `first`, each `(close, volume)`.
fn daily(root: &Path, first: &str, days: &[(String, String)]) -> Result<PathBuf, String> {
    let id = DatasetId::new(
        DataAssetClass::UsEquity,
        DataFeed::Iex,
        Kind::Bars(Timeframe::new(1, TimeUnit::Day).map_err(text)?),
        Symbol::parse("AAPL").map_err(text)?,
    )
    .map_err(text)?;
    let store = Store::new(root);
    let mut day = Date::parse(first).map_err(text)?;
    for (close, volume) in days {
        while day.is_weekend() {
            day = day.next().map_err(text)?;
        }
        let start = at(&format!("{day}T04:00:00Z"))?;
        store
            .put_day(&id, day, &Records::Bars(vec![bar(start, close, volume)?]))
            .map_err(text)?;
        day = day.next().map_err(text)?;
    }
    Ok(store.dataset_dir(&id))
}

/// Twenty-five sessions ending 2026-09-25, closes rising a dollar a day to 255.20, each traded a
/// million shares but the first of the last twenty, which traded one more.
fn sessions() -> Vec<(String, String)> {
    (231..256)
        .map(|close| {
            let volume = if close == 236 { "1000001" } else { "1000000" };
            (format!("{close}.20"), volume.to_owned())
        })
        .collect()
}

/// The broker's bars for `symbol` starting at each of `starts`; the bar starting at 16:5`n` traded
/// `100 + n` shares.
fn minute_bars(symbol: &str, starts: &[&str]) -> Result<MinuteBars, String> {
    Ok(MinuteBars {
        instrument: InstrumentId::new(symbol).map_err(text)?,
        bars: starts
            .iter()
            .map(|start| {
                let minute = start
                    .get(15..16)
                    .ok_or_else(|| format!("no minute digit in {start}"))?;
                Ok(MinuteBar {
                    start: at(start)?,
                    volume: qty(&format!("10{minute}"))?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?,
    })
}

const TRAILING: [&str; 5] = [
    "2026-09-28T16:55:00Z",
    "2026-09-28T16:56:00Z",
    "2026-09-28T16:57:00Z",
    "2026-09-28T16:58:00Z",
    "2026-09-28T16:59:00Z",
];

fn scratch_root(name: &str) -> Result<PathBuf, String> {
    let root = std::env::temp_dir().join(format!("mandate-e77-data-{name}-{}", std::process::id()));
    if root.exists() {
        fs::remove_dir_all(&root).map_err(text)?;
    }
    fs::create_dir_all(&root).map_err(text)?;
    Ok(root)
}

/// The shell parses and maps the stored daily bars and maps the broker's typed minute bars; the
/// figures are `mandate-liquidity`'s: the last close; the lower median 245.20 × 1,000,000 of the
/// last twenty; 20,000,001 ÷ 20 truncated; and 105 + 106 + 107 + 108 + 109.
#[test]
fn the_liquidity_facts_are_the_pure_crates_figures_over_the_snapshots_bars() -> Result<(), String> {
    let root = scratch_root("liquidity")?;
    let daily_dir = daily(&root.join("daily"), "2026-08-24", &sessions())?;
    let facts = liquidity_facts(
        &aapl()?,
        &daily_dir,
        &minute_bars("AAPL", &TRAILING)?,
        at(NOW)?,
    )
    .map_err(text)?;
    assert_eq!(
        facts,
        LiquidityFacts {
            prior_close: Price::parse("255.2").map_err(text)?,
            median_dollar_volume_20d: Usd::parse("245200000").map_err(text)?,
            adv_20d: qty("1000000")?,
            trailing_5m_volume: qty("535")?,
        }
    );
    fs::remove_dir_all(&root).map_err(text)
}

/// Each input that cannot be decided on refuses, with the reason the shell names for it.
#[test]
fn each_unusable_input_refuses_with_its_own_reason() -> Result<(), String> {
    let root = scratch_root("refusals")?;
    let daily_dir = daily(&root.join("daily"), "2026-08-24", &sessions())?;
    let all = sessions();
    let nineteen = all.get(6..).ok_or("twenty-five sessions")?;
    let short = daily(&root.join("short"), "2026-09-01", nineteen)?;
    let duplicated = [TRAILING[3], TRAILING[4], TRAILING[4]];
    let cases = [
        (
            "nineteen sessions",
            short.clone(),
            minute_bars("AAPL", &TRAILING)?,
            NOW,
            Cause::Untrusted {
                what: "fewer than 20 sessions are stored",
            },
        ),
        (
            "another instrument's minute bars",
            daily_dir.clone(),
            minute_bars("MSFT", &TRAILING)?,
            NOW,
            Cause::Absent {
                what: "the instrument's minute bars",
            },
        ),
        (
            "a bar still open at the clock",
            daily_dir.clone(),
            minute_bars("AAPL", &TRAILING)?,
            "2026-09-28T16:59:59.999999999Z",
            Cause::Absent {
                what: "minute bars complete at the run clock",
            },
        ),
        (
            "a minute twice",
            daily_dir.clone(),
            minute_bars("AAPL", &duplicated)?,
            NOW,
            Cause::Absent {
                what: "minute bars in strictly increasing order",
            },
        ),
        (
            "bars older than the window",
            daily_dir.clone(),
            minute_bars("AAPL", &TRAILING)?,
            "2026-09-28T17:04:00.000000001Z",
            Cause::Absent {
                what: "a complete minute bar in the trailing five minutes",
            },
        ),
    ];
    for (name, dir, bars, now, expected) in cases {
        let refused = match liquidity_facts(&aapl()?, &dir, &bars, at(now)?) {
            Err(cause) => cause,
            Ok(facts) => return Err(format!("{name}: accepted as {facts:?}")),
        };
        assert_eq!(text(&refused), text(&expected), "{name}");
    }
    fs::remove_dir_all(&root).map_err(text)
}

/// Every `mandate-liquidity` refusal maps to one cause, none of them a default figure.
#[test]
fn every_liquidity_error_maps_to_its_cause() {
    let cases = [
        (
            LiquidityError::FewerSessions,
            "the stored data cannot be trusted: fewer than 20 sessions are stored",
        ),
        (
            LiquidityError::AheadOfClock,
            "minute bars complete at the run clock",
        ),
        (
            LiquidityError::Unordered,
            "minute bars in strictly increasing order",
        ),
        (
            LiquidityError::EmptyWindow,
            "a complete minute bar in the trailing five minutes",
        ),
        (
            LiquidityError::Time(TimeError::OutOfRange),
            "a run clock inside the calendar",
        ),
    ];
    for (error, expected) in cases {
        assert_eq!(text(refusal(error.clone())), expected, "{error:?}");
    }
    assert!(matches!(
        refusal(LiquidityError::Num(NumError::NotPositive)),
        Cause::Num(NumError::NotPositive)
    ));
}
