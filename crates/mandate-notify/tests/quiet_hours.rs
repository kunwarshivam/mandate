//! E8-10 slice D1a: quiet hours by class (spec §5.8, NT-7, DEC-725 items 2 to 4). Every expected
//! answer is a table written here from the spec with America/New_York's 2026 offsets typed in, or
//! a minute walk of the test's own; none calls the policy to judge it.

mod common;

use common::answer;
use mandate_notify::{Class, NotifyError, Offset, QuietHours, QuietVerdict, quiet_hours};
use mandate_time::{NewYorkTime, UtcNanos};
use proptest::prelude::*;

use Class::{Action, Info, Safety};
use QuietVerdict::{HoldUntil, Send, Suppress};

fn t(s: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(s).unwrap()
}

fn plus(at: UtcNanos, secs: i64) -> UtcNanos {
    UtcNanos::from_parts(at.secs() + secs, at.nanos()).unwrap()
}

fn window(start: (u8, u8), end: (u8, u8)) -> Option<QuietHours> {
    let (start, end) = (
        NewYorkTime::new(start.0, start.1),
        NewYorkTime::new(end.0, end.1),
    );
    Some(QuietHours {
        start: start.unwrap(),
        end: end.unwrap(),
    })
}

/// EST (UTC−5), EDT from 2026-03-08T07:00Z, EST again from 2026-11-01T06:00Z.
fn new_york_2026() -> [Offset; 3] {
    let at = |from: &str, minutes_east| Offset {
        from: t(from),
        minutes_east,
    };
    [
        at("2025-11-02T06:00:00Z", -300),
        at("2026-03-08T07:00:00Z", -240),
        at("2026-11-01T06:00:00Z", -300),
    ]
}

fn verdict(class: Class, w: Option<QuietHours>, at: &str) -> QuietVerdict {
    answer(
        "quiet_hours",
        quiet_hours(class, w, t(at), &new_york_2026()),
    )
}

/// DEC-725 item 3: an attempt part-way into a wall minute. One in the first minute after the window
/// is outside it and sends; one in the last minute inside is held to the window's end. Pins the
/// boundary #1029's review checked by hand, in both DST states and at both edges of the window.
#[test]
fn an_attempt_part_way_into_a_minute_is_judged_by_its_wall_minute() {
    let night = window((23, 0), (7, 0));
    let hold = |s| HoldUntil(t(s));
    #[rustfmt::skip]
    let rows = [
        ("2026-01-16T03:59:30Z", Send, Send),
        ("2026-01-16T04:00:30Z", hold("2026-01-16T12:00:00Z"), Suppress),
        ("2026-01-16T11:59:30Z", hold("2026-01-16T12:00:00Z"), Suppress),
        ("2026-01-16T12:00:30Z", Send, Send),
        ("2026-07-16T10:59:30Z", hold("2026-07-16T11:00:00Z"), Suppress),
        ("2026-07-16T11:00:30Z", Send, Send),
    ];
    for (at, info, action) in rows {
        assert_eq!(verdict(Safety, night, at), Send, "safety at {at}");
        assert_eq!(verdict(Info, night, at), info, "info at {at}");
        assert_eq!(verdict(Action, night, at), action, "action at {at}");
    }
}

/// §5.8, NT-7, DEC-725 items 2, 3: 23:00 to 07:00 in both DST states and across both changes.
#[test]
fn quiet_hours_act_by_class_in_both_dst_states_and_across_each_change() {
    let night = window((23, 0), (7, 0));
    let hold = |s| HoldUntil(t(s));
    #[rustfmt::skip]
    let rows = [
        ("2026-01-16T03:59:59.999999999Z", Send, Send),
        ("2026-01-16T04:00:00Z", hold("2026-01-16T12:00:00Z"), Suppress),
        ("2026-01-16T11:59:59.999999999Z", hold("2026-01-16T12:00:00Z"), Suppress),
        ("2026-01-16T12:00:00Z", Send, Send),
        ("2026-07-16T03:00:00Z", hold("2026-07-16T11:00:00Z"), Suppress),
        ("2026-07-16T11:00:00Z", Send, Send),
        ("2026-03-08T04:00:00Z", hold("2026-03-08T11:00:00Z"), Suppress),
        ("2026-11-01T03:00:00Z", hold("2026-11-01T12:00:00Z"), Suppress),
    ];
    for (at, info, action) in rows {
        assert_eq!(verdict(Safety, night, at), Send, "safety at {at}");
        assert_eq!(verdict(Info, night, at), info, "info at {at}");
        assert_eq!(verdict(Action, night, at), action, "action at {at}");
        for class in [Safety, Info, Action] {
            assert_eq!(verdict(class, None, at), Send, "no window at {at}");
            assert_eq!(
                verdict(class, window((7, 0), (7, 0)), at),
                Send,
                "empty window at {at}"
            );
        }
    }
}

/// DEC-725 item 3: a window the spring gap skips or the autumn hour repeats.
#[test]
fn a_window_in_the_skipped_or_repeated_hour_ends_at_the_first_instant_outside_it() {
    let (gap, half_gap, repeat) = (
        window((1, 30), (2, 30)),
        window((2, 0), (2, 30)),
        window((1, 0), (1, 30)),
    );
    assert_eq!(
        verdict(Info, gap, "2026-03-08T06:45:00Z"),
        HoldUntil(t("2026-03-08T07:00:00Z"))
    );
    assert_eq!(verdict(Action, gap, "2026-03-08T06:45:00Z"), Suppress);
    for at in ["2026-03-08T06:59:59Z", "2026-03-08T07:00:00Z"] {
        assert_eq!(
            verdict(Info, half_gap, at),
            Send,
            "02:00 to 02:30 never occurs, at {at}"
        );
    }
    assert_eq!(verdict(Info, gap, "2026-03-08T07:00:00Z"), Send);
    assert_eq!(
        verdict(Info, repeat, "2026-11-01T05:10:00Z"),
        HoldUntil(t("2026-11-01T05:30:00Z"))
    );
    assert_eq!(verdict(Info, repeat, "2026-11-01T05:30:00Z"), Send);
    assert_eq!(
        verdict(Info, repeat, "2026-11-01T06:10:00Z"),
        HoldUntil(t("2026-11-01T06:30:00Z"))
    );
    assert_eq!(verdict(Action, repeat, "2026-11-01T06:10:00Z"), Suppress);
}

/// DEC-725 items 2 and 4: `safety` reads no offset; the other classes refuse an instant none
/// covers, and of two offsets from one instant the later in the table is in force.
#[test]
fn only_action_and_info_need_an_offset() {
    let (night, at) = (window((23, 0), (7, 0)), t("2026-01-16T04:00:00Z"));
    assert_eq!(answer("safety", quiet_hours(Safety, night, at, &[])), Send);
    let (half_past, from) = (plus(at, -1_800), t("2026-01-01T00:00:00Z"));
    let (est, edt) = (
        Offset {
            from,
            minutes_east: -300,
        },
        Offset {
            from,
            minutes_east: -240,
        },
    );
    assert_eq!(
        quiet_hours(Action, night, half_past, &[est, edt]),
        Ok(Suppress)
    );
    assert_eq!(quiet_hours(Action, night, half_past, &[edt, est]), Ok(Send));
    let late = [Offset {
        from: plus(at, 1),
        minutes_east: -300,
    }];
    for (class, offsets) in [(Info, &[][..]), (Action, &[][..]), (Info, &late[..])] {
        let refused = quiet_hours(class, night, at, offsets);
        assert!(
            matches!(refused, Err(NotifyError::Unrepresentable { .. })),
            "{class:?}"
        );
    }
}

/// The wall minute of the day at `at` under `offsets`, by the test's own lookup.
fn wall_minute(at: i64, offsets: &[(i64, i16)]) -> i64 {
    let east = offsets
        .iter()
        .filter(|o| o.0 <= at)
        .max_by_key(|o| o.0)
        .unwrap()
        .1;
    (at + i64::from(east) * 60).div_euclid(60).rem_euclid(1_440)
}

proptest! {
    /// NT-7 and DEC-725 item 3 against a minute walk, over random offset tables with changes
    /// near `at`: `safety` is never held, with any table or none.
    #[test]
    fn quiet_hours_match_a_minute_walk(
        class in prop::sample::select(vec![Action, Safety, Info]),
        (start, end) in (0u16..1_440, 0u16..1_440),
        at in 1_700_000_000i64..1_800_000_000,
        base in -720i16..=840,
        changes in prop::collection::vec((-2_160i64..2_160, -720i16..=840), 0..3),
    ) {
        let mut table = vec![(at - 200_000, base)];
        table.extend(changes.iter().map(|&(m, east)| ((at.div_euclid(60) + m) * 60, east)));
        let offsets: Vec<Offset> = table.iter().map(|&(from, minutes_east)| {
            Offset { from: UtcNanos::from_parts(from, 0).unwrap(), minutes_east }
        }).collect();
        let hm = |m: u16| ((m / 60) as u8, (m % 60) as u8);
        let w = window(hm(start), hm(end));
        let inside = |s: i64| (wall_minute(s, &table) - i64::from(start)).rem_euclid(1_440)
            < (i64::from(end) - i64::from(start)).rem_euclid(1_440);
        let at_ns = UtcNanos::from_parts(at, 0).unwrap();
        let got = answer("quiet_hours", quiet_hours(class, w, at_ns, &offsets));
        let expected = match class {
            Safety => Send,
            _ if !inside(at) => Send,
            Action => Suppress,
            Info => {
                let next = (1..4_320).map(|k| (at.div_euclid(60) + k) * 60).find(|&s| !inside(s));
                HoldUntil(UtcNanos::from_parts(next.unwrap(), 0).unwrap())
            }
        };
        prop_assert_eq!(got, expected);
        prop_assert_eq!(answer("safety", quiet_hours(Safety, w, at_ns, &[])), Send);
    }
}
