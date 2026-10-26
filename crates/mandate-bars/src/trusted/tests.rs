//! The trust check's cases, moved with it from `mandate-shell` (DEC-2385).

use std::fs;
use std::path::{Path, PathBuf};

use mandate_canon::DecStr;
use mandate_marketdata::actions::{RecordedActions, write_actions};
use mandate_marketdata::dataset::{Store, partition_name};
use mandate_marketdata::inspect::{self, ActionsReport, Problem};
use mandate_marketdata::model::{
    AssetClass, Bar, CorporateActions, DatasetId, DayRange, Feed, Kind, Records, Split, SplitRatio,
    Symbol, TimeUnit, Timeframe,
};
use mandate_num::Price;
use mandate_time::{Date, UtcNanos};

use super::{
    BarsError, listed_days, split_inside, trusted, trusted_daily_bars, trusted_daily_closes,
};

/// A scratch directory, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Result<Scratch, String> {
        let path = std::env::temp_dir().join(format!(
            "mandate-shell-adapters-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).map_err(|e| e.to_string())?;
        Ok(Scratch(path))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn day(text: &str) -> Result<Date, String> {
    Date::parse(text).map_err(|e| e.to_string())
}

fn as_of() -> Result<UtcNanos, String> {
    UtcNanos::parse("2026-09-25T20:00:00.000000000Z").map_err(|error| error.to_string())
}

fn dataset(symbol: &str, timeframe: &str) -> Result<DatasetId, String> {
    DatasetId::new(
        AssetClass::UsEquity,
        Feed::Iex,
        Kind::Bars(timeframe.parse().map_err(|e| format!("{e:?}"))?),
        Symbol::parse(symbol).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

fn bar(on: Date, close: &str) -> Result<Bar, String> {
    let dec = |text: &str| DecStr::parse(text).map_err(|e| e.to_string());
    Ok(Bar {
        start: UtcNanos::parse_rfc3339(&format!("{on}T04:00:00Z")).map_err(|e| e.to_string())?,
        open: dec(close)?,
        high: dec(close)?,
        low: dec(close)?,
        close: dec(close)?,
        volume: dec("50000000")?,
        vwap: dec(close)?,
        trade_count: 400_000,
    })
}

/// One daily bar per weekday from 2026-08-24, a Monday, with these closes; `skip` names
/// weekdays given no partition at all, and `empty` weekdays listed with no bar.
fn write(
    root: &Path,
    id: &DatasetId,
    closes: &[&str],
    skip: &[&str],
    empty: &[&str],
) -> Result<PathBuf, String> {
    let store = Store::new(root);
    let mut on = day("2026-08-24")?;
    for close in closes {
        while on.is_weekend()
            || skip.contains(&on.to_string().as_str())
            || empty.contains(&on.to_string().as_str())
        {
            if empty.contains(&on.to_string().as_str()) {
                store
                    .put_day(id, on, &Records::Bars(Vec::new()))
                    .map_err(|e| e.to_string())?;
            }
            on = on.next().map_err(|e| e.to_string())?;
        }
        store
            .put_day(id, on, &Records::Bars(vec![bar(on, close)?]))
            .map_err(|e| e.to_string())?;
        on = on.next().map_err(|e| e.to_string())?;
    }
    Ok(store.dataset_dir(id))
}

fn rising() -> Vec<String> {
    (231..256).map(|whole| format!("{whole}.20")).collect()
}

fn aapl(root: &Path, skip: &[&str], empty: &[&str]) -> Result<PathBuf, String> {
    let closes = rising();
    let closes: Vec<&str> = closes.iter().map(String::as_str).collect();
    write(root, &dataset("AAPL", "1Day")?, &closes, skip, empty)
}

fn untrusted(answer: Result<Vec<(Date, Price)>, BarsError>) -> Result<&'static str, String> {
    match answer {
        Err(BarsError::Untrusted { what }) => Ok(what),
        other => Err(format!("expected an untrusted refusal, got {other:?}")),
    }
}

/// Storage order and time order differ: the partitions are written newest day first, so only
/// a read in the manifest's date order answers oldest first.
#[test]
fn stored_closes_are_read_oldest_first_exactly_as_stored() -> Result<(), String> {
    let scratch = Scratch::new("read")?;
    let id = dataset("AAPL", "1Day")?;
    let store = Store::new(&scratch.0);
    let closes = rising();
    let mut days = Vec::new();
    let mut on = day("2026-08-24")?;
    while days.len() < closes.len() {
        if !on.is_weekend() {
            days.push(on);
        }
        on = on.next().map_err(|e| e.to_string())?;
    }
    for (on, close) in days.iter().zip(&closes).rev() {
        store
            .put_day(&id, *on, &Records::Bars(vec![bar(*on, close)?]))
            .map_err(|e| e.to_string())?;
    }
    store
        .put_day(&id, day("2026-08-29")?, &Records::Bars(Vec::new()))
        .map_err(|e| e.to_string())?;
    let read = trusted_daily_closes(&store.dataset_dir(&id), "AAPL", as_of()?)
        .map_err(|e| e.to_string())?;
    let expected: Vec<String> = closes.iter().map(|c| canonical(c)).collect();
    let read: Vec<String> = read.iter().map(|(_, close)| close.to_string()).collect();
    assert_eq!(read, expected);
    Ok(())
}

/// The trusted bars are every listed day's one bar, oldest first, each exactly as stored, and the
/// same span the closes are read over: the liquidity figures and the model host rest on one series.
#[test]
fn stored_bars_are_read_whole_and_match_the_closes() -> Result<(), String> {
    let scratch = Scratch::new("bars")?;
    let dir = aapl(&scratch.0, &[], &[])?;
    let bars = trusted_daily_bars(&dir, "AAPL", as_of()?).map_err(|e| e.to_string())?;
    let closes = trusted_daily_closes(&dir, "AAPL", as_of()?).map_err(|e| e.to_string())?;
    assert_eq!(bars.len(), rising().len());
    assert_eq!(bars.len(), closes.len());
    for (bar, (on, close)) in bars.iter().zip(&closes) {
        assert_eq!(*bar, self::bar(*on, &close.to_string())?, "{on}");
    }
    let refused = trusted_daily_bars(&dir, "MSFT", as_of()?);
    assert!(matches!(refused, Err(BarsError::Untrusted { .. })));
    Ok(())
}

#[test]
fn stored_bars_must_end_on_the_last_completed_equity_session() -> Result<(), String> {
    let scratch = Scratch::new("stale")?;
    let dir = aapl(&scratch.0, &[], &[])?;
    let before_monday_close =
        UtcNanos::parse("2026-09-28T19:59:59.000000000Z").map_err(|e| e.to_string())?;
    let fresh =
        trusted_daily_closes(&dir, "AAPL", before_monday_close).map_err(|e| e.to_string())?;
    assert_eq!(
        fresh.len(),
        25,
        "Friday remains the last completed session until Monday's regular close"
    );
    let after_monday_close =
        UtcNanos::parse("2026-09-28T20:00:00.000000000Z").map_err(|e| e.to_string())?;
    let what = untrusted(trusted_daily_closes(&dir, "AAPL", after_monday_close))?;
    assert_eq!(
        what,
        "the dataset does not end on the last completed equity session"
    );
    Ok(())
}

/// Review round 1, major 2: two bars on one listed day, newest first, would put the closes out
/// of time order and make the windows span fewer days than the envelope names.
#[test]
fn a_listed_day_holding_more_than_one_bar_is_refused() -> Result<(), String> {
    let scratch = Scratch::new("two-bars")?;
    let dir = aapl(&scratch.0, &["2026-08-26"], &[])?;
    let on = day("2026-08-26")?;
    let at = |hour: &str, close: &str| -> Result<Bar, String> {
        Ok(Bar {
            start: UtcNanos::parse_rfc3339(&format!("{on}T{hour}:00:00Z"))
                .map_err(|e| e.to_string())?,
            ..bar(on, close)?
        })
    };
    Store::new(&scratch.0)
        .put_day(
            &dataset("AAPL", "1Day")?,
            on,
            &Records::Bars(vec![at("09", "999.99")?, at("04", "111.11")?]),
        )
        .map_err(|e| e.to_string())?;
    let after_last_listed =
        UtcNanos::parse("2026-09-28T20:00:00.000000000Z").map_err(|e| e.to_string())?;
    let what = untrusted(trusted_daily_closes(&dir, "AAPL", after_last_listed))?;
    assert_eq!(what, "a listed day does not hold exactly one bar");
    Ok(())
}
#[test]
fn another_instruments_bars_are_refused() -> Result<(), String> {
    let scratch = Scratch::new("symbol")?;
    let dir = aapl(&scratch.0, &[], &[])?;
    let what = untrusted(trusted_daily_closes(&dir, "MSFT", as_of()?))?;
    assert_eq!(what, "the dataset is another instrument's");
    Ok(())
}

#[test]
fn bars_that_are_not_daily_are_refused() -> Result<(), String> {
    let scratch = Scratch::new("hourly")?;
    let dir = write(
        &scratch.0,
        &dataset("AAPL", "1Hour")?,
        &["231.20"],
        &[],
        &[],
    )?;
    let what = untrusted(trusted_daily_closes(&dir, "AAPL", as_of()?))?;
    assert_eq!(what, "the dataset is not daily bars");
    Ok(())
}

#[test]
fn an_altered_partition_is_refused() -> Result<(), String> {
    let scratch = Scratch::new("altered")?;
    let dir = aapl(&scratch.0, &[], &[])?;
    let path = dir.join(partition_name(day("2026-08-26")?));
    let mut bytes = fs::read(&path).map_err(|e| e.to_string())?;
    bytes.push(0);
    fs::write(&path, bytes).map_err(|e| e.to_string())?;
    let what = untrusted(trusted_daily_closes(&dir, "AAPL", as_of()?))?;
    assert_eq!(what, "a partition cannot be trusted");
    Ok(())
}

#[test]
fn two_bars_with_one_start_are_refused() -> Result<(), String> {
    let scratch = Scratch::new("duplicate")?;
    let dir = aapl(&scratch.0, &["2026-08-26"], &[])?;
    let on = day("2026-08-26")?;
    Store::new(&scratch.0)
        .put_day(
            &dataset("AAPL", "1Day")?,
            on,
            &Records::Bars(vec![bar(on, "233.20")?, bar(on, "233.30")?]),
        )
        .map_err(|e| e.to_string())?;
    let what = untrusted(trusted_daily_closes(&dir, "AAPL", as_of()?))?;
    assert_eq!(what, "two bars share a start");
    Ok(())
}

#[test]
fn a_trading_day_listed_without_a_bar_is_refused() -> Result<(), String> {
    let scratch = Scratch::new("empty")?;
    let dir = aapl(&scratch.0, &[], &["2026-08-26"])?;
    let what = untrusted(trusted_daily_closes(&dir, "AAPL", as_of()?))?;
    assert_eq!(what, "a trading day has no bar");
    Ok(())
}

#[test]
fn a_trading_day_never_fetched_is_refused() -> Result<(), String> {
    let scratch = Scratch::new("gap")?;
    let dir = aapl(&scratch.0, &["2026-08-26"], &[])?;
    let what = untrusted(trusted_daily_closes(&dir, "AAPL", as_of()?))?;
    assert_eq!(what, "a trading day was never fetched");
    Ok(())
}

#[test]
fn a_split_inside_the_span_or_actions_short_of_it_are_refused() -> Result<(), String> {
    let scratch = Scratch::new("split")?;
    let dir = aapl(&scratch.0, &[], &[])?;
    let mut inspection = inspect::inspect(&dir).map_err(|e| e.to_string())?;
    assert!(trusted(&inspection, "AAPL", as_of()?).is_ok());
    let span = inspection.coverage.span.ok_or("no span")?;
    let symbol = Symbol::parse("AAPL").map_err(|e| e.to_string())?;
    let split = |ex_date: Date| -> Result<Split, String> {
        Ok(Split {
            id: "split".to_owned(),
            ex_date,
            ratio: SplitRatio::new(4, 1).map_err(|e| e.to_string())?,
        })
    };
    let with = |splits: Vec<Split>| RecordedActions {
        range: span,
        actions: CorporateActions {
            splits,
            ..CorporateActions::none(symbol.clone())
        },
    };
    let cases = [
        (span.first(), false),
        (span.first().next().map_err(|e| e.to_string())?, true),
        (span.last(), true),
        (span.last().next().map_err(|e| e.to_string())?, false),
    ];
    for (ex_date, refused) in cases {
        inspection.corporate_actions = ActionsReport::Applied {
            recorded: with(vec![split(ex_date)?]),
            as_of: span.last(),
        };
        assert_eq!(split_inside(&inspection), refused, "{ex_date}");
    }
    inspection.corporate_actions = ActionsReport::Applied {
        recorded: with(Vec::new()),
        as_of: span.last(),
    };
    assert!(!split_inside(&inspection));
    inspection.corporate_actions = ActionsReport::Incomplete(with(Vec::new()));
    assert!(split_inside(&inspection));
    assert!(matches!(
        trusted(&inspection, "AAPL", as_of()?),
        Err(BarsError::Untrusted {
            what: "a split lies inside the span, or the corporate actions do not cover it"
        })
    ));
    inspection.corporate_actions = ActionsReport::Applied {
        recorded: with(vec![split(span.last())?]),
        as_of: span.last(),
    };
    inspection.coverage.span = None;
    assert!(!split_inside(&inspection));
    Ok(())
}

#[test]
fn stored_actions_covering_the_span_with_no_split_are_trusted() -> Result<(), String> {
    let scratch = Scratch::new("actions")?;
    let dir = aapl(&scratch.0, &[], &[])?;
    let span = DayRange::new(day("2026-08-24")?, day("2026-09-25")?).map_err(|e| e.to_string())?;
    let symbol = Symbol::parse("AAPL").map_err(|e| e.to_string())?;
    write_actions(
        &dir,
        &RecordedActions {
            range: span,
            actions: CorporateActions::none(symbol),
        },
    )
    .map_err(|e| e.to_string())?;
    let closes = trusted_daily_closes(&dir, "AAPL", as_of()?).map_err(|e| e.to_string())?;
    assert_eq!(closes.len(), 25);
    Ok(())
}

/// `mandate-num`'s canonical text of a decimal: trailing fractional zeros and a bare point
/// dropped, and nothing else, so `"100"` stays `"100"` (#248 review, minor 2).
fn canonical(text: &str) -> String {
    match text.split_once('.') {
        Some((whole, places)) => match places.trim_end_matches('0') {
            "" => whole.to_owned(),
            kept => format!("{whole}.{kept}"),
        },
        None => text.to_owned(),
    }
}

/// The closes as prices, in canonical text (`Price` refuses a trailing fractional zero).
fn prices(closes: &[String]) -> Result<Vec<Price>, String> {
    closes
        .iter()
        .map(|close| Price::parse(&canonical(close)).map_err(|e| e.to_string()))
        .collect()
}

/// #248 review, minor 2: a close whose digits end in 0 is read as itself. The old helper
/// trimmed every trailing `0`, so `"100"` became `"1"`.
#[test]
fn a_close_ending_in_zero_keeps_its_value() -> Result<(), String> {
    let closes = ["100", "250.50", "1000.000", "0.10", "99.9"].map(str::to_owned);
    let read: Vec<String> = prices(&closes)?.iter().map(ToString::to_string).collect();
    assert_eq!(read, ["100", "250.5", "1000", "0.1", "99.9"]);
    Ok(())
}

/// #241 review round 2, minor 3: a partition on disk that the manifest does not list is never
/// read. `inspect` reports it as unlisted, so the whole dataset is refused before any partition
/// is read, and the stray close cannot reach the windows.
#[test]
fn a_partition_the_manifest_does_not_list_is_never_read() -> Result<(), String> {
    let scratch = Scratch::new("unlisted")?;
    let dir = aapl(&scratch.0, &[], &[])?;
    let listed = dir.join(partition_name(day("2026-08-26")?));
    let stray = dir.join(partition_name(day("2026-08-29")?));
    fs::copy(&listed, &stray).map_err(|e| e.to_string())?;
    let problems = inspect::inspect(&dir).map_err(|e| e.to_string())?.problems;
    assert_eq!(
        problems,
        vec![Problem::Unlisted {
            file: "2026-08-29.parquet".to_owned()
        }],
        "the one problem `inspect` finds is the unlisted partition (#248 review, minor 1)"
    );
    let what = untrusted(trusted_daily_closes(&dir, "AAPL", as_of()?))?;
    assert_eq!(what, "a partition cannot be trusted");
    Ok(())
}

/// #248 review, minor 1: the loop in `closes` walks the manifest, not the directory. Called
/// past `trusted`, over a directory holding an unlisted partition with a close of its own,
/// it reads the listed closes and never the stray.
#[test]
fn the_closes_are_read_from_the_manifest_and_never_the_directory() -> Result<(), String> {
    let scratch = Scratch::new("manifest-only")?;
    let dir = aapl(&scratch.0, &[], &[])?;
    let elsewhere = Scratch::new("manifest-only-stray")?;
    let stray = write(&elsewhere.0, &dataset("AAPL", "1Day")?, &["1.23"], &[], &[])?;
    fs::copy(
        stray.join(partition_name(day("2026-08-24")?)),
        dir.join(partition_name(day("2026-08-29")?)),
    )
    .map_err(|e| e.to_string())?;
    let daily = Kind::Bars(Timeframe::new(1, TimeUnit::Day).map_err(|e| e.to_string())?);
    let read: Vec<String> = listed_days(&dir, daily)
        .map_err(|e| e.to_string())?
        .iter()
        .map(|(_, bar)| bar.close.as_str().to_owned())
        .collect();
    let expected: Vec<String> = rising().iter().map(|c| canonical(c)).collect();
    assert_eq!(read, expected, "the stray 1.23 is never read");
    Ok(())
}
