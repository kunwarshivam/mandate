//! E8-2's anti-fatigue bounds (EI-13, DEC-156 item 5, DEC-165 item 6) from the
//! [M7 brief](../../../docs/project/tasks/M7-escalation-v0.md), one named test per bound, across
//! 2026's spring DST change. Every test is pending until the implementation PR and fails on
//! `ApprovalError::Unimplemented` (DEC-77, DEC-110).

mod common;

use common::answer;
use mandate_approval::{AskEvent, AskLedger, AskPermit, RiskClock, Suppression, ask_permit};

/// 2026-03-08T06:00:00Z: 01:00 EST on the day daylight saving starts.
const DST_DAY: i64 = 1_772_949_600;
/// 2026-03-09T00:30:00Z: 20:30 EDT, still 2026-03-08 in New York, but past UTC midnight.
const AFTER_UTC_MIDNIGHT: i64 = 1_773_016_200;
/// 2026-03-09T04:30:00Z: 00:30 EDT on 2026-03-09, the next New York risk day.
const NEXT_RISK_DAY: i64 = 1_773_030_600;

fn requested(n: usize, at: i64) -> AskLedger {
    AskLedger {
        events: (0..n)
            .map(|i| AskEvent::Requested {
                instrument: format!("asset-{i}"),
                at: RiskClock(at + i64::try_from(i).unwrap()),
            })
            .collect(),
    }
}

fn permit(ledger: &AskLedger, instrument: &str, at: i64) -> AskPermit {
    answer("ask_permit", ask_permit(ledger, instrument, RiskClock(at)))
}

/// MC-E25: the eleventh ask of a risk day is suppressed.
#[test]
#[ignore = "pending E8-2"]
fn the_eleventh_ask_of_a_risk_day_is_suppressed() {
    assert_eq!(
        permit(&requested(9, DST_DAY), "asset-x", DST_DAY + 60),
        AskPermit::Ask
    );
    assert_eq!(
        permit(&requested(10, DST_DAY), "asset-x", DST_DAY + 60),
        AskPermit::Suppressed(Suppression::Budget)
    );
}

/// MC-E26, PB-16: the budget resets at New York midnight, across a DST change, not at UTC's.
#[test]
#[ignore = "pending E8-2"]
fn the_budget_resets_at_new_york_midnight_across_a_dst_change() {
    let ledger = requested(10, DST_DAY);
    assert_eq!(
        permit(&ledger, "asset-x", AFTER_UTC_MIDNIGHT),
        AskPermit::Suppressed(Suppression::Budget)
    );
    assert_eq!(permit(&ledger, "asset-x", NEXT_RISK_DAY), AskPermit::Ask);
}

/// MC-E27: after an owner skip, the instrument waits for the next risk day or an applied version.
#[test]
#[ignore = "pending E8-2"]
fn an_owner_skip_suppresses_the_instrument_until_the_next_day_or_version() {
    let mut ledger = AskLedger {
        events: vec![AskEvent::OwnerSkipped {
            instrument: "asset-x".to_owned(),
            at: RiskClock(DST_DAY),
        }],
    };
    assert_eq!(
        permit(&ledger, "asset-x", AFTER_UTC_MIDNIGHT),
        AskPermit::Suppressed(Suppression::SkippedToday)
    );
    assert_eq!(
        permit(&ledger, "asset-y", AFTER_UTC_MIDNIGHT),
        AskPermit::Ask
    );
    assert_eq!(permit(&ledger, "asset-x", NEXT_RISK_DAY), AskPermit::Ask);
    ledger.events.push(AskEvent::VersionApplied {
        at: RiskClock(DST_DAY + 10),
    });
    assert_eq!(permit(&ledger, "asset-x", DST_DAY + 20), AskPermit::Ask);
}

/// MC-E28: after a timeout, the instrument is not asked within the next `timeout_s`.
#[test]
#[ignore = "pending E8-2"]
fn a_timeout_suppresses_the_instrument_for_one_window() {
    let ledger = AskLedger {
        events: vec![AskEvent::TimedOut {
            instrument: "asset-x".to_owned(),
            at: RiskClock(DST_DAY),
            timeout_s: 300,
        }],
    };
    assert_eq!(
        permit(&ledger, "asset-x", DST_DAY + 299),
        AskPermit::Suppressed(Suppression::RecentTimeout)
    );
    assert_eq!(permit(&ledger, "asset-y", DST_DAY + 299), AskPermit::Ask);
    assert_eq!(permit(&ledger, "asset-x", DST_DAY + 300), AskPermit::Ask);
}
