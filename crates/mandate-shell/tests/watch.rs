//! E1b's bounded watch (the first paper trade brief's E1b, FT-11, DEC-853 items 5 and 6,
//! DEC-858): part 1a, the bound and the poll interval. Every expected instant and interval is typed
//! here from the calendar file and the reviewed rule set, never computed by the code under test.

mod common;

use std::time::Duration;

use mandate_executor::ExecutorConfig;
use mandate_shell::{Cause, close_window_bound, poll_interval};
use mandate_time::{Date, UtcNanos};

use common::{FEE, RULES, SNAPSHOT, Stream, model, spy_mandate};

/// Tuesday 2026-09-29, a full session: 13:00 and 15:50 in New York.
const TUESDAY: &str = "2026-09-29T17:00:00Z";
const BOUND: &str = "2026-09-29T19:50:00Z";

fn at(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).unwrap()
}

/// SPY's deployment with its reviewed rule set, the gate configuration's source.
fn stream() -> Stream {
    let snapshot = SNAPSHOT.replace("2026-09-21", "2026-09-25");
    Stream::of(&spy_mandate(&model()), &snapshot, RULES, FEE, &model())
}

fn gate() -> mandate_risk::GateConfig {
    let artifacts = stream()
        .artifacts_on(Date::parse("2026-09-29").unwrap())
        .unwrap();
    artifacts.production_configuration().unwrap().gate.clone()
}

/// DEC-853 item 5: the bound is the session's end less `close_window_minutes`: 15:50 New York on a
/// full day, 12:50 on the day after Thanksgiving (an early close at 13:00), and 15:45 with a
/// fifteen-minute window.
#[test]
#[ignore = "pending E7-19"]
fn the_bound_is_the_close_window_start_of_the_runs_session() {
    let gate = gate();
    assert_eq!(gate.close_window_minutes, 10, "the reviewed rule set");
    let mut wider = gate.clone();
    wider.close_window_minutes = 15;
    let cases = [
        (TUESDAY, &gate, BOUND),
        ("2026-11-27T16:00:00Z", &gate, "2026-11-27T17:50:00Z"),
        (TUESDAY, &wider, "2026-09-29T19:45:00Z"),
    ];
    for (now, gate, bound) in cases {
        let found = close_window_bound(at(now), gate).unwrap_or_else(|e| panic!("{now}: {e:?}"));
        assert_eq!(found, at(bound), "{now} {}", gate.close_window_minutes);
    }
}

/// DEC-853 item 5, FT-8: no bound outside a regular session or inside its close window: pre-market,
/// 15:50 itself, 15:55, and a Saturday are each refused as absent.
#[test]
#[ignore = "pending E7-19"]
fn there_is_no_bound_outside_a_session_before_its_close_window() {
    let gate = gate();
    for now in [
        "2026-09-29T12:00:00Z",
        BOUND,
        "2026-09-29T19:55:00Z",
        "2026-10-03T17:00:00Z",
    ] {
        let found = close_window_bound(at(now), &gate);
        assert!(
            matches!(found, Err(Cause::Absent { .. })),
            "{now}: {found:?}"
        );
    }
}

/// DEC-853 item 6: the least of `exit_step_s`, `unknown_absent_window_s / unknown_absent_lookups`
/// rounded down and at least 1, `bracket_partial_fill_timeout_s` and `max_unprotected_s`; each term
/// is made the least in turn. A zero member is refused rather than skipped (FT-8).
#[test]
#[ignore = "pending E7-19"]
fn the_poll_interval_is_the_least_of_its_four_terms() {
    let config = |[exit, window, lookups, bracket, unprotected]: [i64; 5]| ExecutorConfig {
        exit_step_s: exit,
        unknown_absent_window_s: window,
        unknown_absent_lookups: u32::try_from(lookups).unwrap(),
        bracket_partial_fill_timeout_s: bracket,
        max_unprotected_s: unprotected,
        ..ExecutorConfig::PROPOSED
    };
    let cases = [
        ([5, 15, 3, 60, 60], 5),
        ([3, 15, 3, 60, 60], 3),
        ([9, 14, 3, 60, 60], 4),
        ([5, 2, 3, 60, 60], 1),
        ([9, 15, 3, 2, 60], 2),
        ([9, 15, 3, 60, 1], 1),
    ];
    for (members, secs) in cases {
        let found = poll_interval(&config(members));
        let found = found.unwrap_or_else(|e| panic!("{members:?}: {e:?}"));
        assert_eq!(found, Duration::from_secs(secs), "{members:?}");
    }
    for members in [[0, 15, 3, 60, 60], [5, 15, 0, 60, 60]] {
        let found = poll_interval(&config(members));
        assert!(
            matches!(found, Err(Cause::Absent { .. })),
            "{members:?}: {found:?}"
        );
    }
}
