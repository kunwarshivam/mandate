//! Risk days (`kind: risk_day`, MC-T01 to MC-T05; mandate spec §5.4), story E6-4.
//!
//! A risk day runs 00:00 to 00:00 America/New_York, so it is 23 hours on the spring change day and 25
//! on the autumn one. The oracle here is `mandate-time`'s own zone conversion rather than the five
//! instants the reference cases state: `risk_day` must agree with `new_york_midnight` on every day of
//! a year, which is a claim about the rule and not about five dates someone typed.

use mandate_spec::risk::risk_day;
use mandate_time::{Date, UtcNanos, new_york_midnight};

/// The whole rule, stated independently: for every day of 2026 the risk day containing its noon runs
/// from that day's New York midnight to the next day's, and `length_s` is the difference.
#[test]
#[ignore = "pending E6-4"]
fn every_day_runs_midnight_to_midnight_new_york() {
    let mut day = Date::new(2026, 1, 1).expect("a date");
    let mut lengths = (0_u32, 0_u32, 0_u32);
    for _ in 0..365 {
        let starts = new_york_midnight(day).expect("a New York midnight");
        let next = day.next().expect("the next date");
        let ends = new_york_midnight(next).expect("a New York midnight");
        let inside = UtcNanos::from_parts(starts.secs() + 1, 0).expect("an instant");
        let found = risk_day(inside).unwrap_or_else(|e| panic!("{day:?}: {e}"));
        assert_eq!(
            found.day, day,
            "the day containing one second past its start"
        );
        assert_eq!(found.starts_at, starts);
        assert_eq!(found.ends_at, ends);
        let seconds = u32::try_from(ends.secs() - starts.secs()).expect("a day's length");
        assert_eq!(
            found.length_s, seconds,
            "{day:?}: the length is the distance between the two midnights"
        );
        match seconds {
            82800 => lengths.0 += 1,
            86400 => lengths.1 += 1,
            90000 => lengths.2 += 1,
            other => panic!("{day:?}: {other} s is not a risk-day length"),
        }
        day = next;
    }
    assert_eq!(
        lengths,
        (1, 363, 1),
        "one 23-hour day in March, one 25-hour day in November, and 363 ordinary ones"
    );
}

/// The three lengths §5.4 names, at the boundaries the reference cases pin: the last second before a
/// change day, the change day itself, and the first full day after it.
#[test]
#[ignore = "pending E6-4"]
fn three_lengths() {
    for (at, day, length) in [
        ("2026-03-08T04:59:59.000000000Z", (2026, 3, 7), 86400),
        ("2026-03-08T05:00:00.000000000Z", (2026, 3, 8), 82800),
        ("2026-03-09T04:00:00.000000000Z", (2026, 3, 9), 86400),
        ("2026-11-01T04:00:00.000000000Z", (2026, 11, 1), 90000),
        ("2026-11-02T05:00:00.000000000Z", (2026, 11, 2), 86400),
    ] {
        let found = risk_day(UtcNanos::parse(at).expect("an instant"))
            .unwrap_or_else(|e| panic!("{at}: {e}"));
        let (year, month, date) = day;
        assert_eq!(
            found.day,
            Date::new(year, month, date).expect("a date"),
            "{at} falls in that risk day"
        );
        assert_eq!(found.length_s, length, "{at}: the day's length");
        assert_eq!(
            u32::try_from(found.ends_at.secs() - found.starts_at.secs()).expect("a length"),
            found.length_s,
            "{at}: the reported length is the reported span"
        );
    }
}

/// A risk day is half-open: its own start is inside it and its end is the next day's start, so the
/// days tile the timeline with no gap and no overlap.
#[test]
#[ignore = "pending E6-4"]
fn days_tile_the_timeline_without_gap_or_overlap() {
    let mut at = UtcNanos::parse("2026-03-06T12:00:00.000000000Z").expect("an instant");
    let mut previous: Option<UtcNanos> = None;
    for _ in 0..8 {
        let found = risk_day(at).unwrap_or_else(|e| panic!("{at:?}: {e}"));
        assert!(
            found.starts_at.secs() <= at.secs() && at.secs() < found.ends_at.secs(),
            "{at:?} must be inside the day it is reported in"
        );
        let start = risk_day(found.starts_at).unwrap_or_else(|e| panic!("{at:?}: {e}"));
        assert_eq!(
            start.day, found.day,
            "the first instant of a day belongs to that day"
        );
        if let Some(previous_end) = previous {
            assert_eq!(
                found.starts_at, previous_end,
                "each day starts exactly where the last one ended"
            );
        }
        previous = Some(found.ends_at);
        at = found.ends_at;
    }
}
