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
    INSTRUMENT_1, INSTRUMENT_2, INSTRUMENT_3, Scenario, asset, at, fraction, mandate_with,
    open_order, price, proposal, qty, usd, working_universe,
};
use mandate_risk::{
    AgentId, AgentMode, AgentPosition, AssetClass, ClientOrderId, FlattenInitiator, FlattenInput,
    FlattenPricing, Origin, Purpose, ReasonCode, Session, Side, Verdict, agent_flatten, evaluate,
};

/// An unread working universe is an error, never an allow and never a deny (DEC-129 item 3).
#[test]
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
///
/// The position is 1000 against a 1500 cap, so the target is 0.2 x 1500 = 300 and the trim sells
/// 700 worth. Under a summed factor of 0.9 the target would be 1350, above the position, and no
/// trim would be proposed at all — so the two readings differ in whether a sell exists, not only in
/// its size.
#[test]
fn two_active_rungs_multiply() {
    let mut s = Scenario::allowing();
    s.mandate = mandate_with(common::two_trimming_rungs());
    s.risk.active_rungs = [(0_u8, 120_u64), (1, 120)].into_iter().collect();
    s.risk.size_factor = common::ratio("0.2");
    s.agent.positions.insert(asset(INSTRUMENT_2), qty("10"));
    s.agent
        .market_values
        .insert(asset(INSTRUMENT_2), usd("1000"));
    let mut instruments = BTreeMap::new();
    instruments.insert(asset(INSTRUMENT_2), common::equity_instrument(INSTRUMENT_2));

    let trims = mandate_risk::trim_proposals(
        s.now,
        &s.config,
        &s.mandate,
        &s.risk,
        &s.agent,
        &s.account,
        &instruments,
    )
    .expect("the trims compute");

    assert_eq!(
        trims.len(),
        1,
        "a position of 1000 is above the target 0.2 x 1500 = 300, so one trim is proposed"
    );
    let trim = trims.first().expect("the trim exists");
    assert_eq!(
        (trim.instrument.clone(), trim.qty, trim.purpose),
        (asset(INSTRUMENT_2), qty("7"), Purpose::RiskExit),
        "selling down to 300 of a 1000 position at 100 a share is 7 shares, rounded up"
    );
}

/// A trim rounds the quantity **up** to the increment, so it never leaves the position above its
/// target (§5.5).
#[test]
fn a_trim_rounds_up_to_the_increment() {
    let mut s = Scenario::allowing();
    s.mandate = mandate_with(common::two_trimming_rungs());
    s.risk.active_rungs = [(0_u8, 120_u64)].into_iter().collect();
    s.risk.size_factor = common::ratio("0.5");
    s.agent.positions.insert(asset(INSTRUMENT_2), qty("10"));
    s.agent
        .market_values
        .insert(asset(INSTRUMENT_2), usd("1000"));
    let mut instruments = BTreeMap::new();
    instruments.insert(asset(INSTRUMENT_2), common::equity_instrument(INSTRUMENT_2));

    let trims = mandate_risk::trim_proposals(
        s.now,
        &s.config,
        &s.mandate,
        &s.risk,
        &s.agent,
        &s.account,
        &instruments,
    )
    .expect("the trims compute");
    assert_eq!(
        trims.first().map(|t| t.qty),
        Some(qty("3")),
        "the target is 0.5 x 1500 = 750, so 250 of a 1000 position must go: 2.5 shares rounded up"
    );
}

/// A trim waits for the regular session for an equity (§5.5).
///
/// Both arms, because an empty result on its own is what a stub returns: the same position at
/// 21:00 UTC yields nothing and in the regular session yields the trim, so the session is the only
/// thing that differs and the emptiness is the session's doing.
#[test]
fn a_trim_waits_for_the_regular_session() {
    let scenario_at = |when: &str| {
        let mut s = Scenario::allowing();
        s.now = at(when);
        s.mandate = mandate_with(common::two_trimming_rungs());
        s.risk.active_rungs = [(0_u8, 120_u64)].into_iter().collect();
        s.risk.size_factor = common::ratio("0.5");
        s.agent.positions.insert(asset(INSTRUMENT_2), qty("10"));
        s.agent
            .market_values
            .insert(asset(INSTRUMENT_2), usd("1000"));
        s
    };
    let mut instruments = BTreeMap::new();
    instruments.insert(asset(INSTRUMENT_2), common::equity_instrument(INSTRUMENT_2));
    let trims_at = |when: &str| {
        let s = scenario_at(when);
        mandate_risk::trim_proposals(
            s.now,
            &s.config,
            &s.mandate,
            &s.risk,
            &s.agent,
            &s.account,
            &instruments,
        )
        .expect("the trims compute")
    };

    assert!(
        trims_at("2026-09-22T21:00:00Z").is_empty(),
        "an equity trim runs in the regular session only; after hours there is none to propose"
    );
    assert_eq!(
        trims_at("2026-09-21T15:00:00Z").first().map(|t| t.qty),
        Some(qty("3")),
        "the same position in the regular session is trimmed, so the emptiness above is the \
         session rule and not an unimplemented trim"
    );
}

/// No trim while the goal is Holding (DEC-65).
///
/// Both arms, for the reason [`a_trim_waits_for_the_regular_session`] gives: the goal state is the
/// only difference between the empty result and the trim.
#[test]
fn no_trim_while_holding() {
    let mut instruments = BTreeMap::new();
    instruments.insert(asset(INSTRUMENT_2), common::equity_instrument(INSTRUMENT_2));
    let trims_for = |goal: mandate_risk::spec_types::GoalState| {
        let mut s = Scenario::allowing();
        s.mandate = mandate_risk::ValidatedMandate::from_validated_parts(
            common::two_trimming_rungs(),
            goal,
            false,
            false,
        );
        s.risk.active_rungs = [(0_u8, 120_u64)].into_iter().collect();
        s.risk.size_factor = common::ratio("0.5");
        s.agent.positions.insert(asset(INSTRUMENT_2), qty("10"));
        s.agent
            .market_values
            .insert(asset(INSTRUMENT_2), usd("1000"));
        mandate_risk::trim_proposals(
            s.now,
            &s.config,
            &s.mandate,
            &s.risk,
            &s.agent,
            &s.account,
            &instruments,
        )
        .expect("the trims compute")
    };

    assert!(
        trims_for(mandate_risk::spec_types::GoalState::Holding).is_empty(),
        "a Holding goal is never trimmed"
    );
    assert_eq!(
        trims_for(mandate_risk::spec_types::GoalState::Running)
            .first()
            .map(|t| t.qty),
        Some(qty("3")),
        "the same position under a Running goal is trimmed, so the emptiness above is DEC-65 and \
         not an unimplemented trim"
    );
}

/// The trims of one position held at `market_value` under the confirmed rung at `factor`, with
/// `resting` of the agent's own non-protective sells working and the instrument's minimum order
/// size `minimum`.
fn trims_of(
    held: &str,
    market_value: &str,
    factor: &str,
    resting: &str,
    minimum: &str,
    crypto: bool,
) -> Result<Vec<mandate_risk::TrimProposal>, mandate_risk::GateError> {
    let mut s = Scenario::allowing();
    s.mandate = mandate_with(common::two_trimming_rungs());
    s.risk.active_rungs = [(0_u8, 120_u64)].into_iter().collect();
    s.risk.size_factor = common::ratio(factor);
    s.agent.positions.insert(asset(INSTRUMENT_2), qty(held));
    s.agent
        .market_values
        .insert(asset(INSTRUMENT_2), usd(market_value));
    if resting != "0" {
        let mut sell = open_order(s.agent.agent, INSTRUMENT_2, "0");
        sell.side = Side::Sell;
        sell.opening = false;
        sell.open_qty = qty(resting);
        s.account.working_orders.insert(ClientOrderId(77), sell);
        s.agent.working_orders.insert(ClientOrderId(77));
    }
    let mut instrument = common::equity_instrument(INSTRUMENT_2);
    if crypto {
        instrument.asset_class = AssetClass::Crypto;
        instrument.exchange = None;
        instrument.fractionable = true;
    }
    instrument.min_order_size = qty(minimum);
    let instruments = BTreeMap::from([(asset(INSTRUMENT_2), instrument)]);
    mandate_risk::trim_proposals(
        s.now,
        &s.config,
        &s.mandate,
        &s.risk,
        &s.agent,
        &s.account,
        &instruments,
    )
}

/// A trim that sells everything left to sell is proposed below the instrument's minimum order
/// size, and one that leaves some unsold is still withheld (DEC-423; trading spec §5.3 rule 2's
/// full-close exception; #504 review, M1).
///
/// Each row's trim is worked out here from §5.5 on the 1500 cap: the target is `factor × 1500`,
/// the band 75, and the sell is the excess over the target at the position's own price, rounded
/// up to the grid and capped at what is not already on sale.
/// - 1 share at 1000, factor 0.5: the excess 250 is a quarter share, rounded up to the 1 held.
///   At a 2-share minimum it is proposed, and the position closes.
/// - 2 shares at 1000 each with 1 resting, factor 0.5: the excess 1250 less the 1000 on sale is a
///   quarter share, rounded up to the 1 not on sale. It is proposed at a 2-share minimum.
/// - 0.0002 BTC worth 120, factor 0: the whole 120 is over the zero target, so the sell is the
///   0.0002 held, below a 0.001 minimum, and it is proposed. A 0.0001 grid, as in the review's
///   case, is not an input the gate has yet (the backlog's "real quantity grid" row), so the full
///   close is reached through the target instead.
/// - 10 shares at 100, factor 0.5: the excess 250 is 3 shares of 10, not a full close, so at a
///   4-share minimum it is still withheld.
#[test]
#[ignore = "pending E6-4"]
fn a_trim_that_sells_everything_left_is_never_withheld_for_the_minimum() {
    let full_closes = [
        ("1", "1000", "0.5", "0", "2", false, "1"),
        ("2", "2000", "0.5", "1", "2", false, "1"),
        ("0.0002", "120", "0", "0", "0.001", true, "0.0002"),
    ];
    for (held, value, factor, resting, minimum, crypto, sold) in full_closes {
        let trims = trims_of(held, value, factor, resting, minimum, crypto)
            .unwrap_or_else(|e| panic!("{held} held, {resting} resting: the trims compute: {e}"));
        assert_eq!(
            trims
                .iter()
                .map(|t| (t.instrument.clone(), t.qty, t.purpose))
                .collect::<Vec<_>>(),
            vec![(asset(INSTRUMENT_2), qty(sold), Purpose::RiskExit)],
            "{held} held with {resting} resting sells the {sold} left, below the {minimum} \
             minimum, as a full close"
        );
    }
    let partial = trims_of("10", "1000", "0.5", "0", "4", false)
        .unwrap_or_else(|e| panic!("10 held: the trims compute: {e}"));
    assert!(
        partial.is_empty(),
        "a 3-share trim of 10 is not a full close, so a 4-share minimum still withholds it"
    );
}

/// Only `scale_action: trim_to_target` trims; `limit_buys` never proposes a sell.
///
/// Mandate §5.5 gives the trim to `trim_to_target` alone — under `limit_buys` the size factor only
/// multiplies the order builder's targets, which is stream H's job, not the gate's. Both arms,
/// because an empty result is also what an unimplemented `trim_proposals` returns: the identical
/// position under `trim_to_target` is trimmed, so the action is the only thing that differs.
#[test]
fn a_limit_buys_rung_never_trims() {
    let mut instruments = BTreeMap::new();
    instruments.insert(asset(INSTRUMENT_2), common::equity_instrument(INSTRUMENT_2));
    let trims_under = |limits: mandate_risk::spec_types::RiskLimits| {
        let mut s = Scenario::allowing();
        s.mandate = mandate_with(limits);
        s.risk.active_rungs = [(0_u8, 120_u64)].into_iter().collect();
        s.risk.size_factor = common::ratio("0.5");
        s.agent.positions.insert(asset(INSTRUMENT_2), qty("10"));
        s.agent
            .market_values
            .insert(asset(INSTRUMENT_2), usd("1000"));
        mandate_risk::trim_proposals(
            s.now,
            &s.config,
            &s.mandate,
            &s.risk,
            &s.agent,
            &s.account,
            &instruments,
        )
        .expect("the trims compute")
    };

    assert!(
        trims_under(common::two_scaling_rungs()).is_empty(),
        "a limit_buys rung scales the order builder's targets; it never sells a position down"
    );
    assert_eq!(
        trims_under(common::two_trimming_rungs())
            .first()
            .map(|t| t.qty),
        Some(qty("3")),
        "the same position under trim_to_target is trimmed, so the emptiness above is the scale \
         action and not an unimplemented trim"
    );
}

/// A rung trims only once it has been active for `breach_confirm_s` (mandate §5.5).
///
/// The fixture's `breach_confirm_s` is 60 s, so a rung active for 59 s proposes nothing and the
/// same rung at 60 s proposes the trim. The boundary is on the allowing side, like every other
/// limit comparison in this crate (DEC-129 item 15): at exactly `breach_confirm_s` the trim runs.
#[test]
fn a_rung_trims_only_after_breach_confirm_s() {
    let mut instruments = BTreeMap::new();
    instruments.insert(asset(INSTRUMENT_2), common::equity_instrument(INSTRUMENT_2));
    let trims_after = |active_s: u64| {
        let mut s = Scenario::allowing();
        s.mandate = mandate_with(common::two_trimming_rungs());
        assert_eq!(
            s.mandate.risk().breach_confirm_s,
            60,
            "this test's two arms straddle the fixture's breach_confirm_s"
        );
        s.risk.active_rungs = [(0_u8, active_s)].into_iter().collect();
        s.risk.size_factor = common::ratio("0.5");
        s.agent.positions.insert(asset(INSTRUMENT_2), qty("10"));
        s.agent
            .market_values
            .insert(asset(INSTRUMENT_2), usd("1000"));
        mandate_risk::trim_proposals(
            s.now,
            &s.config,
            &s.mandate,
            &s.risk,
            &s.agent,
            &s.account,
            &instruments,
        )
        .expect("the trims compute")
    };

    assert!(
        trims_after(59).is_empty(),
        "a rung that has not held for breach_confirm_s does not trim yet"
    );
    assert_eq!(
        trims_after(60).first().map(|t| t.qty),
        Some(qty("3")),
        "at exactly breach_confirm_s the trim runs, so the emptiness above is the confirm window \
         and not an unimplemented trim"
    );
}

/// The close window is the last ten minutes of the session the calendar gives, not of 16:00.
#[test]
fn the_close_window_follows_the_early_close_calendar() {
    let at_instant = |instant: &str| {
        let session = mandate_risk::session_at(
            at(instant),
            &common::test_default_config(),
            AssetClass::UsEquity,
        )
        .expect("a covered date has a session");
        (session.session, session.close_window)
    };
    let (regular, after) = (Session::Regular, Session::AfterHours);

    for (day, before, first, last, close) in [
        (
            "a full day",
            "2026-09-22T19:49:59.999999999Z",
            "2026-09-22T19:50:00Z",
            "2026-09-22T19:59:59.999999999Z",
            "2026-09-22T20:00:00Z",
        ),
        (
            "an early-close day",
            "2026-11-27T17:49:59.999999999Z",
            "2026-11-27T17:50:00Z",
            "2026-11-27T17:59:59.999999999Z",
            "2026-11-27T18:00:00Z",
        ),
    ] {
        assert_eq!(
            [
                at_instant(before),
                at_instant(first),
                at_instant(last),
                at_instant(close)
            ],
            [
                (regular, false),
                (regular, true),
                (regular, true),
                (after, false)
            ],
            "on {day} the window is the last ten minutes before the calendar's close: a \
             nanosecond before them is outside it, their first and last instants inside it, and \
             the close itself ends the session and the window"
        );
    }
}

/// `RC-25` step 2: an increase at 15:50 is `close_window`, not `auction_window` (DEC-129 item 18).
#[test]
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

/// An auction window denies a market *opening* and re-prices a market *exit* (DEC-159, amending
/// DEC-129 item 18).
///
/// §4.3 says "no opening orders in either [window]; exits in them use limit orders, never market
/// orders". A market-order exit is therefore sent as a marketable limit, never denied, which is
/// MI-1 and `AGENTS.md` rule 13 and how DEC-129 items 28 and 31 treat a halt. `auction_window`
/// stays a check-3 denial for a market opening. Check 3 reads the session before the auction
/// window (§9.1), so a market opening at 09:29 ET is `session_not_allowed` (pre-market takes no
/// opening); the denial `auction_window` names is reachable in the closing window, at 15:55 ET.
#[test]
fn an_auction_window_denies_a_market_opening_and_reprices_a_market_exit() {
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

    let mut exit = Scenario::allowing();
    exit.now = at("2026-09-22T13:29:00Z");
    exit.agent.positions.insert(asset(INSTRUMENT_3), qty("10"));
    exit.proposed = proposal(INSTRUMENT_3, Side::Sell, "10", "120", Origin::RiskEngine);
    exit.proposed.kind = mandate_risk::ProposedKind::Market;
    let d = evaluate(&exit.input()).expect("the gate decides");
    assert_eq!(
        (
            d.verdict,
            d.reason,
            d.pacing
                .as_ref()
                .map(|p| (p.qty, p.marketable_limit_required))
        ),
        (Verdict::Allow, None, Some((qty("10"), true))),
        "a market-order risk exit in the opening auction is sent as a marketable limit, never \
         denied (spec 4.3, MI-1)"
    );
    assert_eq!(
        d.pacing.map(|p| (p.limit_price, p.applied)),
        Some((price("120"), BTreeSet::new())),
        "the re-priced exit keeps the proposal's limit, and no §9.6 control is applied to it: a \
         risk exit is exempt from §9.6 entirely, so the marketable price is the exit ladder's to \
         set, not a collar's"
    );

    let mut pre_market = Scenario::allowing();
    pre_market.now = at("2026-09-22T13:29:00Z");
    pre_market.proposed.kind = mandate_risk::ProposedKind::Market;
    let mut closing = Scenario::allowing();
    closing.now = at("2026-09-22T19:55:00Z");
    closing.proposed.kind = mandate_risk::ProposedKind::Market;
    let decide = |s: &Scenario| {
        let d = evaluate(&s.input()).expect("the gate decides");
        (d.verdict, d.reason)
    };
    assert_eq!(
        (decide(&pre_market), decide(&closing)),
        (
            (Verdict::Deny, Some(ReasonCode::SessionNotAllowed)),
            (Verdict::Deny, Some(ReasonCode::AuctionWindow)),
        ),
        "a market opening at 09:29 ET meets the session rule first; at 15:55 ET it is \
         auction_window, at check 3, before check 6's close_window"
    );
}

/// `RC-25` step 8: an owner exit outside the session without a confirmed bid defers.
#[test]
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

/// §9.2's `required` counts every component the rule names: the order itself, a sale of the same
/// security earlier today, open same-day positions, and working same-day opening orders elsewhere.
///
/// Each row moves exactly one component, so a `required` that omits any one of them allows a case
/// this denies.
#[test]
fn required_counts_every_component() {
    let base_ledger = mandate_risk::DayTradeLedger {
        window_count: 2,
        flagged_pattern_day_trader: false,
        sold_earlier_today: BTreeSet::new(),
        open_same_day_positions: BTreeSet::new(),
    };
    let rows: [(&str, mandate_risk::DayTradeLedger, bool); 4] = [
        ("required 1, remaining 1", base_ledger.clone(), true),
        (
            "an open same-day position makes required 2",
            mandate_risk::DayTradeLedger {
                open_same_day_positions: [asset(INSTRUMENT_2)].into_iter().collect(),
                ..base_ledger.clone()
            },
            false,
        ),
        (
            "the same security sold earlier today makes required 2",
            mandate_risk::DayTradeLedger {
                sold_earlier_today: [asset(INSTRUMENT_3)].into_iter().collect(),
                ..base_ledger.clone()
            },
            false,
        ),
        (
            "a flagged account below the threshold has remaining 0",
            mandate_risk::DayTradeLedger {
                flagged_pattern_day_trader: true,
                ..base_ledger.clone()
            },
            false,
        ),
    ];
    for (why, ledger, allowed) in rows {
        let mut s = Scenario::allowing();
        s.account.regime = mandate_risk::DayTradeRegime::LegacyPdt;
        s.account.prior_close_equity = usd("10000");
        s.agent.day_trades = ledger;
        let d = evaluate(&s.input()).expect("the gate decides");
        assert_eq!(
            d.reason != Some(ReasonCode::LegacyPdtDayTradeBudget),
            allowed,
            "{why}"
        );
    }
}

/// The window is today plus the four prior trading days (§9.2), not the four prior alone.
#[test]
fn the_window_is_today_plus_four() {
    let mut s = Scenario::allowing();
    s.account.regime = mandate_risk::DayTradeRegime::LegacyPdt;
    s.account.prior_close_equity = usd("10000");
    s.agent.day_trades = mandate_risk::DayTradeLedger {
        window_count: 3,
        flagged_pattern_day_trader: false,
        sold_earlier_today: BTreeSet::new(),
        open_same_day_positions: BTreeSet::new(),
    };

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.reason),
        (Verdict::Deny, Some(ReasonCode::LegacyPdtDayTradeBudget)),
        "three day trades in the window leaves remaining 0, and a window one day short would \
         count two and allow this"
    );
}

/// `intraday_margin` denies nothing per order: a reported deficit is an account state (§9.2).
#[test]
fn a_reported_deficit_is_an_account_state_not_a_denial() {
    let mut s = Scenario::allowing();
    s.account.regime = mandate_risk::DayTradeRegime::IntradayMargin {
        maintenance_excess: usd("0"),
    };

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_ne!(
        d.reason,
        Some(ReasonCode::LegacyPdtDayTradeBudget),
        "check 8 denies nothing under intraday_margin; a deficit reaches check 1 as a mode"
    );
}

/// Crypto never counts toward a day-trade budget (§9.2). E6-6 made check 8 whole; the crypto
/// opening this needs waits for check 2's USD pairs (E6-10, DEC-254 item 7).
#[test]
fn crypto_never_counts() {
    let mut s = Scenario::allowing();
    s.account.regime = mandate_risk::DayTradeRegime::LegacyPdt;
    s.account.prior_close_equity = usd("10000");
    s.instrument.asset_class = AssetClass::Crypto;
    s.instrument.exchange = None;
    s.instrument.quote_currency = Some(mandate_risk::QuoteCurrency::Usd);
    s.instrument.median_dollar_volume_30d = Some(usd("90000000"));
    s.agent.day_trades = mandate_risk::DayTradeLedger {
        window_count: 3,
        flagged_pattern_day_trader: false,
        sold_earlier_today: BTreeSet::new(),
        open_same_day_positions: BTreeSet::new(),
    };

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        d.checks.get(1),
        Some(&mandate_risk::CheckOutcome::Passed(
            mandate_risk::Check::UniverseAndLimits
        )),
        "the crypto opening must pass check 2's floor, or the assertion below is vacuous"
    );
    assert_ne!(
        d.reason,
        Some(ReasonCode::LegacyPdtDayTradeBudget),
        "a crypto order is never a day trade"
    );
}

/// §9.6's participation caps slice a discretionary exit; they never deny one.
#[test]
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

/// Crypto trades continuously, so a crypto sell in a flatten never waits for a session.
#[test]
fn a_crypto_sell_never_waits_for_a_session() {
    let orders = BTreeMap::new();
    let positions = vec![AgentPosition {
        agent: AgentId(1),
        instrument: asset(INSTRUMENT_1),
        asset_class: AssetClass::Crypto,
        qty: qty("0.05"),
    }];
    let broker = BTreeMap::new();
    let plan = agent_flatten(&FlattenInput {
        agent: AgentId(1),
        open_orders: &orders,
        agent_positions: &positions,
        broker_positions: &broker,
        session: Session::AfterHours,
        initiator: FlattenInitiator::RiskLimit,
        owner_confirmed_bid: None,
        max_exit_offset: fraction("0.03"),
        owner_floor_price: None,
    })
    .expect("the flatten plans");
    assert_eq!(
        (plan.sells.len(), plan.deferred_sells.len()),
        (1, 0),
        "crypto has no session to wait for"
    );
}

/// A flatten prices by the session alone: the same position, two sessions, two pricings.
#[test]
fn a_flatten_prices_by_session_alone() {
    let regular = flatten_of(Session::Regular, FlattenInitiator::RiskLimit, None, None);
    let after = flatten_of(
        Session::AfterHours,
        FlattenInitiator::Owner,
        Some("100"),
        None,
    );
    assert_eq!(
        (
            regular.sells.first().map(|s| s.pricing),
            after.sells.first().map(|s| s.pricing)
        ),
        (
            Some(FlattenPricing::MarketOrLadder),
            Some(FlattenPricing::ExitPriceLadder)
        ),
        "the asset class decides whether a sell is deferred, never how it is priced"
    );
}

/// §4.4: a halted instrument takes no opening order.
#[test]
fn a_halted_instrument_denies_an_opening() {
    let mut s = Scenario::allowing();
    s.instrument.halted = true;
    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.reason),
        (Verdict::Deny, Some(ReasonCode::InstrumentHalted)),
        "no new opening orders in a halted instrument"
    );
}

/// §4.4: a dropped status feed is a **presumed** halt, which bars market orders but not openings.
///
/// A real halt and a presumed one are different denials, so they carry different codes (DEC-129
/// item 24): a halted instrument takes no new opening order at all and reports `instrument_halted`
/// at check 3, while a presumed halt leaves openings alone and refuses only market orders, which is
/// check 4's `market_order_not_allowed`. This test pins all three halves.
///
/// The exit arm is the one that constrains the design, and it is a denial the gate must **not**
/// make: §4.4 ends "exits use marketable limit orders" and §5.6 lists presumed halts among the
/// conditions where an exit that must be marketable takes the exit price ladder, so the exit is
/// re-priced rather than refused. Denying it would also put an instrument restriction in front of
/// a risk exit, which is exactly what MI-1 forbids (DEC-129 item 28).
#[test]
fn a_dropped_status_feed_is_a_presumed_halt() {
    let mut s = Scenario::allowing();
    s.instrument.status_feed_current = false;
    let opening = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (opening.verdict, opening.reason),
        (Verdict::Allow, None),
        "a presumed halt is not a halt: a limit opening passes it untouched"
    );

    let mut market_opening = s.clone();
    market_opening.proposed.kind = mandate_risk::ProposedKind::Market;
    let denied = evaluate(&market_opening.input()).expect("the gate decides");
    assert_eq!(
        (denied.verdict, denied.reason),
        (Verdict::Deny, Some(ReasonCode::MarketOrderNotAllowed)),
        "a presumed halt takes marketable limits, never a market order to open"
    );

    s.agent.positions.insert(asset(INSTRUMENT_3), qty("10"));
    s.proposed = proposal(INSTRUMENT_3, Side::Sell, "10", "100", Origin::RiskEngine);
    s.proposed.kind = mandate_risk::ProposedKind::Market;
    let exit = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        exit.verdict,
        Verdict::Allow,
        "§4.4 and §5.6 re-price an exit under a presumed halt; MI-1 forbids denying it"
    );
    assert_eq!(
        exit.pacing.as_ref().map(|p| p.marketable_limit_required),
        Some(true),
        "the allow carries the marketable-limit requirement the exit price ladder needs"
    );
    assert_eq!(
        exit.pacing.map(|p| (p.qty, p.limit_price, p.applied)),
        Some((qty("10"), price("100"), BTreeSet::new())),
        "the re-priced exit keeps the proposal's quantity and limit, and no §9.6 control is \
         applied to it: a risk exit is exempt from §9.6 entirely, so the marketable price is the \
         exit ladder's to set (§4.4, §5.6)"
    );
}

/// §9.6's tier threshold is `≥ 50 M`, so a volume exactly at it is the **liquid** tier.
///
/// The two tiers only differ between their bounds. Against the fixture's ask of 100.05 the liquid
/// bound is 100.05 × 1.01 = 101.0505 and the illiquid bound is 100.05 × 1.02 = 102.051, so a limit
/// of 101.5 is aggressive for a liquid instrument and passive enough for an illiquid one. A limit
/// below both — 101, say — is allowed either way and would tell the tiers apart not at all.
#[test]
fn a_median_dollar_volume_exactly_at_the_threshold_is_liquid() {
    let mut liquid = Scenario::allowing();
    liquid.instrument.median_dollar_volume_20d = Some(usd("50000000"));
    liquid.proposed = proposal(INSTRUMENT_3, Side::Buy, "1", "101.5", Origin::OrderBuilder);
    let at_threshold = evaluate(&liquid.input()).expect("the gate decides");

    let mut illiquid = Scenario::allowing();
    illiquid.instrument.median_dollar_volume_20d = Some(usd("49999999"));
    illiquid.proposed = proposal(INSTRUMENT_3, Side::Buy, "1", "101.5", Origin::OrderBuilder);
    let below_threshold = evaluate(&illiquid.input()).expect("the gate decides");

    assert_eq!(
        (at_threshold.reason, below_threshold.reason),
        (Some(ReasonCode::PriceOutsideCollar), None),
        "at exactly 50 M the tier is liquid and x is 1 percent, so 101.5 is above 101.0505; one \
         dollar below the threshold x is 2 percent and 101.5 is within 102.051"
    );
}

/// §9.6: the collar binds aggressive prices only; a passive price inside the band is allowed.
#[test]
fn a_passive_price_inside_the_band_is_allowed() {
    let mut s = Scenario::allowing();
    s.proposed = proposal(INSTRUMENT_3, Side::Buy, "1", "90", Origin::OrderBuilder);

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_ne!(
        d.reason,
        Some(ReasonCode::PriceOutsideCollar),
        "90 is far below the ask, which is passive and inside the 20 percent band"
    );
}

/// §5.3: the day's count is of submitted orders, rejected ones included.
#[test]
fn a_rejected_order_still_counts() {
    let mut limits = common::two_stock_swing_limits();
    limits.max_orders_per_day = 2;
    let mut s = Scenario::allowing();
    s.mandate = mandate_with(limits);
    s.agent.orders_today = 2;

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.reason),
        (Verdict::Deny, Some(ReasonCode::MaxOrdersPerDay)),
        "each client_order_id counts once, including rejected ones"
    );
}

/// MI-19: a removed instrument is exits-only in that instrument, and nowhere else.
#[test]
fn a_removed_instrument_restricts_only_itself() {
    let mut s = Scenario::allowing();
    s.agent.instrument_restrictions.insert(
        asset(INSTRUMENT_2),
        [mandate_risk::InstrumentRestriction::RemovedInstrument]
            .into_iter()
            .collect(),
    );
    let other = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        other.verdict,
        Verdict::Allow,
        "a restriction on instrument 2 says nothing about instrument 3"
    );

    s.instrument = common::equity_instrument(INSTRUMENT_2);
    s.proposed = proposal(INSTRUMENT_2, Side::Buy, "1", "100", Origin::OrderBuilder);
    let restricted = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (restricted.verdict, restricted.reason),
        (Verdict::Deny, Some(ReasonCode::NotInWorkingUniverse)),
        "a removed instrument is the working-universe check (DEC-129 item 23)"
    );
}

/// §8.2: an opening needs a fresh quote-based risk mark; a reduction takes any source.
#[test]
fn an_opening_needs_a_fresh_quote() {
    let mut s = Scenario::allowing();
    s.market.quote = None;
    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.reason),
        (Verdict::Deny, Some(ReasonCode::StaleMark)),
        "an opening order requires a fresh quote-based risk mark"
    );
}

#[test]
fn a_risk_exit_accepts_a_last_trade_mark() {
    let mut s = Scenario::allowing();
    s.market.quote = None;
    s.agent.positions.insert(asset(INSTRUMENT_3), qty("10"));
    s.proposed = proposal(INSTRUMENT_3, Side::Sell, "10", "100", Origin::RiskEngine);

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        d.verdict,
        Verdict::Allow,
        "risk-reducing orders accept any mark source"
    );
}

/// §4.3: an opening outside the regular session is denied, and an extended-hours one needs a limit.
#[test]
fn an_opening_outside_the_regular_session_is_denied() {
    let mut s = Scenario::allowing();
    s.now = at("2026-09-22T21:00:00Z");
    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.reason),
        (Verdict::Deny, Some(ReasonCode::SessionNotAllowed)),
        "equity openings are regular-session only"
    );
}

#[test]
fn an_extended_hours_opening_needs_a_limit() {
    let mut s = Scenario::allowing();
    s.now = at("2026-09-22T21:00:00Z");
    s.proposed.extended_hours = true;
    s.proposed.kind = mandate_risk::ProposedKind::Market;

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.reason),
        (
            Verdict::Deny,
            Some(ReasonCode::ExtendedHoursOpeningNotAllowed)
        ),
        "extended hours takes limit orders only, and openings not at all in v1"
    );
}

/// §9.6: self-trade prevention across the owner's related accounts.
#[test]
fn an_opposite_side_rest_in_a_related_account_blocks_an_opening() {
    let mut s = Scenario::allowing();
    s.account.related_account_resting.insert(
        asset(INSTRUMENT_3),
        [mandate_risk::RestingSide::Sell].into_iter().collect(),
    );
    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.reason),
        (Verdict::Deny, Some(ReasonCode::ConductLimitBreached)),
        "an opposite-side rest in a related account would be a self-trade"
    );
}

/// §9.6: the order-to-fill ratio is only evaluated after twenty orders.
#[test]
fn the_order_to_fill_ratio_needs_twenty_orders() {
    let mut s = Scenario::allowing();
    s.conduct
        .orders_today_per_instrument
        .insert(asset(INSTRUMENT_3), 19);
    s.conduct.filled_today.insert(asset(INSTRUMENT_3), 0);

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_ne!(
        d.reason,
        Some(ReasonCode::ConductLimitBreached),
        "19 orders is below the 20-order floor, whatever the ratio"
    );
}

/// §5.3: the constraints report the first failing rule of the list, not the worst.
#[test]
fn order_constraints_report_the_first_failing_rule() {
    let mut s = Scenario::allowing();
    s.agent.positions.insert(asset(INSTRUMENT_3), qty("5"));
    s.account.unknown_orders.insert(asset(INSTRUMENT_3));
    s.proposed = proposal(INSTRUMENT_3, Side::Sell, "6", "100", Origin::OrderBuilder);

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        d.reason,
        Some(ReasonCode::WouldCrossZero),
        "rule 3 fails before rule 9, so the zero crossing is reported"
    );
}

/// §5.3: the first pass excludes the agent's own protective and resting opening orders.
#[test]
fn the_first_pass_excludes_the_agents_own_protective_orders() {
    let mut s = Scenario::allowing();
    let mut protective = open_order(AgentId(1), INSTRUMENT_3, "0");
    protective.protective = true;
    protective.side = Side::Sell;
    s.account
        .working_orders
        .insert(ClientOrderId(5), protective);
    s.agent.working_orders.insert(ClientOrderId(5));
    s.agent.positions.insert(asset(INSTRUMENT_3), qty("10"));
    s.proposed = proposal(INSTRUMENT_3, Side::Sell, "10", "100", Origin::RiskEngine);

    s.pass = mandate_risk::GatePass::First;
    let first = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        first.verdict,
        Verdict::Allow,
        "the executor cancels the protective order first, so the first pass excludes it"
    );

    s.pass = mandate_risk::GatePass::BeforeSubmission;
    let resubmit = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        resubmit.reason,
        Some(ReasonCode::SellExceedsAvailable),
        "the re-run before submission applies every rule in full"
    );
}

/// §9.1: when two checks would fail, the earlier one is reported.
#[test]
fn two_simultaneous_failures_report_the_earlier_check() {
    let mut s = Scenario::allowing();
    s.universe = working_universe(&[]);
    s.proposed = proposal(INSTRUMENT_3, Side::Buy, "11", "100", Origin::OrderBuilder);

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        d.reason,
        Some(ReasonCode::NotInWorkingUniverse),
        "the universe is check 2's first item and order size its third, so the universe wins"
    );
}

/// §7.3: three consecutive unexplained 403s restrict the account.
#[test]
fn three_consecutive_unexplained_403s_restrict_the_account() {
    let mut s = Scenario::allowing();
    s.account.state = mandate_risk::AccountState::ClosingOnly;
    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.reason),
        (Verdict::Deny, Some(ReasonCode::AccountRestricted)),
        "the threshold is the connector's to count; the gate reads the resulting state"
    );
}

/// §9.6: a sliced exit reports which control sliced it.
#[test]
fn a_sliced_exit_reports_what_it_applied() {
    let mut s = Scenario::allowing();
    s.market.adv_20d = Some(qty("1000"));
    s.agent.positions.insert(asset(INSTRUMENT_3), qty("900"));
    s.proposed = proposal(INSTRUMENT_3, Side::Sell, "900", "100", Origin::OrderBuilder);

    let d = evaluate(&s.input()).expect("the gate decides");
    let pacing = d.pacing.expect("a paced exit says what it applied");
    assert!(
        !pacing.applied.is_empty(),
        "a slice that names no control cannot be audited"
    );
}

/// Every reason code the gate can emit is registered in the founder-owned case file, with exactly
/// one known exception.
///
/// `not_in_working_universe` is the code mandate spec §5.3 gives the working-universe check and
/// `MC-G14` and `MC-G16` expect, but the trading-domain `reason_codes` registry does not carry it:
/// that registry has `not_in_universe` for the same condition, which `RC-16` step 7 expects. The
/// two suites cannot both be satisfied, which is DEC-129 item 25, proposed to the founder. This
/// test pins the gap rather than papering over it: the exception is named, and **any other**
/// unregistered code fails, so the day a new one is minted this test says so.
#[test]
fn every_reason_code_is_registered_in_the_case_file() {
    /// DEC-129 item 25, `Proposed (founder)`. Removing this entry is what closes that decision.
    const KNOWN_UNREGISTERED: [&str; 1] = ["not_in_working_universe"];

    assert_eq!(
        ReasonCode::ALL.len(),
        ReasonCode::ALL
            .iter()
            .map(|c| c.as_str())
            .collect::<BTreeSet<_>>()
            .len(),
        "ReasonCode::ALL lists every variant exactly once; a duplicate entry here is how a subset \
         goes stale"
    );

    let declared = declared_reason_code_variants();
    assert_eq!(
        ReasonCode::ALL.len(),
        declared.len(),
        "ReasonCode::ALL is missing {} of the {} variants the enum declares; a hand-listed subset \
         is exactly the mistake that would have made MC-G07 unpassable",
        declared.len().saturating_sub(ReasonCode::ALL.len()),
        declared.len()
    );

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

    let missing: Vec<&str> = ReasonCode::ALL
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
        "DEC-129 item 25 is settled for {resolved:?}: drop it from KNOWN_UNREGISTERED"
    );
}

/// The variants the `ReasonCode` enum declares, read out of the source rather than out of
/// `ReasonCode::ALL`.
///
/// An independent oracle, because the array cannot check its own completeness: comparing `ALL`
/// against itself catches a duplicate but never a variant nobody added, which is the failure
/// review round 4 found the doc claiming was covered. `as_str` forces a new variant to be *named*
/// somewhere; only this count forces it into `ALL`.
fn declared_reason_code_variants() -> BTreeSet<String> {
    let source = include_str!("../src/lib.rs");
    let block: Vec<&str> = source
        .lines()
        .skip_while(|l| !l.starts_with("pub enum ReasonCode {"))
        .skip(1)
        .take_while(|l| !l.starts_with('}'))
        .collect();
    assert!(
        !block.is_empty(),
        "the ReasonCode enum was not found where this oracle expects it; a moved declaration must \
         move this reader too, not silently pass"
    );
    let variants: BTreeSet<String> = block
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with("///") && !l.starts_with("#["))
        .filter_map(|l| l.strip_suffix(','))
        .map(str::to_owned)
        .collect();
    assert!(
        variants.len() > 30,
        "the ReasonCode enum reader found only {} variants, so it is no longer reading the enum",
        variants.len()
    );
    variants
}

/// DEC-401: a proposal of zero quantity is no order, and the gate refuses it by name before any
/// check, whatever else it would be. The grid is every origin, both sides, flat and held, every
/// agent mode, both passes, a working universe read or not, and four quotes: an ordinary one and
/// the three over which a collar cannot be computed (DEC-383's: the passive end overflowing at
/// the decimal's maximum, truncating to zero at `0.0000833`, and a configured `x` of one). On
/// every row the oracle is the quantity alone: zero is `zero_quantity`, ahead of the unread
/// universe's own error and of the collar's arithmetic (DEC-401 item 3), and the smallest
/// quantity above zero is never refused that way, so the refusal is the zero and not a row it
/// happens to sit on.
#[test]
fn a_proposal_of_zero_is_refused_by_name_whatever_else_it_would_be() {
    const COLLARS: [Option<(&str, bool)>; 4] = [
        None,
        Some(("79228162514264337593.543950335", false)),
        Some(("0.0000833", false)),
        Some(("99.95", true)),
    ];
    let origins = [
        Origin::OrderBuilder,
        Origin::GoalCompletion,
        Origin::RemovedInstrument,
        Origin::RiskEngine,
        Origin::TrimToTarget,
        Origin::StopWatchdog,
        Origin::AutomatedKillSwitch,
        Origin::OwnerClose,
        Origin::OwnerKillSwitch,
        Origin::ProtectiveLeg,
    ];
    let modes = [
        AgentMode::Normal,
        AgentMode::ExitsOnly,
        AgentMode::Paused,
        AgentMode::Stopped,
    ];
    let passes = [
        mandate_risk::GatePass::First,
        mandate_risk::GatePass::BeforeSubmission,
    ];
    let mut rows = 0_u32;
    for origin in origins {
        for side in [Side::Buy, Side::Sell] {
            for held in ["0", "10"] {
                for mode in modes {
                    for pass in passes {
                        for universe_read in [true, false] {
                            for collar in COLLARS {
                                for (quantity, zero) in [("0", true), ("0.000000001", false)] {
                                    let mut s = Scenario::allowing();
                                    if let Some((quote, x_is_one)) = collar {
                                        s.market.quote = Some(mandate_risk::SaneQuote {
                                            bid: price(quote),
                                            ask: price(quote),
                                            at: s.now,
                                        });
                                        if x_is_one {
                                            s.config.collar_liquid_x = fraction("1");
                                            s.config.collar_other_x = fraction("1");
                                            s.config.collar_crypto_x = fraction("1");
                                        }
                                    }
                                    s.pass = pass;
                                    s.agent.mode = mode;
                                    s.agent.positions.insert(asset(INSTRUMENT_3), qty(held));
                                    s.proposed =
                                        proposal(INSTRUMENT_3, side, quantity, "100", origin);
                                    if !universe_read {
                                        s.universe = mandate_risk::WorkingUniverse::Unavailable;
                                    }
                                    let decided = evaluate(&s.input());
                                    let refused_as_zero = matches!(
                                        decided,
                                        Err(mandate_risk::GateError::ZeroQuantity)
                                    );
                                    assert_eq!(
                                        refused_as_zero, zero,
                                        "{origin:?} {side:?} of {quantity} with {held} held, {mode:?}, \
                                     {pass:?}, universe read {universe_read}, quote {collar:?}: \
                                     {decided:?}"
                                    );
                                    rows = rows.saturating_add(1);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(
        rows,
        10 * 2 * 2 * 4 * 2 * 2 * 4 * 2,
        "every row of the grid was decided"
    );
}

/// DEC-401's refusal has its own stable code, distinct from every other refusal's.
#[test]
fn a_zero_proposal_reports_the_zero_quantity_code() {
    let mut s = Scenario::allowing();
    s.proposed = proposal(INSTRUMENT_3, Side::Buy, "0", "100", Origin::OrderBuilder);
    let refused = evaluate(&s.input());
    assert_eq!(
        refused.as_ref().map_err(mandate_risk::GateError::code),
        Err("zero_quantity"),
        "the refusal names the zero: {refused:?}"
    );
}
