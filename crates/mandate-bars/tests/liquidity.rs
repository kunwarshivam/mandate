//! The liquidity figures both gates read (DEC-2274): from a store's daily and one-minute datasets
//! of an instrument, at the reader's clock, each figure recomputed here its own way, and every
//! input that cannot be trusted refused, never a figure.

use std::fs;
use std::path::{Path, PathBuf};

use mandate_bars::{BarsError, LiquidityFacts, daily_dir, liquidity, minute_dir};
use mandate_canon::DecStr;
use mandate_marketdata::dataset::Store;
use mandate_marketdata::model::{AssetClass, Bar, DatasetId, Feed, Kind, Records, Symbol};
use mandate_num::{Price, Qty, Usd};
use mandate_time::{Date, ExchangeCalendar, UtcNanos, new_york_midnight};

fn at(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).unwrap()
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mandate-bars-liquidity-{name}-{}",
        std::process::id()
    ));
    fs::remove_dir_all(&dir).ok();
    dir
}

/// The clock the tests read at: 15:00 New York on Monday 2026-10-26, inside the regular session.
const NOW: &str = "2026-10-26T19:00:00Z";
/// The last completed session at [`NOW`].
const LAST: (u16, u8, u8) = (2026, 10, 23);

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

/// The 30 trading days ending on [`LAST`], oldest first.
fn sessions() -> Vec<Date> {
    let calendar = ExchangeCalendar::us_equities().unwrap();
    let last = Date::new(LAST.0, LAST.1, LAST.2).unwrap();
    let mut day = Date::new(2026, 8, 1).unwrap();
    let mut days = Vec::new();
    while day <= last {
        if calendar.is_trading_day(day).unwrap() {
            days.push(day);
        }
        day = day.next().unwrap();
    }
    days.split_off(days.len() - 30)
}

/// Session `n`'s close and volume: both vary, so the median and the mean differ from any one bar.
fn session(n: usize) -> (String, String) {
    (
        format!("{}.5", 100 + (n * 7) % 13),
        format!("{}", 1_000_000 + ((n * 37) % 29) * 10_000),
    )
}

/// Stores `symbol`'s 30 daily sessions under `root`.
fn daily(root: &Path, symbol: &str) {
    let store = Store::new(root);
    let id = dataset(symbol, "1Day");
    for (n, day) in sessions().into_iter().enumerate() {
        let (close, volume) = session(n);
        let bars = vec![bar(new_york_midnight(day).unwrap(), &close, &volume)];
        store.put_day(&id, day, &Records::Bars(bars)).unwrap();
    }
}

/// Stores `symbol`'s regular session on 2026-10-26, minute `n` with volume `100 + n`.
fn minutes(root: &Path, symbol: &str) {
    minutes_on(root, symbol, "2026-10-26");
}

/// Stores `symbol`'s regular session on `day`, opening at 13:30Z, minute `n` with volume `100 + n`.
fn minutes_on(root: &Path, symbol: &str, day: &str) {
    let open = at(&format!("{day}T13:30:00Z")).secs();
    let bars: Vec<Bar> = (0..390)
        .map(|n| {
            bar(
                UtcNanos::from_parts(open + 60 * n, 0).unwrap(),
                "110",
                &(100 + n).to_string(),
            )
        })
        .collect();
    Store::new(root)
        .put_day(
            &dataset(symbol, "1Min"),
            Date::parse(day).unwrap(),
            &Records::Bars(bars),
        )
        .unwrap();
}

fn stored(name: &str) -> PathBuf {
    let root = scratch(name);
    daily(&root, "QQQ");
    minutes(&root, "QQQ");
    root
}

/// The figures recomputed from [`session`] and [`minutes`]' rules: the last 20 sessions' lower
/// median of `close × volume`, their whole-share mean volume, the last close, and the minutes
/// that start in the five before `now` and end by it.
fn oracle(now: UtcNanos) -> LiquidityFacts {
    let window: Vec<(String, String)> = (10..30).map(session).collect();
    let mut dollars: Vec<Usd> = window
        .iter()
        .map(|(close, volume)| {
            Qty::parse(volume)
                .unwrap()
                .notional(Price::parse(close).unwrap())
                .unwrap()
        })
        .collect();
    dollars.sort();
    let total: u64 = window.iter().map(|(_, v)| v.parse::<u64>().unwrap()).sum();
    let open = at("2026-10-26T13:30:00Z").secs();
    let trailing: i64 = (0..390_i64)
        .filter(|n| {
            let start = open + 60 * n;
            start >= now.secs() - 300 && start + 60 <= now.secs()
        })
        .map(|n| 100 + n)
        .sum();
    LiquidityFacts {
        prior_close: Price::parse(&window[19].0).unwrap(),
        median_dollar_volume_20d: dollars[9],
        adv_20d: Qty::parse(&(total / 20).to_string()).unwrap(),
        trailing_5m_volume: Qty::parse(&trailing.to_string()).unwrap(),
    }
}

#[test]
fn the_figures_are_the_rules_over_the_stored_bars() {
    let root = stored("figures");
    for now in [NOW, "2026-10-26T19:00:30Z", "2026-10-26T13:36:00Z"] {
        let now = at(now);
        assert_eq!(
            liquidity(&root, "QQQ", now).unwrap(),
            oracle(now),
            "{now:?}"
        );
    }
    fs::remove_dir_all(&root).ok();
}

/// The datasets are found by `mandate-marketdata`'s layout under the root.
#[test]
fn the_datasets_are_found_by_the_store_layout() {
    let root = PathBuf::from("/r");
    assert_eq!(
        daily_dir(&root, "QQQ").unwrap(),
        root.join("alpaca/iex/bars-1Day/QQQ")
    );
    assert_eq!(
        minute_dir(&root, "QQQ").unwrap(),
        root.join("alpaca/iex/bars-1Min/QQQ")
    );
    assert!(daily_dir(&root, "qqq").is_err());
    assert!(minute_dir(&root, "").is_err());
}

/// (a) Nothing the reader cannot trust becomes a figure: a store with no dataset for the symbol,
/// one with daily bars but no minutes, an empty trailing window, too few sessions, or a stale
/// series each refuses.
#[test]
fn an_untrusted_input_is_refused_never_a_figure() {
    let root = stored("refused");
    assert!(
        liquidity(&root, "SPY", at(NOW)).is_err(),
        "no dataset for the symbol"
    );
    let daily_only = scratch("daily-only");
    daily(&daily_only, "QQQ");
    assert!(
        liquidity(&daily_only, "QQQ", at(NOW)).is_err(),
        "no minutes"
    );
    for (now, why) in [
        ("2026-10-26T13:30:30Z", "no complete minute yet"),
        ("2026-10-26T21:00:00Z", "after the close"),
        ("2026-10-27T15:00:00Z", "a day whose minutes are not stored"),
    ] {
        assert!(liquidity(&root, "QQQ", at(now)).is_err(), "{why}");
    }
    minutes_on(&root, "QQQ", "2026-11-03");
    assert!(
        matches!(
            liquidity(&root, "QQQ", at("2026-11-03T15:00:00Z")),
            Err(BarsError::Untrusted { .. })
        ),
        "a daily series that no longer ends on the last completed session, though the minutes are there"
    );
    assert!(
        matches!(
            liquidity(&root, "QQQ", at("2026-10-26T13:30:30Z")),
            Err(BarsError::Liquidity(_))
        ),
        "an empty window is the trailing rule's own refusal"
    );
    let few = scratch("few");
    let store = Store::new(&few);
    let id = dataset("QQQ", "1Day");
    for (n, day) in sessions().into_iter().enumerate().skip(12) {
        let (close, volume) = session(n);
        store
            .put_day(
                &id,
                day,
                &Records::Bars(vec![bar(new_york_midnight(day).unwrap(), &close, &volume)]),
            )
            .unwrap();
    }
    minutes(&few, "QQQ");
    assert!(
        matches!(
            liquidity(&few, "QQQ", at(NOW)),
            Err(BarsError::Liquidity(_))
        ),
        "18 sessions are fewer than the 20 the figures span"
    );
    for dir in [root, daily_only, few] {
        fs::remove_dir_all(&dir).ok();
    }
}

/// (b) The figures rest on the partitions and the clock alone: the same partitions copied to
/// another root, read at the same clock, give the same figures, and a later read changes nothing
/// stored.
#[test]
fn the_same_partitions_at_the_same_clock_give_the_same_figures() {
    let root = stored("replay");
    let first = liquidity(&root, "QQQ", at(NOW)).unwrap();
    let copy = scratch("replay-copy");
    copy_tree(&root, &copy);
    assert_eq!(liquidity(&copy, "QQQ", at(NOW)).unwrap(), first);
    assert_eq!(liquidity(&root, "QQQ", at(NOW)).unwrap(), first);
    for dir in [root, copy] {
        fs::remove_dir_all(&dir).ok();
    }
}

/// (c) Each reader's clock is its own: the trailing volume read at the binding clock is that
/// clock's window, not the one an earlier reader saw.
#[test]
fn each_clock_reads_its_own_window() {
    let root = stored("clocks");
    let advisory = liquidity(&root, "QQQ", at(NOW)).unwrap();
    let binding = liquidity(&root, "QQQ", at("2026-10-26T19:02:00Z")).unwrap();
    assert_ne!(advisory.trailing_5m_volume, binding.trailing_5m_volume);
    assert_eq!(binding, oracle(at("2026-10-26T19:02:00Z")));
    fs::remove_dir_all(&root).ok();
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}
