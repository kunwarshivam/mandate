//! Property tests for E8-1's and E8-2's invariants in the
//! [M7 brief](../../../docs/project/tasks/M7-escalation-v0.md), each against an oracle that shares
//! no code with the crate (AGENTS.md, "Independent oracles"):
//!
//! - **The sentinel scanner** (EI-9): distinctive values in every request field, searched for in
//!   every notification payload's bytes.
//! - **The budget counter** (EI-13): its own America/New_York day from a hard-coded 2026 DST table,
//!   not `mandate-time`, and its own suppression windows.
//! - **Content separation** (EI-14): two generated contents share a hash exactly when they are
//!   equal.
//!
//! E8-3's properties (the check table, the clock accumulator, the principal generator, the
//! assertion ledger, scaled-integer drift, the field comparer, and the kill-switch probe) come with
//! E8-3's tests PR, and the runtime-level invariants with the runtime's (DEC-165 item 1). Every
//! property is pending until the implementation PR and fails on `ApprovalError::Unimplemented`
//! (DEC-77, DEC-110).

mod common;

use common::{DEADLINE, content, request};
use mandate_approval::{
    ApprovalRef, AskEvent, AskLedger, AskPermit, RiskClock, Suppression, ask_permit, content_hash,
    notification_for, notification_payload,
};
use mandate_canon::to_canonical;
use mandate_num::{Price, Qty};
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;

fn check<S>(strategy: S, body: impl Fn(S::Value) -> Result<(), TestCaseError>)
where
    S: Strategy,
    S::Value: std::fmt::Debug,
{
    let config = ProptestConfig {
        cases: 256,
        failure_persistence: None,
        ..ProptestConfig::default()
    };
    if let Err(failure) = proptest::test_runner::TestRunner::new(config).run(&strategy, body) {
        panic!("{failure}");
    }
}

fn fail<E: std::fmt::Debug>(what: &str) -> impl FnOnce(E) -> TestCaseError + '_ {
    move |e| TestCaseError::fail(format!("{what} answers, not {e:?}"))
}

/// EI-9, PB-9: no notification payload carries any sentinel from the request it points at.
#[test]
#[ignore = "pending E8-1"]
fn no_notification_carries_a_sentinel() {
    check((0u32..1_000_000, "[0-9A-HJKMNP-TV-Z]{26}"), |(n, id)| {
        let mut r = request();
        r.id = ApprovalRef::of_requested_event(&id);
        r.content.bound.instrument = format!("sentinelinstrument{n}");
        r.content.bound.qty = Qty::parse(&format!("777{n}")).unwrap();
        r.content.bound.limit = Price::parse(&format!("31337.{n}1")).unwrap();
        r.content.bound.mandate_version = format!("sentinelversion{n}");
        r.content.bound.decided_by = format!("rule:sentinelrule{n}");
        for e in &mut r.content.evidence {
            e.event_id = format!("sentinelthesis{n}");
        }
        let note = notification_for(&r).map_err(fail("notification_for"))?;
        let payload = notification_payload(&note).map_err(fail("notification_payload"))?;
        let bytes = String::from_utf8(to_canonical(&payload)).unwrap();
        prop_assert!(bytes.contains(&id), "the payload names its approval");
        for sentinel in [
            "sentinel",
            "777",
            "31337",
            "1872",
            "0.62",
            "large_order",
            "2026",
            &n.to_string(),
        ] {
            if id.contains(sentinel) {
                continue;
            }
            prop_assert!(!bytes.contains(sentinel), "{bytes} carries {sentinel}");
        }
        prop_assert_eq!(payload.as_object().map(|o| o.len()), Some(2));
        Ok(())
    });
}

/// 2026's America/New_York day of an instant, from the year's two DST instants alone: EDT (UTC−4)
/// from 2026-03-08T07:00:00Z until 2026-11-01T06:00:00Z, EST (UTC−5) otherwise.
fn new_york_day(at: i64) -> i64 {
    let edt = (1_772_953_200..1_793_512_800).contains(&at);
    (at - if edt { 4 * 3600 } else { 5 * 3600 }).div_euclid(86_400)
}

#[derive(Debug, Clone)]
enum Step {
    Ask(u8),
    Skip(u8),
    Timeout(u8, i64),
    Version,
}

/// EI-13, PB-16: the budget and both suppressions match the counter, across both 2026 DST changes.
#[test]
#[ignore = "pending E8-2"]
fn the_ask_budget_matches_the_counter() {
    let start = prop::sample::select(vec![1_772_935_200i64, 1_793_487_600]);
    let step = prop_oneof![
        6 => (0u8..3).prop_map(Step::Ask),
        1 => (0u8..3).prop_map(Step::Skip),
        1 => ((0u8..3), 60i64..600).prop_map(|(i, w)| Step::Timeout(i, w)),
        1 => Just(Step::Version),
    ];
    let steps = prop::collection::vec((step, 0i64..5_400), 1..40);
    check((start, steps), |(start, steps)| {
        let mut ledger = AskLedger::default();
        let mut at = start;
        for (s, gap) in steps {
            at += gap;
            let instrument = |i: u8| format!("asset-{i}");
            let Step::Ask(i) = s else {
                ledger.events.push(match s {
                    Step::Skip(i) => AskEvent::OwnerSkipped {
                        instrument: instrument(i),
                        at: RiskClock(at),
                    },
                    Step::Timeout(i, w) => AskEvent::TimedOut {
                        instrument: instrument(i),
                        at: RiskClock(at),
                        timeout_s: w,
                    },
                    _ => AskEvent::VersionApplied { at: RiskClock(at) },
                });
                continue;
            };
            let day = new_york_day(at);
            let mut asked = 0;
            let mut skipped = false;
            let mut timed_out = false;
            for e in &ledger.events {
                match e {
                    AskEvent::Requested { at: t, .. } if new_york_day(t.0) == day => asked += 1,
                    AskEvent::OwnerSkipped {
                        instrument: x,
                        at: t,
                    } if *x == instrument(i) && new_york_day(t.0) == day => skipped = true,
                    AskEvent::VersionApplied { .. } => skipped = false,
                    AskEvent::TimedOut {
                        instrument: x,
                        at: t,
                        timeout_s,
                    } if *x == instrument(i) && at < t.0 + timeout_s => timed_out = true,
                    _ => {}
                }
            }
            let expected = if asked >= 10 {
                AskPermit::Suppressed(Suppression::Budget)
            } else if skipped {
                AskPermit::Suppressed(Suppression::SkippedToday)
            } else if timed_out {
                AskPermit::Suppressed(Suppression::RecentTimeout)
            } else {
                AskPermit::Ask
            };
            let got =
                ask_permit(&ledger, &instrument(i), RiskClock(at)).map_err(fail("ask_permit"))?;
            prop_assert_eq!(got, expected);
            if got == AskPermit::Ask {
                ledger.events.push(AskEvent::Requested {
                    instrument: instrument(i),
                    at: RiskClock(at),
                });
            }
        }
        Ok(())
    });
}

/// EI-14: two requests share a content hash exactly when their content is equal.
#[test]
#[ignore = "pending E8-1"]
fn the_content_hash_separates_every_bound_field() {
    check((0u8..9, 1u32..1000), |(field, n)| {
        let base = content();
        let mut other = content();
        match field {
            0 => other.bound.instrument = format!("asset-{n}"),
            1 => other.bound.qty = Qty::parse(&format!("{}", n + 10)).unwrap(),
            2 => other.bound.limit = Price::parse(&format!("187.{n}1")).unwrap(),
            3 => other.bound.mandate_version = format!("v{}", n + 3),
            4 => other.bound.decided_by = format!("rule:r{n}"),
            5 => other.deadline = RiskClock(DEADLINE + i64::from(n)),
            6 => other.bound.independent_required = true,
            7 => other.bound.reference_mark = None,
            _ => {}
        }
        let a = content_hash(&base).map_err(fail("content_hash"))?;
        let b = content_hash(&other).map_err(fail("content_hash"))?;
        prop_assert_eq!(a == b, base == other);
        Ok(())
    });
}

/// Live: the budget counter's own New York day, shown against hand-computed boundaries, so a
/// property failure is the crate's, not the oracle's.
#[test]
fn the_budget_counters_new_york_day_is_right_at_the_dst_change() {
    assert_eq!(new_york_day(1_773_016_200), new_york_day(1_772_949_600));
    assert_ne!(new_york_day(1_773_030_600), new_york_day(1_773_016_200));
    assert_eq!(new_york_day(1_793_509_200), new_york_day(1_793_516_400));
    assert_eq!(new_york_day(1_793_516_400), new_york_day(1_793_595_540));
    assert_ne!(new_york_day(1_793_595_540), new_york_day(1_793_595_600));
}
