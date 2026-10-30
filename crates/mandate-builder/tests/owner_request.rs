//! Mandate spec §6.2 step 5d and MI-35 ([DEC-2859](../../../docs/project/decisions/DEC-2859.md),
//! the founder's): an owner's own request whose ASK a reviewer holds is never AUTO, and one whose
//! only ASK is the autonomy rules' or the default's is always AUTO, labelled `owner_request`.
//!
//! **The oracle shares no code with the crate.** It judges each reviewer from the generated
//! figures themselves: the admission ceiling from the generated `new_instrument` and admission
//! setting, the review date by a day count of its own ([`day_number`]), never `Date`'s ordering,
//! and the two-approver threshold by comparing whole dollars as integers, never `Usd`. It tells the
//! cases apart by the step-5b decision `classify` returns, which the first-match oracle of
//! `properties.rs` already pins, and never by the decision's label.
//!
//! **Non-vacuity.** The first property counts, by the oracle's own reading, how many generated
//! owner ASKs were held by each reviewer and how many were not, and fails unless every branch was
//! reached.

mod common;

use std::sync::atomic::{AtomicU32, Ordering};

use common::{BTC_INSTRUMENT, asset, compare, decimal, flag, policy, rule_id, unit, usd};
use mandate_builder::{
    ActionContext, Classification, DecidedBy, RequestedBy, classify, held_for_review, owner_request,
};
use mandate_domain::{AssetClass, AutonomyDecision, MarketSession, Purpose};
use mandate_num::Signed;
use mandate_spec::condition::{Condition, ConditionField, ConditionValue, Operator};
use mandate_spec::document::{Autonomy, Rule};
use mandate_time::Date;
use proptest::prelude::*;
use proptest::test_runner::TestRunner;

const DAYS: [&str; 6] = [
    "2026-12-22",
    "2026-12-23",
    "2026-12-24",
    "2026-12-31",
    "2027-01-01",
    "2027-03-01",
];

const PURPOSES: [Purpose; 6] = [
    Purpose::Open,
    Purpose::Increase,
    Purpose::DiscretionaryExit,
    Purpose::OwnerExit,
    Purpose::RiskExit,
    Purpose::Protective,
];

const RULE_IDS: [&str; 3] = ["first", "second", "third"];

/// The figures one case is generated from, kept beside the policy and the action so the oracle
/// reads its own copy rather than the crate's types.
#[derive(Debug, Clone)]
struct Case {
    policy: Autonomy,
    action: ActionContext,
    new_instrument: bool,
    admission: AutonomyDecision,
    review_by: Option<&'static str>,
    risk_day: &'static str,
    order_dollars: i128,
    threshold_dollars: Option<i128>,
}

fn decisions() -> impl Strategy<Value = AutonomyDecision> {
    prop::sample::select(vec![
        AutonomyDecision::Auto,
        AutonomyDecision::Ask,
        AutonomyDecision::Deny,
    ])
}

/// A rule an owner's request can be decided by: no rule here reads `combined_score`, which such a
/// request does not carry (DEC-2858 item 8, pinned elsewhere).
fn leaf() -> impl Strategy<Value = Condition> {
    prop_oneof![
        (
            prop::sample::select(vec![Operator::Gt, Operator::Lte]),
            0i128..=3_000
        )
            .prop_map(|(op, dollars)| compare(
                ConditionField::OrderUsd,
                op,
                decimal(&dollars.to_string())
            )),
        prop::sample::select(vec!["open".to_owned(), "increase".to_owned()]).prop_map(|member| {
            compare(ConditionField::Purpose, Operator::Eq, common::text(&member))
        }),
        any::<bool>().prop_map(|value| compare(
            ConditionField::NewInstrument,
            Operator::Eq,
            flag(value)
        )),
        Just(Condition::Compare {
            field: ConditionField::Purpose,
            op: Operator::In,
            value: ConditionValue::List(vec!["increase".to_owned(), "open".to_owned()]),
        }),
    ]
}

fn day(text: &str) -> Date {
    common::day(text)
}

/// A day's place in the calendar by this file's own arithmetic, 32 days a month and 13 months a
/// year, which keeps the order of real dates and shares nothing with `Date`.
fn day_number(text: &str) -> u32 {
    let parts: Vec<u32> = text.split('-').filter_map(|p| p.parse().ok()).collect();
    match parts.as_slice() {
        [year, month, day] => (year * 13 + month) * 32 + day,
        _ => panic!("`{text}` is a calendar date"),
    }
}

prop_compose! {
    fn cases()(
        conditions in prop::collection::vec(leaf(), 0..=3),
        outcomes in prop::collection::vec(decisions(), 3),
        default in decisions(),
        admission in decisions(),
        threshold_dollars in prop::option::weighted(0.6, 1i128..=3_000),
        review_by in prop::option::weighted(0.6, prop::sample::select(DAYS.to_vec())),
        risk_day in prop::sample::select(DAYS.to_vec()),
        purpose in prop::sample::select(PURPOSES.to_vec()),
        order_dollars in 0i128..=3_000,
        new_instrument in any::<bool>(),
        requested_by in prop::sample::select(vec![
            RequestedBy::Owner, RequestedBy::Owner, RequestedBy::Owner, RequestedBy::Agent, RequestedBy::Client,
        ]),
    ) -> Case {
        let rules = conditions
            .into_iter()
            .enumerate()
            .map(|(index, when)| Rule { id: rule_id(RULE_IDS[index]), when, then: outcomes[index] })
            .collect();
        let threshold = threshold_dollars.map(|t| t.to_string());
        let policy = Autonomy {
            review_by: review_by.map(day),
            ..policy(rules, default, admission, threshold.as_deref())
        };
        let owner = requested_by != RequestedBy::Agent;
        let action = ActionContext {
            purpose,
            order_usd: usd(&order_dollars.to_string()),
            combined_score: if owner { None } else { Some(unit("0.8")) },
            instrument: asset(BTC_INSTRUMENT),
            asset_class: AssetClass::Crypto,
            session: MarketSession::Crypto,
            first_trade_in_instrument: false,
            new_instrument,
            thesis_confidence: unit("0.5"),
            drawdown: mandate_num::Unit::ZERO,
            daily_pnl_fraction: Signed::ZERO,
            position_usd_after: usd("0"),
            gross_usd_after: usd("0"),
            bought_today_usd: usd("0"),
            position_pnl_fraction: Signed::ZERO,
            requested_by,
            risk_day: day(risk_day),
        };
        Case { policy, action, new_instrument, admission, review_by, risk_day, order_dollars, threshold_dollars }
    }
}

/// The oracle's reviewers, from the case's own figures (MI-35): an `ask` admission on a new
/// instrument, a risk day after the review date, and an order above the two-approver threshold.
fn oracle_held(case: &Case) -> (bool, bool, bool) {
    let admission = case.new_instrument && case.admission == AutonomyDecision::Ask;
    let review = case
        .review_by
        .is_some_and(|review_by| day_number(case.risk_day) > day_number(review_by));
    let threshold = case
        .threshold_dollars
        .is_some_and(|threshold| case.order_dollars > threshold);
    (admission, review, threshold)
}

fn run<S>(strategy: S, cases: u32, body: impl Fn(S::Value) -> Result<(), TestCaseError>)
where
    S: Strategy,
    S::Value: std::fmt::Debug,
{
    let config = ProptestConfig {
        cases,
        failure_persistence: None,
        ..ProptestConfig::default()
    };
    if let Err(failure) = TestRunner::new(config).run(&strategy, body) {
        panic!("{failure}");
    }
}

fn classified(case: &Case) -> Result<Classification, TestCaseError> {
    classify(&case.policy, &case.action)
        .map_err(|e| TestCaseError::fail(format!("classify decides, not {e}")))
}

fn stepped(case: &Case, before: Classification) -> Result<Classification, TestCaseError> {
    owner_request(&case.policy, &case.action, before)
        .map_err(|e| TestCaseError::fail(format!("step 5d decides, not {e}")))
}

/// MI-35, both halves, against the oracle: an owner's ASK a reviewer holds is never AUTO and keeps
/// its label and approver count; an owner's ASK no reviewer holds is AUTO, labelled
/// `owner_request`, with no approval; a DENY and an AUTO, and the agent's and a client's requests,
/// pass through unchanged. `held_for_review` agrees with the oracle on every case.
#[test]
fn an_owner_request_a_reviewer_holds_is_never_auto_and_one_none_holds_is_never_asked() {
    static HELD_BY_ADMISSION: AtomicU32 = AtomicU32::new(0);
    static HELD_BY_REVIEW: AtomicU32 = AtomicU32::new(0);
    static HELD_BY_THRESHOLD: AtomicU32 = AtomicU32::new(0);
    static CONVERTED: AtomicU32 = AtomicU32::new(0);
    run(cases(), 2048, |case| {
        let (admission, review, threshold) = oracle_held(&case);
        let held = admission || review || threshold;
        let reported = held_for_review(&case.policy, &case.action)
            .map_err(|e| TestCaseError::fail(format!("held_for_review answers, not {e}")))?;
        prop_assert_eq!(reported, held, "the reviewers are the oracle's");
        let before = classified(&case)?;
        let after = stepped(&case, before.clone())?;
        let owners_ask = case.action.requested_by == RequestedBy::Owner
            && before.decision == AutonomyDecision::Ask;
        if owners_ask && held {
            prop_assert_ne!(
                after.decision,
                AutonomyDecision::Auto,
                "MI-35: a held owner's request is never AUTO"
            );
            prop_assert_eq!(
                &after,
                &before,
                "a held ASK keeps its label and its approvers"
            );
            for (reached, counter) in [
                (admission, &HELD_BY_ADMISSION),
                (review, &HELD_BY_REVIEW),
                (threshold, &HELD_BY_THRESHOLD),
            ] {
                if reached {
                    counter.fetch_add(1, Ordering::Relaxed);
                }
            }
        } else if owners_ask {
            prop_assert_eq!(
                after.decision,
                AutonomyDecision::Auto,
                "MI-35: an owner's ASK no reviewer holds is AUTO"
            );
            prop_assert_eq!(after.by.label(), "owner_request");
            prop_assert_eq!(after.by, DecidedBy::OwnerRequest);
            prop_assert!(after.approval.is_none(), "an AUTO asks no one");
            CONVERTED.fetch_add(1, Ordering::Relaxed);
        } else {
            prop_assert_eq!(
                &after,
                &before,
                "step 5d leaves a DENY, an AUTO and anyone else's request"
            );
        }
        Ok(())
    });
    for (name, counter) in [
        ("held by the admission ceiling", &HELD_BY_ADMISSION),
        ("held by the review ceiling", &HELD_BY_REVIEW),
        ("held by the two-approver threshold", &HELD_BY_THRESHOLD),
        ("converted", &CONVERTED),
    ] {
        assert!(
            counter.load(Ordering::Relaxed) > 0,
            "no generated owner's ASK was {name}"
        );
    }
}

prop_compose! {
    /// An owner's opening or increase whose ASK can come only from the rules or the default: no new
    /// instrument, a review date the risk day has not passed or none, and a threshold the order does
    /// not exceed or none. The asking rule, when there is one, is first and matches every opening.
    fn rule_asks()(
        purpose in prop::sample::select(vec![Purpose::Open, Purpose::Increase]),
        order_dollars in 0i128..=3_000,
        headroom in prop::option::of(0i128..=3_000),
        reviewed in any::<bool>(),
        by_rule in any::<bool>(),
        admission in decisions(),
    ) -> (Autonomy, ActionContext) {
        let rules = if by_rule {
            vec![Rule {
                id: rule_id("asks"),
                when: Condition::Compare {
                    field: ConditionField::Purpose,
                    op: Operator::In,
                    value: ConditionValue::List(vec!["increase".to_owned(), "open".to_owned()]),
                },
                then: AutonomyDecision::Ask,
            }]
        } else {
            Vec::new()
        };
        let threshold = headroom.map(|h| (order_dollars + h).max(1).to_string());
        let policy = Autonomy {
            review_by: reviewed.then(|| day("2026-12-23")),
            ..policy(rules, AutonomyDecision::Ask, admission, threshold.as_deref())
        };
        let action = ActionContext {
            purpose,
            order_usd: usd(&order_dollars.to_string()),
            combined_score: None,
            instrument: asset(BTC_INSTRUMENT),
            asset_class: AssetClass::Crypto,
            session: MarketSession::Crypto,
            first_trade_in_instrument: false,
            new_instrument: false,
            thesis_confidence: unit("0.5"),
            drawdown: mandate_num::Unit::ZERO,
            daily_pnl_fraction: Signed::ZERO,
            position_usd_after: usd("0"),
            gross_usd_after: usd("0"),
            bought_today_usd: usd("0"),
            position_pnl_fraction: Signed::ZERO,
            requested_by: RequestedBy::Owner,
            risk_day: day("2026-12-23"),
        };
        (policy, action)
    }
}

/// DEC-2159 as DEC-2859 keeps it: an owner's own request whose only ASK is the autonomy rules' or
/// the default's is always AUTO, labelled `owner_request`, whatever the admission setting (the
/// order is not a new instrument), on the review date itself, and at the threshold itself.
#[test]
fn an_owner_request_whose_only_ask_is_the_rules_is_always_auto() {
    run(rule_asks(), 1024, |(policy, action)| {
        let before = classify(&policy, &action)
            .map_err(|e| TestCaseError::fail(format!("classify decides, not {e}")))?;
        prop_assert_eq!(
            before.decision,
            AutonomyDecision::Ask,
            "the generator makes the rules ask"
        );
        let after = owner_request(&policy, &action, before)
            .map_err(|e| TestCaseError::fail(format!("step 5d decides, not {e}")))?;
        prop_assert_eq!(after.decision, AutonomyDecision::Auto);
        prop_assert_eq!(after.by, DecidedBy::OwnerRequest);
        prop_assert!(after.approval.is_none());
        Ok(())
    });
}

/// MC-D31's shape by hand: one dollar over the threshold holds the owner's ASK for two approvers,
/// and the threshold itself does not.
#[test]
fn the_threshold_holds_only_an_order_above_it() -> Result<(), String> {
    let threshold = Some("500");
    let asking = policy(
        Vec::new(),
        AutonomyDecision::Ask,
        AutonomyDecision::Ask,
        threshold,
    );
    let owners = |dollars: &str| ActionContext {
        purpose: Purpose::Open,
        order_usd: usd(dollars),
        combined_score: None,
        instrument: asset(BTC_INSTRUMENT),
        asset_class: AssetClass::Crypto,
        session: MarketSession::Crypto,
        first_trade_in_instrument: false,
        new_instrument: false,
        thesis_confidence: unit("0.5"),
        drawdown: mandate_num::Unit::ZERO,
        daily_pnl_fraction: Signed::ZERO,
        position_usd_after: usd("0"),
        gross_usd_after: usd("0"),
        bought_today_usd: usd("0"),
        position_pnl_fraction: Signed::ZERO,
        requested_by: RequestedBy::Owner,
        risk_day: day(common::REFERENCE_DAY),
    };
    for (dollars, decision, approvers) in [
        ("500", AutonomyDecision::Auto, None),
        ("500.01", AutonomyDecision::Ask, Some(2)),
    ] {
        let action = owners(dollars);
        let before = classify(&asking, &action).map_err(|e| e.to_string())?;
        let after = owner_request(&asking, &action, before).map_err(|e| e.to_string())?;
        assert_eq!(after.decision, decision, "{dollars}");
        assert_eq!(
            after.approval.map(|a| a.approvers_required.get()),
            approvers,
            "{dollars}"
        );
    }
    Ok(())
}
