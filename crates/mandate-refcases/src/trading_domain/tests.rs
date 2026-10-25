//! The harness's own readers at the edges no reference case reaches (#2343): a backtest bar's
//! session at every boundary minute of spec §4.3, an early close's included; a list of dates read
//! back whole; and a config's `extends` chain at and past its depth limit. Each expectation is
//! written out here from the spec's times, never derived from the code under test.

use mandate_accounting::AssetClass;
use mandate_sim::Session;
use mandate_time::{Date, TradingCalendar, UtcNanos};
use serde_json::{Value as Json, json};

use super::backtest::label;
use super::{MAX_EXTENDS, dates, resolved_config};

fn at(text: &str) -> Result<UtcNanos, String> {
    UtcNanos::parse(text).map_err(|e| format!("{text}: {e}"))
}

fn day(text: &str) -> Result<Date, String> {
    Date::parse(text).map_err(|e| format!("{text}: {e}"))
}

/// Each bar's start, in UTC (New York is five hours behind in January and November), its session,
/// and that session's first instant: 04:00, 09:30, 16:00 (13:00 on an early close) and 20:00 ET,
/// the overnight session starting at the previous evening's 20:00.
#[test]
fn a_bars_session_changes_exactly_at_each_boundary_minute() -> Result<(), String> {
    let calendar = TradingCalendar::new(day("2026-01-01")?, day("2026-12-31")?, [], [])
        .map_err(|e| e.to_string())?;
    let early = [day("2026-11-27")?];
    let cases = [
        (
            "2026-01-06T08:59:00.000000000Z",
            Session::Overnight,
            "2026-01-06T01:00:00.000000000Z",
        ),
        (
            "2026-01-06T09:00:00.000000000Z",
            Session::PreMarket,
            "2026-01-06T09:00:00.000000000Z",
        ),
        (
            "2026-01-06T14:29:00.000000000Z",
            Session::PreMarket,
            "2026-01-06T09:00:00.000000000Z",
        ),
        (
            "2026-01-06T14:30:00.000000000Z",
            Session::Regular,
            "2026-01-06T14:30:00.000000000Z",
        ),
        (
            "2026-01-06T20:59:00.000000000Z",
            Session::Regular,
            "2026-01-06T14:30:00.000000000Z",
        ),
        (
            "2026-01-06T21:00:00.000000000Z",
            Session::AfterHours,
            "2026-01-06T21:00:00.000000000Z",
        ),
        (
            "2026-01-07T00:59:00.000000000Z",
            Session::AfterHours,
            "2026-01-06T21:00:00.000000000Z",
        ),
        (
            "2026-01-07T01:00:00.000000000Z",
            Session::Overnight,
            "2026-01-07T01:00:00.000000000Z",
        ),
        (
            "2026-11-27T17:59:00.000000000Z",
            Session::Regular,
            "2026-11-27T14:30:00.000000000Z",
        ),
        (
            "2026-11-27T18:00:00.000000000Z",
            Session::AfterHours,
            "2026-11-27T18:00:00.000000000Z",
        ),
        (
            "2026-11-26T21:00:00.000000000Z",
            Session::AfterHours,
            "2026-11-26T21:00:00.000000000Z",
        ),
    ];
    for (start, session, session_start) in cases {
        let (got, got_start, _) = label(at(start)?, AssetClass::UsEquity, &calendar, &early)?;
        if (got, got_start) != (session, at(session_start)?) {
            return Err(format!(
                "{start}: {got:?} from {got_start:?}, not {session:?} from {session_start}"
            ));
        }
    }
    Ok(())
}

/// A list of dates reads back as every date in it, in order; a member that is not a date is
/// refused.
#[test]
fn a_list_of_dates_reads_back_every_date() -> Result<(), String> {
    let listed = json!(["2026-11-26", "2026-11-27"]);
    if dates(&listed, "early_closes")? != [day("2026-11-26")?, day("2026-11-27")?] {
        return Err("the dates did not read back".to_owned());
    }
    if dates(&json!([]), "early_closes")? != Vec::<Date>::new() {
        return Err("an empty list read back as dates".to_owned());
    }
    if dates(&json!(["2026-11-26", 7]), "early_closes").is_ok() {
        return Err("a number read as a date".to_owned());
    }
    Ok(())
}

/// A fixture whose config `c0` extends `c1`, and so on to `c{last}`, which extends nothing; each
/// sets its own `depth_<n>` member.
fn chain(last: usize) -> (Json, Json) {
    let mut configs = serde_json::Map::new();
    for (n, next) in (0..=last).zip(1..) {
        let mut config = serde_json::Map::new();
        config.insert(format!("depth_{n}"), json!(n));
        if n < last {
            config.insert("extends".to_owned(), json!(format!("c{next}")));
        }
        configs.insert(format!("c{n}"), Json::Object(config));
    }
    (json!({ "configs": configs }), json!({ "config": "c0" }))
}

/// A chain of exactly the limit's configs resolves, every member of every config merged and
/// `extends` dropped; one config more is refused as too deep.
#[test]
fn a_config_chain_resolves_up_to_its_depth_limit_and_no_further() -> Result<(), String> {
    let deepest = MAX_EXTENDS.checked_sub(1).ok_or("a limit of no configs")?;
    let (fixture, case) = chain(deepest);
    let resolved = resolved_config(&fixture, &case)?;
    for n in 0..MAX_EXTENDS {
        if resolved.get(format!("depth_{n}")) != Some(&json!(n)) {
            return Err(format!("depth_{n} was not merged: {resolved}"));
        }
    }
    if resolved.get("extends").is_some() {
        return Err(format!("`extends` survived the merge: {resolved}"));
    }
    let (fixture, case) = chain(MAX_EXTENDS);
    match resolved_config(&fixture, &case) {
        Err(message) if message.contains("extends too deeply") => Ok(()),
        other => Err(format!("a chain past the limit resolved: {other:?}")),
    }
}
