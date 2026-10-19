//! DEC-2605 row 20: the gate's dry run, shared by the shell and the cell's runtime. Each
//! expectation is the gate's own answer or the proposal's own field, never this crate's.

use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::{AssetClass, InstrumentId, Side};
use mandate_canon::Value;
use mandate_num::{Price, Qty};
use mandate_ports::{AdvisoryGateContext, AdvisoryOrderFacts, PortError, evaluate, proposed_order};
use mandate_risk::{GateError, GateInput};
use mandate_runtime::{Proposal, Purpose};
use mandate_time::UtcNanos;
use proptest::prelude::*;

const ASSET: &str = "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415";

fn proposal(qty: &str) -> Proposal {
    Proposal {
        instrument: InstrumentId::new(ASSET).unwrap(),
        asset_class: AssetClass::UsEquity,
        side: Side::Buy,
        qty: Qty::parse(qty).unwrap(),
        limit: Price::parse("12.34").unwrap(),
        purpose: Purpose::Open,
        exit_origin: None,
        exit_conviction: Some(Value::Str("1".to_owned())),
        buy_conviction: Some(Value::Str("1".to_owned())),
        combined_score: Value::Str("0.8".to_owned()),
        outputs_used: BTreeSet::new(),
        model_weights: BTreeMap::new(),
        clips_applied: Vec::new(),
        execution: None,
    }
}

fn context() -> AdvisoryGateContext {
    let asset = mandate_risk::AssetId::new(ASSET).unwrap();
    let zero_fraction = mandate_num::Fraction::parse("0").unwrap();
    AdvisoryGateContext {
        now: UtcNanos::parse("2026-09-25T20:00:00.000000000Z").unwrap(),
        pass: mandate_risk::GatePass::First,
        config: mandate_risk::GateConfig {
            price_floor: mandate_num::Usd::ZERO,
            liquidity_floor_usd: mandate_num::Usd::ZERO,
            crypto_liquidity_floor_usd: mandate_num::Usd::ZERO,
            collar_liquid_threshold_usd: mandate_num::Usd::ZERO,
            collar_liquid_x: zero_fraction,
            collar_other_x: zero_fraction,
            collar_crypto_x: zero_fraction,
            collar_passive_band: zero_fraction,
            opposite_fill_interval_s: 0,
            min_resting_time_s: 0,
            order_to_fill_max: 0,
            order_to_fill_min_orders: 0,
            order_size_participation: zero_fraction,
            daily_participation: zero_fraction,
            close_window_minutes: 0,
            legacy_pdt_equity_threshold: mandate_num::Usd::ZERO,
            etp_classification_max_age_s: 0,
        },
        mandate: mandate_risk::ValidatedMandate::from_validated_parts(
            mandate_risk::spec_types::RiskLimits {
                max_position_usd: mandate_num::Usd::ZERO,
                max_position_fraction: zero_fraction,
                max_order_usd: mandate_num::Usd::ZERO,
                max_gross_exposure_usd: mandate_num::Usd::ZERO,
                max_orders_per_day: 0,
                reentry_cooldown_s: 0,
                rebalance_band: zero_fraction,
                breach_confirm_s: 0,
                drawdown_ladder: Vec::new(),
            },
            mandate_risk::spec_types::GoalState::Running,
            false,
            false,
        ),
        risk: mandate_risk::RiskSnapshot {
            agent_equity: mandate_num::Usd::ZERO,
            high_water_mark: mandate_num::Usd::ZERO,
            day_start_equity: mandate_num::Usd::ZERO,
            capital_base: mandate_num::Usd::ZERO,
            inherited_loss: mandate_num::Usd::ZERO,
            latched: BTreeSet::new(),
            active_rungs: BTreeMap::new(),
            size_factor: mandate_num::Ratio::parse("1").unwrap(),
            agent_mode: mandate_risk::AgentMode::Normal,
        },
        account: mandate_risk::AccountSnapshot {
            account_type: mandate_risk::AccountType::Cash,
            state: mandate_risk::AccountState::Active,
            crypto_active: false,
            regime: mandate_risk::DayTradeRegime::LegacyPdt,
            equity: mandate_num::Usd::ZERO,
            prior_close_equity: mandate_num::Usd::ZERO,
            model_buying_power: mandate_num::Usd::ZERO,
            broker_buying_power: mandate_num::Usd::ZERO,
            broker_non_marginable_buying_power: mandate_num::Usd::ZERO,
            positions: BTreeMap::new(),
            market_values: BTreeMap::new(),
            working_orders: BTreeMap::new(),
            unknown_orders: BTreeSet::new(),
            related_account_resting: BTreeMap::new(),
        },
        agent: mandate_risk::AgentSnapshot {
            agent: mandate_risk::AgentId(1),
            mode: mandate_risk::AgentMode::Normal,
            instrument_restrictions: BTreeMap::new(),
            positions: BTreeMap::new(),
            market_values: BTreeMap::new(),
            working_orders: BTreeSet::new(),
            instrument_groups: BTreeMap::new(),
            last_exit_fill_at: BTreeMap::new(),
            orders_today: 0,
            day_trades: mandate_risk::DayTradeLedger::default(),
        },
        instrument: mandate_risk::InstrumentSnapshot {
            instrument: asset,
            asset_class: mandate_risk::AssetClass::UsEquity,
            exchange: None,
            status_active: false,
            tradable: false,
            fractionable: false,
            ipo: false,
            ptp_no_exception: false,
            etp: mandate_risk::EtpClass::Plain,
            etp_classified_at: None,
            quote_currency: None,
            prior_close: None,
            median_dollar_volume_20d: None,
            median_dollar_volume_30d: None,
            min_order_size: Qty::ZERO,
            qty_increment: Qty::parse("1").unwrap(),
            halted: false,
            status_feed_current: false,
        },
        market: mandate_risk::MarketSnapshot {
            quote: None,
            last_trade: None,
            trailing_5m_volume: None,
            adv_20d: None,
        },
        conduct: mandate_risk::ConductState::default(),
        universe: mandate_risk::WorkingUniverse::Unavailable,
        order: AdvisoryOrderFacts {
            kind: mandate_risk::ProposedKind::Plain,
            tif: mandate_risk::TimeInForce::Day,
            extended_hours: false,
            origin: mandate_risk::Origin::OrderBuilder,
            owner_confirmed_bid: None,
            client_order_id: mandate_risk::ClientOrderId(1),
            fee_reservation: mandate_num::Usd::ZERO,
        },
    }
}

/// The gate called directly over `context`, the oracle the shared port must equal.
fn direct(proposal: &Proposal, context: &AdvisoryGateContext) -> String {
    let proposed = mandate_risk::ProposedOrder {
        instrument: mandate_risk::AssetId::new(proposal.instrument.as_str()).unwrap(),
        side: proposal.side,
        qty: proposal.qty,
        limit_price: proposal.limit,
        kind: context.order.kind.clone(),
        tif: context.order.tif,
        extended_hours: context.order.extended_hours,
        origin: context.order.origin,
        owner_confirmed_bid: context.order.owner_confirmed_bid,
        client_order_id: context.order.client_order_id,
        fee_reservation: context.order.fee_reservation,
    };
    format!(
        "{:?}",
        mandate_risk::evaluate(&GateInput {
            now: context.now,
            pass: context.pass,
            config: &context.config,
            mandate: &context.mandate,
            risk: &context.risk,
            account: &context.account,
            agent: &context.agent,
            instrument: &context.instrument,
            market: &context.market,
            conduct: &context.conduct,
            universe: &context.universe,
            proposed: &proposed,
        })
    )
}

#[test]
fn the_proposed_order_copies_only_the_proposals_four_fields() {
    let proposal = proposal("3");
    let facts = AdvisoryOrderFacts {
        kind: mandate_risk::ProposedKind::Bracket {
            take_profit: Price::parse("15").unwrap(),
            stop: Price::parse("10").unwrap(),
        },
        tif: mandate_risk::TimeInForce::Gtc,
        extended_hours: true,
        origin: mandate_risk::Origin::OrderBuilder,
        owner_confirmed_bid: Some(Price::parse("12").unwrap()),
        client_order_id: mandate_risk::ClientOrderId(71),
        fee_reservation: mandate_num::Usd::parse("1.23").unwrap(),
    };
    let mapped = proposed_order(&proposal, &facts).unwrap();
    assert_eq!(mapped.instrument.as_str(), ASSET);
    assert_eq!(
        (mapped.side, mapped.qty, mapped.limit_price),
        (proposal.side, proposal.qty, proposal.limit)
    );
    assert_eq!(mapped.kind, facts.kind);
    assert_eq!(
        (mapped.tif, mapped.extended_hours, mapped.origin),
        (facts.tif, facts.extended_hours, facts.origin)
    );
    assert_eq!(mapped.owner_confirmed_bid, facts.owner_confirmed_bid);
    assert_eq!(mapped.client_order_id, facts.client_order_id);
    assert_eq!(mapped.fee_reservation, facts.fee_reservation);
}

#[test]
fn the_shared_gate_is_the_real_gate_over_the_mapped_order() {
    assert!(matches!(
        evaluate(&proposal("0"), &context()),
        Err(PortError::Gate(GateError::ZeroQuantity))
    ));
    let decision = evaluate(&proposal("1"), &context()).unwrap();
    assert_eq!(
        format!("Ok({decision:?})"),
        direct(&proposal("1"), &context()),
        "the port answers as the gate itself"
    );
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, failure_persistence: None, ..ProptestConfig::default() })]

    /// The port never changes the gate's answer: for any quantity, the shared evaluation and the
    /// gate called directly agree, refusals included.
    #[test]
    fn the_port_answers_as_the_gate_for_any_quantity(units in 0_u32..1000) {
        let proposal = proposal(&units.to_string());
        let ours = evaluate(&proposal, &context());
        let theirs = direct(&proposal, &context());
        match ours {
            Ok(decision) => prop_assert_eq!(format!("Ok({decision:?})"), theirs),
            Err(PortError::Gate(error)) => prop_assert_eq!(format!("Err({error:?})"), theirs),
            Err(other) => prop_assert!(false, "{other}"),
        }
    }
}
