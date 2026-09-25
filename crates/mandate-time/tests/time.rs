//! Journal spec §4.7 timestamps and dates. The property oracle counts days naively, independent of
//! the library's civil-date arithmetic.

use mandate_time::{Date, TimeError, UtcNanos};
use proptest::prelude::*;

#[expect(
    clippy::manual_is_multiple_of,
    reason = "the oracle is written differently from the library"
)]
fn is_leap(y: u16) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn month_len(y: u16, m: u8) -> u8 {
    [
        31,
        if is_leap(y) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ][usize::from(m - 1)]
}

fn naive_secs(y: u16, mo: u8, d: u8, h: u8, mi: u8, s: u8) -> i64 {
    let mut days: i64 = 0;
    for year in 1970..y {
        days += if is_leap(year) { 366 } else { 365 };
    }
    for month in 1..mo {
        days += i64::from(month_len(y, month));
    }
    days += i64::from(d) - 1;
    days * 86_400 + i64::from(h) * 3_600 + i64::from(mi) * 60 + i64::from(s)
}

#[test]
fn known_instants() {
    let cases = [
        ("1970-01-01T00:00:00.000000000Z", 0, 0),
        ("2026-09-21T13:00:00.000120000Z", 1_789_995_600, 120_000),
        ("2000-02-29T23:59:59.999999999Z", 951_868_799, 999_999_999),
        (
            "9999-12-31T23:59:59.999999999Z",
            253_402_300_799,
            999_999_999,
        ),
    ];
    for (text, secs, nanos) in cases {
        let t = UtcNanos::parse(text).unwrap();
        assert_eq!((t.secs(), t.nanos()), (secs, nanos), "{text}");
        assert_eq!(t.to_string(), text);
        assert_eq!(UtcNanos::from_parts(secs, nanos).unwrap(), t);
    }
    assert_eq!(
        UtcNanos::EPOCH.to_string(),
        "1970-01-01T00:00:00.000000000Z"
    );
}

#[test]
fn rejects_malformed_timestamps() {
    let syntax = [
        "",
        "2026-09-21T13:00:00Z",
        "2026-09-21T13:00:00.00000000Z",
        "2026-09-21T13:00:00.0000000000Z",
        "2026-09-21 13:00:00.000000000Z",
        "2026-09-21T13:00:00.000000000z",
        "2026-09-21T13:00:00.000000000+00:00",
        " 2026-09-21T13:00:00.000000000Z",
        "2026-9-21T13:00:00.0000000000Z",
        "+026-09-21T13:00:00.000000000Z",
        "2026-09-21T13:00:00.00000000\u{661}Z",
    ];
    for text in syntax {
        assert_eq!(UtcNanos::parse(text), Err(TimeError::Syntax), "{text:?}");
    }
    let invalid = [
        "2026-02-29T00:00:00.000000000Z",
        "2100-02-29T00:00:00.000000000Z",
        "2026-13-01T00:00:00.000000000Z",
        "2026-00-01T00:00:00.000000000Z",
        "2026-04-31T00:00:00.000000000Z",
        "2026-01-00T00:00:00.000000000Z",
        "2026-06-30T24:00:00.000000000Z",
        "2026-06-30T23:60:00.000000000Z",
        "2026-06-30T23:59:60.000000000Z",
    ];
    for text in invalid {
        assert_eq!(
            UtcNanos::parse(text),
            Err(TimeError::InvalidDate),
            "{text:?}"
        );
    }
    assert_eq!(
        UtcNanos::parse("1969-12-31T23:59:59.999999999Z"),
        Err(TimeError::OutOfRange)
    );
    assert_eq!(
        UtcNanos::parse("0000-01-01T00:00:00.000000000Z"),
        Err(TimeError::OutOfRange)
    );
}

#[test]
fn from_parts_bounds() {
    assert_eq!(UtcNanos::from_parts(-1, 0), Err(TimeError::OutOfRange));
    assert_eq!(
        UtcNanos::from_parts(253_402_300_800, 0),
        Err(TimeError::OutOfRange)
    );
    assert_eq!(
        UtcNanos::from_parts(0, 1_000_000_000),
        Err(TimeError::OutOfRange)
    );
    assert!(UtcNanos::from_parts(253_402_300_799, 999_999_999).is_ok());
}

#[test]
fn dates() {
    for text in ["1970-01-01", "2024-02-29", "2026-09-21", "9999-12-31"] {
        assert_eq!(Date::parse(text).unwrap().to_string(), text);
    }
    assert_eq!(Date::parse("2026-09-21T"), Err(TimeError::Syntax));
    assert_eq!(Date::parse("2026/09/21"), Err(TimeError::Syntax));
    assert_eq!(Date::parse("2023-02-29"), Err(TimeError::InvalidDate));
    assert_eq!(Date::parse("1969-12-31"), Err(TimeError::OutOfRange));
    let d = Date::new(2026, 9, 21).unwrap();
    assert_eq!((d.year(), d.month(), d.day()), (2026, 9, 21));
    assert_eq!("2026-09-21".parse::<Date>(), Ok(d));
    assert_eq!(
        "2026-09-21T13:00:00.000000000Z"
            .parse::<UtcNanos>()
            .map(UtcNanos::date),
        Ok(d)
    );
    assert_eq!(
        TimeError::Syntax.code().to_owned()
            + TimeError::InvalidDate.code()
            + TimeError::OutOfRange.code(),
        "syntaxinvalid_dateout_of_range"
    );
}

fn civil() -> impl Strategy<Value = (u16, u8, u8)> {
    (1970u16..=9999, 1u8..=12).prop_flat_map(|(y, m)| (Just(y), Just(m), 1..=month_len(y, m)))
}

proptest! {
    #[test]
    fn parse_matches_naive_count_and_round_trips(
        (y, mo, d) in civil(), h in 0u8..24, mi in 0u8..60, s in 0u8..60, n in 0u32..1_000_000_000
    ) {
        let text = format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{n:09}Z");
        let t = UtcNanos::parse(&text).unwrap();
        prop_assert_eq!(t.secs(), naive_secs(y, mo, d, h, mi, s));
        prop_assert_eq!(t.nanos(), n);
        prop_assert_eq!(t.to_string(), text);
        prop_assert_eq!(t.date(), Date::new(y, mo, d).unwrap());
        prop_assert_eq!(UtcNanos::from_parts(t.secs(), n).unwrap(), t);
    }

    #[test]
    fn order_is_chronological(a in 0i64..=253_402_300_799, an in 0u32..1_000_000_000,
                              b in 0i64..=253_402_300_799, bn in 0u32..1_000_000_000) {
        let (ta, tb) = (UtcNanos::from_parts(a, an).unwrap(), UtcNanos::from_parts(b, bn).unwrap());
        // Fixed-width text sorts chronologically, independently of the struct's derived order.
        prop_assert_eq!(ta.cmp(&tb), ta.to_string().cmp(&tb.to_string()));
        prop_assert_eq!(UtcNanos::parse(&ta.to_string()).unwrap(), ta);
    }

    #[test]
    fn invalid_days_are_rejected(y in 1970u16..=9999, m in 1u8..=12, extra in 1u8..=10) {
        let day = month_len(y, m) + extra;
        let text = format!("{y:04}-{m:02}-{day:02}");
        prop_assert!(day > 99 || Date::parse(&text) == Err(TimeError::InvalidDate));
    }
}
