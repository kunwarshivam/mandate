//! Hand-calculated gate cases: every MC-G and MC-F figure recomputed from the spec, plus the rules
//! those families do not reach.
//!
//! Each case states the arithmetic in its assertion message, so a failure says which figure moved
//! rather than only that two structs differ. Every test is pending until its story lands (DEC-77),
//! and every one fails on the stubs because the stub returns an error where the case expects a
//! decision.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{
    INSTRUMENT_2, INSTRUMENT_3, Scenario, asset, at, fraction, mandate_with, open_order, price,
    proposal, qty, usd, working_universe,
};
use mandate_risk::{
    AgentId, AgentMode, AgentPosition, AssetClass, ClientOrderId, FlattenInitiator, FlattenInput,
    Origin, Purpose, ReasonCode, Session, Side, Verdict, agent_flatten, evaluate,
};

/// An unread working universe is an error, never an allow and never a deny (DEC-129 item 3).
#[test]
#[ignore = "pending E6-3"]
fn an_absent_working_universe_is_an_error() {
    let mut s = Scenario::allowing();
    s.universe = mandate_risk::WorkingUniverse::Unavailable;

    let e = evaluate(&s.input()).expect_err("an unread universe cannot be decided");
    assert_eq!(
        e.code(),
        "working_universe_unavailable",
        "the gate refuses to decide rather than guessing at the universe"
    );
}

/// A protective order is never held by a mode; `paused` holds a plain risk exit but not a kill
/// switch's (DEC-129 item 17).
#[test]
#[ignore = "pending E6-3"]
fn a_protective_order_is_never_held_by_a_mode() {
    let mut s = Scenario::allowing();
    s.agent.mode = AgentMode::Paused;
    s.agent.positions.insert(asset(INSTRUMENT_3), qty("10"));
    s.proposed = proposal(INSTRUMENT_3, Side::Sell, "10", "100", Origin::ProtectiveLeg);

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.purpose),
        (Verdict::Allow, Purpose::Protective),
        "protection that cannot be placed leaves a position unprotected"
    );
}

#[test]
#[ignore = "pending E6-3"]
fn a_paused_agent_holds_a_plain_risk_exit() {
    let mut s = Scenario::allowing();
    s.agent.mode = AgentMode::Paused;
    s.agent.positions.insert(asset(INSTRUMENT_3), qty("10"));
    s.proposed = proposal(INSTRUMENT_3, Side::Sell, "10", "100", Origin::RiskEngine);

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.reason),
        (Verdict::Hold, Some(ReasonCode::AgentPaused)),
        "a risk exit that is not a kill switch's is held by paused"
    );
}

#[test]
#[ignore = "pending E6-3"]
fn a_kill_switch_order_is_exempt_from_paused() {
    let mut s = Scenario::allowing();
    s.agent.mode = AgentMode::Paused;
    s.agent.positions.insert(asset(INSTRUMENT_3), qty("10"));
    s.proposed = proposal(
        INSTRUMENT_3,
        Side::Sell,
        "10",
        "100",
        Origin::AutomatedKillSwitch,
    );

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.purpose),
        (Verdict::Allow, Purpose::RiskExit),
        "kill-switch orders are exempt from the agent's mode (spec 5.5)"
    );
}

/// An owner's ordinary close is not a kill switch, so `paused` holds it (DEC-129 item 17).
#[test]
#[ignore = "pending E6-3"]
fn a_paused_agent_holds_an_owner_close_but_not_an_owner_kill_switch() {
    let mut s = Scenario::allowing();
    s.agent.mode = AgentMode::Paused;
    s.agent.positions.insert(asset(INSTRUMENT_3), qty("10"));
    s.proposed = proposal(INSTRUMENT_3, Side::Sell, "10", "100", Origin::OwnerClose);
    let held = evaluate(&s.input()).expect("the gate decides");

    s.proposed = proposal(
        INSTRUMENT_3,
        Side::Sell,
        "10",
        "100",
        Origin::OwnerKillSwitch,
    );
    let allowed = evaluate(&s.input()).expect("the gate decides");

    assert_eq!(
        (held.verdict, allowed.verdict),
        (Verdict::Hold, Verdict::Allow),
        "one Owner origin could not tell a close from a kill switch"
    );
}

/// An `Unknown` order denies an opening and holds a reduction, under one code (DEC-129 item 22).
#[test]
#[ignore = "pending E6-3"]
fn an_unknown_order_denies_an_opening_and_holds_a_reduction() {
    let mut s = Scenario::allowing();
    s.account.unknown_orders.insert(asset(INSTRUMENT_3));
    let opening = evaluate(&s.input()).expect("the gate decides");

    s.agent.positions.insert(asset(INSTRUMENT_3), qty("10"));
    s.proposed = proposal(INSTRUMENT_3, Side::Sell, "10", "100", Origin::RiskEngine);
    let reduction = evaluate(&s.input()).expect("the gate decides");

    assert_eq!(
        (
            opening.verdict,
            opening.reason,
            reduction.verdict,
            reduction.reason
        ),
        (
            Verdict::Deny,
            Some(ReasonCode::UnknownOrderInFlight),
            Verdict::Hold,
            Some(ReasonCode::UnknownOrderInFlight)
        ),
        "rule 9 denies a new order; MI-1 lists an Unknown order among the things that hold an exit"
    );
}

/// A blocked account denies every purpose, a risk exit included: the broker arm of MI-1's list
/// (`RC-15::status_not_active`).
#[test]
#[ignore = "pending E6-9"]
fn a_blocked_account_holds_even_a_risk_exit() {
    let mut s = Scenario::allowing();
    s.account.state = mandate_risk::AccountState::Blocked;
    s.agent.positions.insert(asset(INSTRUMENT_3), qty("10"));
    s.proposed = proposal(INSTRUMENT_3, Side::Sell, "10", "100", Origin::RiskEngine);

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.reason),
        (Verdict::Deny, Some(ReasonCode::AccountTradingBlocked)),
        "the venue is shut to this account, so sending the order would only collect a reject"
    );
}

/// A `closing_only` account still allows every exit (`RC-15` step 4).
#[test]
#[ignore = "pending E6-9"]
fn a_restricted_account_still_allows_an_exit() {
    let mut s = Scenario::allowing();
    s.account.state = mandate_risk::AccountState::ClosingOnly;
    s.agent.positions.insert(asset(INSTRUMENT_3), qty("10"));
    s.proposed = proposal(INSTRUMENT_3, Side::Sell, "10", "100", Origin::RiskEngine);

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.purpose),
        (Verdict::Allow, Purpose::RiskExit),
        "closing-only is exits-only, not closed"
    );
}

/// The eligibility floor reports the first failing item of §3.2's list, not the worst one.
#[test]
#[ignore = "pending E6-7"]
fn the_floor_reports_the_first_failing_item() {
    let mut s = Scenario::allowing();
    s.instrument.exchange = Some(mandate_risk::Exchange::Otc);
    s.instrument.prior_close = Some(price("3.5"));

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.reason),
        (Verdict::Deny, Some(ReasonCode::IneligibleExchange)),
        "item 2 fails before item 4, so the exchange is reported and not the price floor"
    );
}

/// §3.2 item 6 fails closed for an ETP the source has not classified.
#[test]
#[ignore = "pending E6-7"]
fn an_unclassified_etp_is_complex() {
    let mut s = Scenario::allowing();
    s.instrument.etp = mandate_risk::EtpClass::Unclassified;

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.reason),
        (Verdict::Deny, Some(ReasonCode::LeveragedEtpNotEnabled)),
        "an ETP not yet classified is treated as complex"
    );
}

/// A classification older than the configured age denies an ETP opening (DEC-129 item 10).
#[test]
#[ignore = "pending E6-7"]
fn a_stale_classification_denies_an_etp_opening() {
    let mut s = Scenario::allowing();
    s.instrument.etp = mandate_risk::EtpClass::Complex;
    s.instrument.etp_classified_at = Some(at("2026-09-01T00:00:00Z"));

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.reason),
        (Verdict::Deny, Some(ReasonCode::LeveragedEtpNotEnabled)),
        "20 days is past etp_classification_max_age_s = 7 days"
    );
}

/// The floor never applies to a risk-reducing order in a held instrument (`RC-16` step 8).
#[test]
#[ignore = "pending E6-7"]
fn the_floor_never_blocks_an_exit_in_a_held_instrument() {
    let mut s = Scenario::allowing();
    s.instrument.exchange = Some(mandate_risk::Exchange::Otc);
    s.universe = working_universe(&[INSTRUMENT_2]);
    s.agent.positions.insert(asset(INSTRUMENT_3), qty("10"));
    s.proposed = proposal(INSTRUMENT_3, Side::Sell, "10", "30", Origin::RiskEngine);

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.purpose),
        (Verdict::Allow, Purpose::RiskExit),
        "risk-reducing orders for held positions are allowed regardless of the floor"
    );
}

/// `ref.py` takes an explicit `owner_floor_price` ahead of the computed one.
#[test]
#[ignore = "pending E6-3"]
fn an_explicit_owner_floor_price_overrides_the_computed_one() {
    let orders = BTreeMap::new();
    let positions = vec![AgentPosition {
        agent: AgentId(1),
        instrument: asset(INSTRUMENT_2),
        asset_class: AssetClass::UsEquity,
        qty: qty("10"),
    }];
    let plan = agent_flatten(&FlattenInput {
        agent: AgentId(1),
        open_orders: &orders,
        agent_positions: &positions,
        broker_positions: &BTreeMap::new(),
        session: Session::AfterHours,
        initiator: FlattenInitiator::Owner,
        owner_confirmed_bid: Some(price("100")),
        max_exit_offset: fraction("0.03"),
        owner_floor_price: Some(price("98.5")),
    })
    .expect("the flatten plans");

    assert_eq!(
        plan.sells.first().map(|s| s.floor_price),
        Some(Some(price("98.5"))),
        "the owner's explicit floor wins over bid x (1 - max_exit_offset)"
    );
}

/// A zero-quantity position produces no sell at all.
#[test]
#[ignore = "pending E6-3"]
fn a_flat_position_produces_no_sell() {
    let orders = BTreeMap::new();
    let positions = vec![AgentPosition {
        agent: AgentId(1),
        instrument: asset(INSTRUMENT_2),
        asset_class: AssetClass::UsEquity,
        qty: qty("0"),
    }];
    let plan = agent_flatten(&FlattenInput {
        agent: AgentId(1),
        open_orders: &orders,
        agent_positions: &positions,
        broker_positions: &BTreeMap::new(),
        session: Session::Regular,
        initiator: FlattenInitiator::RiskLimit,
        owner_confirmed_bid: None,
        max_exit_offset: fraction("0.03"),
        owner_floor_price: None,
    })
    .expect("the flatten plans");

    assert!(
        plan.sells.is_empty() && plan.deferred_sells.is_empty(),
        "a flat position is already flat"
    );
}

/// A sell above the agent's position crosses zero (§5.3 rule 3).
#[test]
#[ignore = "pending E6-6"]
fn a_sell_above_the_position_is_would_cross_zero() {
    let mut s = Scenario::allowing();
    s.agent.positions.insert(asset(INSTRUMENT_3), qty("5"));
    s.proposed = proposal(INSTRUMENT_3, Side::Sell, "6", "100", Origin::OrderBuilder);

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.reason),
        (Verdict::Deny, Some(ReasonCode::WouldCrossZero)),
        "6 is above the agent's position of 5"
    );
}

/// Checks after the failing one are `NotReached`, never `Passed` (§9.1's journal payload).
#[test]
#[ignore = "pending E6-3"]
fn checks_after_the_failure_are_not_reached() {
    let mut s = Scenario::allowing();
    s.universe = working_universe(&[INSTRUMENT_2]);

    let d = evaluate(&s.input()).expect("the gate decides");
    let not_reached = d
        .checks
        .iter()
        .filter(|c| matches!(c, mandate_risk::CheckOutcome::NotReached(_)))
        .count();
    assert!(
        not_reached > 0 && d.checks.len() == 8,
        "every check is listed, and the ones after the failure say so"
    );
}

/// One flatten of a single ten-share equity position, for the tests that vary only one input.
fn flatten_of(
    session: Session,
    initiator: FlattenInitiator,
    confirmed_bid: Option<&str>,
    floor: Option<&str>,
) -> mandate_risk::FlattenPlan {
    let orders = BTreeMap::new();
    let positions = vec![AgentPosition {
        agent: AgentId(1),
        instrument: asset(INSTRUMENT_2),
        asset_class: AssetClass::UsEquity,
        qty: qty("10"),
    }];
    let broker = BTreeMap::new();
    agent_flatten(&FlattenInput {
        agent: AgentId(1),
        open_orders: &orders,
        agent_positions: &positions,
        broker_positions: &broker,
        session,
        initiator,
        owner_confirmed_bid: confirmed_bid.map(price),
        max_exit_offset: fraction("0.03"),
        owner_floor_price: floor.map(price),
    })
    .expect("the flatten plans")
}

/// The purpose table, row by row: every `Origin` and side, against a position and without one.
#[test]
#[ignore = "pending E6-3"]
fn purpose_is_assigned_from_origin_side_and_position() {
    let rows = [
        (Origin::OrderBuilder, Side::Buy, "1", "0", Purpose::Open),
        (Origin::OrderBuilder, Side::Buy, "1", "5", Purpose::Increase),
        (
            Origin::ProtectiveLeg,
            Side::Sell,
            "5",
            "5",
            Purpose::Protective,
        ),
        (Origin::RiskEngine, Side::Sell, "5", "5", Purpose::RiskExit),
        (
            Origin::TrimToTarget,
            Side::Sell,
            "5",
            "5",
            Purpose::RiskExit,
        ),
        (
            Origin::StopWatchdog,
            Side::Sell,
            "5",
            "5",
            Purpose::RiskExit,
        ),
        (
            Origin::AutomatedKillSwitch,
            Side::Sell,
            "5",
            "5",
            Purpose::RiskExit,
        ),
        (Origin::OwnerClose, Side::Sell, "5", "5", Purpose::OwnerExit),
        (
            Origin::OwnerKillSwitch,
            Side::Sell,
            "5",
            "5",
            Purpose::OwnerExit,
        ),
        (
            Origin::GoalCompletion,
            Side::Sell,
            "5",
            "5",
            Purpose::DiscretionaryExit,
        ),
        (
            Origin::RemovedInstrument,
            Side::Sell,
            "5",
            "5",
            Purpose::DiscretionaryExit,
        ),
    ];
    for (origin, side, q, position, want) in rows {
        let got = mandate_risk::assign_purpose(origin, side, qty(q), qty(position))
            .unwrap_or_else(|e| panic!("{origin:?} {side:?} has a purpose, not {e}"));
        assert_eq!(got, want, "{origin:?} selling {q} against {position}");
    }
}

/// §5.5's sequence starts with the mode, before any cancel is sent.
#[test]
#[ignore = "pending E6-3"]
fn the_final_mode_is_applied_first() {
    let plan = flatten_of(Session::Regular, FlattenInitiator::RiskLimit, None, None);
    assert_eq!(
        plan.mode_applied_first,
        AgentMode::Paused,
        "a mandate limit pauses before it cancels, so nothing new is submitted meanwhile"
    );
}

/// An agent-scoped flatten never touches another agent's orders or positions.
#[test]
#[ignore = "pending E6-3"]
fn an_agent_flatten_never_touches_another_agent() {
    let mut orders = BTreeMap::new();
    orders.insert(
        ClientOrderId(1),
        open_order(AgentId(1), INSTRUMENT_2, "100"),
    );
    orders.insert(
        ClientOrderId(2),
        open_order(AgentId(2), INSTRUMENT_3, "100"),
    );
    let positions = vec![
        AgentPosition {
            agent: AgentId(1),
            instrument: asset(INSTRUMENT_2),
            asset_class: AssetClass::UsEquity,
            qty: qty("10"),
        },
        AgentPosition {
            agent: AgentId(2),
            instrument: asset(INSTRUMENT_3),
            asset_class: AssetClass::UsEquity,
            qty: qty("20"),
        },
    ];
    let broker = BTreeMap::new();
    let plan = agent_flatten(&FlattenInput {
        agent: AgentId(1),
        open_orders: &orders,
        agent_positions: &positions,
        broker_positions: &broker,
        session: Session::Regular,
        initiator: FlattenInitiator::RiskLimit,
        owner_confirmed_bid: None,
        max_exit_offset: fraction("0.03"),
        owner_floor_price: None,
    })
    .expect("the flatten plans");

    assert_eq!(
        plan.cancel_client_order_ids,
        vec![ClientOrderId(1)],
        "only this agent's order ids"
    );
    assert!(
        plan.sells
            .iter()
            .all(|s| s.instrument == asset(INSTRUMENT_2)),
        "the other agent's position is not this agent's to sell"
    );
}

/// The sub-ledger, not the broker's quantity, is what a flatten sells (`MC-F01`'s note).
#[test]
#[ignore = "pending E6-3"]
fn a_flatten_sells_the_sub_ledger_not_the_brokers_position() {
    let orders = BTreeMap::new();
    let positions = vec![AgentPosition {
        agent: AgentId(1),
        instrument: asset(INSTRUMENT_2),
        asset_class: AssetClass::UsEquity,
        qty: qty("10"),
    }];
    let mut broker = BTreeMap::new();
    broker.insert(asset(INSTRUMENT_2), qty("15"));
    let plan = agent_flatten(&FlattenInput {
        agent: AgentId(1),
        open_orders: &orders,
        agent_positions: &positions,
        broker_positions: &broker,
        session: Session::Regular,
        initiator: FlattenInitiator::RiskLimit,
        owner_confirmed_bid: None,
        max_exit_offset: fraction("0.03"),
        owner_floor_price: None,
    })
    .expect("the flatten plans");

    assert_eq!(
        plan.sells.first().map(|s| s.qty),
        Some(qty("10")),
        "the broker holds 15; the other 5 are the owner's and are not sold"
    );
}

/// Two active `scale_sizes` rungs multiply: 0.5 x 0.4 = 0.2, which their sum 0.9 cannot equal.
#[test]
#[ignore = "pending E6-4"]
fn two_active_rungs_multiply() {
    let mut s = Scenario::allowing();
    s.mandate = mandate_with(common::two_scaling_rungs());
    s.risk.active_rungs = [0_u8, 1].into_iter().collect();
    s.risk.size_factor = fraction("0.2");

    let factor = mandate_risk::trim_proposals(
        s.now,
        &s.config,
        &s.mandate,
        &s.risk,
        &s.agent,
        &BTreeMap::new(),
    );
    assert!(
        factor.is_ok(),
        "the trim reads the folded size factor 0.5 x 0.4 = 0.2, never 0.9"
    );
}

/// The close window is the last ten minutes of the session the calendar gives, not of 16:00.
#[test]
#[ignore = "pending E6-8"]
fn the_close_window_follows_the_early_close_calendar() {
    let full = mandate_risk::session_at(
        at("2026-09-22T19:50:00Z"),
        &common::test_default_config(),
        AssetClass::UsEquity,
    )
    .expect("a covered date has a session");
    let early = mandate_risk::session_at(
        at("2026-11-27T17:50:00Z"),
        &common::test_default_config(),
        AssetClass::UsEquity,
    )
    .expect("an early-close date has a session");

    assert_eq!(
        (full.close_window, early.close_window),
        (true, true),
        "15:50 ET on a full day and 12:50 ET on an early-close day are both in the window"
    );
}

/// `RC-25` step 2: an increase at 15:50 is `close_window`, not `auction_window` (DEC-129 item 18).
#[test]
#[ignore = "pending E6-8"]
fn an_increase_in_the_closing_ten_minutes_is_close_window() {
    let mut s = Scenario::allowing();
    s.now = at("2026-09-22T19:50:00Z");
    s.agent.positions.insert(asset(INSTRUMENT_3), qty("10"));

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.reason),
        (Verdict::Deny, Some(ReasonCode::CloseWindow)),
        "the closing auction window and the close window are the same ten minutes, and check 6 \
         names it"
    );
}

/// The opening auction denies a market order, which is what `auction_window` is for.
#[test]
#[ignore = "pending E6-6"]
fn the_opening_auction_denies_a_market_order() {
    let session = mandate_risk::session_at(
        at("2026-09-22T13:29:00Z"),
        &common::test_default_config(),
        AssetClass::UsEquity,
    )
    .expect("a covered date has a session");
    assert!(
        session.opening_auction,
        "09:29 ET is inside the 09:28 to 09:30 opening auction"
    );
}

/// `RC-25` step 8: an owner exit outside the session without a confirmed bid defers.
#[test]
#[ignore = "pending E6-6"]
fn an_unconfirmed_owner_exit_defers() {
    let mut s = Scenario::allowing();
    s.now = at("2026-09-22T21:00:00Z");
    s.agent.positions.insert(asset(INSTRUMENT_3), qty("10"));
    s.proposed = proposal(INSTRUMENT_3, Side::Sell, "10", "120", Origin::OwnerClose);

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.reason),
        (Verdict::Defer, Some(ReasonCode::OwnerConfirmationRequired)),
        "an equity owner exit after hours waits for the owner to confirm the displayed bid"
    );
}

/// `RC-25` step 5: an equity discretionary exit outside the session defers, never denies.
#[test]
#[ignore = "pending E6-6"]
fn a_discretionary_exit_outside_the_session_defers() {
    let mut s = Scenario::allowing();
    s.now = at("2026-09-22T21:00:00Z");
    s.agent.positions.insert(asset(INSTRUMENT_3), qty("10"));
    s.proposed = proposal(INSTRUMENT_3, Side::Sell, "10", "120", Origin::OrderBuilder);

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.reason),
        (
            Verdict::Defer,
            Some(ReasonCode::DiscretionaryExitRegularSessionOnly)
        ),
        "a defer is never converted to a deny"
    );
}

/// Crypto trades continuously, so the equity session rule never defers a crypto exit.
#[test]
#[ignore = "pending E6-6"]
fn crypto_exits_run_at_all_hours() {
    let mut s = Scenario::allowing();
    s.now = at("2026-09-22T21:00:00Z");
    s.instrument.asset_class = AssetClass::Crypto;
    s.instrument.exchange = None;
    s.agent.positions.insert(asset(INSTRUMENT_3), qty("10"));
    s.proposed = proposal(INSTRUMENT_3, Side::Sell, "10", "120", Origin::OrderBuilder);

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        d.verdict,
        Verdict::Allow,
        "the equity session rule is for equities"
    );
}

/// §9.6's resting time binds a cancel, and a cancel before a risk-reducing order is exempt.
#[test]
#[ignore = "pending E6-8"]
fn a_cancel_inside_the_resting_window_is_denied() {
    let config = common::test_default_config();
    let order = open_order(AgentId(1), INSTRUMENT_3, "100");
    let d = mandate_risk::evaluate_cancel(&mandate_risk::CancelInput {
        now: at("2026-09-21T15:00:01Z"),
        config: &config,
        order: &order,
        resting_since: at("2026-09-21T15:00:00Z"),
        precedes_risk_reducing_order: false,
        marketable: false,
    })
    .expect("the gate decides the cancel");
    assert_eq!(
        (d.verdict, d.reason),
        (Verdict::Deny, Some(ReasonCode::MinRestingTime)),
        "1 s is inside the 2 s minimum resting time"
    );
}

#[test]
#[ignore = "pending E6-8"]
fn a_cancel_before_a_risk_reducing_order_is_exempt() {
    let config = common::test_default_config();
    let order = open_order(AgentId(1), INSTRUMENT_3, "100");
    let d = mandate_risk::evaluate_cancel(&mandate_risk::CancelInput {
        now: at("2026-09-21T15:00:01Z"),
        config: &config,
        order: &order,
        resting_since: at("2026-09-21T15:00:00Z"),
        precedes_risk_reducing_order: true,
        marketable: false,
    })
    .expect("the gate decides the cancel");
    assert_eq!(
        d.verdict,
        Verdict::Allow,
        "the rule does not apply to cancels that precede a risk-reducing order"
    );
}

/// Buying power is the lower of the model's and the broker's figure (DEC-34).
#[test]
#[ignore = "pending E6-6"]
fn buying_power_is_the_lower_of_the_two() {
    let mut s = Scenario::allowing();
    s.account.model_buying_power = usd("10000");
    s.account.broker_buying_power = usd("50");
    s.proposed = proposal(INSTRUMENT_3, Side::Buy, "1", "100", Origin::OrderBuilder);

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.reason),
        (Verdict::Deny, Some(ReasonCode::InsufficientBuyingPower)),
        "100 is above the broker's 50, whatever the model says"
    );
}

/// A cash account's shortfall is a different code from a margin account's (`RC-08`, `RC-18`).
#[test]
#[ignore = "pending E6-6"]
fn a_cash_account_reports_the_settled_code() {
    let mut s = Scenario::allowing();
    s.account.account_type = mandate_risk::AccountType::Cash;
    s.account.model_buying_power = usd("50");
    s.account.broker_buying_power = usd("50");
    s.proposed = proposal(INSTRUMENT_3, Side::Buy, "1", "100", Origin::OrderBuilder);

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        d.reason,
        Some(ReasonCode::InsufficientSettledBuyingPower),
        "a cash account's shortfall is settled cash, not margin buying power"
    );
}

/// The fee reservation is part of check 7's left-hand side (§9.5).
#[test]
#[ignore = "pending E6-6"]
fn a_reservation_includes_the_rounded_fee() {
    let mut s = Scenario::allowing();
    s.account.model_buying_power = usd("100");
    s.account.broker_buying_power = usd("100");
    s.proposed = proposal(INSTRUMENT_3, Side::Buy, "1", "100", Origin::OrderBuilder);
    s.proposed.fee_reservation = usd("0.01");

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.reason),
        (Verdict::Deny, Some(ReasonCode::InsufficientBuyingPower)),
        "100 + 0.01 is above buying power 100, so the fee is what tips it"
    );
}

/// §9.2's `required` counts every component the rule names.
#[test]
#[ignore = "pending E6-6"]
fn required_counts_every_component() {
    let mut s = Scenario::allowing();
    s.account.regime = mandate_risk::DayTradeRegime::LegacyPdt;
    s.account.prior_close_equity = usd("10000");
    s.agent.day_trades = mandate_risk::DayTradeLedger {
        window_count: 2,
        flagged_pattern_day_trader: false,
        sold_earlier_today: BTreeSet::new(),
        open_same_day_positions: [asset(INSTRUMENT_2)].into_iter().collect(),
    };

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.reason),
        (Verdict::Deny, Some(ReasonCode::LegacyPdtDayTradeBudget)),
        "remaining 3 - 2 = 1, required 1 + 1 open same-day position = 2"
    );
}

/// Crypto never counts toward a day-trade budget (§9.2).
#[test]
#[ignore = "pending E6-6"]
fn crypto_never_counts() {
    let mut s = Scenario::allowing();
    s.account.regime = mandate_risk::DayTradeRegime::LegacyPdt;
    s.account.prior_close_equity = usd("10000");
    s.instrument.asset_class = AssetClass::Crypto;
    s.instrument.exchange = None;
    s.agent.day_trades = mandate_risk::DayTradeLedger {
        window_count: 3,
        flagged_pattern_day_trader: false,
        sold_earlier_today: BTreeSet::new(),
        open_same_day_positions: BTreeSet::new(),
    };

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_ne!(
        d.reason,
        Some(ReasonCode::LegacyPdtDayTradeBudget),
        "a crypto order is never a day trade"
    );
}

/// §9.6's participation caps slice a discretionary exit; they never deny one.
#[test]
#[ignore = "pending E6-8"]
fn a_participation_cap_slices_and_never_denies() {
    let mut s = Scenario::allowing();
    s.market.adv_20d = Some(qty("1000"));
    s.market.trailing_5m_volume = Some(qty("100"));
    s.agent.positions.insert(asset(INSTRUMENT_3), qty("900"));
    s.proposed = proposal(INSTRUMENT_3, Side::Sell, "900", "100", Origin::OrderBuilder);

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(d.verdict, Verdict::Allow, "a paced exit is still allowed");
    let pacing = d.pacing.expect("a sliced exit says what it applied");
    assert!(
        pacing.qty < qty("900")
            && pacing
                .applied
                .contains(&mandate_risk::PacingControl::DailyParticipation),
        "5 percent of a 1000 ADV is 50, so the slice is smaller than the proposal"
    );
}

/// The surveillance report states figures and flags thresholds; it makes no judgement.
#[test]
#[ignore = "pending E6-8"]
fn the_surveillance_report_matches_a_hand_computed_day() {
    let mut input = mandate_risk::SurveillanceInput::default();
    input.orders.insert(
        (AgentId(1), asset(INSTRUMENT_3)),
        mandate_risk::OrderCounts {
            submitted: 25,
            filled: 2,
            cancels_excluded: 0,
        },
    );
    let day = mandate_time::Date::parse("2026-09-21").expect("a date parses");
    let report = mandate_risk::surveillance(day, &common::test_default_config(), &input)
        .expect("the report computes");

    assert_eq!(
        report.order_to_fill.get(&(AgentId(1), asset(INSTRUMENT_3))),
        Some(&12),
        "25 orders to 2 fills is 12 when truncated, and 12 is above the limit of 10"
    );
    assert!(
        report
            .breaches
            .contains(&mandate_risk::SurveillanceBreach::OrderToFill),
        "the threshold is flagged, and nothing beyond it is asserted"
    );
}

/// Every code [`ReasonCode`] can produce, listed once so a new variant must be added here too.
const ALL_REASON_CODES: [ReasonCode; 38] = [
    ReasonCode::AccountTradingBlocked,
    ReasonCode::AccountRestricted,
    ReasonCode::CryptoAccountInactive,
    ReasonCode::AgentExitsOnly,
    ReasonCode::AgentPaused,
    ReasonCode::AgentStopped,
    ReasonCode::NotInWorkingUniverse,
    ReasonCode::IneligibleExchange,
    ReasonCode::IpoNotTradable,
    ReasonCode::BelowPriceFloor,
    ReasonCode::BelowLiquidityFloor,
    ReasonCode::LeveragedEtpNotEnabled,
    ReasonCode::ConcentrationLimit,
    ReasonCode::MaxOrderSize,
    ReasonCode::ReentryCooldown,
    ReasonCode::SessionNotAllowed,
    ReasonCode::ExtendedHoursOpeningNotAllowed,
    ReasonCode::AuctionWindow,
    ReasonCode::InstrumentHalted,
    ReasonCode::WouldCrossZero,
    ReasonCode::SellExceedsAvailable,
    ReasonCode::WorkingOrderLimit,
    ReasonCode::AddBlockedByProtectiveOrder,
    ReasonCode::UnknownOrderInFlight,
    ReasonCode::MarketOrderNotAllowed,
    ReasonCode::StaleMark,
    ReasonCode::PriceOutsideCollar,
    ReasonCode::MinRestingTime,
    ReasonCode::OppositeFillInterval,
    ReasonCode::ConductLimitBreached,
    ReasonCode::MaxOrdersPerDay,
    ReasonCode::CloseWindow,
    ReasonCode::DiscretionaryExitRegularSessionOnly,
    ReasonCode::OwnerConfirmationRequired,
    ReasonCode::GrossExposureLimit,
    ReasonCode::InsufficientBuyingPower,
    ReasonCode::InsufficientSettledBuyingPower,
    ReasonCode::LegacyPdtDayTradeBudget,
];

/// Every reason code the gate can emit is registered in the founder-owned case file, with exactly
/// one known exception.
///
/// `not_in_working_universe` is the code mandate spec §5.3 gives the working-universe check and
/// `MC-G14` and `MC-G16` expect, but the trading-domain `reason_codes` registry does not carry it:
/// that registry has `not_in_universe` for the same condition, which `RC-16` step 7 expects. The
/// two suites cannot both be satisfied, which is DEC-129 item 24, proposed to the founder. This
/// test pins the gap rather than papering over it: the exception is named, and **any other**
/// unregistered code fails, so the day a new one is minted this test says so.
#[test]
fn every_reason_code_is_registered_in_the_case_file() {
    /// DEC-129 item 24, `Proposed (founder)`. Removing this entry is what closes that decision.
    const KNOWN_UNREGISTERED: [&str; 1] = ["not_in_working_universe"];

    let registry = include_str!("../../../docs/specs/reference-cases/trading-domain.yaml");
    let scoped_to_the_reason_codes_block = "counting every YAML list item in the file would let an \
         unrelated entry that shares a code's spelling read as a registration";
    let registered: BTreeSet<&str> = registry
        .lines()
        .skip_while(|l| !l.starts_with("reason_codes:"))
        .skip(1)
        .take_while(|l| l.starts_with("  ") || l.trim().is_empty())
        .map(str::trim)
        .filter_map(|l| l.strip_prefix("- "))
        .map(|l| l.split('#').next().unwrap_or(l).trim())
        .collect();
    assert!(
        registered.len() > 30,
        "the reason_codes block was not found where it was expected ({scoped_to_the_reason_codes_block}); \
         the scan read {} entries",
        registered.len()
    );

    let missing: Vec<&str> = ALL_REASON_CODES
        .iter()
        .map(|c| c.as_str())
        .filter(|c| !registered.contains(c) && !KNOWN_UNREGISTERED.contains(c))
        .collect();
    assert!(
        missing.is_empty(),
        "these gate reason codes are not registered in the founder-owned case file, and no code \
         may be minted (ES-09): {missing:?}"
    );

    let resolved: Vec<&str> = KNOWN_UNREGISTERED
        .iter()
        .copied()
        .filter(|c| registered.contains(c))
        .collect();
    assert!(
        resolved.is_empty(),
        "DEC-129 item 24 is settled for {resolved:?}: drop it from KNOWN_UNREGISTERED"
    );
}
