//! An `alpaca-iex` root's generations (data-plane spec §3.8, DEC-2283 items 4 and 7): a reader at
//! clock `now` reads the daily generation of `now`'s last completed regular session, and the minute
//! generation of `now`'s whole minute or failing it one of the two before, and refuses when the
//! generation it names is missing. A `cell-recorded` root reads as before. The oracles are the paths
//! written out by hand and the figures a flat root holding the same bars gives.

use std::fs;
use std::path::{Path, PathBuf};

use mandate_bars::generations::{
    GENERATIONS_ROOT, daily_dir_at, daily_generation, minute_dir_at, minute_generation,
};
use mandate_bars::{daily_dir, liquidity, minute_dir};
use mandate_canon::DecStr;
use mandate_marketdata::dataset::Store;
use mandate_marketdata::model::{AssetClass, Bar, DatasetId, Feed, Kind, Records, Symbol};
use mandate_time::{Date, ExchangeCalendar, UtcNanos, new_york_midnight};

fn at(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).unwrap()
}

fn day(text: &str) -> Date {
    Date::parse(text).unwrap()
}

/// A fresh scratch directory whose last component is `alpaca-iex`.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mandate-bars-generations-{name}-{}",
        std::process::id()
    ));
    fs::remove_dir_all(&dir).ok();
    dir.join(GENERATIONS_ROOT)
}

fn bar(start: UtcNanos, close: &str, volume: &str) -> Bar {
    let price = DecStr::parse(close).unwrap();
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

fn dataset(symbol: &str, timeframe: &str) -> DatasetId {
    DatasetId::new(
        AssetClass::UsEquity,
        Feed::Iex,
        Kind::Bars(timeframe.parse().unwrap()),
        Symbol::parse(symbol).unwrap(),
    )
    .unwrap()
}

/// The 30 trading days ending on `last`, oldest first.
fn sessions_to(last: Date) -> Vec<Date> {
    let calendar = ExchangeCalendar::us_equities().unwrap();
    let mut d = Date::new(2026, 8, 1).unwrap();
    let mut days = Vec::new();
    while d <= last {
        if calendar.is_trading_day(d).unwrap() {
            days.push(d);
        }
        d = d.next().unwrap();
    }
    days.split_off(days.len() - 30)
}

/// Stores `symbol`'s 30 sessions ending on `last` under `root`, session `n` closing at
/// `100 + n % 7` on `1_000_000 + n` shares.
fn daily_into(root: &Path, symbol: &str, last: Date) {
    let store = Store::new(root);
    let id = dataset(symbol, "1Day");
    for (n, d) in sessions_to(last).into_iter().enumerate() {
        let bars = vec![bar(
            new_york_midnight(d).unwrap(),
            &format!("{}.5", 100 + n % 7),
            &(1_000_000 + n).to_string(),
        )];
        store.put_day(&id, d, &Records::Bars(bars)).unwrap();
    }
}

/// Stores `symbol`'s one-minute bars from `from` up to, not including, `to`, minute `n` on
/// `200 + n` shares.
fn minutes_into(root: &Path, symbol: &str, from: &str, to: &str) {
    let (from, to) = (at(from).secs(), at(to).secs());
    let bars: Vec<Bar> = (0..)
        .map(|n| from + 60 * n)
        .take_while(|start| *start < to)
        .enumerate()
        .map(|(n, start)| {
            bar(
                UtcNanos::from_parts(start, 0).unwrap(),
                "110",
                &(200 + n).to_string(),
            )
        })
        .collect();
    let d = UtcNanos::from_parts(from, 0).unwrap().date();
    Store::new(root)
        .put_day(&dataset(symbol, "1Min"), d, &Records::Bars(bars))
        .unwrap();
}

#[test]
fn generations_are_named_by_their_session_and_whole_minute() {
    let root = PathBuf::from("/s/alpaca-iex");
    assert_eq!(
        daily_generation(&root, day("2026-10-23")),
        PathBuf::from("/s/alpaca-iex/daily/2026-10-23")
    );
    for minute in [
        "2026-10-27T14:30:00Z",
        "2026-10-27T14:30:59.999999999Z",
        "2026-10-27T14:30:17Z",
    ] {
        assert_eq!(
            minute_generation(&root, at(minute)).unwrap(),
            PathBuf::from("/s/alpaca-iex/minutes/2026-10-27T1430"),
            "{minute}"
        );
    }
    assert_eq!(
        minute_generation(&root, at("2026-01-02T09:05:00Z")).unwrap(),
        PathBuf::from("/s/alpaca-iex/minutes/2026-01-02T0905")
    );
}

#[test]
fn a_reader_reads_the_daily_generation_of_its_last_completed_session() {
    let root = PathBuf::from("/s/alpaca-iex");
    for (now, session) in [
        ("2026-10-26T19:00:00Z", "2026-10-23"),
        ("2026-10-26T19:59:59Z", "2026-10-23"),
        ("2026-10-26T20:00:00Z", "2026-10-26"),
        ("2026-10-24T15:00:00Z", "2026-10-23"),
        ("2026-11-27T19:00:00Z", "2026-11-27"),
    ] {
        assert_eq!(
            daily_dir_at(&root, "QQQ", at(now)).unwrap(),
            PathBuf::from(format!(
                "/s/alpaca-iex/daily/{session}/alpaca/iex/bars-1Day/QQQ"
            )),
            "{now}"
        );
    }
    assert!(daily_dir_at(&root, "qqq", at("2026-10-26T19:00:00Z")).is_err());
    assert!(
        daily_dir_at(&root, "QQQ", at("2035-01-02T19:00:00Z")).is_err(),
        "a clock past the calendar names no session"
    );
}

#[test]
fn any_other_root_reads_its_one_dataset_as_before() {
    let now = at("2026-10-26T19:00:00Z");
    for root in ["/s/cell-recorded", "/s/alpaca-iex/daily", "/s/alpaca-iexx"] {
        let root = Path::new(root);
        assert_eq!(
            daily_dir_at(root, "QQQ", now).unwrap(),
            daily_dir(root, "QQQ").unwrap()
        );
        assert_eq!(
            minute_dir_at(root, "QQQ", now).unwrap(),
            minute_dir(root, "QQQ").unwrap()
        );
    }
}

#[test]
fn a_reader_takes_its_minute_or_failing_it_one_of_the_two_before() {
    let root = scratch("minutes");
    let now = at("2026-10-26T19:00:30Z");
    let named =
        |minute: &str| minute_dir(&minute_generation(&root, at(minute)).unwrap(), "QQQ").unwrap();
    assert_eq!(
        minute_dir_at(&root, "QQQ", now).unwrap(),
        named("2026-10-26T19:00:00Z"),
        "with no generation, the reader names its own minute's, which does not read"
    );
    for (written, read) in [
        ("2026-10-26T18:58:00Z", "2026-10-26T18:58:00Z"),
        ("2026-10-26T18:59:00Z", "2026-10-26T18:59:00Z"),
        ("2026-10-26T19:00:00Z", "2026-10-26T19:00:00Z"),
    ] {
        minutes_into(
            &minute_generation(&root, at(written)).unwrap(),
            "QQQ",
            "2026-10-26T18:50:00Z",
            written,
        );
        assert_eq!(minute_dir_at(&root, "QQQ", now).unwrap(), named(read));
    }
    let older = scratch("older");
    minutes_into(
        &minute_generation(&older, at("2026-10-26T18:57:00Z")).unwrap(),
        "QQQ",
        "2026-10-26T18:50:00Z",
        "2026-10-26T18:57:00Z",
    );
    assert_eq!(
        minute_dir_at(&older, "QQQ", now).unwrap(),
        minute_dir(&minute_generation(&older, now).unwrap(), "QQQ").unwrap(),
        "a generation three minutes back is not read"
    );
    let other = scratch("other-symbol");
    minutes_into(
        &minute_generation(&other, now).unwrap(),
        "SPY",
        "2026-10-26T18:50:00Z",
        "2026-10-26T19:00:00Z",
    );
    minutes_into(
        &minute_generation(&other, at("2026-10-26T18:59:00Z")).unwrap(),
        "QQQ",
        "2026-10-26T18:50:00Z",
        "2026-10-26T18:59:00Z",
    );
    assert_eq!(
        minute_dir_at(&other, "QQQ", now).unwrap(),
        minute_dir(
            &minute_generation(&other, at("2026-10-26T18:59:00Z")).unwrap(),
            "QQQ"
        )
        .unwrap(),
        "a generation without the symbol's dataset is passed over"
    );
    for dir in [root, older, other] {
        fs::remove_dir_all(dir.parent().unwrap()).ok();
    }
}

#[test]
fn a_generations_figures_are_the_flat_stores_and_a_missing_generation_refuses() {
    let root = scratch("figures");
    let flat = scratch("flat").with_file_name("flat");
    let now = at("2026-10-26T19:00:30Z");
    daily_into(
        &daily_generation(&root, day("2026-10-23")),
        "QQQ",
        day("2026-10-23"),
    );
    minutes_into(
        &minute_generation(&root, at("2026-10-26T19:00:00Z")).unwrap(),
        "QQQ",
        "2026-10-26T18:40:00Z",
        "2026-10-26T19:00:00Z",
    );
    daily_into(&flat, "QQQ", day("2026-10-23"));
    minutes_into(&flat, "QQQ", "2026-10-26T18:40:00Z", "2026-10-26T19:00:00Z");
    assert_eq!(
        liquidity(&root, "QQQ", now).unwrap(),
        liquidity(&flat, "QQQ", now).unwrap()
    );
    assert!(
        liquidity(&root, "QQQ", at("2026-10-26T20:30:00Z")).is_err(),
        "after Monday's close the reader names Monday's daily generation, not yet written"
    );
    assert!(
        liquidity(&root, "QQQ", at("2026-10-26T19:04:30Z")).is_err(),
        "four minutes on, no minute generation is near enough"
    );
    let stale = scratch("stale");
    daily_into(
        &daily_generation(&stale, day("2026-10-22")),
        "QQQ",
        day("2026-10-22"),
    );
    minutes_into(
        &minute_generation(&stale, at("2026-10-26T19:00:00Z")).unwrap(),
        "QQQ",
        "2026-10-26T18:40:00Z",
        "2026-10-26T19:00:00Z",
    );
    assert!(
        liquidity(&stale, "QQQ", now).is_err(),
        "the generation of an earlier session is never read in its place"
    );
    for dir in [
        root.parent().unwrap().to_owned(),
        flat.parent().unwrap().to_owned(),
        stale.parent().unwrap().to_owned(),
    ] {
        fs::remove_dir_all(dir).ok();
    }
}
