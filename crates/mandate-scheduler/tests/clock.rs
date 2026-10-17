//! The tick, trading-day, and measurement rules of journal spec §9.20 (E21-29, DEC-1010), each
//! checked against an oracle that states the rule its own way.

use mandate_scheduler::measure::{self, Schedule, seconds};
use mandate_scheduler::{Measurement, Sample, Trigger, tick};
use mandate_time::{Date, ExchangeCalendar, UtcNanos, new_york_date_and_hour};
use proptest::prelude::*;

fn at(secs: i64, nanos: u32) -> UtcNanos {
    UtcNanos::from_parts(secs, nanos).unwrap()
}

fn calendar() -> ExchangeCalendar {
    ExchangeCalendar::us_equities().unwrap()
}

/// 2026-10-16T13:00:00Z, a Friday, 09:00 in New York: 30 minutes before the regular session.
const FRIDAY_0900: i64 = 1_792_155_600;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Every step writes a tick; each is a whole second, later than the last by at least one, at
    /// or after the host clock's second, and either the host's second or the last plus one.
    #[test]
    fn ticks_never_stop_repeat_or_run_backwards(
        start in 1_700_000_000i64..1_800_000_000,
        moves in prop::collection::vec((-7_200i64..7_200, 0u32..1_000_000_000), 1..60),
    ) {
        let mut host = start;
        let mut last: Option<UtcNanos> = None;
        for (step, nanos) in moves {
            host += step;
            let tick = tick::next(last, at(host, nanos)).unwrap();
            prop_assert_eq!(tick.nanos(), 0);
            prop_assert!(tick.secs() >= host);
            if let Some(last) = last {
                prop_assert!(tick.secs() > last.secs());
                prop_assert!(tick.secs() == host || tick.secs() == last.secs() + 1);
            } else {
                prop_assert_eq!(tick.secs(), host);
            }
            last = Some(tick);
        }
    }

    /// Over ticks ten minutes apart across two weeks, the days started are each trading date the
    /// ticks reach, in order, once, at the first tick on that New York date.
    #[test]
    fn each_trading_date_starts_once_at_its_first_tick(offset in 0i64..86_400) {
        let calendar = calendar();
        let mut last_day: Option<Date> = None;
        let mut started = Vec::new();
        let mut previous_date: Option<Date> = None;
        for minute in 0..(14 * 24 * 6) {
            let clock = at(FRIDAY_0900 - 86_400 + offset + minute * 600, 0);
            let (date, _) = new_york_date_and_hour(clock).unwrap();
            if let Some(day) = tick::trading_day(&calendar, clock, last_day) {
                prop_assert_eq!(day, date);
                prop_assert!(previous_date != Some(date), "not the first tick of {date}");
                last_day = Some(day);
                started.push(day);
            }
            previous_date = Some(date);
        }
        let mut expected: Vec<Date> = Vec::new();
        for minute in 0..(14 * 24 * 6) {
            let (date, _) = new_york_date_and_hour(at(FRIDAY_0900 - 86_400 + offset + minute * 600, 0)).unwrap();
            if calendar.is_trading_day(date).unwrap() && expected.last() != Some(&date) {
                expected.push(date);
            }
        }
        prop_assert_eq!(started, expected);
    }

    /// The measured offset is the median by magnitude, the larger of two, from the answers that
    /// count; the oracle filters and sorts on its own terms.
    #[test]
    fn a_measurement_takes_the_median_by_magnitude_of_the_answers_that_count(
        answers in prop::collection::vec(
            (0u8..20, -500_000_000i64..500_000_000, -10i64..50_000_000, 0usize..8),
            0..10,
        ),
        tolerance in 1u64..200_000_000,
    ) {
        let samples: Vec<Sample> = answers
            .iter()
            .map(|&(stratum, offset_ns, delay_ns, server)| Sample {
                server: format!("s{server}"),
                stratum,
                offset_ns,
                delay_ns,
            })
            .collect();
        let judged = Measurement::judge(Trigger::Periodic, samples.clone(), tolerance);
        let mut kept: Vec<&Sample> = Vec::new();
        let mut by_server = samples.clone();
        by_server.retain(|s| s.stratum >= 1 && s.stratum <= 15 && s.delay_ns >= 0);
        by_server.sort_by(|a, b| a.server.cmp(&b.server));
        for s in &by_server {
            if kept.last().is_none_or(|k| k.server != s.server) {
                kept.push(s);
            }
        }
        prop_assert_eq!(judged.samples.len(), kept.len());
        let mut magnitudes: Vec<(u64, i64)> = kept.iter().map(|s| (s.offset_ns.unsigned_abs(), s.offset_ns)).collect();
        magnitudes.sort_unstable();
        let expected = magnitudes.get(magnitudes.len() / 2).map(|&(_, o)| o);
        prop_assert_eq!(judged.offset_ns, expected);
        prop_assert_eq!(
            judged.within_tolerance,
            expected.is_some_and(|o| o.unsigned_abs() <= tolerance)
        );
    }

    /// `seconds` writes the decimal §4.6 normalizes to: its digits read back as the nanoseconds.
    #[test]
    fn seconds_round_trip_through_their_digits(nanos in any::<i64>()) {
        let text = seconds(nanos);
        prop_assert!(!text.ends_with('0') || !text.contains('.'));
        prop_assert!(text != "-0");
        let negative = text.starts_with('-');
        let digits = text.trim_start_matches('-');
        let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
        let value = i128::from(whole.parse::<u64>().unwrap()) * 1_000_000_000
            + i128::from(format!("{fraction:0<9}").parse::<u64>().unwrap());
        prop_assert_eq!(if negative { -value } else { value }, i128::from(nanos));
    }
}

/// Zero is `0`, never `-0`, which §4.6 does not normalize to.
#[test]
fn zero_seconds_carry_no_sign() {
    assert_eq!(seconds(0), "0");
    assert_eq!(seconds(-1), "-0.000000001");
}

#[test]
fn a_measurement_no_server_answers_is_out_of_tolerance() {
    let none = Measurement::judge(Trigger::Startup, Vec::new(), measure::TOLERANCE_NS);
    assert_eq!((none.offset_ns, none.within_tolerance), (None, false));
    let unsynchronized = vec![
        Sample {
            server: "a".into(),
            stratum: 0,
            offset_ns: 0,
            delay_ns: 0,
        },
        Sample {
            server: "b".into(),
            stratum: 16,
            offset_ns: 0,
            delay_ns: 0,
        },
    ];
    let judged = Measurement::judge(Trigger::Startup, unsynchronized, measure::TOLERANCE_NS);
    assert_eq!((judged.offset_ns, judged.within_tolerance), (None, false));
}

#[test]
fn a_tolerance_holds_up_to_its_bound_inclusive() {
    let one = |offset_ns| {
        Measurement::judge(
            Trigger::Periodic,
            vec![Sample {
                server: "a".into(),
                stratum: 1,
                offset_ns,
                delay_ns: 0,
            }],
            50_000_000,
        )
        .within_tolerance
    };
    assert!(one(50_000_000));
    assert!(one(-50_000_000));
    assert!(!one(50_000_001));
    assert!(!one(-50_000_001));
}

#[test]
fn measurements_are_due_at_start_before_the_session_and_every_five_minutes() {
    let calendar = calendar();
    let fresh = Schedule::default();
    assert_eq!(
        measure::due(&fresh, &calendar, at(FRIDAY_0900, 0)),
        Some(Trigger::Startup)
    );
    let measured_at = |secs: i64| Schedule {
        measured_since_start: true,
        last_measured: Some(at(secs, 0)),
        before_session_on: None,
    };
    let early = FRIDAY_0900 - 3_600;
    assert_eq!(
        measure::due(&measured_at(early), &calendar, at(early, 0)),
        None
    );
    assert_eq!(
        measure::due(&measured_at(early), &calendar, at(early + 299, 0)),
        None
    );
    assert_eq!(
        measure::due(&measured_at(early), &calendar, at(early + 300, 0)),
        Some(Trigger::Periodic)
    );
    let open = FRIDAY_0900 + 1_800;
    let window = at(open - 900, 0);
    assert_eq!(
        measure::due(&measured_at(open - 901), &calendar, window),
        Some(Trigger::BeforeSession)
    );
    assert_eq!(
        measure::due(&measured_at(open - 2), &calendar, at(open - 1, 0)),
        Some(Trigger::BeforeSession)
    );
    assert_eq!(
        measure::due(&measured_at(open - 2), &calendar, at(open, 0)),
        None
    );
    assert_eq!(
        measure::due(&measured_at(open - 902), &calendar, at(open - 901, 0)),
        None
    );
    let friday = Date::new(2026, 10, 16).unwrap();
    let done = Schedule {
        before_session_on: Some(friday),
        ..measured_at(open - 60)
    };
    assert_eq!(measure::due(&done, &calendar, at(open - 30, 0)), None);
    let saturday_0900 = at(FRIDAY_0900 + 86_400, 0);
    assert_eq!(
        measure::due(
            &measured_at(FRIDAY_0900 + 86_400 - 1),
            &calendar,
            saturday_0900
        ),
        None
    );
}
