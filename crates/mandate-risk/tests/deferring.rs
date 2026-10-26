//! The advisory gate's deferred checks (DEC-2378 item 11): the cell's runtime dry-runs an opening
//! without the facts nothing journals yet, so it defers, by name, instrument eligibility, the
//! liquidity floor, participation, and the day-trade budget. A deferred check is never reported
//! passed, an exit defers nothing, and with nothing deferred the advisory pass is the binding
//! gate's own decision, which the executor's gate still makes over every check (rule 1).

#![allow(
    clippy::disallowed_methods,
    reason = "these tests pin the advisory pass itself against the binding gate (DEC-2378 item 11)"
)]

mod common;

use std::collections::BTreeSet;

use common::{Scenario, usd};
use mandate_risk::{
    Check, CheckOutcome, DayTradeLedger, DayTradeRegime, Deferred, Origin, ReasonCode, Side,
    Verdict, evaluate, evaluate_deferring,
};
use proptest::prelude::*;

fn deferring(set: &[Deferred]) -> BTreeSet<Deferred> {
    set.iter().copied().collect()
}

/// Each deferred check, the one change to the allowing scenario that fails it, and the reason
/// the binding gate denies with.
fn failing(check: Deferred) -> (Scenario, ReasonCode) {
    let mut s = Scenario::allowing();
    let reason = fail(&mut s, check);
    (s, reason)
}

/// Changes `s` so that `check`'s facts fail it, and the reason the binding gate gives for it.
fn fail(s: &mut Scenario, check: Deferred) -> ReasonCode {
    match check {
        Deferred::Eligibility => {
            s.instrument.tradable = false;
            ReasonCode::NotInWorkingUniverse
        }
        Deferred::LiquidityFloor => {
            s.instrument.median_dollar_volume_20d = None;
            ReasonCode::BelowLiquidityFloor
        }
        Deferred::Participation => {
            s.market.trailing_5m_volume = None;
            ReasonCode::ConductLimitBreached
        }
        Deferred::DayTradeBudget => {
            s.account.regime = DayTradeRegime::LegacyPdt;
            s.account.prior_close_equity = usd("10000");
            s.agent.day_trades = DayTradeLedger {
                window_count: 3,
                flagged_pattern_day_trader: false,
                sold_earlier_today: BTreeSet::new(),
                open_same_day_positions: BTreeSet::new(),
            };
            ReasonCode::LegacyPdtDayTradeBudget
        }
        unseen => panic!("{unseen:?} is never skipped, so no fact of it is lifted"),
    }
}

const ALL: [Deferred; 4] = [
    Deferred::Eligibility,
    Deferred::LiquidityFloor,
    Deferred::Participation,
    Deferred::DayTradeBudget,
];

#[test]
fn with_nothing_deferred_the_advisory_pass_is_the_binding_decision() {
    let mut cases = vec![Scenario::allowing()];
    cases.extend(ALL.map(|check| failing(check).0));
    for s in cases {
        let advisory = evaluate_deferring(&s.input(), &BTreeSet::new()).unwrap();
        assert_eq!(advisory.decision, evaluate(&s.input()).unwrap());
        assert!(advisory.deferred.is_empty());
    }
}

/// Condition (ii): each check is evaluated whenever it is not deferred, and moves from deferred to
/// evaluated with nothing else changed.
#[test]
fn a_deferred_check_is_skipped_and_named_and_the_same_check_not_deferred_decides() {
    for check in ALL {
        let (s, reason) = failing(check);
        let binding = evaluate(&s.input()).unwrap();
        assert_eq!(
            (binding.verdict, binding.reason),
            (Verdict::Deny, Some(reason)),
            "{check:?}"
        );
        let advisory = evaluate_deferring(&s.input(), &deferring(&[check])).unwrap();
        assert_eq!(advisory.decision.verdict, Verdict::Allow, "{check:?}");
        assert_eq!(advisory.deferred, deferring(&[check]), "{check:?}");
        let others: Vec<Deferred> = ALL.into_iter().filter(|other| *other != check).collect();
        let evaluated = evaluate_deferring(&s.input(), &deferring(&others)).unwrap();
        assert_eq!(evaluated.decision.reason, Some(reason), "{check:?}");
    }
}

/// Condition (i): every check the caller asked to defer is named whether or not its facts would
/// have passed, so a pass is never claimed for a check that did not run.
#[test]
fn every_deferred_check_is_named_even_where_its_facts_would_pass() {
    let s = Scenario::allowing();
    let advisory = evaluate_deferring(&s.input(), &deferring(&ALL)).unwrap();
    assert_eq!(advisory.decision.verdict, Verdict::Allow);
    assert_eq!(advisory.deferred, deferring(&ALL));
}

/// A check before the deferred ones that denies still denies: deferring never lifts a check it
/// does not name.
#[test]
fn a_check_not_deferred_still_denies() {
    let mut s = Scenario::allowing();
    s.instrument.tradable = false;
    s.market.quote = None;
    let advisory = evaluate_deferring(&s.input(), &deferring(&ALL)).unwrap();
    assert_eq!(advisory.decision.verdict, Verdict::Deny, "{advisory:?}");
}

/// An exit defers nothing: risk reduction never waits on the advisory pass (rule 13), and its
/// decision is the binding gate's.
#[test]
fn an_exit_defers_nothing() {
    let (mut s, _) = failing(Deferred::Participation);
    s.agent
        .positions
        .insert(s.proposed.instrument.clone(), common::qty("5"));
    s.proposed = common::proposal(
        s.proposed.instrument.as_str(),
        Side::Sell,
        "1",
        "99",
        Origin::RiskEngine,
    );
    let advisory = evaluate_deferring(&s.input(), &deferring(&ALL)).unwrap();
    assert!(advisory.deferred.is_empty(), "{advisory:?}");
    assert_eq!(advisory.decision, evaluate(&s.input()).unwrap());
}

/// Condition (i) in the decision itself: a check the pass skipped part or all of reads
/// `Deferred`, never `Passed`, for every set of deferrals, and every other check of an allowed
/// opening reads `Passed`.
#[test]
fn no_deferred_check_reads_as_passed() {
    let s = Scenario::allowing();
    for mask in 0..(1_u8 << ALL.len()) {
        let set: Vec<Deferred> = ALL
            .into_iter()
            .enumerate()
            .filter(|(i, _)| mask & (1 << i) != 0)
            .map(|(_, d)| d)
            .collect();
        let touched: BTreeSet<Check> = set.iter().map(|d| d.check()).collect();
        let advisory = evaluate_deferring(&s.input(), &deferring(&set)).unwrap();
        assert_eq!(advisory.decision.verdict, Verdict::Allow, "{set:?}");
        for outcome in &advisory.decision.checks {
            match outcome {
                CheckOutcome::Deferred(check) => assert!(touched.contains(check), "{set:?}"),
                CheckOutcome::Passed(check) => assert!(!touched.contains(check), "{set:?}"),
                other => panic!("{set:?}: {other:?}"),
            }
        }
    }
}

/// Faults the advisory pass may never defer: no quote, so check 5 refuses; and the day's orders at
/// the cap, which check 6 refuses beside the participation clause it may skip.
const NO_QUOTE: usize = ALL.len();
const ORDERS_CAPPED: usize = ALL.len() + 1;

proptest! {
    /// Deferral only removes the named checks' refusals (DEC-2378 item 11): for any faults and any
    /// deferrals, the advisory decision is the binding gate's over the same scenario with the
    /// deferred faults never applied, an oracle that runs no deferral. So a check not deferred
    /// keeps its own reason, a deferred one never lifts another's, and deny under deferral is
    /// deny binding.
    #[test]
    fn deferral_removes_only_the_deferred_checks_refusals(
        faults in prop::collection::btree_set(0..=ORDERS_CAPPED, 0..=ORDERS_CAPPED + 1),
        deferred in prop::collection::btree_set(0..ALL.len(), 0..=ALL.len()),
    ) {
        let set: Vec<Deferred> = deferred.iter().map(|i| ALL[*i]).collect();
        let mut s = Scenario::allowing();
        let mut oracle = Scenario::allowing();
        for fault in &faults {
            if *fault == NO_QUOTE {
                s.market.quote = None;
                oracle.market.quote = None;
            } else if *fault == ORDERS_CAPPED {
                s.agent.orders_today = common::two_stock_swing_limits().max_orders_per_day;
                oracle.agent.orders_today = common::two_stock_swing_limits().max_orders_per_day;
            } else {
                fail(&mut s, ALL[*fault]);
                if !deferred.contains(fault) {
                    fail(&mut oracle, ALL[*fault]);
                }
            }
        }
        let advisory = evaluate_deferring(&s.input(), &deferring(&set)).unwrap();
        let expected = evaluate(&oracle.input()).unwrap();
        prop_assert_eq!(
            (advisory.decision.verdict, advisory.decision.reason),
            (expected.verdict, expected.reason),
            "faults {:?}, deferred {:?}", faults, set
        );
        if expected.verdict == Verdict::Deny {
            let binding = evaluate(&s.input()).unwrap();
            prop_assert_eq!(binding.verdict, Verdict::Deny, "deny under deferral is deny binding");
        }
    }
}

/// The parts of a check the cell's runtime cannot see a fact for (DEC-2388 item 8): the gate
/// evaluates them on what its caller gives, so a refusal still refuses, but never reports them
/// passed.
const UNSEEN: [Deferred; 5] = [
    Deferred::ReentryCooldown,
    Deferred::Halt,
    Deferred::ConductHistory,
    Deferred::RelatedAccounts,
    Deferred::FeeReservation,
];

/// Changes `s` so that the unseen part `part` refuses it, and the reason the binding gate gives.
fn refuse_on(s: &mut Scenario, part: Deferred) -> ReasonCode {
    let instrument = s.proposed.instrument.clone();
    match part {
        Deferred::ReentryCooldown => {
            s.agent
                .last_exit_fill_at
                .insert(instrument, common::at("2026-09-21T14:59:00Z"));
            ReasonCode::ReentryCooldown
        }
        Deferred::Halt => {
            s.instrument.halted = true;
            ReasonCode::InstrumentHalted
        }
        Deferred::ConductHistory => {
            s.agent.orders_today = common::two_stock_swing_limits().max_orders_per_day;
            ReasonCode::MaxOrdersPerDay
        }
        Deferred::RelatedAccounts => {
            s.account.related_account_resting.insert(
                instrument,
                BTreeSet::from([mandate_risk::RestingSide::Sell]),
            );
            ReasonCode::ConductLimitBreached
        }
        Deferred::FeeReservation => {
            s.proposed.fee_reservation = usd("1000000000");
            ReasonCode::InsufficientBuyingPower
        }
        skipped => panic!("{skipped:?} is a check the pass skips, not an unseen part"),
    }
}

/// An unseen part is never skipped: deferred, it still refuses with the binding gate's reason, so
/// naming it can only stop a pass from being claimed, never lift a refusal.
#[test]
fn an_unseen_part_deferred_still_refuses_with_the_binding_reason() {
    for part in UNSEEN {
        let mut s = Scenario::allowing();
        let reason = refuse_on(&mut s, part);
        let binding = evaluate(&s.input()).unwrap();
        assert_eq!(binding.reason, Some(reason), "{part:?}");
        let mut every = ALL.to_vec();
        every.extend(UNSEEN);
        let advisory = evaluate_deferring(&s.input(), &deferring(&every)).unwrap();
        assert_eq!(
            (advisory.decision.verdict, advisory.decision.reason),
            (binding.verdict, binding.reason),
            "{part:?}"
        );
    }
}

/// An unseen part that passes is reported deferred on its check, never passed, and named.
#[test]
fn an_unseen_part_that_passes_reads_as_deferred_on_its_check() {
    let s = Scenario::allowing();
    for part in UNSEEN {
        let advisory = evaluate_deferring(&s.input(), &deferring(&[part])).unwrap();
        assert_eq!(advisory.decision.verdict, Verdict::Allow, "{part:?}");
        assert_eq!(advisory.deferred, deferring(&[part]), "{part:?}");
        assert!(
            advisory
                .decision
                .checks
                .contains(&CheckOutcome::Deferred(part.check())),
            "{part:?}: {advisory:?}"
        );
        assert!(
            !advisory
                .decision
                .checks
                .contains(&CheckOutcome::Passed(part.check())),
            "{part:?}"
        );
    }
}

#[test]
fn each_unseen_part_belongs_to_the_check_that_reads_it() {
    assert_eq!(Deferred::ReentryCooldown.check(), Check::UniverseAndLimits);
    assert_eq!(Deferred::Halt.check(), Check::SessionAndHalt);
    assert_eq!(Deferred::ConductHistory.check(), Check::ConductControls);
    assert_eq!(Deferred::RelatedAccounts.check(), Check::ConductControls);
    assert_eq!(
        Deferred::FeeReservation.check(),
        Check::BuyingPowerAndExposure
    );
}

/// Over every set of the five unseen parts and the four skipped checks together, an allowed
/// opening reports each touched check deferred and every other passed.
#[test]
fn no_check_an_unseen_part_touches_reads_as_passed() {
    let s = Scenario::allowing();
    let mut every = ALL.to_vec();
    every.extend(UNSEEN);
    for mask in 0..(1_u16 << every.len()) {
        let set: Vec<Deferred> = every
            .iter()
            .enumerate()
            .filter(|(i, _)| mask & (1 << i) != 0)
            .map(|(_, d)| *d)
            .collect();
        let touched: BTreeSet<Check> = set.iter().map(|d| d.check()).collect();
        let advisory = evaluate_deferring(&s.input(), &deferring(&set)).unwrap();
        assert_eq!(advisory.decision.verdict, Verdict::Allow, "{set:?}");
        for outcome in &advisory.decision.checks {
            match outcome {
                CheckOutcome::Deferred(check) => assert!(touched.contains(check), "{set:?}"),
                CheckOutcome::Passed(check) => assert!(!touched.contains(check), "{set:?}"),
                other => panic!("{set:?}: {other:?}"),
            }
        }
    }
}
