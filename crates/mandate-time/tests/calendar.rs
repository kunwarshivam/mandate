//! Trading calendars, equity trade dates, settlement dates, and New York wall-clock conversions
//! (trading-domain spec §2.2, §8.3, §8.4). The oracle applies the US daylight-saving rule in force
//! since 2007 (second Sunday of March to first Sunday of November, switching at 02:00 local) with
//! naive day counting, independent of the time-zone database.

use std::collections::BTreeSet;

use mandate_time::{
    Date, TimeError, TradingCalendar, UtcNanos, new_york_date_and_hour, new_york_midnight,
};
use proptest::collection::btree_set;
use proptest::prelude::*;

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn month_len(y: i64, m: i64) -> i64 {
    match m {
        2 => {
            if is_leap(y) {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn naive_days(y: i64, m: i64, d: i64) -> i64 {
    let mut days = 0;
    for year in 1970..y {
        days += if is_leap(year) { 366 } else { 365 };
    }
    for month in 1..m {
        days += month_len(y, month);
    }
    days + d - 1
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

/// 0 = Monday … 6 = Sunday.
fn weekday(days: i64) -> i64 {
    (days + 3) % 7
}

/// Day number of the n-th Sunday (1-based) of a month.
fn nth_sunday(y: i64, m: i64, n: i64) -> i64 {
    let first = naive_days(y, m, 1);
    let to_sunday = (6 - weekday(first) + 7) % 7;
    first + to_sunday + 7 * (n - 1)
}

/// Offset from UTC in seconds at a UTC instant.
fn oracle_offset(secs: i64) -> i64 {
    let (y, _, _) = naive_date(secs / 86_400);
    let start = nth_sunday(y, 3, 2) * 86_400 + 7 * 3_600;
    let end = nth_sunday(y, 11, 1) * 86_400 + 6 * 3_600;
    if (start..end).contains(&secs) {
        -4 * 3_600
    } else {
        -5 * 3_600
    }
}

fn date_of(days: i64) -> Date {
    let (y, m, d) = naive_date(days);
    Date::new(y as u16, m as u8, d as u8).unwrap()
}

fn days_of(date: Date) -> i64 {
    naive_days(
        i64::from(date.year()),
        i64::from(date.month()),
        i64::from(date.day()),
    )
}

/// 2007-01-01 to 2099-12-31 in seconds.
fn modern_secs() -> impl Strategy<Value = i64> {
    naive_days(2007, 1, 1) * 86_400..naive_days(2100, 1, 1) * 86_400
}

fn instant(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).unwrap()
}

/// A calendar over a random range with random holidays, and the oracle's view of it.
#[derive(Debug, Clone)]
struct Fixture {
    from: i64,
    to: i64,
    trading: BTreeSet<i64>,
    settlement: BTreeSet<i64>,
}

impl Fixture {
    fn calendar(&self) -> TradingCalendar {
        TradingCalendar::new(
            date_of(self.from),
            date_of(self.to),
            self.trading.iter().map(|d| date_of(*d)),
            self.settlement.iter().map(|d| date_of(*d)),
        )
        .unwrap()
    }

    fn trading_day(&self, day: i64) -> bool {
        weekday(day) < 5 && !self.trading.contains(&day)
    }

    fn settlement_day(&self, day: i64) -> bool {
        self.trading_day(day) && !self.settlement.contains(&day)
    }
}

fn fixture() -> impl Strategy<Value = Fixture> {
    (naive_days(2007, 1, 1)..naive_days(2099, 1, 1)).prop_flat_map(|from| {
        let to = from + 120;
        (btree_set(from..=to, 0..30), btree_set(from..=to, 0..30)).prop_map(
            move |(trading, settlement)| Fixture {
                from,
                to,
                trading,
                settlement,
            },
        )
    })
}

proptest! {
    #[test]
    fn new_york_date_and_hour_follow_the_daylight_saving_rule(secs in modern_secs()) {
        let local = secs + oracle_offset(secs);
        let (date, hour) = new_york_date_and_hour(instant(secs)).unwrap();
        prop_assert_eq!(date, date_of(local.div_euclid(86_400)));
        prop_assert_eq!(i64::from(hour), local.rem_euclid(86_400) / 3_600);
    }

    #[test]
    fn new_york_midnight_is_the_first_instant_of_the_local_day(day in naive_days(2007, 1, 1)..naive_days(2099, 12, 31)) {
        let midnight = new_york_midnight(date_of(day)).unwrap();
        let standard = day * 86_400 + 5 * 3_600;
        let expected = if oracle_offset(standard - 3_600) == -4 * 3_600 { standard - 3_600 } else { standard };
        prop_assert_eq!(midnight.secs(), expected);
        prop_assert_eq!(midnight.nanos(), 0);
    }

    #[test]
    fn trading_and_settlement_days_follow_the_rule(f in fixture(), offset in 0i64..=120) {
        let calendar = f.calendar();
        let day = f.from + offset;
        prop_assert_eq!(calendar.is_trading_day(date_of(day)).unwrap(), f.trading_day(day));
        prop_assert_eq!(calendar.is_settlement_day(date_of(day)).unwrap(), f.settlement_day(day));
    }

    #[test]
    fn equity_trade_date_is_the_trading_day_of_execution_with_the_evening_cutoff(f in fixture(), offset in 0i64..=100 * 86_400) {
        let calendar = f.calendar();
        let secs = f.from * 86_400 + offset;
        let local = secs + oracle_offset(secs);
        let mut day = local.div_euclid(86_400);
        if local.rem_euclid(86_400) >= 20 * 3_600 {
            day += 1;
        }
        let expected = if day < f.from { None } else { (day..=f.to).find(|d| f.trading_day(*d)) };
        let actual = calendar.equity_trade_date(instant(secs));
        match expected {
            Some(d) => prop_assert_eq!(actual, Ok(date_of(d))),
            None => prop_assert_eq!(actual, Err(TimeError::OutsideCalendar)),
        }
    }

    #[test]
    fn settlement_is_the_next_settlement_day_after_the_trade_date(f in fixture(), offset in 0i64..=120) {
        let calendar = f.calendar();
        let trade = f.from + offset;
        let expected = (trade + 1..=f.to).find(|d| f.settlement_day(*d));
        let actual = calendar.settlement_date(date_of(trade));
        match expected {
            Some(d) => prop_assert_eq!(actual, Ok(date_of(d))),
            None => prop_assert_eq!(actual, Err(TimeError::OutsideCalendar)),
        }
        if let Ok(settles) = actual {
            prop_assert!(days_of(settles) > trade);
        }
    }

    #[test]
    fn rfc3339_offsets_are_subtracted(
        secs in modern_secs(),
        sign in prop_oneof![Just(1i64), Just(-1i64)],
        oh in 0i64..=23,
        om in 0i64..=59,
    ) {
        let offset = sign * (oh * 3_600 + om * 60);
        let local = secs + offset;
        let (y, m, d) = naive_date(local.div_euclid(86_400));
        let of_day = local.rem_euclid(86_400);
        let sign_char = if sign < 0 { '-' } else { '+' };
        let text = format!(
            "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}{sign_char}{oh:02}:{om:02}",
            of_day / 3_600,
            of_day % 3_600 / 60,
            of_day % 60,
        );
        prop_assert_eq!(UtcNanos::parse_rfc3339(&text), Ok(instant(secs)), "{}", text);
        let zulu = format!("{}Z", &text[..19]);
        prop_assert_eq!(UtcNanos::parse_rfc3339(&zulu), Ok(instant(local)), "{}", zulu);
    }

    #[test]
    fn next_and_weekend_agree_with_naive_counting(day in 0i64..naive_days(9999, 12, 31)) {
        prop_assert_eq!(date_of(day).next(), Ok(date_of(day + 1)));
        prop_assert_eq!(date_of(day).is_weekend(), weekday(day) >= 5);
    }
}

fn d(s: &str) -> Date {
    Date::parse(s).unwrap()
}

fn at(s: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(s).unwrap()
}

fn us_2026() -> TradingCalendar {
    TradingCalendar::new(
        d("2026-09-01"),
        d("2026-12-31"),
        [d("2026-11-26"), d("2026-12-25")],
        [d("2026-10-12"), d("2026-11-11")],
    )
    .unwrap()
}

#[test]
fn reference_case_trade_and_settlement_dates() {
    let calendar = us_2026();
    let cases = [
        ("2026-09-21T21:00:00-04:00", "2026-09-22", "2026-09-23"),
        ("2026-10-09T10:00:00-04:00", "2026-10-09", "2026-10-13"),
        ("2026-10-12T10:00:00-04:00", "2026-10-12", "2026-10-13"),
        ("2026-11-10T10:00:00-05:00", "2026-11-10", "2026-11-12"),
        ("2026-09-21T10:00:00-04:00", "2026-09-21", "2026-09-22"),
        ("2026-09-21T19:59:59-04:00", "2026-09-21", "2026-09-22"),
        ("2026-09-21T20:00:00-04:00", "2026-09-22", "2026-09-23"),
        ("2026-10-09T20:30:00-04:00", "2026-10-12", "2026-10-13"),
        ("2026-11-25T20:00:00-05:00", "2026-11-27", "2026-11-30"),
        ("2026-09-26T12:00:00-04:00", "2026-09-28", "2026-09-29"),
    ];
    for (executed, trade, settles) in cases {
        let trade_date = calendar.equity_trade_date(at(executed)).unwrap();
        assert_eq!(trade_date, d(trade), "{executed}");
        assert_eq!(
            calendar.settlement_date(trade_date).unwrap(),
            d(settles),
            "{executed}"
        );
    }
}

#[test]
fn midnights_and_offsets_around_the_2026_transitions() {
    assert_eq!(
        new_york_midnight(d("2026-09-22")).unwrap(),
        at("2026-09-22T04:00:00Z")
    );
    assert_eq!(
        new_york_midnight(d("2026-11-01")).unwrap(),
        at("2026-11-01T04:00:00Z")
    );
    assert_eq!(
        new_york_midnight(d("2026-11-02")).unwrap(),
        at("2026-11-02T05:00:00Z")
    );
    assert_eq!(
        new_york_midnight(d("2026-03-08")).unwrap(),
        at("2026-03-08T05:00:00Z")
    );
    assert_eq!(
        new_york_midnight(d("2026-03-09")).unwrap(),
        at("2026-03-09T04:00:00Z")
    );
    let hour = |s: &str| new_york_date_and_hour(at(s)).unwrap();
    assert_eq!(hour("2026-11-01T05:59:59Z"), (d("2026-11-01"), 1));
    assert_eq!(hour("2026-11-01T06:00:00Z"), (d("2026-11-01"), 1));
    assert_eq!(hour("2026-03-08T06:59:59Z"), (d("2026-03-08"), 1));
    assert_eq!(hour("2026-03-08T07:00:00Z"), (d("2026-03-08"), 3));
    assert_eq!(hour("2026-09-22T00:00:00Z"), (d("2026-09-21"), 20));
}

#[test]
fn calendar_errors() {
    let calendar = us_2026();
    assert_eq!(
        calendar.is_trading_day(d("2026-08-31")),
        Err(TimeError::OutsideCalendar)
    );
    assert_eq!(
        calendar.is_settlement_day(d("2027-01-01")),
        Err(TimeError::OutsideCalendar)
    );
    assert_eq!(
        calendar.settlement_date(d("2026-12-31")),
        Err(TimeError::OutsideCalendar)
    );
    assert_eq!(
        calendar.settlement_date(d("2026-08-31")),
        Err(TimeError::OutsideCalendar)
    );
    assert_eq!(
        calendar.equity_trade_date(at("2026-12-31T20:00:00-05:00")),
        Err(TimeError::OutsideCalendar)
    );
    assert_eq!(
        TradingCalendar::new(d("2026-12-31"), d("2026-09-01"), [], []),
        Err(TimeError::InvalidCalendar)
    );
    assert_eq!(
        TradingCalendar::new(d("2026-09-01"), d("2026-12-31"), [d("2027-01-01")], []),
        Err(TimeError::InvalidCalendar)
    );
    assert_eq!(
        TradingCalendar::new(d("2026-09-01"), d("2026-12-31"), [], [d("2026-08-31")]),
        Err(TimeError::InvalidCalendar)
    );
    assert_eq!(d("9999-12-31").next(), Err(TimeError::OutOfRange));
    for (error, code) in [
        (TimeError::OutsideCalendar, "outside_calendar"),
        (TimeError::InvalidCalendar, "invalid_calendar"),
        (TimeError::TimeZone, "time_zone"),
    ] {
        assert_eq!(error.code(), code);
    }
}

#[test]
fn rfc3339_rejects_other_forms() {
    for text in [
        "2026-09-21T10:00:00",
        "2026-09-21T10:00:00.5Z",
        "2026-09-21T10:00:00z",
        "2026-09-21 10:00:00Z",
        "2026-09-21T10:00:00-0400",
        "2026-09-21T10:00:00.000000000Z",
    ] {
        assert_eq!(
            UtcNanos::parse_rfc3339(text),
            Err(TimeError::Syntax),
            "{text}"
        );
    }
    assert_eq!(
        UtcNanos::parse_rfc3339("2026-09-21T10:00:00-24:00"),
        Err(TimeError::InvalidDate)
    );
    assert_eq!(
        UtcNanos::parse_rfc3339("2026-09-21T10:00:00+00:60"),
        Err(TimeError::InvalidDate)
    );
    assert_eq!(
        UtcNanos::parse_rfc3339("2026-02-30T10:00:00Z"),
        Err(TimeError::InvalidDate)
    );
    assert_eq!(
        UtcNanos::parse_rfc3339("1970-01-01T00:00:00+00:01"),
        Err(TimeError::OutOfRange)
    );
}
