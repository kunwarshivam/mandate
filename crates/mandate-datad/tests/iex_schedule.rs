//! DEC-2283 items 4 and 5, data-plane spec §3.8 "When": a daily generation is due once its
//! session's last calendar span has ended plus `SETTLE` and the workspace's own offset; a minute
//! generation at each whole minute of the regular session, past the workspace's offset into it.
//! The oracles are the instants written out by hand from the calendar's published hours.

use mandate_datad::iex::schedule::{
    CONFIRM_EVERY_S, CONFIRM_S, DAILY_JITTER_S, Jitter, MINUTE_JITTER_S, SESSIONS, SETTLE_S,
    daily_target, minute_target, span,
};
use mandate_time::{Date, ExchangeCalendar, UtcNanos};

fn at(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).unwrap()
}

fn day(text: &str) -> Date {
    Date::parse(text).unwrap()
}

fn calendar() -> ExchangeCalendar {
    ExchangeCalendar::us_equities().unwrap()
}

const NONE: Jitter = Jitter {
    daily_s: 0,
    minute_s: 0,
};

#[test]
fn the_spec_s_constants() {
    assert_eq!(
        (
            SETTLE_S,
            CONFIRM_S,
            CONFIRM_EVERY_S,
            DAILY_JITTER_S,
            MINUTE_JITTER_S,
            SESSIONS
        ),
        (900, 7_200, 300, 300, 20, 30)
    );
}

#[test]
fn a_workspace_s_offsets_are_its_own_fixed_and_bounded() {
    let ids = ["ws-1", "ws-2", "01KDEM4814371439000000000D", ""];
    let offsets: Vec<Jitter> = ids.iter().map(|id| Jitter::of(id)).collect();
    for (id, jitter) in ids.iter().zip(&offsets) {
        assert_eq!(Jitter::of(id), *jitter, "{id}: fixed");
        assert!(
            (0..DAILY_JITTER_S).contains(&jitter.daily_s),
            "{id}: {jitter:?}"
        );
        assert!(
            (0..MINUTE_JITTER_S).contains(&jitter.minute_s),
            "{id}: {jitter:?}"
        );
    }
    assert_ne!(offsets[0], offsets[1], "two workspaces do not ask at once");
}

#[test]
fn a_daily_generation_is_due_once_its_session_has_settled() {
    let calendar = calendar();
    for (now, jitter, due) in [
        ("2026-10-26T19:59:59Z", NONE, Some("2026-10-23")),
        ("2026-10-26T20:00:00Z", NONE, None),
        ("2026-10-27T00:14:59Z", NONE, None),
        ("2026-10-27T00:15:00Z", NONE, Some("2026-10-26")),
        ("2026-10-27T13:00:00Z", NONE, Some("2026-10-26")),
        (
            "2026-10-27T00:16:39Z",
            Jitter {
                daily_s: 100,
                minute_s: 0,
            },
            None,
        ),
        (
            "2026-10-27T00:16:40Z",
            Jitter {
                daily_s: 100,
                minute_s: 0,
            },
            Some("2026-10-26"),
        ),
        ("2026-10-24T15:00:00Z", NONE, Some("2026-10-23")),
        ("2026-11-27T22:14:59Z", NONE, None),
        ("2026-11-27T22:15:00Z", NONE, Some("2026-11-27")),
        ("2026-11-26T19:00:00Z", NONE, Some("2026-11-25")),
    ] {
        assert_eq!(
            daily_target(&calendar, at(now), jitter).unwrap(),
            due.map(day),
            "{now} {jitter:?}"
        );
    }
    assert!(daily_target(&calendar, at("2035-01-02T19:00:00Z"), NONE).is_err());
}

#[test]
fn a_daily_generation_spans_the_thirty_sessions_ending_on_its_own() {
    let calendar = calendar();
    let got = span(&calendar, day("2026-10-26")).unwrap();
    let mut expected = Vec::new();
    let mut d = day("2026-08-01");
    while d <= day("2026-10-26") {
        if calendar.is_trading_day(d).unwrap() {
            expected.push(d);
        }
        d = d.next().unwrap();
    }
    let expected = expected.split_off(expected.len() - 30);
    assert_eq!(got, expected);
    assert_eq!(got.first().copied(), Some(day("2026-09-15")));
    assert!(
        span(&calendar, day("2026-10-25")).is_err(),
        "a Sunday is no session"
    );
}

#[test]
fn a_minute_generation_is_due_at_each_whole_minute_of_the_regular_session() {
    let calendar = calendar();
    let five = Jitter {
        daily_s: 0,
        minute_s: 5,
    };
    for (now, jitter, due) in [
        ("2026-10-27T14:30:10Z", five, Some("2026-10-27T14:30:00Z")),
        ("2026-10-27T14:30:05Z", five, Some("2026-10-27T14:30:00Z")),
        ("2026-10-27T14:30:04Z", five, None),
        ("2026-10-27T13:30:00Z", NONE, Some("2026-10-27T13:30:00Z")),
        ("2026-10-27T13:29:59Z", NONE, None),
        ("2026-10-27T19:59:59Z", NONE, Some("2026-10-27T19:59:00Z")),
        ("2026-10-27T20:00:00Z", NONE, None),
        ("2026-10-25T15:00:00Z", NONE, None),
        ("2026-11-27T17:59:00Z", NONE, Some("2026-11-27T17:59:00Z")),
        ("2026-11-27T18:00:00Z", NONE, None),
    ] {
        assert_eq!(
            minute_target(&calendar, at(now), jitter).unwrap(),
            due.map(at),
            "{now} {jitter:?}"
        );
    }
}
