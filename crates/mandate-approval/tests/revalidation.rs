//! E8-3's re-validation and drift clauses of the
//! [M7 brief](../../../docs/project/tasks/M7-escalation-v0.md) (checks 8 to 12, DEC-156 items 2
//! to 4): one named test per clause, each naming the MC-E case or planted bug (PB-n) it stands
//! for. Every test is pending until the implementation PR and fails on
//! `ApprovalError::Unimplemented` (DEC-77, DEC-110).

mod common;

use common::{answer, bound, current, price, request};
use mandate_approval::{
    AssetClass, Classification, DryRun, ModeNow, Revalidation, SkipReason, band_bp, revalidate,
    within_band,
};

fn acted(now: &mandate_approval::Current) -> Revalidation {
    answer("revalidate", revalidate(&request(), now))
}

/// The paired positive of every one-sided skip below: the current state each test varies is one
/// in which the grant acts, so a stub that skips for one constant reason passes none of them.
fn the_unchanged_fixture_acts() {
    assert!(
        matches!(acted(&current()), Revalidation::Act(_)),
        "the unchanged fixture acts"
    );
}

/// MC-E01, EI-4, EI-5, PB-8: a grant acts with exactly the bound order, never re-priced to the
/// moved mark, and only while every check passes.
#[test]
fn a_grant_acts_with_the_bound_order_only_while_every_check_passes() {
    let mut now = current();
    now.mark_now = Some(price("188"));
    match acted(&now) {
        Revalidation::Act(order) => assert_eq!(*order.order(), bound()),
        other => panic!("expected an act, got {other:?}"),
    }
    now.mark_now = Some(price("189"));
    assert_eq!(
        acted(&now),
        Revalidation::Skip(SkipReason::Drift),
        "189 is 107 bp from 187"
    );
}

/// MC-E17, PB-5, check 8.
#[test]
fn a_changed_version_skips() {
    the_unchanged_fixture_acts();
    let now = mandate_approval::Current {
        mandate_version: "v4".to_owned(),
        ..current()
    };
    assert_eq!(acted(&now), Revalidation::Skip(SkipReason::VersionChanged));
}

/// Check 9: any mode stricter than normal skips. No step the spec names reaches it, because such a
/// step cancels every pending approval first (MC-E18; DEC-318, DEC-430); it stays as defence in depth.
#[test]
fn a_mode_other_than_normal_skips() {
    the_unchanged_fixture_acts();
    for mode in [ModeNow::ExitsOnly, ModeNow::Paused, ModeNow::Stopped] {
        let now = mandate_approval::Current { mode, ..current() };
        assert_eq!(
            acted(&now),
            Revalidation::Skip(SkipReason::Mode),
            "{mode:?}"
        );
    }
}

/// Check 9: a restricted instrument, or one no longer in the working universe, skips.
#[test]
fn a_restricted_or_removed_instrument_skips() {
    the_unchanged_fixture_acts();
    let restricted = mandate_approval::Current {
        instrument_restricted: true,
        ..current()
    };
    let removed = mandate_approval::Current {
        in_working_universe: false,
        ..current()
    };
    for now in [restricted, removed] {
        assert_eq!(
            acted(&now),
            Revalidation::Skip(SkipReason::InstrumentRestricted)
        );
    }
}

/// MC-E19, PB-6, check 10: `deny` is never overridden.
#[test]
fn a_reclassified_deny_skips() {
    the_unchanged_fixture_acts();
    let now = mandate_approval::Current {
        classification: Classification::Deny,
        ..current()
    };
    assert_eq!(
        acted(&now),
        Revalidation::Skip(SkipReason::ReclassifiedDeny)
    );
}

/// MC-E20, check 10: an ask by another trigger is a question the owner has not seen; `auto` and
/// the same trigger act.
#[test]
fn an_ask_by_another_trigger_skips_and_auto_or_the_same_trigger_acts() {
    let other = mandate_approval::Current {
        classification: Classification::Ask {
            decided_by: "default".to_owned(),
        },
        ..current()
    };
    assert_eq!(
        acted(&other),
        Revalidation::Skip(SkipReason::ReclassifiedOtherTrigger)
    );
    let auto = mandate_approval::Current {
        classification: Classification::Auto,
        ..current()
    };
    assert!(matches!(acted(&auto), Revalidation::Act(_)));
    assert!(matches!(acted(&current()), Revalidation::Act(_)));
}

/// Check 11, FR-6.5: a dry-run denial skips with the gate's own reason.
#[test]
fn a_gate_denial_skips_with_the_gates_reason() {
    the_unchanged_fixture_acts();
    let now = mandate_approval::Current {
        dry_run: DryRun::Deny {
            reason: "close_window".to_owned(),
        },
        ..current()
    };
    assert_eq!(
        acted(&now),
        Revalidation::Skip(SkipReason::Gate {
            reason: "close_window".to_owned()
        })
    );
}

/// DEC-156 item 2: the first failing re-validation check names the skip.
#[test]
fn the_first_failing_revalidation_check_names_the_skip() {
    let now = mandate_approval::Current {
        mandate_version: "v4".to_owned(),
        mode: ModeNow::Paused,
        classification: Classification::Deny,
        mark_now: None,
        ..current()
    };
    assert_eq!(acted(&now), Revalidation::Skip(SkipReason::VersionChanged));
    let now = mandate_approval::Current {
        classification: Classification::Deny,
        dry_run: DryRun::Deny {
            reason: "gross_limit".to_owned(),
        },
        ..current()
    };
    assert_eq!(
        acted(&now),
        Revalidation::Skip(SkipReason::ReclassifiedDeny)
    );
}

fn band(req: &str, now: &str, class: AssetClass) -> bool {
    answer(
        "within_band",
        within_band(Some(price(req)), Some(price(now)), class),
    )
}

/// MC-E21, MC-E22, PB-7: exactly at the band is inside; one unit over, either way, is outside.
#[test]
fn drift_exactly_at_the_band_is_inside_and_one_unit_over_either_way_is_not() {
    the_unchanged_fixture_acts();
    assert!(band("100", "101", AssetClass::UsEquity));
    assert!(band("100", "99", AssetClass::UsEquity));
    assert!(!band("100", "101.000000001", AssetClass::UsEquity));
    assert!(!band("100", "98.999999999", AssetClass::UsEquity));
    let mut now = current();
    now.mark_now = Some(price("185.12"));
    assert_eq!(acted(&now), Revalidation::Skip(SkipReason::Drift));
}

/// MC-E23: no mark at the request or now is outside the band (fail closed).
#[test]
fn no_mark_is_outside_the_band() {
    the_unchanged_fixture_acts();
    let p = Some(price("100"));
    assert!(!answer(
        "within_band",
        within_band(None, p, AssetClass::Crypto)
    ));
    assert!(!answer(
        "within_band",
        within_band(p, None, AssetClass::Crypto)
    ));
    assert!(
        answer("within_band", within_band(p, p, AssetClass::Crypto)),
        "an unmoved mark is inside"
    );
    let mut now = current();
    now.mark_now = None;
    assert_eq!(acted(&now), Revalidation::Skip(SkipReason::Drift));
}

/// MC-E24, DEC-156 item 3: 100 bp for equities, 200 bp for crypto.
#[test]
fn crypto_uses_200_bp_and_equities_100() {
    assert_eq!(answer("band_bp", band_bp(AssetClass::UsEquity)), 100);
    assert_eq!(answer("band_bp", band_bp(AssetClass::Crypto)), 200);
    assert!(band("100", "102", AssetClass::Crypto));
    assert!(!band("100", "102", AssetClass::UsEquity));
    assert!(!band("100", "102.000000001", AssetClass::Crypto));
}
