//! An owner's or a client's requested buy (DEC-2858, mandate spec §6.2 step 5a): clipped to the
//! §5.3 limits as `propose`'s budget is and never sized, each limit that cut it named, the
//! requested quantity unclipped when a clip would leave nothing, so the gate names the limit
//! (DEC-2858 item 7), and classified with no combined score, so a rule that reads the score denies
//! by its own id (DEC-2858 item 8). The base is `two_stock_swing`: a 1000 order cap, a position cap
//! of min(1500, 0.2 × 10000) = 1500, a gross room of min(2000, 10000) = 2000, and the ask at 100.

mod common;

use std::collections::BTreeSet;

use common::{
    account, base_policy, base_rules, btc_accumulator, compare, crypto_market, decimal,
    flat_account, frac, policy, price, qty, quiet_risk, rule_id, swing_market, two_stock_swing,
    unit, usd,
};
use mandate_builder::{DecidedBy, RequestCut, RequestedBuy, RequestedBy, classify, requested_buy};
use mandate_domain::{AutonomyDecision, MarketSession, Purpose};
use mandate_spec::condition::{Condition, ConditionField, Operator};
use mandate_spec::document::Rule;
use proptest::prelude::*;
use proptest::test_runner::{Config as ProptestConfig, TestCaseError, TestRunner};

fn owners(quantity: &str) -> RequestedBuy {
    owners_on(&flat_account(), quantity)
}

fn owners_on(held: &mandate_builder::AccountSnapshot, quantity: &str) -> RequestedBuy {
    requested_buy(
        &two_stock_swing(),
        held,
        &swing_market(),
        &quiet_risk(),
        RequestedBy::Owner,
        qty(quantity),
    )
    .unwrap_or_else(|e| panic!("{e}"))
    .unwrap_or_else(|| panic!("an owner's buy of {quantity} is proposed"))
}

fn cuts(names: &[RequestCut]) -> BTreeSet<RequestCut> {
    names.iter().copied().collect()
}

/// Five at the 100 ask is 500, under every limit: the owner's five, at the ask, unscored, with
/// the owner as the requester and no limit named.
#[test]
#[ignore = "pending E10-30"]
fn an_owners_buy_within_every_limit_is_the_owners_quantity_at_the_ask() {
    let buy = owners("5");
    assert_eq!(buy.qty, qty("5"));
    assert_eq!(buy.limit_price, price("100"));
    assert_eq!(buy.order_usd, usd("500"));
    assert_eq!(buy.cut_by, BTreeSet::new());
    assert_eq!(buy.action.purpose, Purpose::Open);
    assert_eq!(buy.action.order_usd, usd("500"));
    assert_eq!(
        buy.action.combined_score, None,
        "no model output decided it"
    );
    assert_eq!(buy.action.requested_by, RequestedBy::Owner);
    assert_eq!(buy.action.position_usd_after, usd("500"));
}

/// Twelve is 1200: over the 1000 order cap alone, so ten shares, the order cap named. Twenty-five
/// is 2500: over the order cap, the 1500 position room and the 2000 gross room, so ten shares and
/// all three named.
#[test]
#[ignore = "pending E10-30"]
fn each_limit_below_the_requested_notional_is_named_and_the_quantity_clipped() {
    let order_capped = owners("12");
    assert_eq!(order_capped.qty, qty("10"));
    assert_eq!(order_capped.cut_by, cuts(&[RequestCut::MaxOrderUsd]));
    let every = owners("25");
    assert_eq!(every.qty, qty("10"));
    assert_eq!(
        every.cut_by,
        cuts(&[
            RequestCut::MaxOrderUsd,
            RequestCut::PositionCap,
            RequestCut::GrossExposureCap,
        ])
    );
}

/// Holding 12 marked at 100 leaves 1500 − 1200 = 300 of position room: five (500) is cut to three
/// by the position cap alone, as an increase.
#[test]
#[ignore = "pending E10-30"]
fn a_held_position_leaves_only_its_room_under_the_position_cap() {
    let buy = owners_on(&account("12", "100"), "5");
    assert_eq!(buy.qty, qty("3"));
    assert_eq!(buy.cut_by, cuts(&[RequestCut::PositionCap]));
    assert_eq!(buy.action.purpose, Purpose::Increase);
}

/// Holding 15 at 100 fills the 1500 cap, and 20 is past it: a clip would leave no share, so the
/// requested quantity goes on unclipped and no limit is named, for the gate's own reason to name
/// the cap that refuses it (DEC-2858 item 7).
#[test]
#[ignore = "pending E10-30"]
fn a_clip_that_leaves_nothing_hands_the_gate_the_requested_quantity() {
    for held in ["15", "20"] {
        let buy = owners_on(&account(held, "100"), "2");
        assert_eq!(buy.qty, qty("2"), "holding {held}");
        assert_eq!(buy.cut_by, BTreeSet::new(), "holding {held}");
        assert_eq!(buy.order_usd, usd("200"), "holding {held}");
    }
}

/// Under a 120 order cap and a 150 minimum order, twelve (1200) would clip to one share (100),
/// below the minimum: that clip is not applied either, and the twelve go to the gate unclipped
/// with no limit named (DEC-2858 item 7).
#[test]
#[ignore = "pending E10-30"]
fn a_clip_below_the_minimum_order_hands_the_gate_the_requested_quantity() {
    let mut mandate = two_stock_swing();
    mandate.limits.max_order_usd = usd("120");
    let mut market = swing_market();
    market.min_order_usd = usd("150");
    let buy = requested_buy(
        &mandate,
        &flat_account(),
        &market,
        &quiet_risk(),
        RequestedBy::Owner,
        qty("12"),
    )
    .unwrap_or_else(|e| panic!("{e}"))
    .unwrap_or_else(|| panic!("an owner's buy is proposed"));
    assert_eq!(buy.qty, qty("12"));
    assert_eq!(buy.cut_by, BTreeSet::new());
}

/// A client's request carries the client as its requester; the ceiling is §6.2 step 5a's.
#[test]
#[ignore = "pending E10-30"]
fn a_clients_buy_names_the_client() {
    let buy = requested_buy(
        &two_stock_swing(),
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        RequestedBy::Client,
        qty("5"),
    )
    .unwrap_or_else(|e| panic!("{e}"))
    .unwrap_or_else(|| panic!("a client's buy is proposed"));
    assert_eq!(buy.action.requested_by, RequestedBy::Client);
    assert_eq!(buy.qty, qty("5"));
}

/// Nothing is proposed for a quantity of zero, for a request labelled as the builder's own, or
/// under an `accumulate` goal, whose bounds no journal clip of a request names (`AGENTS.md` rule
/// 3).
#[test]
#[ignore = "pending E10-30"]
fn nothing_is_proposed_for_no_quantity_the_agent_or_an_accumulate_goal() {
    let none = |mandate, market, by, quantity| {
        requested_buy(
            &mandate,
            &flat_account(),
            &market,
            &quiet_risk(),
            by,
            qty(quantity),
        )
        .unwrap_or_else(|e| panic!("{e}"))
    };
    assert_eq!(
        none(two_stock_swing(), swing_market(), RequestedBy::Owner, "0"),
        None
    );
    assert_eq!(
        none(two_stock_swing(), swing_market(), RequestedBy::Agent, "5"),
        None
    );
    assert_eq!(
        none(
            btc_accumulator(),
            crypto_market("60000", "60000", "0.0001"),
            RequestedBy::Owner,
            "0.01"
        ),
        None
    );
}

/// A requested buy is refused where any buy is: a crossed quote and the overnight session.
#[test]
#[ignore = "pending E10-30"]
fn a_crossed_quote_and_the_overnight_session_refuse_a_requested_buy() {
    let code = |market| {
        requested_buy(
            &two_stock_swing(),
            &flat_account(),
            &market,
            &quiet_risk(),
            RequestedBy::Owner,
            qty("1"),
        )
        .map(|buy| buy.map(|b| b.qty))
        .map_err(|e| e.code())
    };
    let mut crossed = swing_market();
    crossed.bid = price("100.01");
    assert_eq!(code(crossed), Err("crossed_quote"));
    let mut overnight = swing_market();
    overnight.session = MarketSession::Overnight;
    assert_eq!(code(overnight), Err("untradable_session"));
}

/// `base_rules` walk `large_orders` (order above 900 asks), then `low_score` (score below 0.65
/// asks), then `routine`. An unscored 500 passes `large_orders` and reaches `low_score`, which
/// cannot be evaluated without a score: deny, by that rule (DEC-2858 item 8). An unscored 1000
/// matches `large_orders` first, which asks as for any proposal.
#[test]
#[ignore = "pending E10-30"]
fn a_rule_reading_the_score_denies_a_request_by_its_own_id() {
    let small = owners("5").action;
    let denied = classify(&base_policy(), &small).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(denied.decision, AutonomyDecision::Deny);
    assert_eq!(denied.by, DecidedBy::Rule(rule_id("low_score")));
    assert_eq!(denied.approval, None);
    let large = owners("10").action;
    let asked = classify(&base_policy(), &large).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(asked.decision, AutonomyDecision::Ask);
    assert_eq!(asked.by, DecidedBy::Rule(rule_id("large_orders")));
}

/// However deep the score sits in a rule's condition, and whatever the rule would have decided,
/// the rule cannot be evaluated for a request and denies it; a client's request is denied the
/// same way, the ceiling only tightening.
#[test]
#[ignore = "pending E10-30"]
fn a_score_anywhere_in_a_rule_denies_a_request() {
    let score = compare(ConditionField::CombinedScore, Operator::Gt, decimal("0.5"));
    let order = compare(ConditionField::OrderUsd, Operator::Gt, decimal("100000"));
    for when in [
        Condition::Not(Box::new(score.clone())),
        Condition::Any(vec![order.clone(), score.clone()]),
        Condition::All(vec![Condition::Not(Box::new(order)), score]),
    ] {
        let rules = vec![Rule {
            id: rule_id("scored"),
            when,
            then: AutonomyDecision::Auto,
        }];
        let walked = policy(rules, AutonomyDecision::Auto, AutonomyDecision::Ask, None);
        for by in [RequestedBy::Owner, RequestedBy::Client] {
            let mut action = owners("5").action;
            action.requested_by = by;
            let classified = classify(&walked, &action).unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(classified.decision, AutonomyDecision::Deny, "{by:?}");
            assert_eq!(classified.by, DecidedBy::Rule(rule_id("scored")), "{by:?}");
        }
    }
}

/// A request is classified on its other facts as any opening is: with `low_score` removed, the
/// owner's 500 reaches `routine`, which is automatic; a scored action under the same rules is
/// unchanged.
#[test]
fn a_request_no_rule_reads_the_score_of_is_classified_on_its_facts() {
    let without_score: Vec<Rule> = base_rules()
        .into_iter()
        .filter(|rule| rule.id != rule_id("low_score"))
        .collect();
    let walked = policy(
        without_score,
        AutonomyDecision::Ask,
        AutonomyDecision::Ask,
        None,
    );
    let mut action = mandate_builder::buy_action(
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        unit("1"),
        qty("5"),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    action.combined_score = None;
    action.requested_by = RequestedBy::Owner;
    let classified = classify(&walked, &action).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(classified.decision, AutonomyDecision::Auto);
    assert_eq!(classified.by, DecidedBy::Rule(rule_id("routine")));
}

/// The requested buy as computed here independently, in thousandths of a dollar on whole
/// dollars and whole shares: the limits below the requested notional, then the budget's whole
/// shares, or the requested quantity with nothing named when no share is left.
fn oracle(case: &Case) -> (u64, BTreeSet<RequestCut>) {
    let milli = |dollars: u64| dollars * 1000;
    let requested = milli(case.quantity * case.ask);
    let cap = milli(case.max_position).min(case.equity * case.fraction_permille);
    let held = milli(case.held * case.ask + case.working);
    let position_room = cap.saturating_sub(held);
    let gross_room = milli(case.max_gross.min(case.equity)).saturating_sub(milli(case.gross));
    let mut cut = BTreeSet::new();
    for (room, name) in [
        (milli(case.max_order), RequestCut::MaxOrderUsd),
        (position_room, RequestCut::PositionCap),
        (gross_room, RequestCut::GrossExposureCap),
    ] {
        if room < requested {
            cut.insert(name);
        }
    }
    let budget = requested
        .min(milli(case.max_order))
        .min(position_room)
        .min(gross_room);
    let shares = budget / milli(case.ask);
    if cut.is_empty() || shares == 0 {
        (case.quantity, BTreeSet::new())
    } else {
        (shares, cut)
    }
}

#[derive(Debug, Clone)]
struct Case {
    equity: u64,
    fraction_permille: u64,
    max_position: u64,
    max_order: u64,
    max_gross: u64,
    held: u64,
    working: u64,
    gross: u64,
    ask: u64,
    quantity: u64,
}

fn cases() -> impl Strategy<Value = Case> {
    (
        (
            1_000u64..20_000,
            1u64..=1000,
            1u64..20_000,
            1u64..5_000,
            1u64..20_000,
        ),
        (0u64..30, 0u64..2_000, 0u64..5_000, 1u64..500, 1u64..60),
    )
        .prop_map(
            |(
                (equity, fraction_permille, max_position, max_order, max_gross),
                (held, working, gross, ask, quantity),
            )| Case {
                equity,
                fraction_permille,
                max_position,
                max_order,
                max_gross,
                held,
                working,
                gross,
                ask,
                quantity,
            },
        )
}

/// A fraction of a thousand as canonical decimal text: 250 is `0.25`, 1000 is `1`.
fn permille(value: u64) -> String {
    let text = format!("{}.{:03}", value / 1000, value % 1000);
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn one_case(case: &Case) -> Result<(), TestCaseError> {
    let mut mandate = two_stock_swing();
    mandate.limits.max_position_usd = usd(&case.max_position.to_string());
    mandate.limits.max_position_fraction = frac(&permille(case.fraction_permille));
    mandate.limits.max_order_usd = usd(&case.max_order.to_string());
    mandate.limits.max_gross_exposure_usd = usd(&case.max_gross.to_string());
    let ask = case.ask.to_string();
    let mut held = account(&case.held.to_string(), &ask);
    held.agent_equity = usd(&case.equity.to_string());
    held.working_opening_cost = usd(&case.working.to_string());
    held.gross_usd = usd(&case.gross.to_string());
    let mut market = swing_market();
    market.bid = price(&ask);
    market.ask = price(&ask);
    let buy = requested_buy(
        &mandate,
        &held,
        &market,
        &quiet_risk(),
        RequestedBy::Owner,
        qty(&case.quantity.to_string()),
    )
    .map_err(|e| TestCaseError::fail(e.to_string()))?
    .ok_or_else(|| TestCaseError::fail("an owner's buy is proposed"))?;
    let (shares, cut) = oracle(case);
    prop_assert_eq!(buy.qty, qty(&shares.to_string()));
    prop_assert_eq!(buy.cut_by, cut);
    prop_assert_eq!(buy.action.combined_score, None);
    Ok(())
}

/// Every requested buy is the oracle's: never more than the owner asked, never past a limit it
/// was clipped to, every limit below the requested notional named, and the requested quantity
/// whole whenever no limit cut it or a clip would leave nothing.
#[test]
#[ignore = "pending E10-30"]
fn every_requested_buy_is_clipped_exactly_as_the_oracle_says() -> Result<(), String> {
    TestRunner::new(ProptestConfig::with_cases(512))
        .run(&cases(), |case| one_case(&case))
        .map_err(|e| e.to_string())
}
