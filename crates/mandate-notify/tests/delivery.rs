//! E8-10 slice D1b: the coalescing fold, the retry schedule, and one notice per cause (spec §3.4,
//! §5.3, §5.4, NT-6, DEC-725 items 5 to 7). Every expected answer is a table written here from the
//! spec, or a tally of the test's own; none calls the policy to judge it.

mod common;

use std::collections::BTreeMap;

use common::answer;
use mandate_notify::{
    Alert, Class, Message, NotifyError, Pass, Read, Reason, Retry, coalesce, next_attempt,
    notice_keys,
};
use mandate_time::UtcNanos;
use proptest::prelude::*;

use Class::{Action, Info, Safety};

fn read(cause: u32, class: Class, committed: i64) -> Read<u32> {
    Read {
        cause,
        class,
        committed: UtcNanos::from_parts(committed, 0).unwrap(),
    }
}

fn pass(at: i64, reads: Vec<Read<u32>>) -> Pass<u32> {
    Pass {
        at: UtcNanos::from_parts(at, 0).unwrap(),
        reads,
    }
}

fn message(send_at: i64, causes: &[u32]) -> Message<u32> {
    Message {
        send_at: UtcNanos::from_parts(send_at, 0).unwrap(),
        causes: causes.to_vec(),
    }
}

const T: i64 = 1_790_000_000;

/// §5.4, DEC-700 item 5, DEC-725 item 5: the first message at once; a later pass joins the window,
/// which ends at the earlier of its opening plus 60 s and its earliest joined commit plus 60 s; a
/// stale cause goes alone; `action` and `info` are never coalesced.
#[test]
#[ignore = "pending E8-10"]
fn safety_causes_coalesce_per_window_and_none_waits_past_its_bound() {
    #[rustfmt::skip]
    let passes = [
        pass(T, vec![read(1, Safety, T - 1), read(2, Safety, T)]),
        pass(T + 10, vec![read(3, Safety, T + 5), read(4, Action, T + 9)]),
        pass(T + 30, vec![read(5, Safety, T + 29), read(6, Info, T + 30)]),
        pass(T + 60, vec![read(7, Safety, T)]),
        pass(T + 70, vec![read(8, Safety, T + 69)]),
        pass(T + 80, vec![read(9, Safety, T + 75), read(10, Safety, T + 22)]),
    ];
    #[rustfmt::skip]
    let expected = [
        message(T, &[1, 2]), message(T + 10, &[4]), message(T + 30, &[6]), message(T + 60, &[3, 5]),
        message(T + 60, &[7]), message(T + 70, &[8]), message(T + 82, &[9, 10]),
    ];
    assert_eq!(answer("coalesce", coalesce(&passes)), expected);
    let backwards = [pass(T, vec![]), pass(T - 1, vec![read(1, Safety, T - 2)])];
    assert!(matches!(
        coalesce(&backwards),
        Err(NotifyError::Unrepresentable { .. })
    ));
}

proptest! {
    /// NT-6 by a tally of the reads, never the fold's own state: every cause is in exactly one
    /// message, none before its pass, a `safety` one by its commit plus 60 s or at its pass if
    /// read later, and an `action` or `info` one alone at its pass.
    #[test]
    #[ignore = "pending E8-10"]
    fn no_safety_cause_is_dropped_or_held_past_its_bound(
        passes in prop::collection::vec(
            (0i64..90, prop::collection::vec((0u8..4, 0i64..120), 0..6)), 1..12),
    ) {
        let (mut at, mut next, mut built, mut tally) = (T, 0u32, Vec::new(), BTreeMap::new());
        for (step, reads) in passes {
            at += step;
            let reads = reads.into_iter().map(|(c, age)| {
                next += 1;
                let class = [Safety, Safety, Action, Info][usize::from(c)];
                tally.insert(next, (class, at - age, at, 0u32));
                read(next, class, at - age)
            }).collect();
            built.push(pass(at, reads));
        }
        let messages = answer("coalesce", coalesce(&built));
        prop_assert!(messages.windows(2).all(|m| m[0].send_at <= m[1].send_at));
        for m in &messages {
            for cause in &m.causes {
                let (class, committed, read_at, seen) = tally.get_mut(cause).unwrap();
                *seen += 1;
                let send_at = m.send_at.secs();
                prop_assert!(send_at >= *read_at, "cause {} sent before it was read", cause);
                if *class == Safety {
                    prop_assert!(send_at <= (*committed + 60).max(*read_at), "cause {}", cause);
                } else {
                    prop_assert!(m.causes.len() == 1 && send_at == *read_at, "cause {}", cause);
                }
            }
        }
        prop_assert!(tally.values().all(|v| v.3 == 1), "dropped or sent twice: {:?}", tally);
    }
}

const RETRYABLE: [Reason; 3] = [Reason::Timeout, Reason::RateLimited, Reason::ProviderError];

/// §5.3, DEC-438 item 9, DEC-725 item 6: the gap after attempt n.
fn gap(n: u32) -> i64 {
    [15, 60, 300].get(n as usize - 1).copied().unwrap_or(900)
}

fn retry(class: Class, first: i64, last: i64, attempts: u32, reason: Reason) -> Retry {
    let at = |s| UtcNanos::from_parts(s, 0).unwrap();
    answer(
        "next_attempt",
        next_attempt(class, at(first), at(last), attempts, reason),
    )
}

/// §5.3, DEC-725 item 6: 15 s, 60 s, 5 min, then 15 min; a window has room for an attempt due at
/// its end and none after; a reason that is not retryable stops at once.
#[test]
#[ignore = "pending E8-10"]
fn retries_follow_the_schedule_until_the_window_ends() {
    let at = |s| Retry::At(UtcNanos::from_parts(s, 0).unwrap());
    for (n, last, due) in [
        (1, T, T + 15),
        (2, T + 15, T + 75),
        (3, T + 75, T + 375),
        (4, T + 375, T + 1_275),
        (9, T + 9_000, T + 9_900),
    ] {
        assert_eq!(
            retry(Safety, T, last, n, Reason::Timeout),
            at(due),
            "after attempt {n}"
        );
    }
    for (class, window) in [(Safety, 86_400), (Info, 21_600)] {
        assert_eq!(
            retry(class, T, T + window - 900, 50, Reason::RateLimited),
            at(T + window)
        );
        assert_eq!(
            retry(class, T, T + window - 899, 50, Reason::RateLimited),
            Retry::RetryWindowEnded
        );
    }
    assert_eq!(
        retry(Action, T, T + 864_000, 1_000, Reason::ProviderError),
        at(T + 864_900)
    );
    for reason in Reason::ALL.into_iter().filter(|r| !RETRYABLE.contains(r)) {
        for class in [Action, Safety, Info] {
            assert_eq!(
                retry(class, T, T, 1, reason),
                Retry::Failed(reason),
                "{reason:?}"
            );
        }
    }
    let none = next_attempt(Safety, UtcNanos::EPOCH, UtcNanos::EPOCH, 0, Reason::Timeout);
    assert!(matches!(none, Err(NotifyError::Unrepresentable { .. })));
}

proptest! {
    /// §5.3: the next attempt is after the latest by the schedule's gap, which never shrinks, and
    /// only a class with a window ever runs out of one.
    #[test]
    #[ignore = "pending E8-10"]
    fn retries_are_monotonic(
        class in prop::sample::select(vec![Action, Safety, Info]),
        reason in prop::sample::select(RETRYABLE.to_vec()),
        attempts in 1u32..2_000,
        since in 0i64..200_000,
    ) {
        let window = match class { Safety => Some(86_400), Info => Some(21_600), Action => None };
        let (last, due) = (T + since, T + since + gap(attempts));
        let expected = match window {
            Some(w) if due > T + w => Retry::RetryWindowEnded,
            _ => Retry::At(UtcNanos::from_parts(due, 0).unwrap()),
        };
        prop_assert_eq!(retry(class, T, last, attempts, reason), expected);
        prop_assert!(gap(attempts + 1) >= gap(attempts));
    }
}

/// §3.4, DEC-725 item 7: one user kill switch is one notice, however many stream owners name its
/// command; every other alert is its own cause, and a replayed one is not issued again.
#[test]
#[ignore = "pending E8-10"]
fn one_cause_is_one_notice() {
    let alert = |event: &'static str, command| Alert {
        event,
        owner_command: command,
    };
    let kill = Some("command");
    #[rustfmt::skip]
    let alerts = [
        alert("tripwire", None), alert("api", kill), alert("runtime", kill),
        alert("restriction", None), alert("executor", kill), alert("tripwire", None),
    ];
    let keys = answer("notice_keys", notice_keys(&alerts));
    assert_eq!(keys, ["tripwire", "command", "restriction"]);
}
