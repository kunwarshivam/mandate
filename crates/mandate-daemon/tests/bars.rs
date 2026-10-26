//! The runtime's reader of the liquidity inputs (DEC-2274 item 3): an opening's dry run reads the
//! stored bars at its own clock, and gets the same figures `mandate_bars::liquidity` gives; with
//! no root, no dataset for the instrument, or an empty window it gets none, so both checks stay
//! deferred; and an exit, a protective order or a flatten never reads it (`AGENTS.md` rule 13).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use mandate_canon::DecStr;
use mandate_daemon::bars::{BARS_ENV, BarsReader, liquidity_deferred};
use mandate_marketdata::dataset::Store;
use mandate_marketdata::model::{AssetClass, Bar, DatasetId, Feed, Kind, Records, Symbol};
use mandate_risk::{Deferred, Purpose};
use mandate_time::{Date, ExchangeCalendar, UtcNanos, new_york_midnight};

const NOW: &str = "2026-10-26T19:00:00Z";

fn at(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).unwrap()
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

/// QQQ's last 30 sessions to 2026-10-23 and its minutes of 2026-10-26, under a fresh root.
fn stored(name: &str) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("mandate-daemon-bars-{name}-{}", std::process::id()));
    fs::remove_dir_all(&root).ok();
    let store = Store::new(&root);
    let calendar = ExchangeCalendar::us_equities().unwrap();
    let mut days = Vec::new();
    let mut day = Date::new(2026, 8, 1).unwrap();
    while day <= Date::new(2026, 10, 23).unwrap() {
        if calendar.is_trading_day(day).unwrap() {
            days.push(day);
        }
        day = day.next().unwrap();
    }
    for day in days.split_off(days.len() - 30) {
        let bars = vec![bar(new_york_midnight(day).unwrap(), "140", "2000000")];
        store
            .put_day(&dataset("QQQ", "1Day"), day, &Records::Bars(bars))
            .unwrap();
    }
    let open = at("2026-10-26T13:30:00Z").secs();
    let minutes: Vec<Bar> = (0..390)
        .map(|n| {
            bar(
                UtcNanos::from_parts(open + 60 * n, 0).unwrap(),
                "140",
                "5000",
            )
        })
        .collect();
    store
        .put_day(
            &dataset("QQQ", "1Min"),
            Date::new(2026, 10, 26).unwrap(),
            &Records::Bars(minutes),
        )
        .unwrap();
    root
}

fn reader(root: &Path) -> BarsReader {
    BarsReader::new(root.to_str())
}

#[test]
fn an_opening_reads_the_shared_figures_at_its_own_clock() {
    let root = stored("opening");
    for purpose in [Purpose::Open, Purpose::Increase] {
        for now in [NOW, "2026-10-26T13:33:00Z"] {
            let facts = reader(&root).opening(purpose, "QQQ", at(now));
            assert_eq!(
                facts,
                Some(mandate_bars::liquidity(&root, "QQQ", at(now)).unwrap()),
                "{purpose:?} at {now}"
            );
            assert_eq!(liquidity_deferred(facts.as_ref()), BTreeSet::new());
        }
    }
    let early = reader(&root).opening(Purpose::Open, "QQQ", at("2026-10-26T13:33:00Z"));
    let later = reader(&root).opening(Purpose::Open, "QQQ", at(NOW));
    assert_ne!(
        early.map(|f| f.trailing_5m_volume),
        later.map(|f| f.trailing_5m_volume),
        "each clock reads its own window"
    );
    fs::remove_dir_all(&root).ok();
}

/// (a) Nothing it cannot read becomes a figure, and both checks then stay deferred.
#[test]
fn without_trusted_bars_both_checks_stay_deferred() {
    let root = stored("deferred");
    let both = BTreeSet::from([Deferred::LiquidityFloor, Deferred::Participation]);
    for (reader, symbol, now, why) in [
        (BarsReader::new(None), "QQQ", NOW, "no root"),
        (BarsReader::new(Some("")), "QQQ", NOW, "an empty root"),
        (reader(&root), "SPY", NOW, "no dataset for the instrument"),
        (
            reader(&root),
            "QQQ",
            "2026-10-26T13:30:30Z",
            "an empty window",
        ),
        (
            reader(&root),
            "QQQ",
            "2026-10-26T22:00:00Z",
            "after the close",
        ),
        (
            reader(Path::new("/nonexistent/bars")),
            "QQQ",
            NOW,
            "a root that is not there",
        ),
    ] {
        let facts = reader.opening(Purpose::Open, symbol, at(now));
        assert_eq!(facts, None, "{why}");
        assert_eq!(liquidity_deferred(facts.as_ref()), both, "{why}");
    }
    fs::remove_dir_all(&root).ok();
}

/// (d) An exit, a protective order or a flatten never reads the bars, even where an opening
/// would: risk reduction never waits on a figure.
#[test]
fn no_exit_reads_the_bars() {
    let root = stored("exits");
    let readable = reader(&root);
    assert!(readable.opening(Purpose::Open, "QQQ", at(NOW)).is_some());
    for purpose in [
        Purpose::DiscretionaryExit,
        Purpose::RiskExit,
        Purpose::OwnerExit,
        Purpose::Protective,
    ] {
        assert_eq!(
            readable.opening(purpose, "QQQ", at(NOW)),
            None,
            "{purpose:?}"
        );
    }
    fs::remove_dir_all(&root).ok();
}

#[test]
fn the_root_is_named_by_the_chart_s_variable() {
    assert_eq!(BARS_ENV, "MANDATE_BARS");
    assert_eq!(BarsReader::new(Some("")), BarsReader::default());
}
