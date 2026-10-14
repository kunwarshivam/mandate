//! The shell watch's end pins (E7-26's shell-side row; LT-10, DEC-912 item 3, DEC-858 items 4
//! and 5, DEC-853 item 5, DEC-877 item 1): the per-wake question the shell's own watch asks in
//! place of the bare terminal read it kept before E7-26, wired into `Session::watch` since the
//! row's implementation PR (DEC-77, DEC-137, DEC-914 item 7). The paper run's one whole share
//! can never fill partly (DEC-877 item 3), so the uncovered half is pinned here over the facts
//! handed in, never over a broker read the paper broker cannot produce. Every fact is typed, no
//! value is computed by the code under test, and nothing here reaches a host, writes a journal
//! or names an order (`AGENTS.md` rules 7 and 8).

use mandate_shell::{Cause, ShellError, Stage, watch_may_end};
use mandate_time::UtcNanos;

/// Tuesday 2026-09-29, a full session: 13:00 New York, the bound at 15:50, the cap at 16:00
/// (DEC-853 item 5, DEC-877 item 1).
const TUESDAY: &str = "2026-09-29T17:00:00Z";
const TUESDAY_BOUND: &str = "2026-09-29T19:50:00Z";
const TUESDAY_CAP: &str = "2026-09-29T20:00:00Z";

/// Thursday 2026-12-24, an early close at 13:00 New York: the bound at 12:50, the cap at
/// 13:00, never 16:00 (DEC-877 item 1).
const EVE_BOUND: &str = "2026-12-24T17:50:00Z";
const EVE_CAP: &str = "2026-12-24T18:00:00Z";

fn at(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).expect("the pinned instant parses")
}

/// LT-10 and DEC-912 item 3: "The run may end when the entry is **terminal** and any filled
/// quantity is covered — nothing filled (flat) or the profile's strongest protection resting
/// over the whole of it". A terminal entry with nothing uncovered may end the run, in each of
/// DEC-858 item 4's four terminal states.
#[test]
fn a_terminal_covered_or_flat_entry_may_end_the_run() {
    for read in ["filled", "canceled", "expired", "rejected"] {
        let ended = watch_may_end(Some(read), false, &[], at(TUESDAY), at(TUESDAY_BOUND));
        assert!(matches!(ended, Ok(true)), "{read}: {ended:?}");
    }
}

/// LT-10's first half and DEC-912 item 3: "Until either half holds the run keeps reading" — a
/// filled quantity left with no protection resting over it keeps the run reading, inside its
/// bounds.
#[test]
fn an_uncovered_quantity_keeps_the_run_reading() {
    let held = watch_may_end(Some("filled"), true, &[], at(TUESDAY), at(TUESDAY_BOUND));
    assert!(matches!(held, Ok(false)), "{held:?}");
}

/// LT-10's second half and DEC-912 item 3: "It may also end with a filled quantity uncovered,
/// but only once the unprotected interval is journaled and its alert key is in the run's
/// report" — notifications spec §4.2's `protection` row keys it
/// `unprotected_interval_limit`.
#[test]
fn an_uncovered_quantity_ends_only_with_its_alert_key_reported() {
    let ended = watch_may_end(
        Some("filled"),
        true,
        &["unprotected_interval_limit"],
        at(TUESDAY),
        at(TUESDAY_BOUND),
    );
    assert!(matches!(ended, Ok(true)), "{ended:?}");
}

/// Notifications spec §4.2's `protection` row: "executor keys `unprotected_interval_limit`,
/// `stop_watchdog`" — only the interval's own key reports an uncovered run. The watchdog's
/// key is another alarm's and shows nothing for this interval, so a report holding it still
/// keeps the run reading.
#[test]
fn another_interval_alert_key_holds_the_run_open() {
    let held = watch_may_end(
        Some("filled"),
        true,
        &["stop_watchdog"],
        at(TUESDAY),
        at(TUESDAY_BOUND),
    );
    assert!(matches!(held, Ok(false)), "{held:?}");
}

/// DEC-912 item 3: "Until either half holds the run keeps reading — the entry still working
/// included", and `AGENTS.md rule 3`: an entry read back working, and a read the broker did
/// not answer at all, is never a terminal the run ends on. The report holding the interval's
/// own key releases nothing here either: the entry's terminal read is LT-10's first half, and
/// only the two halves together end the run — so the facts carry the key, and the run reads
/// on anyway.
#[test]
fn an_entry_still_working_keeps_the_run_reading() {
    for read in [Some("accepted"), None] {
        let held = watch_may_end(
            read,
            false,
            &["unprotected_interval_limit"],
            at(TUESDAY),
            at(TUESDAY_BOUND),
        );
        assert!(matches!(held, Ok(false)), "{read:?}: {held:?}");
    }
}

/// DEC-858 item 4: "The watch ends when the broker reports the entry `filled`, `canceled`,
/// `expired` or `rejected`" — the exact four and nothing else. Every state the executor's
/// codec names (`mandate-executor`'s `codec.rs` `STATES` table, thirteen in all) is walked
/// here against those four: the four end the run, and `intent`, `submitting`, `accepted`,
/// `partially_filled`, `pending_cancel`, `pending_replace`, `unknown`, `replaced` and
/// `abandoned` keep it reading — `replaced` and `abandoned` terminal to the executor's
/// reservation, never to the watch (#1311's review (i) named the same gap in the paper crate).
#[test]
fn the_watch_ends_only_on_the_codecs_four_terminal_states() {
    let states = [
        ("intent", false),
        ("submitting", false),
        ("accepted", false),
        ("partially_filled", false),
        ("pending_cancel", false),
        ("pending_replace", false),
        ("unknown", false),
        ("filled", true),
        ("canceled", true),
        ("rejected", true),
        ("expired", true),
        ("replaced", false),
        ("abandoned", false),
    ];
    for (read, ends) in states {
        let found = watch_may_end(Some(read), false, &[], at(TUESDAY), at(TUESDAY_BOUND));
        assert_eq!(
            matches!(found, Ok(true)),
            ends,
            "{read} ends the run: {found:?}"
        );
    }
}

/// DEC-853 item 5: "the watch runs at most until the end of the regular session"; DEC-877
/// item 1: "16:00 New York on a full day". At the cap itself, inclusive, a watch whose LT-10
/// does not hold fails closed with the shell's existing `cancel_unconfirmed` refusal — the
/// watch never reads again — and a watch whose LT-10 holds ends clean at it, needing no
/// further read: the settled answer comes before the cap's, so a settled run ends before the
/// cap ever refuses it.
#[test]
fn the_cap_fails_an_unsettled_watch_closed_on_a_full_day() {
    let unsettled = watch_may_end(
        Some("filled"),
        true,
        &[],
        at(TUESDAY_CAP),
        at(TUESDAY_BOUND),
    );
    assert!(
        matches!(unsettled, Err(ShellError::CancelUnconfirmed)),
        "{unsettled:?}"
    );
    let settled = watch_may_end(
        Some("filled"),
        true,
        &["unprotected_interval_limit"],
        at(TUESDAY_CAP),
        at(TUESDAY_BOUND),
    );
    assert!(matches!(settled, Ok(true)), "{settled:?}");
}

/// DEC-877 item 1: "the early close on a short one" — Thursday 2026-12-24 closes at 13:00
/// New York, so the cap is 18:00 UTC, never 16:00. At it, inclusive, a watch whose LT-10 does
/// not hold fails closed with the shell's existing `cancel_unconfirmed` refusal.
#[test]
fn the_cap_fails_an_unsettled_watch_closed_at_the_early_close() {
    let unsettled = watch_may_end(Some("canceled"), true, &[], at(EVE_CAP), at(EVE_BOUND));
    assert!(
        matches!(unsettled, Err(ShellError::CancelUnconfirmed)),
        "{unsettled:?}"
    );
}

/// FT-8 and DEC-853 item 5: "E1b pins that the run refuses before any `POST` when the calendar
/// cannot place the run inside the regular session" — at the watch, a bound no regular
/// session holds fails the run closed as absent (`AGENTS.md` rule 3), never a cap it guesses:
/// a Saturday, the evening after the session, the morning before it.
#[test]
fn a_bound_no_session_holds_fails_the_watch_closed() {
    for bound in [
        "2026-10-03T17:00:00Z",
        "2026-09-29T22:00:00Z",
        "2026-09-29T12:00:00Z",
    ] {
        let found = watch_may_end(Some("filled"), false, &[], at(TUESDAY), at(bound));
        assert!(
            matches!(
                found,
                Err(ShellError::Refused {
                    stage: Stage::Executor,
                    cause: Cause::Absent { .. },
                })
            ),
            "{bound}: {found:?}"
        );
    }
}
