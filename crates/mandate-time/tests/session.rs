//! The US-equities exchange calendar and its sessions (trading-domain spec §2.2, §4.3). Two
//! oracles, both independent of the implementation and of the checked-in data: the NYSE holiday and
//! early-close lists for 2018 to 2028, typed from NYSE's published schedules and its press releases
//! for the two unscheduled closures, and the US daylight-saving rule in force since 2007 (second
//! Sunday of March to first Sunday of November, switching at 02:00 local) with naive day counting.

use std::collections::BTreeSet;

use mandate_time::{
    CalendarDataError, Date, ExchangeCalendar, NewYorkTime, Session, TimeError, UtcNanos,
    new_york_instant,
};
use proptest::prelude::*;

/// NYSE full closures, 2018 to 2028: "Holidays & Trading Hours" (live page for 2026 to 2028, the
/// Internet Archive's copies for earlier years) and the press releases for 2018-12-05 and
/// 2025-01-09.
const NYSE_CLOSED: [&str; 106] = [
    "2018-01-01",
    "2018-01-15",
    "2018-02-19",
    "2018-03-30",
    "2018-05-28",
    "2018-07-04",
    "2018-09-03",
    "2018-11-22",
    "2018-12-05",
    "2018-12-25",
    "2019-01-01",
    "2019-01-21",
    "2019-02-18",
    "2019-04-19",
    "2019-05-27",
    "2019-07-04",
    "2019-09-02",
    "2019-11-28",
    "2019-12-25",
    "2020-01-01",
    "2020-01-20",
    "2020-02-17",
    "2020-04-10",
    "2020-05-25",
    "2020-07-03",
    "2020-09-07",
    "2020-11-26",
    "2020-12-25",
    "2021-01-01",
    "2021-01-18",
    "2021-02-15",
    "2021-04-02",
    "2021-05-31",
    "2021-07-05",
    "2021-09-06",
    "2021-11-25",
    "2021-12-24",
    "2022-01-17",
    "2022-02-21",
    "2022-04-15",
    "2022-05-30",
    "2022-06-20",
    "2022-07-04",
    "2022-09-05",
    "2022-11-24",
    "2022-12-26",
    "2023-01-02",
    "2023-01-16",
    "2023-02-20",
    "2023-04-07",
    "2023-05-29",
    "2023-06-19",
    "2023-07-04",
    "2023-09-04",
    "2023-11-23",
    "2023-12-25",
    "2024-01-01",
    "2024-01-15",
    "2024-02-19",
    "2024-03-29",
    "2024-05-27",
    "2024-06-19",
    "2024-07-04",
    "2024-09-02",
    "2024-11-28",
    "2024-12-25",
    "2025-01-01",
    "2025-01-09",
    "2025-01-20",
    "2025-02-17",
    "2025-04-18",
    "2025-05-26",
    "2025-06-19",
    "2025-07-04",
    "2025-09-01",
    "2025-11-27",
    "2025-12-25",
    "2026-01-01",
    "2026-01-19",
    "2026-02-16",
    "2026-04-03",
    "2026-05-25",
    "2026-06-19",
    "2026-07-03",
    "2026-09-07",
    "2026-11-26",
    "2026-12-25",
    "2027-01-01",
    "2027-01-18",
    "2027-02-15",
    "2027-03-26",
    "2027-05-31",
    "2027-06-18",
    "2027-07-05",
    "2027-09-06",
    "2027-11-25",
    "2027-12-24",
    "2028-01-17",
    "2028-02-21",
    "2028-04-14",
    "2028-05-29",
    "2028-06-19",
    "2028-07-04",
    "2028-09-04",
    "2028-11-23",
    "2028-12-25",
];

/// NYSE early closes (1:00 p.m., late trading sessions closing at 5:00 p.m.), 2018 to 2027.
const NYSE_EARLY_CLOSES: [&str; 21] = [
    "2018-07-03",
    "2018-11-23",
    "2018-12-24",
    "2019-07-03",
    "2019-11-29",
    "2019-12-24",
    "2020-11-27",
    "2020-12-24",
    "2021-11-26",
    "2022-11-25",
    "2023-07-03",
    "2023-11-24",
    "2024-07-03",
    "2024-11-29",
    "2024-12-24",
    "2025-07-03",
    "2025-11-28",
    "2025-12-24",
    "2026-11-27",
    "2026-12-24",
    "2027-11-26",
];

const NYSE_EARLY_CLOSES_2028: [&str; 2] = ["2028-07-03", "2028-11-24"];

const HEADER: &str = "valid 2026-01-01 2026-12-31\nhours 04:00 09:30 16:00 20:00\n";

fn us() -> ExchangeCalendar {
    ExchangeCalendar::us_equities().unwrap()
}

fn date(s: &str) -> Date {
    Date::parse(s).unwrap()
}

fn at(s: &str) -> UtcNanos {
    UtcNanos::parse(s).unwrap()
}

fn time(h: u8, m: u8) -> NewYorkTime {
    NewYorkTime::new(h, m).unwrap()
}

fn parse(body: &str) -> Result<ExchangeCalendar, CalendarDataError> {
    ExchangeCalendar::parse(&format!("{HEADER}{body}"))
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn month_len(y: i64, m: i64) -> i64 {
    match m {
        2 if is_leap(y) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn naive_days(y: i64, m: i64, d: i64) -> i64 {
    let years: i64 = (1970..y).map(|y| if is_leap(y) { 366 } else { 365 }).sum();
    let months: i64 = (1..m).map(|m| month_len(y, m)).sum();
    years + months + d - 1
}

fn naive_date(mut days: i64) -> (i64, i64, i64) {
    let mut y = 1970;
    while days >= if is_leap(y) { 366 } else { 365 } {
        days -= if is_leap(y) { 366 } else { 365 };
        y += 1;
    }
    let mut m = 1;
    while days >= month_len(y, m) {
        days -= month_len(y, m);
        m += 1;
    }
    (y, m, days + 1)
}

fn day_text(days: i64) -> String {
    let (y, m, d) = naive_date(days);
    format!("{y:04}-{m:02}-{d:02}")
}

fn days_of(text: &str) -> i64 {
    let n = |r: std::ops::Range<usize>| text[r].parse::<i64>().unwrap();
    naive_days(n(0..4), n(5..7), n(8..10))
}

/// 0 = Monday … 6 = Sunday.
fn weekday(days: i64) -> i64 {
    (days + 3) % 7
}

fn nth_sunday(y: i64, m: i64, n: i64) -> i64 {
    let first = naive_days(y, m, 1);
    first + (6 - weekday(first) + 7) % 7 + 7 * (n - 1)
}

/// Hours New York is behind UTC at a UTC instant.
fn behind_utc_at(secs: i64) -> i64 {
    let (y, _, _) = naive_date(secs / 86_400);
    let start = nth_sunday(y, 3, 2) * 86_400 + 7 * 3_600;
    let end = nth_sunday(y, 11, 1) * 86_400 + 6 * 3_600;
    if (start..end).contains(&secs) { 4 } else { 5 }
}

/// Hours New York is behind UTC at 03:00 or later local time on a local date.
fn behind_utc_on(days: i64) -> i64 {
    let (y, _, _) = naive_date(days);
    if (nth_sunday(y, 3, 2)..nth_sunday(y, 11, 1)).contains(&days) {
        4
    } else {
        5
    }
}

/// Seconds since the epoch of `minute` minutes past local midnight on a local date.
fn local_to_utc(days: i64, minute: i64) -> i64 {
    days * 86_400 + minute * 60 + behind_utc_on(days) * 3_600
}

/// The published schedule for one local date: `None` when closed, else the regular and
/// after-hours closes in minutes past midnight.
fn published(days: i64) -> Option<(i64, i64)> {
    let text = day_text(days);
    if weekday(days) >= 5 || NYSE_CLOSED.contains(&text.as_str()) {
        None
    } else if NYSE_EARLY_CLOSES.contains(&text.as_str())
        || NYSE_EARLY_CLOSES_2028.contains(&text.as_str())
    {
        Some((13 * 60, 17 * 60))
    } else {
        Some((16 * 60, 20 * 60))
    }
}

/// The session at a UTC instant under the published schedule and the spec §4.3 hours.
fn oracle_session(secs: i64) -> Option<Session> {
    let local = secs - behind_utc_at(secs) * 3_600;
    let (days, minute) = (local.div_euclid(86_400), local.rem_euclid(86_400) / 60);
    if minute >= 20 * 60 {
        return published(days + 1).map(|_| Session::Overnight);
    }
    let (close, end) = published(days)?;
    match minute {
        m if m < 4 * 60 => Some(Session::Overnight),
        m if m < 9 * 60 + 30 => Some(Session::PreMarket),
        m if m < close => Some(Session::Regular),
        m if m < end => Some(Session::AfterHours),
        _ => None,
    }
}

#[test]
#[ignore = "pending E2-4"]
fn the_checked_in_calendar_covers_2018_to_2028() {
    let calendar = us();
    assert_eq!(calendar.valid_from(), date("2018-01-01"));
    assert_eq!(calendar.valid_to(), date("2028-12-31"));
}

#[test]
#[ignore = "pending E2-4"]
fn every_weekday_from_2018_to_2028_trades_exactly_when_nyse_published_it_open() {
    let calendar = us();
    let mut closed = BTreeSet::new();
    let mut early = BTreeSet::new();
    for days in days_of("2018-01-01")..=days_of("2028-12-31") {
        let text = day_text(days);
        let trades = calendar.is_trading_day(date(&text)).unwrap();
        assert_eq!(trades, published(days).is_some(), "{text}");
        let sessions = calendar.sessions(date(&text)).unwrap();
        match published(days) {
            None => assert!(sessions.is_empty(), "{text} has sessions"),
            Some((close, end)) => {
                let got: Vec<(Session, i64, u32, i64, u32)> = sessions
                    .iter()
                    .map(|s| {
                        let (start, finish) = (s.start(), s.end());
                        (
                            s.session(),
                            start.secs(),
                            start.nanos(),
                            finish.secs(),
                            finish.nanos(),
                        )
                    })
                    .collect();
                let opens = local_to_utc(days, 4 * 60);
                let bell = local_to_utc(days, 9 * 60 + 30);
                let closes = local_to_utc(days, close);
                assert_eq!(
                    got,
                    [
                        (
                            Session::Overnight,
                            local_to_utc(days - 1, 20 * 60),
                            0,
                            opens,
                            0
                        ),
                        (Session::PreMarket, opens, 0, bell, 0),
                        (Session::Regular, bell, 0, closes, 0),
                        (Session::AfterHours, closes, 0, local_to_utc(days, end), 0),
                    ],
                    "{text}"
                );
                if close != 16 * 60 {
                    early.insert(text.clone());
                }
            }
        }
        if weekday(days) < 5 && !trades {
            closed.insert(text);
        }
    }
    let nyse: BTreeSet<String> = NYSE_CLOSED.iter().map(|s| (*s).to_owned()).collect();
    assert_eq!(closed, nyse);
    let nyse_early: BTreeSet<String> = NYSE_EARLY_CLOSES
        .iter()
        .chain(NYSE_EARLY_CLOSES_2028.iter())
        .map(|s| (*s).to_owned())
        .collect();
    assert_eq!(early, nyse_early);
}

#[test]
#[ignore = "pending E2-4"]
fn a_full_day_has_four_sessions_at_the_published_hours() {
    let sessions = us().sessions(date("2026-09-24")).unwrap();
    let got: Vec<(Session, UtcNanos, UtcNanos)> = sessions
        .iter()
        .map(|s| (s.session(), s.start(), s.end()))
        .collect();
    assert_eq!(
        got,
        [
            (
                Session::Overnight,
                at("2026-09-24T00:00:00.000000000Z"),
                at("2026-09-24T08:00:00.000000000Z")
            ),
            (
                Session::PreMarket,
                at("2026-09-24T08:00:00.000000000Z"),
                at("2026-09-24T13:30:00.000000000Z")
            ),
            (
                Session::Regular,
                at("2026-09-24T13:30:00.000000000Z"),
                at("2026-09-24T20:00:00.000000000Z")
            ),
            (
                Session::AfterHours,
                at("2026-09-24T20:00:00.000000000Z"),
                at("2026-09-25T00:00:00.000000000Z")
            ),
        ]
    );
}

#[test]
#[ignore = "pending E2-4"]
fn a_session_span_holds_its_start_but_not_its_end() {
    let before = |t: UtcNanos| UtcNanos::from_parts(t.secs() - 1, 999_999_999).unwrap();
    let after = |t: UtcNanos| UtcNanos::from_parts(t.secs(), 1).unwrap();
    let sessions = us().sessions(date("2026-09-24")).unwrap();
    assert_eq!(sessions.len(), 4);
    for span in sessions {
        let (start, end) = (span.start(), span.end());
        let name = span.session();
        assert!(
            !span.contains(before(start)),
            "{name} holds the nanosecond before its start"
        );
        assert!(span.contains(start), "{name} misses its start");
        assert!(
            span.contains(after(start)),
            "{name} misses the nanosecond after its start"
        );
        assert!(
            span.contains(before(end)),
            "{name} misses the nanosecond before its end"
        );
        assert!(!span.contains(end), "{name} holds its end");
        assert!(
            !span.contains(after(end)),
            "{name} holds the nanosecond after its end"
        );
    }
}

#[test]
#[ignore = "pending E2-4"]
fn early_closes_end_the_regular_session_at_13_00_and_after_hours_at_17_00() {
    let calendar = us();
    let winter = calendar.sessions(date("2026-11-27")).unwrap();
    assert_eq!(winter[1].end(), at("2026-11-27T14:30:00.000000000Z"));
    assert_eq!(winter[2].end(), at("2026-11-27T18:00:00.000000000Z"));
    assert_eq!(winter[3].start(), at("2026-11-27T18:00:00.000000000Z"));
    assert_eq!(winter[3].end(), at("2026-11-27T22:00:00.000000000Z"));
    let summer = calendar.sessions(date("2025-07-03")).unwrap();
    assert_eq!(summer[2].end(), at("2025-07-03T17:00:00.000000000Z"));
    assert_eq!(summer[3].end(), at("2025-07-03T21:00:00.000000000Z"));
    let christmas_eve = calendar.sessions(date("2026-12-24")).unwrap();
    assert_eq!(christmas_eve[2].end(), at("2026-12-24T18:00:00.000000000Z"));
}

#[test]
#[ignore = "pending E2-4"]
fn holidays_and_unscheduled_closures_have_no_session() {
    let calendar = us();
    for closed in [
        "2026-11-26",
        "2026-12-25",
        "2018-12-05",
        "2025-01-09",
        "2026-09-26",
    ] {
        assert_eq!(calendar.is_trading_day(date(closed)), Ok(false), "{closed}");
        assert_eq!(calendar.sessions(date(closed)), Ok(Vec::new()), "{closed}");
    }
    assert_eq!(calendar.is_trading_day(date("2026-11-27")), Ok(true));
}

#[test]
#[ignore = "pending E2-4"]
fn daylight_saving_changes_move_sessions_in_utc_but_not_in_new_york() {
    let calendar = us();
    let friday = calendar.sessions(date("2026-03-06")).unwrap();
    let monday = calendar.sessions(date("2026-03-09")).unwrap();
    assert_eq!(friday[2].start(), at("2026-03-06T14:30:00.000000000Z"));
    assert_eq!(friday[3].end(), at("2026-03-07T01:00:00.000000000Z"));
    assert_eq!(monday[0].start(), at("2026-03-09T00:00:00.000000000Z"));
    assert_eq!(monday[2].start(), at("2026-03-09T13:30:00.000000000Z"));
    assert_eq!(
        monday[0].start().secs() - friday[3].end().secs(),
        47 * 3_600
    );

    let friday = calendar.sessions(date("2026-10-30")).unwrap();
    let monday = calendar.sessions(date("2026-11-02")).unwrap();
    assert_eq!(friday[2].start(), at("2026-10-30T13:30:00.000000000Z"));
    assert_eq!(friday[3].end(), at("2026-10-31T00:00:00.000000000Z"));
    assert_eq!(monday[0].start(), at("2026-11-02T01:00:00.000000000Z"));
    assert_eq!(monday[2].start(), at("2026-11-02T14:30:00.000000000Z"));
    assert_eq!(
        monday[0].start().secs() - friday[3].end().secs(),
        49 * 3_600
    );

    let plain = calendar.sessions(date("2026-09-28")).unwrap();
    let before = calendar.sessions(date("2026-09-25")).unwrap();
    assert_eq!(plain[0].start().secs() - before[3].end().secs(), 48 * 3_600);
}

#[test]
#[ignore = "pending E2-4"]
fn new_york_instants_reject_times_a_daylight_saving_change_skips_or_repeats() {
    let spring = date("2026-03-08");
    let fall = date("2026-11-01");
    assert_eq!(
        new_york_instant(spring, time(2, 30)),
        Err(TimeError::InvalidDate)
    );
    assert_eq!(
        new_york_instant(spring, time(2, 0)),
        Err(TimeError::InvalidDate)
    );
    assert_eq!(
        new_york_instant(fall, time(1, 30)),
        Err(TimeError::InvalidDate)
    );
    assert_eq!(
        new_york_instant(fall, time(1, 0)),
        Err(TimeError::InvalidDate)
    );
    assert_eq!(
        new_york_instant(spring, time(1, 59)),
        Ok(at("2026-03-08T06:59:00.000000000Z"))
    );
    assert_eq!(
        new_york_instant(spring, time(3, 0)),
        Ok(at("2026-03-08T07:00:00.000000000Z"))
    );
    assert_eq!(
        new_york_instant(fall, time(0, 59)),
        Ok(at("2026-11-01T04:59:00.000000000Z"))
    );
    assert_eq!(
        new_york_instant(fall, time(2, 0)),
        Ok(at("2026-11-01T07:00:00.000000000Z"))
    );
    assert_eq!(
        new_york_instant(date("2026-01-15"), time(23, 59)),
        Ok(at("2026-01-16T04:59:00.000000000Z"))
    );
}

#[test]
#[ignore = "pending E2-4"]
fn session_at_names_the_session_holding_an_instant_at_each_boundary() {
    let calendar = us();
    let cases = [
        ("2026-09-24T07:59:59.999999999Z", Some(Session::Overnight)),
        ("2026-09-24T08:00:00.000000000Z", Some(Session::PreMarket)),
        ("2026-09-24T13:29:59.999999999Z", Some(Session::PreMarket)),
        ("2026-09-24T13:30:00.000000000Z", Some(Session::Regular)),
        ("2026-09-24T19:59:59.999999999Z", Some(Session::Regular)),
        ("2026-09-24T20:00:00.000000000Z", Some(Session::AfterHours)),
        ("2026-09-24T23:59:59.999999999Z", Some(Session::AfterHours)),
        ("2026-09-25T00:00:00.000000000Z", Some(Session::Overnight)),
        ("2026-09-26T00:00:00.000000000Z", None),
        ("2026-09-26T16:00:00.000000000Z", None),
        ("2026-09-27T23:59:59.999999999Z", None),
        ("2026-09-28T00:00:00.000000000Z", Some(Session::Overnight)),
        ("2026-11-27T17:59:59.999999999Z", Some(Session::Regular)),
        ("2026-11-27T18:00:00.000000000Z", Some(Session::AfterHours)),
        ("2026-11-27T21:59:59.999999999Z", Some(Session::AfterHours)),
        ("2026-11-27T22:00:00.000000000Z", None),
        ("2026-11-26T01:00:00.000000000Z", None),
        ("2026-11-26T15:00:00.000000000Z", None),
        ("2026-11-27T01:00:00.000000000Z", Some(Session::Overnight)),
        ("2026-09-07T00:00:00.000000000Z", None),
        ("2026-09-08T00:00:00.000000000Z", Some(Session::Overnight)),
    ];
    for (instant, expected) in cases {
        assert_eq!(calendar.session_at(at(instant)), Ok(expected), "{instant}");
    }
}

#[test]
#[ignore = "pending E2-4"]
fn the_evening_after_an_early_close_is_closed_until_the_next_overnight_session() {
    let calendar = parse("early_close 2026-09-24 13:00 17:00 test\n").unwrap();
    let cases = [
        ("2026-09-24T20:59:59.999999999Z", Some(Session::AfterHours)),
        ("2026-09-24T21:00:00.000000000Z", None),
        ("2026-09-24T23:59:59.999999999Z", None),
        ("2026-09-25T00:00:00.000000000Z", Some(Session::Overnight)),
    ];
    for (instant, expected) in cases {
        assert_eq!(calendar.session_at(at(instant)), Ok(expected), "{instant}");
    }
}

#[test]
#[ignore = "pending E2-4"]
fn every_date_outside_the_validity_range_is_an_error() {
    let calendar = us();
    for outside in ["2017-12-29", "2017-12-31", "2029-01-01", "2029-01-02"] {
        assert_eq!(
            calendar.is_trading_day(date(outside)),
            Err(TimeError::OutsideCalendar),
            "{outside}"
        );
        assert_eq!(
            calendar.sessions(date(outside)),
            Err(TimeError::OutsideCalendar),
            "{outside}"
        );
    }
    assert_eq!(calendar.is_trading_day(date("2018-01-01")), Ok(false));
    assert_eq!(calendar.is_trading_day(date("2018-01-02")), Ok(true));
    assert_eq!(calendar.is_trading_day(date("2028-12-29")), Ok(true));
    assert_eq!(calendar.is_trading_day(date("2028-12-31")), Ok(false));
    let cases = [
        (
            "2018-01-01T04:59:59.999999999Z",
            Err(TimeError::OutsideCalendar),
        ),
        ("2018-01-01T05:00:00.000000000Z", Ok(None)),
        ("2028-12-29T20:59:59.999999999Z", Ok(Some(Session::Regular))),
        ("2028-12-31T15:00:00.000000000Z", Ok(None)),
        ("2029-01-01T00:59:59.999999999Z", Ok(None)),
        (
            "2029-01-01T01:00:00.000000000Z",
            Err(TimeError::OutsideCalendar),
        ),
    ];
    for (instant, expected) in cases {
        assert_eq!(calendar.session_at(at(instant)), expected, "{instant}");
    }
}

#[test]
fn session_names_are_stable() {
    let names: Vec<String> = [
        Session::Overnight,
        Session::PreMarket,
        Session::Regular,
        Session::AfterHours,
    ]
    .iter()
    .map(|s| format!("{s}={}", s.as_str()))
    .collect();
    assert_eq!(
        names,
        [
            "overnight=overnight",
            "pre_market=pre_market",
            "regular=regular",
            "after_hours=after_hours"
        ]
    );
}

#[test]
#[ignore = "pending E2-4"]
fn new_york_times_are_hh_mm_within_a_day() {
    assert_eq!(
        NewYorkTime::new(23, 59).map(|t| (t.hour(), t.minute())),
        Ok((23, 59))
    );
    assert_eq!(
        NewYorkTime::new(0, 0).map(|t| t.to_string()),
        Ok("00:00".to_owned())
    );
    assert_eq!(NewYorkTime::new(24, 0), Err(TimeError::InvalidDate));
    assert_eq!(NewYorkTime::new(0, 60), Err(TimeError::InvalidDate));
    assert_eq!(NewYorkTime::parse("09:30"), Ok(time(9, 30)));
    assert_eq!(NewYorkTime::parse("23:59"), Ok(time(23, 59)));
    assert_eq!(NewYorkTime::parse("24:00"), Err(TimeError::InvalidDate));
    for bad in [
        "0930", "9:30", "09:3", "09:300", "a9:30", "09:3a", "09:30 ", "", ":",
    ] {
        assert_eq!(NewYorkTime::parse(bad), Err(TimeError::Syntax), "{bad:?}");
    }
    assert!(time(9, 30) < time(13, 0) && time(13, 0) < time(13, 1));
}

#[test]
#[ignore = "pending E2-4"]
fn a_calendar_needs_only_its_header_and_skips_comments_and_blank_lines() {
    let text = format!("# a comment\n\n{HEADER}# another\n\nclosed 2026-01-02 test\n");
    let calendar = ExchangeCalendar::parse(&text).unwrap();
    assert_eq!(calendar.valid_from(), date("2026-01-01"));
    assert_eq!(calendar.valid_to(), date("2026-12-31"));
    assert_eq!(calendar.is_trading_day(date("2026-01-02")), Ok(false));
    assert_eq!(calendar.is_trading_day(date("2026-01-05")), Ok(true));
    let one_day =
        ExchangeCalendar::parse("valid 2026-06-01 2026-06-01\nhours 04:00 09:30 16:00 20:00");
    assert_eq!(one_day.map(|c| c.valid_to()), Ok(date("2026-06-01")));
    let edges = parse("closed 2026-01-01 first\nclosed 2026-12-31 last\n").unwrap();
    assert_eq!(edges.is_trading_day(date("2026-01-01")), Ok(false));
    assert_eq!(edges.is_trading_day(date("2026-12-31")), Ok(false));
}

#[test]
#[ignore = "pending E2-4"]
fn malformed_records_are_syntax_errors_on_their_line() {
    let cases = [
        "bogus 1",
        "closed",
        "closed 2026-01-02",
        "closed 2026-01-02 ",
        "early_close 2026-11-27 13:00 17:00",
        "early_close 2026-11-27 13:00 17:00 ",
        "early_close 2026-11-27 13:00",
        "Closed 2026-01-02 test",
    ];
    for body in cases {
        assert_eq!(
            parse(&format!("{body}\n")),
            Err(CalendarDataError::Syntax { line: 3 }),
            "{body:?}"
        );
    }
    for header in [
        "valid 2026-01-01\nhours 04:00 09:30 16:00 20:00",
        "valid 2026-01-01 2026-12-31 2027-01-01\nhours 04:00 09:30 16:00 20:00",
        "valid 2026-01-01  2026-12-31\nhours 04:00 09:30 16:00 20:00",
    ] {
        assert_eq!(
            ExchangeCalendar::parse(header),
            Err(CalendarDataError::Syntax { line: 1 }),
            "{header:?}"
        );
    }
    for hours in [
        "hours 04:00 09:30 16:00",
        "hours 04:00 09:30 16:00 20:00 21:00",
    ] {
        assert_eq!(
            ExchangeCalendar::parse(&format!("valid 2026-01-01 2026-12-31\n{hours}")),
            Err(CalendarDataError::Syntax { line: 2 }),
            "{hours:?}"
        );
    }
}

#[test]
#[ignore = "pending E2-4"]
fn bad_dates_and_times_are_value_errors_with_their_cause() {
    let value = |line, source| Err(CalendarDataError::Value { line, source });
    assert_eq!(
        parse("closed 2026-02-30 test\n"),
        value(3, TimeError::InvalidDate)
    );
    assert_eq!(
        parse("closed 2026-1-02 test\n"),
        value(3, TimeError::Syntax)
    );
    assert_eq!(
        parse("early_close 2026-11-27 13:60 17:00 test\n"),
        value(3, TimeError::InvalidDate)
    );
    assert_eq!(
        parse("early_close 2026-11-27 13:00 5pm test\n"),
        value(3, TimeError::Syntax)
    );
    assert_eq!(
        parse("early_close 2026-13-27 13:00 17:00 test\n"),
        value(3, TimeError::InvalidDate)
    );
    assert_eq!(
        ExchangeCalendar::parse("valid 1969-12-31 2026-12-31\n"),
        value(1, TimeError::OutOfRange)
    );
    assert_eq!(
        ExchangeCalendar::parse("valid 2026-01-01 2026-12-32\n"),
        value(1, TimeError::InvalidDate)
    );
    assert_eq!(
        ExchangeCalendar::parse("valid 2026-01-01 2026-12-31\r\n"),
        value(1, TimeError::Syntax)
    );
    for (field, hours) in [
        (TimeError::Syntax, "4:00 09:30 16:00 20:00"),
        (TimeError::InvalidDate, "04:00 09:60 16:00 20:00"),
        (TimeError::InvalidDate, "04:00 09:30 25:00 20:00"),
        (TimeError::Syntax, "04:00 09:30 16:00 8pm"),
    ] {
        assert_eq!(
            ExchangeCalendar::parse(&format!("valid 2026-01-01 2026-12-31\nhours {hours}")),
            value(2, field),
            "{hours}"
        );
    }
}

#[test]
#[ignore = "pending E2-4"]
fn valid_comes_first_and_hours_second_each_exactly_once() {
    let header = |line| Err(CalendarDataError::Header { line });
    assert_eq!(
        ExchangeCalendar::parse("hours 04:00 09:30 16:00 20:00\nvalid 2026-01-01 2026-12-31"),
        header(1)
    );
    assert_eq!(
        ExchangeCalendar::parse("valid 2026-01-01 2026-12-31\nclosed 2026-01-02 test"),
        header(2)
    );
    assert_eq!(
        ExchangeCalendar::parse("closed 2026-01-02 test\nvalid 2026-01-01 2026-12-31"),
        header(1)
    );
    assert_eq!(
        ExchangeCalendar::parse(
            "valid 2026-01-01 2026-12-31\nearly_close 2026-11-27 13:00 17:00 t"
        ),
        header(2)
    );
    assert_eq!(parse("valid 2026-01-01 2026-12-31\n"), header(3));
    assert_eq!(parse("hours 04:00 09:30 16:00 20:00\n"), header(3));
    assert_eq!(
        ExchangeCalendar::parse(""),
        Err(CalendarDataError::Incomplete)
    );
    assert_eq!(
        ExchangeCalendar::parse("# only a comment\n"),
        Err(CalendarDataError::Incomplete)
    );
    assert_eq!(
        ExchangeCalendar::parse("valid 2026-01-01 2026-12-31\n"),
        Err(CalendarDataError::Incomplete)
    );
}

#[test]
#[ignore = "pending E2-4"]
fn the_range_and_the_session_hours_must_increase() {
    assert_eq!(
        ExchangeCalendar::parse("valid 2026-12-31 2026-01-01\n"),
        Err(CalendarDataError::InvertedRange { line: 1 })
    );
    for hours in [
        "04:00 04:00 16:00 20:00",
        "09:31 09:30 16:00 20:00",
        "04:00 09:30 09:30 20:00",
        "04:00 16:01 16:00 20:00",
        "04:00 09:30 16:00 16:00",
        "04:00 09:30 20:01 20:00",
    ] {
        assert_eq!(
            ExchangeCalendar::parse(&format!("valid 2026-01-01 2026-12-31\nhours {hours}")),
            Err(CalendarDataError::Hours { line: 2 }),
            "{hours}"
        );
    }
    assert!(
        ExchangeCalendar::parse("valid 2026-01-01 2026-12-31\nhours 04:00 04:01 04:02 04:03")
            .is_ok()
    );
}

#[test]
#[ignore = "pending E2-4"]
fn listed_dates_are_weekdays_inside_the_range_in_increasing_order() {
    assert_eq!(
        parse("closed 2025-12-31 test\n"),
        Err(CalendarDataError::OutsideRange { line: 3 })
    );
    assert_eq!(
        parse("early_close 2027-01-01 13:00 17:00 test\n"),
        Err(CalendarDataError::OutsideRange { line: 3 })
    );
    assert_eq!(
        parse("closed 2026-01-03 saturday\n"),
        Err(CalendarDataError::Weekend { line: 3 })
    );
    assert_eq!(
        parse("early_close 2026-01-04 13:00 17:00 sunday\n"),
        Err(CalendarDataError::Weekend { line: 3 })
    );
    let order = |line| Err(CalendarDataError::Order { line });
    assert_eq!(
        parse("closed 2026-01-02 a\nclosed 2026-01-02 b\n"),
        order(4)
    );
    assert_eq!(
        parse("closed 2026-01-05 a\nclosed 2026-01-02 b\n"),
        order(4)
    );
    assert_eq!(
        parse("closed 2026-11-27 a\nearly_close 2026-11-27 13:00 17:00 b\n"),
        order(4)
    );
    assert_eq!(
        parse("early_close 2026-11-27 13:00 17:00 a\nclosed 2026-11-26 b\n"),
        order(4)
    );
}

#[test]
#[ignore = "pending E2-4"]
fn an_early_close_ends_inside_the_usual_sessions() {
    let early = |line| Err(CalendarDataError::EarlyClose { line });
    for (regular, after) in [
        ("09:30", "17:00"),
        ("09:29", "17:00"),
        ("16:00", "17:00"),
        ("16:01", "17:00"),
        ("13:00", "13:00"),
        ("13:00", "12:59"),
        ("13:00", "20:01"),
    ] {
        assert_eq!(
            parse(&format!("early_close 2026-11-27 {regular} {after} test\n")),
            early(3),
            "{regular} {after}"
        );
    }
    let widest = parse("early_close 2026-11-27 09:31 20:00 test\n").unwrap();
    let sessions = widest.sessions(date("2026-11-27")).unwrap();
    assert_eq!(sessions[2].end(), at("2026-11-27T14:31:00.000000000Z"));
    assert_eq!(sessions[3].end(), at("2026-11-28T01:00:00.000000000Z"));
    let narrowest = parse("early_close 2026-11-27 15:59 16:00 test\n").unwrap();
    let sessions = narrowest.sessions(date("2026-11-27")).unwrap();
    assert_eq!(sessions[2].end(), at("2026-11-27T20:59:00.000000000Z"));
    assert_eq!(sessions[3].end(), at("2026-11-27T21:00:00.000000000Z"));
}

#[test]
#[ignore = "pending E2-4"]
fn line_numbers_count_comments_and_blank_lines() {
    let text = format!("# header\n{HEADER}\n# note\nclosed 2026-01-03 saturday\n");
    assert_eq!(
        ExchangeCalendar::parse(&text),
        Err(CalendarDataError::Weekend { line: 6 })
    );
}

#[test]
fn calendar_data_errors_have_stable_codes_and_name_their_line() {
    let errors = [
        CalendarDataError::Syntax { line: 1 },
        CalendarDataError::Value {
            line: 1,
            source: TimeError::Syntax,
        },
        CalendarDataError::Header { line: 1 },
        CalendarDataError::InvertedRange { line: 1 },
        CalendarDataError::Hours { line: 1 },
        CalendarDataError::OutsideRange { line: 1 },
        CalendarDataError::Weekend { line: 1 },
        CalendarDataError::Order { line: 1 },
        CalendarDataError::EarlyClose { line: 1 },
    ];
    let codes: Vec<&str> = errors.iter().map(CalendarDataError::code).collect();
    assert_eq!(
        codes,
        [
            "syntax",
            "value",
            "header",
            "inverted_range",
            "hours",
            "outside_range",
            "weekend",
            "order",
            "early_close"
        ]
    );
    for error in errors {
        assert!(error.to_string().starts_with("line 1: "), "{error}");
    }
    assert_eq!(CalendarDataError::Incomplete.code(), "incomplete");
}

/// 2018-01-01T05:00:00Z (midnight in New York) to 2029-01-01T01:00:00Z (20:00 on the last valid
/// day, after which the next day's overnight session would be needed).
fn in_range_secs() -> impl Strategy<Value = i64> {
    let first = days_of("2018-01-01") * 86_400 + 5 * 3_600;
    let last = days_of("2029-01-01") * 86_400 + 3_600;
    first..last
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2_000))]

    #[test]
    #[ignore = "pending E2-4"]
    fn session_at_agrees_with_the_published_schedule_at_every_instant(
        secs in in_range_secs(),
        nanos in 0u32..1_000_000_000,
    ) {
        let calendar = us();
        let instant = UtcNanos::from_parts(secs, nanos).unwrap();
        prop_assert_eq!(calendar.session_at(instant), Ok(oracle_session(secs)));
    }

    #[test]
    #[ignore = "pending E2-4"]
    fn a_trading_days_sessions_tile_from_the_previous_evening_to_its_after_hours_close(
        days in days_of("2018-01-01")..=days_of("2028-12-31"),
    ) {
        let calendar = us();
        let sessions = calendar.sessions(date(&day_text(days))).unwrap();
        match published(days) {
            None => prop_assert!(sessions.is_empty()),
            Some((close, end)) => {
                let kinds: Vec<Session> = sessions.iter().map(|s| s.session()).collect();
                prop_assert_eq!(
                    kinds,
                    vec![Session::Overnight, Session::PreMarket, Session::Regular, Session::AfterHours]
                );
                let bounds = [
                    local_to_utc(days - 1, 20 * 60),
                    local_to_utc(days, 4 * 60),
                    local_to_utc(days, 9 * 60 + 30),
                    local_to_utc(days, close),
                    local_to_utc(days, end),
                ];
                for (span, window) in sessions.iter().zip(bounds.windows(2)) {
                    prop_assert_eq!((span.start().secs(), span.end().secs()), (window[0], window[1]));
                    prop_assert_eq!((span.start().nanos(), span.end().nanos()), (0, 0));
                }
            }
        }
    }
}
