//! The trailing minutes: only the complete one-minute bars inside the five minutes before the
//! clock, from a dataset of the instrument's one-minute bars, counted here by their starts.

use std::fs;
use std::path::PathBuf;

use mandate_bars::{BarsError, trailing_minutes};
use mandate_canon::DecStr;
use mandate_marketdata::dataset::Store;
use mandate_marketdata::model::{AssetClass, Bar, DatasetId, Feed, Kind, Records, Symbol};
use mandate_time::{Date, UtcNanos};

fn at(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).unwrap()
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mandate-bars-{name}-{}", std::process::id()));
    fs::remove_dir_all(&dir).ok();
    dir
}

fn bar(start: UtcNanos, volume: &str) -> Bar {
    let price = DecStr::parse("150").unwrap();
    Bar {
        start,
        open: price.clone(),
        high: price.clone(),
        low: price.clone(),
        close: price.clone(),
        volume: DecStr::parse(volume).unwrap(),
        vwap: price,
        trade_count: 10,
    }
}

/// `count` one-minute bars of `symbol` from `first`, a minute apart, the n-th with volume n + 1.
fn minutes(root: &PathBuf, symbol: &str, timeframe: &str, first: &str, count: i64) -> PathBuf {
    let kind = Kind::Bars(timeframe.parse().unwrap());
    let dataset = DatasetId::new(
        AssetClass::UsEquity,
        Feed::Iex,
        kind,
        Symbol::parse(symbol).unwrap(),
    )
    .unwrap();
    let start = at(first).secs();
    let bars: Vec<Bar> = (0..count)
        .map(|n| {
            bar(
                UtcNanos::from_parts(start + 60 * n, 0).unwrap(),
                &(n + 1).to_string(),
            )
        })
        .collect();
    let day = Date::parse(&first[..10]).unwrap();
    let store = Store::new(root);
    store.put_day(&dataset, day, &Records::Bars(bars)).unwrap();
    store.dataset_dir(&dataset)
}

#[test]
fn only_the_complete_minutes_of_the_window_are_handed_out() {
    let root = scratch("window");
    let dir = minutes(&root, "SPY", "1Min", "2026-10-26T13:30:00Z", 390);
    for (now, first, last) in [
        (
            "2026-10-26T15:00:00Z",
            "2026-10-26T14:55:00Z",
            "2026-10-26T14:59:00Z",
        ),
        (
            "2026-10-26T15:00:59Z",
            "2026-10-26T14:56:00Z",
            "2026-10-26T14:59:00Z",
        ),
        (
            "2026-10-26T13:32:00Z",
            "2026-10-26T13:30:00Z",
            "2026-10-26T13:31:00Z",
        ),
    ] {
        let read = trailing_minutes(&dir, "SPY", at(now)).unwrap();
        let starts: Vec<i64> = read.iter().map(|m| m.start.secs()).collect();
        let expected: Vec<i64> = (at(first).secs()..=at(last).secs()).step_by(60).collect();
        assert_eq!(starts, expected, "{now}");
    }
    let first = trailing_minutes(&dir, "SPY", at("2026-10-26T13:31:00Z")).unwrap();
    assert_eq!(first.len(), 1, "the session's first minute, ended");
    assert_eq!(first[0].volume.to_string(), "1");
    assert!(
        trailing_minutes(&dir, "SPY", at("2026-10-26T13:30:59Z"))
            .unwrap()
            .is_empty()
    );
    assert!(
        trailing_minutes(&dir, "SPY", at("2026-10-27T15:00:00Z"))
            .unwrap()
            .is_empty(),
        "another day"
    );
    fs::remove_dir_all(&root).ok();
}

/// With earlier days stored too, the clock's own day is read: an earlier day's bars are never the
/// trailing window.
#[test]
fn the_clocks_own_day_is_read_among_several() {
    let root = scratch("days");
    minutes(&root, "SPY", "1Min", "2026-10-23T13:30:00Z", 390);
    let dir = minutes(&root, "SPY", "1Min", "2026-10-26T13:30:00Z", 390);
    let read = trailing_minutes(&dir, "SPY", at("2026-10-26T15:00:00Z")).unwrap();
    assert_eq!(read.len(), 5);
    assert_eq!(read[0].start, at("2026-10-26T14:55:00Z"));
    fs::remove_dir_all(&root).ok();
}

#[test]
fn another_instrument_or_another_timeframe_is_refused() {
    let root = scratch("refused");
    let other = minutes(&root, "QQQ", "1Min", "2026-10-26T13:30:00Z", 30);
    let daily = minutes(&root, "SPY", "1Day", "2026-10-26T04:00:00Z", 1);
    let now = at("2026-10-26T14:00:00Z");
    for (dir, what) in [
        (&other, "the minute bars are another instrument's"),
        (&daily, "the dataset is not one-minute bars"),
    ] {
        match trailing_minutes(dir, "SPY", now) {
            Err(BarsError::Untrusted { what: said }) => assert_eq!(said, what),
            other => panic!("expected {what}, got {other:?}"),
        }
    }
    fs::remove_dir_all(&root).ok();
}
