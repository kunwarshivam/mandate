//! The advisory gate's deferred checks (DEC-2378 item 11): the cell's runtime dry-runs an opening
//! without the facts nothing journals yet, so it defers, by name, instrument eligibility, the
//! liquidity floor, participation, and the day-trade budget. A deferred check is never reported
//! passed, an exit defers nothing, and with nothing deferred the advisory pass is the binding
//! gate's own decision, which the executor's gate still makes over every check (rule 1).

mod common;

use std::collections::BTreeSet;

use common::{Scenario, usd};
use mandate_risk::{
    DayTradeLedger, DayTradeRegime, Deferred, Origin, ReasonCode, Side, Verdict, evaluate,
    evaluate_deferring,
};

fn deferring(set: &[Deferred]) -> BTreeSet<Deferred> {
    set.iter().copied().collect()
}

/// Each deferred check, the one change to the allowing scenario that fails it, and the reason
/// the binding gate denies with.
fn failing(check: Deferred) -> (Scenario, ReasonCode) {
    let mut s = Scenario::allowing();
    let reason = match check {
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
    };
    (s, reason)
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
