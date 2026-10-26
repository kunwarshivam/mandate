//! The recorded series laid onto the calendar: the sessions the dataset holds, each session's close
//! as a function of its date alone, and the dataset the store ends up with, judged by
//! `mandate-marketdata`'s own inspection with the trust check's conditions, not by this crate.

use std::fs;
use std::path::PathBuf;

use mandate_datad::series::{self, Anchor, SESSIONS};
use mandate_marketdata::inspect::{GapClass, inspect};
use mandate_num::Usd;
use mandate_time::{Date, ExchangeCalendar, UtcNanos};
use proptest::prelude::*;

fn at(rfc3339: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(rfc3339).unwrap()
}

fn anchor() -> Anchor {
    Anchor::parse("2026-01-02=600").unwrap()
}

/// The trading days in `[from, to]`, walked one calendar day at a time: the oracle's own count.
fn trading_days(calendar: &ExchangeCalendar, from: Date, to: Date) -> Vec<Date> {
    let mut days = Vec::new();
    let mut day = from;
    while day <= to {
        if calendar.is_trading_day(day).unwrap() {
            days.push(day);
        }
        day = day.next().unwrap();
    }
    days
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mandate-datad-{name}-{}", std::process::id()));
    fs::remove_dir_all(&dir).ok();
    dir
}

/// Every instant from 2026-02-01 to 2026-12-31, to the second.
fn instants() -> impl Strategy<Value = UtcNanos> {
    let from = at("2026-02-01T00:00:00Z").secs();
    let to = at("2026-12-31T23:59:59Z").secs();
    (from..to).prop_map(|s| UtcNanos::from_parts(s, 0).unwrap())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// The sessions are the last [`SESSIONS`] trading days, ending on the session the model host
    /// and the shell's trust check both name: `last_completed_regular_session(now)`.
    #[test]
    fn the_sessions_end_on_the_last_completed_session(now in instants()) {
        let calendar = ExchangeCalendar::us_equities().unwrap();
        let latest = calendar.last_completed_regular_session(now).unwrap().unwrap();
        let sessions = series::sessions(&calendar, now).unwrap();
        prop_assert_eq!(sessions.len(), SESSIONS);
        prop_assert_eq!(sessions.last().copied(), Some(latest));
        let first = *sessions.first().unwrap();
        prop_assert_eq!(trading_days(&calendar, first, latest), sessions);
    }

    /// A session's close depends on its date alone: the anchor's close plus one step per trading
    /// day after the anchor, counted by the oracle's own walk, so a later clock never rewrites a
    /// stored day.
    #[test]
    fn a_close_is_a_function_of_its_date(now in instants()) {
        let calendar = ExchangeCalendar::us_equities().unwrap();
        let a = anchor();
        let step = Usd::parse(series::STEP).unwrap();
        for day in series::sessions(&calendar, now).unwrap() {
            let mut expected = Usd::parse("600").unwrap();
            if day >= a.date {
                for _ in 1..trading_days(&calendar, a.date, day).len() {
                    expected = expected.checked_add(step).unwrap();
                }
            } else {
                for _ in 1..trading_days(&calendar, day, a.date).len() {
                    expected = expected.checked_sub(step).unwrap();
                }
            }
            let close = series::close_on(&calendar, &a, day).unwrap();
            prop_assert_eq!(close.to_string(), expected.to_string(), "{}", day);
        }
    }
}

/// The closes rise session over session, so the moving-average crossover's fast mean is above its
/// slow one on every session (DEC-961 item 3's series, laid by date).
#[test]
fn the_closes_rise_every_session() {
    let calendar = ExchangeCalendar::us_equities().unwrap();
    let sessions = series::sessions(&calendar, at("2026-10-26T01:00:00Z")).unwrap();
    let closes: Vec<Usd> = sessions
        .iter()
        .map(|d| {
            Usd::parse(
                &series::close_on(&calendar, &anchor(), *d)
                    .unwrap()
                    .to_string(),
            )
            .unwrap()
        })
        .collect();
    assert!(closes.windows(2).all(|w| w[0] < w[1]), "{closes:?}");
}

/// An anchor is a date and a positive close; a close the walk would take to zero or below, or an
/// anchor too far from the session, is refused, never clamped.
#[test]
fn an_anchor_and_its_reach_are_checked() {
    for bad in [
        "",
        "2026-01-02",
        "2026-01-02=",
        "=600",
        "2026-13-02=600",
        "2026-01-02=-1",
        "2026-01-02=0",
        "2026-01-02=abc",
    ] {
        assert!(Anchor::parse(bad).is_err(), "{bad}");
    }
    let calendar = ExchangeCalendar::us_equities().unwrap();
    let low = Anchor::parse("2026-10-23=0.5").unwrap();
    let before = Date::new(2026, 10, 1).unwrap();
    assert!(
        series::close_on(&calendar, &low, before).is_err(),
        "a close at or below zero"
    );
    let far = Date::new(2024, 6, 3).unwrap();
    assert!(
        series::close_on(&calendar, &anchor(), far).is_err(),
        "past the reach"
    );
}

/// The stored dataset is one the shell's trust check admits: daily bars of the symbol, no problem,
/// duplicate, empty trading day or true gap, ending on the last completed session. Writing it again
/// at the same clock leaves every day unchanged, and at a later clock adds the new session without
/// a conflict.
#[test]
fn the_dataset_is_trusted_and_rewriting_it_conflicts_with_nothing() {
    let root = scratch("trusted");
    let calendar = ExchangeCalendar::us_equities().unwrap();
    let now = at("2026-10-26T01:00:00Z");
    let written = series::publish(&root, "SPY", &anchor(), &calendar, now).unwrap();
    assert_eq!(written.outcomes.len(), SESSIONS);
    let inspection = inspect(&written.dir).unwrap();
    assert_eq!(inspection.dataset.symbol().as_str(), "SPY");
    assert!(inspection.problems.is_empty(), "{:?}", inspection.problems);
    assert!(inspection.duplicates.is_empty());
    assert!(inspection.coverage.empty.is_empty());
    assert!(!inspection.gaps.iter().any(|g| {
        g.stretches
            .iter()
            .any(|s| matches!(s.class, GapClass::TrueGap | GapClass::Unclassified))
    }));
    let latest = calendar.last_completed_regular_session(now).unwrap();
    assert_eq!(inspection.coverage.span.map(|s| s.last()), latest);

    let again = series::publish(&root, "SPY", &anchor(), &calendar, now).unwrap();
    assert!(again.outcomes.iter().all(|o| !o.written), "unchanged");
    let later = series::publish(
        &root,
        "SPY",
        &anchor(),
        &calendar,
        at("2026-10-27T21:00:00Z"),
    )
    .unwrap();
    let added: Vec<Date> = later
        .outcomes
        .iter()
        .filter(|o| o.written)
        .map(|o| o.day)
        .collect();
    assert_eq!(
        added,
        [
            Date::new(2026, 10, 26).unwrap(),
            Date::new(2026, 10, 27).unwrap()
        ],
        "the two sessions completed since, and no other"
    );
    let inspection = inspect(&later.dir).unwrap();
    assert_eq!(
        inspection.coverage.span.map(|s| s.last()),
        Some(Date::new(2026, 10, 27).unwrap())
    );
    fs::remove_dir_all(&root).ok();
}

/// The dataset's directory is under the store root, beneath `cell-recorded/`, so its path names it
/// a recording rather than a download.
#[test]
fn the_dataset_lives_under_the_recorded_root() {
    let root = scratch("root");
    let calendar = ExchangeCalendar::us_equities().unwrap();
    let written = series::publish(
        &root,
        "SPY",
        &anchor(),
        &calendar,
        at("2026-10-26T01:00:00Z"),
    )
    .unwrap();
    assert!(
        written.dir.starts_with(root.join(series::RECORDED_ROOT)),
        "{}",
        written.dir.display()
    );
    fs::remove_dir_all(&root).ok();
}
