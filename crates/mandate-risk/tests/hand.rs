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
    INSTRUMENT_1, INSTRUMENT_2, INSTRUMENT_3, INSTRUMENT_4, Scenario, asset, at, fraction,
    mandate_with, open_order, price, proposal, qty, two_stock_swing_limits, usd, working_universe,
};
use mandate_risk::{
    AgentId, AgentMode, AgentPosition, AssetClass, ClientOrderId, FlattenInitiator, FlattenInput,
    FlattenPricing, GroupId, Origin, Purpose, ReasonCode, Session, Side, Verdict, agent_flatten,
    evaluate,
};

/// `MC-G01`: position 1000 + working 300 + proposed 3 × 100 = 1600 > min(1500, 0.30 × 10000 = 3000).
#[test]
#[ignore = "pending E6-3"]
fn mc_g01_the_per_instrument_cap_counts_position_working_and_proposed() {
    let mut s = Scenario::allowing();
    s.instrument = common::equity_instrument(INSTRUMENT_2);
    s.agent
        .market_values
        .insert(asset(INSTRUMENT_2), usd("1000"));
    s.agent.positions.insert(asset(INSTRUMENT_2), qty("10"));
    s.account.working_orders.insert(
        ClientOrderId(9),
        open_order(AgentId(1), INSTRUMENT_2, "300"),
    );
    s.agent.working_orders.insert(ClientOrderId(9));
    s.agent.orders_today = 3;
    s.proposed = proposal(INSTRUMENT_2, Side::Buy, "3", "100", Origin::OrderBuilder);

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.reason),
        (Verdict::Deny, Some(ReasonCode::ConcentrationLimit)),
        "1000 + 300 + 300 = 1600 is above the cap min(1500, 0.30 x 10000) = 1500"
    );
}

/// `MC-G02`: the same position at 2 × 100 reaches exactly 1500, and a value at the limit passes.
#[test]
#[ignore = "pending E6-3"]
fn mc_g02_a_total_exactly_at_the_cap_is_allowed() {
    let mut s = Scenario::allowing();
    s.instrument = common::equity_instrument(INSTRUMENT_2);
    s.agent
        .market_values
        .insert(asset(INSTRUMENT_2), usd("1000"));
    s.agent.positions.insert(asset(INSTRUMENT_2), qty("10"));
    s.account.working_orders.insert(
        ClientOrderId(9),
        open_order(AgentId(1), INSTRUMENT_2, "300"),
    );
    s.agent.working_orders.insert(ClientOrderId(9));
    s.agent.orders_today = 3;
    s.proposed = proposal(INSTRUMENT_2, Side::Buy, "2", "100", Origin::OrderBuilder);

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.reason),
        (Verdict::Allow, None),
        "1000 + 300 + 200 = 1500 equals the cap, and the comparison is strictly greater"
    );
}

/// `MC-G03`: 11 × 100 = 1100 is above `max_order_usd` 1000.
#[test]
#[ignore = "pending E6-3"]
fn mc_g03_an_order_above_max_order_usd_is_denied() {
    let mut s = Scenario::allowing();
    s.proposed = proposal(INSTRUMENT_3, Side::Buy, "11", "100", Origin::OrderBuilder);

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.reason),
        (Verdict::Deny, Some(ReasonCode::MaxOrderSize)),
        "11 x 100 = 1100 is above max_order_usd 1000"
    );
}

/// `MC-G04`: a working order in another instrument counts toward gross exposure.
#[test]
#[ignore = "pending E6-3"]
fn mc_g04_gross_exposure_counts_working_orders_in_other_instruments() {
    let mut s = Scenario::allowing();
    s.agent
        .market_values
        .insert(asset(INSTRUMENT_2), usd("1000"));
    s.agent.positions.insert(asset(INSTRUMENT_2), qty("10"));
    s.account.working_orders.insert(
        ClientOrderId(9),
        open_order(AgentId(1), INSTRUMENT_2, "300"),
    );
    s.agent.working_orders.insert(ClientOrderId(9));
    s.proposed = proposal(INSTRUMENT_3, Side::Buy, "8", "100", Origin::OrderBuilder);

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.reason),
        (Verdict::Deny, Some(ReasonCode::GrossExposureLimit)),
        "1000 + 300 + 800 = 2100 is above min(2000, equity 10000) = 2000"
    );
}

/// `MC-G05`: with equity 9500 and the fraction at 0.15, the fraction binds at 1425.
#[test]
#[ignore = "pending E6-3"]
fn mc_g05_the_fraction_binds_when_equity_falls() {
    let mut limits = two_stock_swing_limits();
    limits.max_position_fraction = fraction("0.15");
    let mut s = Scenario::allowing();
    s.mandate = mandate_with(limits);
    s.instrument = common::equity_instrument(INSTRUMENT_2);
    s.risk = common::healthy_risk("9500");
    s.account.equity = usd("9500");
    s.agent
        .market_values
        .insert(asset(INSTRUMENT_2), usd("1300"));
    s.agent.positions.insert(asset(INSTRUMENT_2), qty("13"));
    s.proposed = proposal(INSTRUMENT_2, Side::Buy, "2", "100", Origin::OrderBuilder);

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.reason),
        (Verdict::Deny, Some(ReasonCode::ConcentrationLimit)),
        "1300 + 200 = 1500 is above min(1500, 0.15 x 9500 = 1425) = 1425"
    );
}

/// `MC-G06`: equity below `max_gross_exposure_usd` is what caps gross exposure.
#[test]
#[ignore = "pending E6-3"]
fn mc_g06_equity_caps_gross_exposure_below_the_configured_limit() {
    let mut limits = two_stock_swing_limits();
    limits.max_position_usd = usd("10000");
    limits.max_position_fraction = fraction("1");
    limits.max_gross_exposure_usd = usd("10000");
    let mut s = Scenario::allowing();
    s.mandate = mandate_with(limits);
    s.risk = common::healthy_risk("9500");
    s.account.equity = usd("9500");
    s.agent
        .market_values
        .insert(asset(INSTRUMENT_2), usd("9000"));
    s.agent.positions.insert(asset(INSTRUMENT_2), qty("90"));
    s.proposed = proposal(INSTRUMENT_3, Side::Buy, "6", "100", Origin::OrderBuilder);

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.reason),
        (Verdict::Deny, Some(ReasonCode::GrossExposureLimit)),
        "9000 + 600 = 9600 is above min(10000, equity 9500) = 9500"
    );
}

/// `MC-G07`: the count is of submitted opening orders, and the proposal is the one that tips it.
#[test]
#[ignore = "pending E6-3"]
fn mc_g07_orders_per_day_counts_the_proposal_itself() {
    let mut limits = two_stock_swing_limits();
    limits.max_orders_per_day = 3;
    let mut s = Scenario::allowing();
    s.mandate = mandate_with(limits);
    s.agent.orders_today = 3;

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.reason),
        (Verdict::Deny, Some(ReasonCode::MaxOrdersPerDay)),
        "3 submitted + 1 proposed is above max_orders_per_day 3"
    );
}

/// `MC-G08`: an exit is allowed although the day's order count is at the limit (MI-1).
#[test]
#[ignore = "pending E6-3"]
fn mc_g08_an_exit_is_allowed_at_the_orders_per_day_limit() {
    let mut limits = two_stock_swing_limits();
    limits.max_orders_per_day = 3;
    let mut s = Scenario::allowing();
    s.mandate = mandate_with(limits);
    s.instrument = common::equity_instrument(INSTRUMENT_2);
    s.agent.orders_today = 3;
    s.agent.positions.insert(asset(INSTRUMENT_2), qty("10"));
    s.agent
        .market_values
        .insert(asset(INSTRUMENT_2), usd("1000"));
    s.proposed = proposal(INSTRUMENT_2, Side::Sell, "10", "100", Origin::OrderBuilder);

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.purpose),
        (Verdict::Allow, Purpose::DiscretionaryExit),
        "a discretionary exit is never denied by a mandate limit (MI-1)"
    );
}

/// `MC-G09`: a risk exit larger than `max_order_usd` is allowed.
#[test]
#[ignore = "pending E6-3"]
fn mc_g09_a_risk_exit_above_max_order_usd_is_allowed() {
    let mut s = Scenario::allowing();
    s.instrument = common::equity_instrument(INSTRUMENT_2);
    s.agent.positions.insert(asset(INSTRUMENT_2), qty("12"));
    s.agent
        .market_values
        .insert(asset(INSTRUMENT_2), usd("1200"));
    s.proposed = proposal(INSTRUMENT_2, Side::Sell, "12", "100", Origin::RiskEngine);

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.purpose),
        (Verdict::Allow, Purpose::RiskExit),
        "12 x 100 = 1200 is above max_order_usd 1000, and a risk exit is exempt"
    );
}

/// `MC-G10`: an owner exit is allowed by every mandate limit.
#[test]
#[ignore = "pending E6-3"]
fn mc_g10_an_owner_exit_is_allowed_by_every_mandate_limit() {
    let mut limits = two_stock_swing_limits();
    limits.max_orders_per_day = 3;
    let mut s = Scenario::allowing();
    s.mandate = mandate_with(limits);
    s.instrument = common::equity_instrument(INSTRUMENT_2);
    s.agent.orders_today = 3;
    s.agent.positions.insert(asset(INSTRUMENT_2), qty("12"));
    s.agent
        .market_values
        .insert(asset(INSTRUMENT_2), usd("1200"));
    s.proposed = proposal(INSTRUMENT_2, Side::Sell, "12", "100", Origin::OwnerClose);

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.purpose),
        (Verdict::Allow, Purpose::OwnerExit),
        "an owner exit passes every mandate limit"
    );
}

/// `MC-G11`: 14:30 to 15:00 is 1800 s, which is not yet past `reentry_cooldown_s` 1800.
#[test]
#[ignore = "pending E6-3"]
fn mc_g11_the_reentry_cooldown_denies_inside_the_window() {
    let mut s = Scenario::allowing();
    s.agent
        .last_exit_fill_at
        .insert(asset(INSTRUMENT_3), at("2026-09-21T14:30:00Z"));

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.reason),
        (Verdict::Deny, Some(ReasonCode::ReentryCooldown)),
        "15:00 - 14:30 = 1800 s is not strictly past reentry_cooldown_s 1800"
    );
}

/// `MC-G12`: the cooldown covers the whole instrument group, not the proposed instrument alone.
#[test]
#[ignore = "pending E6-3"]
fn mc_g12_the_reentry_cooldown_covers_the_instrument_group() {
    let mut s = Scenario::allowing();
    s.agent
        .last_exit_fill_at
        .insert(asset(INSTRUMENT_4), at("2026-09-21T14:30:00Z"));
    s.agent
        .instrument_groups
        .insert(asset(INSTRUMENT_3), GroupId(7));
    s.agent
        .instrument_groups
        .insert(asset(INSTRUMENT_4), GroupId(7));

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.reason),
        (Verdict::Deny, Some(ReasonCode::ReentryCooldown)),
        "the exit fill was in another instrument of the same group"
    );
}

/// `MC-G13`: at 15:30 the cooldown has passed, and a value exactly at it would also pass.
#[test]
#[ignore = "pending E6-3"]
fn mc_g13_the_reentry_cooldown_lifts_once_it_has_elapsed() {
    let mut s = Scenario::allowing();
    s.now = at("2026-09-21T15:30:00Z");
    s.agent
        .last_exit_fill_at
        .insert(asset(INSTRUMENT_3), at("2026-09-21T14:30:00Z"));

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.reason),
        (Verdict::Allow, None),
        "15:30 - 14:30 = 3600 s is past reentry_cooldown_s 1800"
    );
}

/// `MC-G14`: an opening outside the working universe is denied, whatever the limits say.
#[test]
#[ignore = "pending E6-3"]
fn mc_g14_an_opening_outside_the_working_universe_is_denied() {
    let mut s = Scenario::allowing();
    s.universe = working_universe(&[INSTRUMENT_2]);

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.reason),
        (Verdict::Deny, Some(ReasonCode::NotInWorkingUniverse)),
        "the proposed instrument is not in the working universe"
    );
}

/// `MC-G15`: an exit outside the working universe is allowed (MI-1, MI-19).
#[test]
#[ignore = "pending E6-3"]
fn mc_g15_an_exit_outside_the_working_universe_is_allowed() {
    let mut s = Scenario::allowing();
    s.universe = working_universe(&[INSTRUMENT_2]);
    s.agent.positions.insert(asset(INSTRUMENT_3), qty("1"));
    s.proposed = proposal(INSTRUMENT_3, Side::Sell, "1", "100", Origin::OrderBuilder);

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.purpose),
        (Verdict::Allow, Purpose::DiscretionaryExit),
        "a removed instrument is exits-only in that instrument, never closed to exits"
    );
}

/// `MC-G16`: an empty working universe is known, and denies every opening.
#[test]
#[ignore = "pending E6-3"]
fn mc_g16_an_empty_working_universe_denies_every_opening() {
    let mut s = Scenario::allowing();
    s.instrument = common::equity_instrument(INSTRUMENT_2);
    s.universe = working_universe(&[]);
    s.agent
        .market_values
        .insert(asset(INSTRUMENT_2), usd("1000"));
    s.agent.positions.insert(asset(INSTRUMENT_2), qty("10"));
    s.proposed = proposal(INSTRUMENT_2, Side::Buy, "1", "100", Origin::OrderBuilder);

    let d = evaluate(&s.input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.reason),
        (Verdict::Deny, Some(ReasonCode::NotInWorkingUniverse)),
        "an empty universe denies, and is not the same thing as an unread one"
    );
}

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

/// `MC-F01`: an automated flatten in the regular session touches only this agent.
#[test]
#[ignore = "pending E6-3"]
fn mc_f01_an_automated_flatten_touches_only_its_own_agent() {
    let mut orders = BTreeMap::new();
    orders.insert(
        ClientOrderId(1),
        open_order(AgentId(1), INSTRUMENT_2, "100"),
    );
    orders.insert(
        ClientOrderId(2),
        open_order(AgentId(1), INSTRUMENT_1, "100"),
    );
    orders.insert(
        ClientOrderId(3),
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
            agent: AgentId(1),
            instrument: asset(INSTRUMENT_1),
            asset_class: AssetClass::Crypto,
            qty: qty("0.05"),
        },
        AgentPosition {
            agent: AgentId(2),
            instrument: asset(INSTRUMENT_3),
            asset_class: AssetClass::UsEquity,
            qty: qty("20"),
        },
    ];
    let plan = agent_flatten(&FlattenInput {
        agent: AgentId(1),
        open_orders: &orders,
        agent_positions: &positions,
        session: Session::Regular,
        initiator: FlattenInitiator::RiskLimit,
        owner_confirmed_bid: None,
        max_exit_offset: fraction("0.03"),
        owner_floor_price: None,
    })
    .expect("the flatten plans");

    assert_eq!(
        (
            plan.mode_applied_first,
            plan.purpose,
            plan.cancel_client_order_ids.clone(),
            plan.cancel_all_endpoint,
            plan.close_position_endpoint,
            plan.sells.len(),
            plan.deferred_sells.len(),
        ),
        (
            AgentMode::Paused,
            Purpose::RiskExit,
            vec![ClientOrderId(1), ClientOrderId(2)],
            false,
            false,
            2,
            0,
        ),
        "the broker holds 15 of instrument 2 but this agent's sub-ledger is 10"
    );
    assert_eq!(
        plan.sells.first().map(|s| (s.qty, s.pricing)),
        Some((qty("10"), FlattenPricing::MarketOrLadder)),
        "a sell in the regular session is priced market_or_ladder"
    );
}

/// `MC-F02`: after hours an automated flatten defers equities and sends crypto through the ladder.
#[test]
#[ignore = "pending E6-3"]
fn mc_f02_an_after_hours_automated_flatten_defers_equities_and_sends_crypto() {
    let orders = BTreeMap::new();
    let positions = vec![
        AgentPosition {
            agent: AgentId(1),
            instrument: asset(INSTRUMENT_2),
            asset_class: AssetClass::UsEquity,
            qty: qty("10"),
        },
        AgentPosition {
            agent: AgentId(1),
            instrument: asset(INSTRUMENT_1),
            asset_class: AssetClass::Crypto,
            qty: qty("0.05"),
        },
    ];
    let plan = agent_flatten(&FlattenInput {
        agent: AgentId(1),
        open_orders: &orders,
        agent_positions: &positions,
        session: Session::AfterHours,
        initiator: FlattenInitiator::RiskLimit,
        owner_confirmed_bid: None,
        max_exit_offset: fraction("0.03"),
        owner_floor_price: None,
    })
    .expect("the flatten plans");

    assert_eq!(
        (
            plan.sells.len(),
            plan.sells.first().map(|s| s.pricing),
            plan.deferred_sells.len(),
        ),
        (1, Some(FlattenPricing::ExitPriceLadder), 1),
        "outside the regular session the crypto sell goes now, priced by the ladder"
    );
}

/// `MC-F03`: an owner kill switch after hours with the bid confirmed sells to a floor of 97.
#[test]
#[ignore = "pending E6-3"]
fn mc_f03_an_owner_kill_switch_prices_to_the_confirmed_floor() {
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
        session: Session::AfterHours,
        initiator: FlattenInitiator::Owner,
        owner_confirmed_bid: Some(price("100")),
        max_exit_offset: fraction("0.03"),
        owner_floor_price: None,
    })
    .expect("the flatten plans");

    assert_eq!(
        (
            plan.mode_applied_first,
            plan.purpose,
            plan.sells.first().map(|s| s.floor_price),
            plan.sells
                .first()
                .map(|s| s.rests_at_floor_then_waits_for_open),
            plan.deferred_sells.len(),
        ),
        (
            AgentMode::Stopped,
            Purpose::OwnerExit,
            Some(Some(price("97"))),
            Some(true),
            0,
        ),
        "100 x (1 - 0.03) = 97, and the owner kill switch stops the agent first"
    );
}

/// `MC-F04`: without a confirmed bid the equity sells wait for the session.
#[test]
#[ignore = "pending E6-3"]
fn mc_f04_an_unconfirmed_owner_kill_switch_defers_equities() {
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
        session: Session::AfterHours,
        initiator: FlattenInitiator::Owner,
        owner_confirmed_bid: None,
        max_exit_offset: fraction("0.03"),
        owner_floor_price: None,
    })
    .expect("the flatten plans");

    assert_eq!(
        (
            plan.mode_applied_first,
            plan.sells.len(),
            plan.deferred_sells.len()
        ),
        (AgentMode::Stopped, 0, 1),
        "without the owner's confirmation an equity sell waits for the regular session"
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
    let registered: BTreeSet<&str> = registry
        .lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix("- "))
        .map(|l| l.split('#').next().unwrap_or(l).trim())
        .collect();

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
