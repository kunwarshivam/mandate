//! The risk day (`kind: risk_day`, MC-T01 to MC-T05; [§5.4]).
//!
//! §5.4 gives the risk day one sentence — 00:00 to 00:00 America/New_York, 23 or 25 hours on a
//! daylight-saving change day — and every daily-loss rule in §5.4 and §5.6 hangs off it: E₀ is set at
//! the start, a breach still confirming at the rollover keeps confirming against the previous day's E₀,
//! and the daily-loss lift waits for a new day. A day boundary an hour out moves all of them.
//!
//! The five reference cases pin 2026's two change days. These add the boundaries either side of them and
//! the properties no finite set of cases can state: that the days tile the year without gap or overlap,
//! and that `length_s` is the distance between the bounds rather than a number computed beside them.
//!
//! [§5.4]: ../../../docs/specs/mandate.md#54-daily-loss-dec-40-dec-49-dec-54

use mandate_spec::risk::risk_day;
use mandate_time::{Date, UtcNanos, new_york_midnight};
use proptest::prelude::*;

fn at(text: &str) -> UtcNanos {
    UtcNanos::parse(text).expect("an instant")
}

fn date(text: &str) -> Date {
    Date::parse(text).expect("a date")
}

/// MC-T01 to MC-T05, recomputed rather than copied: each `starts_at` is 00:00 New York on the day
/// itself, each `ends_at` is 00:00 New York on the next day, and `length_s` is the distance between
/// them. That is the whole of §5.4's first sentence, and it is why the DST days come out at 23 and 25
/// hours without the rule naming either number.
#[test]
#[ignore = "pending E6-4"]
fn the_five_reference_days_are_midnight_to_midnight_in_new_york() {
    for (instant, day, seconds) in [
        ("2026-03-08T04:59:59.000000000Z", "2026-03-07", 86_400),
        ("2026-03-08T05:00:00.000000000Z", "2026-03-08", 82_800),
        ("2026-03-09T04:00:00.000000000Z", "2026-03-09", 86_400),
        ("2026-11-01T04:00:00.000000000Z", "2026-11-01", 90_000),
        ("2026-11-02T05:00:00.000000000Z", "2026-11-02", 86_400),
    ] {
        let day = date(day);
        let computed = risk_day(at(instant)).expect("a risk day");
        assert_eq!(computed.day, day, "{instant} falls in this risk day");
        assert_eq!(
            computed.starts_at,
            new_york_midnight(day).expect("midnight"),
            "{instant}: the day starts at 00:00 New York on the day itself"
        );
        assert_eq!(
            computed.ends_at,
            new_york_midnight(day.next().expect("the next date")).expect("midnight"),
            "{instant}: the day ends at 00:00 New York on the next day"
        );
        assert_eq!(
            u64::from(computed.length_s),
            computed
                .ends_at
                .secs()
                .checked_sub(computed.starts_at.secs())
                .and_then(|d| u64::try_from(d).ok())
                .expect("a forward duration"),
            "{instant}: length_s is the distance between the bounds, not a number beside them"
        );
        assert_eq!(u64::from(computed.length_s), seconds, "{instant}");
    }
}

/// The boundary is the start of the day it opens, never the end of the one it closes.
///
/// MC-T01 and MC-T02 are one second apart across 2026-03-08T05:00:00Z, so the pair already pins this;
/// stated as its own test because an off-by-one here dates every daily-loss breach to the wrong day. All
/// four boundaries are covered: the midnight that opens the short day and the one that closes it, and the
/// midnight that opens the long day and the one that closes it.
#[test]
#[ignore = "pending E6-4"]
fn midnight_belongs_to_the_day_it_opens() {
    for (before, boundary, opening) in [
        (
            "2026-03-08T04:59:59.999999999Z",
            "2026-03-08T05:00:00.000000000Z",
            "2026-03-08",
        ),
        (
            "2026-03-09T03:59:59.999999999Z",
            "2026-03-09T04:00:00.000000000Z",
            "2026-03-09",
        ),
        (
            "2026-11-01T03:59:59.999999999Z",
            "2026-11-01T04:00:00.000000000Z",
            "2026-11-01",
        ),
        (
            "2026-11-02T04:59:59.999999999Z",
            "2026-11-02T05:00:00.000000000Z",
            "2026-11-02",
        ),
    ] {
        let opening = date(opening);
        assert_eq!(
            risk_day(at(boundary)).expect("a risk day").day,
            opening,
            "{boundary} is the first instant of {opening}"
        );
        let earlier = risk_day(at(before)).expect("a risk day");
        assert_ne!(
            earlier.day, opening,
            "{before} is still the previous day, one nanosecond earlier"
        );
        assert_eq!(
            earlier.ends_at,
            at(boundary),
            "the previous day ends exactly where {opening} begins"
        );
    }
}

/// A day is 23, 24, or 25 hours, and only the two change days are not 24.
///
/// The independent oracle is the *count*: 2026 has 365 risk days, exactly one of 82800 s and one of
/// 90000 s, and their sum is the year. A rule that got the DST direction backwards would still produce
/// one short and one long day, so the test also names which date is which.
#[test]
#[ignore = "pending E6-4"]
fn the_year_is_tiled_by_days_of_which_exactly_two_are_not_twenty_four_hours() {
    let mut day = date("2026-01-01");
    let mut total: u64 = 0;
    let mut short = Vec::new();
    let mut long = Vec::new();
    let mut counted = 0;
    let mut previous_end = new_york_midnight(day).expect("midnight");
    while day <= date("2026-12-31") {
        let computed = risk_day(previous_end).expect("a risk day");
        assert_eq!(computed.day, day, "the days run in order without a gap");
        assert_eq!(
            computed.starts_at, previous_end,
            "{day} starts where the previous day ended, so the days neither gap nor overlap"
        );
        match u64::from(computed.length_s) {
            82_800 => short.push(day),
            86_400 => {}
            90_000 => long.push(day),
            other => panic!("{day} is {other} s, which §5.4 does not allow"),
        }
        total = total
            .checked_add(u64::from(computed.length_s))
            .expect("no overflow");
        counted += 1;
        previous_end = computed.ends_at;
        day = day.next().expect("the next date");
    }
    assert_eq!(counted, 365, "2026 has 365 risk days");
    assert_eq!(
        short,
        vec![date("2026-03-08")],
        "only the DST start day is 23 hours"
    );
    assert_eq!(
        long,
        vec![date("2026-11-01")],
        "only the DST end day is 25 hours"
    );
    assert_eq!(
        total,
        365 * 86_400,
        "the days sum to the year: the hour lost in March is the hour gained in November"
    );
}

proptest! {
    /// Every instant lies inside the day it is given, and nowhere else.
    ///
    /// The oracle is containment rather than a recomputed boundary: `starts_at <= at < ends_at` is what
    /// "the risk day containing an instant" means, and it is checked without reference to how the
    /// boundary was found.
    #[test]
    #[ignore = "pending E6-4"]
    fn an_instant_lies_in_its_own_day_and_the_days_abut(offset_s in 0i64..(3 * 365 * 86_400)) {
        let instant = UtcNanos::from_parts(
            at("2026-01-01T05:00:00.000000000Z").secs().saturating_add(offset_s),
            0,
        )
        .expect("an instant");
        let computed = risk_day(instant).expect("a risk day");
        prop_assert!(
            computed.starts_at <= instant && instant < computed.ends_at,
            "{instant:?} must lie in [{:?}, {:?})",
            computed.starts_at,
            computed.ends_at
        );
        let next = risk_day(computed.ends_at).expect("a risk day");
        prop_assert_eq!(next.starts_at, computed.ends_at, "the next day starts where this one ends");
        prop_assert_ne!(next.day, computed.day, "and it is a different day");
    }
}
