//! DEC-2605 row 20: the gate's dry run, shared by the shell and the cell's runtime. Each
//! expectation is the gate's own answer or the proposal's own field, never this crate's.

#![allow(
    clippy::disallowed_methods,
    reason = "these tests pin the advisory port itself against the binding gate (DEC-2378 item 11)"
)]

use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::{AssetClass, InstrumentId, Side};
use mandate_canon::Value;
use mandate_num::{Price, Qty};
use mandate_ports::{
    AdvisoryGateContext, AdvisoryOrderFacts, PortError, evaluate, evaluate_deferring,
    proposed_order,
};
use mandate_risk::{Deferred, GateError, GateInput};
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
    format!(
        "{:?}",
        with_input(proposal, context, mandate_risk::evaluate)
    )
}

/// The advisory pass called directly over `context`, deferring `deferred`.
fn direct_deferring(
    proposal: &Proposal,
    context: &AdvisoryGateContext,
    deferred: &BTreeSet<Deferred>,
) -> String {
    format!(
        "{:?}",
        with_input(proposal, context, |input| mandate_risk::evaluate_deferring(
            input, deferred
        ))
    )
}

fn with_input<T>(
    proposal: &Proposal,
    context: &AdvisoryGateContext,
    gate: impl FnOnce(&GateInput<'_>) -> T,
) -> T {
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
    gate(&GateInput {
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
    for qty in ["1", "250"] {
        let ours = match evaluate(&proposal(qty), &context()) {
            Ok(decision) => format!("Ok({decision:?})"),
            Err(PortError::Gate(error)) => format!("Err({error:?})"),
            Err(other) => panic!("{other}"),
        };
        assert_eq!(
            ours,
            direct(&proposal(qty), &context()),
            "the port answers as the gate itself"
        );
    }
}

/// A context in which the gate allows an opening buy of up to 10 shares at 100: the gate's own
/// unit-test baseline, so a varied input reaches every check rather than stopping at the first.
fn allowing() -> AdvisoryGateContext {
    let usd = |text: &str| mandate_num::Usd::parse(text).unwrap();
    let fraction = |text: &str| mandate_num::Fraction::parse(text).unwrap();
    let asset = mandate_risk::AssetId::new(ASSET).unwrap();
    let mut context = context();
    context.now = UtcNanos::parse("2026-09-21T15:00:00.000000000Z").unwrap();
    context.config = mandate_risk::GateConfig {
        price_floor: usd("5"),
        liquidity_floor_usd: usd("1000000"),
        crypto_liquidity_floor_usd: usd("2000000"),
        collar_liquid_threshold_usd: usd("50000000"),
        collar_liquid_x: fraction("0.01"),
        collar_other_x: fraction("0.02"),
        collar_crypto_x: fraction("0.02"),
        collar_passive_band: fraction("0.2"),
        opposite_fill_interval_s: 60,
        min_resting_time_s: 2,
        order_to_fill_max: 10,
        order_to_fill_min_orders: 20,
        order_size_participation: fraction("0.05"),
        daily_participation: fraction("0.05"),
        close_window_minutes: 10,
        legacy_pdt_equity_threshold: usd("25000"),
        etp_classification_max_age_s: 604_800,
    };
    context.mandate = mandate_risk::ValidatedMandate::from_validated_parts(
        mandate_risk::spec_types::RiskLimits {
            max_position_usd: usd("1500"),
            max_position_fraction: fraction("0.2"),
            max_order_usd: usd("1000"),
            max_gross_exposure_usd: usd("2000"),
            max_orders_per_day: 50,
            reentry_cooldown_s: 3600,
            rebalance_band: fraction("0.05"),
            breach_confirm_s: 60,
            drawdown_ladder: Vec::new(),
        },
        mandate_risk::spec_types::GoalState::Running,
        false,
        false,
    );
    for equity in [
        &mut context.risk.agent_equity,
        &mut context.risk.high_water_mark,
        &mut context.risk.day_start_equity,
        &mut context.risk.capital_base,
        &mut context.account.equity,
    ] {
        *equity = usd("10000");
    }
    context.account.account_type = mandate_risk::AccountType::Margin;
    context.account.crypto_active = true;
    context.account.prior_close_equity = usd("30000");
    context.account.model_buying_power = usd("100000");
    context.account.broker_buying_power = usd("100000");
    context.account.broker_non_marginable_buying_power = usd("100000");
    context.instrument.exchange = Some(mandate_risk::Exchange::Nasdaq);
    context.instrument.status_active = true;
    context.instrument.tradable = true;
    context.instrument.etp_classified_at =
        Some(UtcNanos::parse("2026-09-21T00:00:00.000000000Z").unwrap());
    context.instrument.quote_currency = Some(mandate_risk::QuoteCurrency::Usd);
    context.instrument.prior_close = Some(Price::parse("100").unwrap());
    context.instrument.median_dollar_volume_20d = Some(usd("90000000"));
    context.instrument.min_order_size = Qty::parse("1").unwrap();
    context.instrument.status_feed_current = true;
    context.market = mandate_risk::MarketSnapshot {
        quote: Some(mandate_risk::SaneQuote {
            bid: Price::parse("99.95").unwrap(),
            ask: Price::parse("100.05").unwrap(),
            at: UtcNanos::parse("2026-09-21T14:59:59.000000000Z").unwrap(),
        }),
        last_trade: None,
        trailing_5m_volume: Some(Qty::parse("100000").unwrap()),
        adv_20d: Some(Qty::parse("1000000").unwrap()),
    };
    context.universe = mandate_risk::WorkingUniverse::Known {
        instruments: [asset].into(),
        pinned: true,
    };
    context
}

/// The varied property is not vacuous: over its own base, the port allows an opening, denies one
/// the account blocks or the fee reservation overspends, holds an exit the agent's pause stops,
/// and defers an owner exit off hours until the owner's bid is confirmed, each as the gate itself
/// does.
#[test]
fn the_allowing_base_reaches_allow_deny_and_hold_through_the_port() {
    let mut buy = proposal("5");
    buy.limit = Price::parse("100").unwrap();
    let verdict = |context: &AdvisoryGateContext, proposal: &Proposal| {
        let decision = evaluate(proposal, context).unwrap();
        assert_eq!(format!("Ok({decision:?})"), direct(proposal, context));
        decision.verdict
    };
    assert_eq!(verdict(&allowing(), &buy), mandate_risk::Verdict::Allow);
    let mut blocked = allowing();
    blocked.account.state = mandate_risk::AccountState::Blocked;
    assert_eq!(verdict(&blocked, &buy), mandate_risk::Verdict::Deny);
    let mut paused = allowing();
    paused.agent.mode = mandate_risk::AgentMode::Paused;
    paused.agent.positions.insert(
        mandate_risk::AssetId::new(ASSET).unwrap(),
        Qty::parse("10").unwrap(),
    );
    let mut sell = buy.clone();
    sell.side = Side::Sell;
    assert_ne!(verdict(&paused, &sell), mandate_risk::Verdict::Allow);
    assert_ne!(
        verdict(&paused, &sell),
        mandate_risk::Verdict::Deny,
        "an exit is never denied"
    );
    let mut costly = allowing();
    costly.order.fee_reservation = mandate_num::Usd::parse("999999.99").unwrap();
    assert_eq!(
        verdict(&costly, &buy),
        mandate_risk::Verdict::Deny,
        "the fee reservation reaches the buying-power check"
    );
    let mut owner_exit = allowing();
    owner_exit.now = UtcNanos::parse("2026-09-21T21:30:00.000000000Z").unwrap();
    owner_exit.order.origin = mandate_risk::Origin::OwnerClose;
    owner_exit.agent.positions.insert(
        mandate_risk::AssetId::new(ASSET).unwrap(),
        Qty::parse("10").unwrap(),
    );
    assert_eq!(
        verdict(&owner_exit, &sell),
        mandate_risk::Verdict::Defer,
        "off hours, an owner exit waits for the owner's confirmed bid"
    );
    owner_exit.order.owner_confirmed_bid = Some(Price::parse("99.95").unwrap());
    assert_ne!(
        verdict(&owner_exit, &sell),
        mandate_risk::Verdict::Defer,
        "the confirmed bid reaches the session check"
    );
}

/// The gate's inputs other than the order, varied so a port that drops, swaps or fixes one of
/// them answers differently from the gate itself.
#[derive(Debug, Clone)]
struct Varied {
    pass: mandate_risk::GatePass,
    risk_mode: mandate_risk::AgentMode,
    account_state: mandate_risk::AccountState,
    agent_mode: mandate_risk::AgentMode,
    eligible: bool,
    collar_breaking_bid: bool,
    participation_today: u32,
    held: u32,
    protective_open: u32,
    side: Side,
    origin: mandate_risk::Origin,
    tif: mandate_risk::TimeInForce,
    extended_hours: bool,
    owner_bid: bool,
    fee: &'static str,
    after_hours: bool,
    fractional: bool,
}

fn origin() -> impl Strategy<Value = mandate_risk::Origin> {
    prop_oneof![
        12 => Just(mandate_risk::Origin::OrderBuilder),
        1 => Just(mandate_risk::Origin::GoalCompletion),
        1 => Just(mandate_risk::Origin::RemovedInstrument),
        1 => Just(mandate_risk::Origin::RiskEngine),
        1 => Just(mandate_risk::Origin::TrimToTarget),
        1 => Just(mandate_risk::Origin::StopWatchdog),
        1 => Just(mandate_risk::Origin::AutomatedKillSwitch),
        1 => Just(mandate_risk::Origin::OwnerClose),
        1 => Just(mandate_risk::Origin::OwnerKillSwitch),
        1 => Just(mandate_risk::Origin::ProtectiveLeg),
    ]
}

fn mode() -> impl Strategy<Value = mandate_risk::AgentMode> {
    prop_oneof![
        4 => Just(mandate_risk::AgentMode::Normal),
        1 => Just(mandate_risk::AgentMode::ExitsOnly),
        1 => Just(mandate_risk::AgentMode::Paused),
        1 => Just(mandate_risk::AgentMode::Stopped),
    ]
}

fn varied() -> impl Strategy<Value = Varied> {
    (
        (
            prop_oneof![
                Just(mandate_risk::GatePass::First),
                Just(mandate_risk::GatePass::BeforeSubmission),
            ],
            mode(),
            prop_oneof![
                4 => Just(mandate_risk::AccountState::Active),
                1 => Just(mandate_risk::AccountState::ClosingOnly),
                1 => Just(mandate_risk::AccountState::Blocked),
            ],
            mode(),
            prop::bool::weighted(0.8),
        ),
        (
            prop::bool::weighted(0.2),
            prop_oneof![4 => Just(0_u32), 1 => Just(49_995)],
            0_u32..12,
            0_u32..6,
            prop_oneof![Just(Side::Buy), Just(Side::Sell)],
        ),
        (
            origin(),
            prop_oneof![
                Just(mandate_risk::TimeInForce::Day),
                Just(mandate_risk::TimeInForce::Gtc),
            ],
            prop::bool::weighted(0.2),
            prop::bool::weighted(0.2),
            prop_oneof![Just("0"), Just("0.05"), Just("999999.99")],
        ),
        (prop::bool::weighted(0.3), prop::bool::weighted(0.3)),
    )
        .prop_map(
            |(
                (pass, risk_mode, account_state, agent_mode, eligible),
                (collar_breaking_bid, participation_today, held, protective_open, side),
                (origin, tif, extended_hours, owner_bid, fee),
                (after_hours, fractional),
            )| Varied {
                pass,
                risk_mode,
                account_state,
                agent_mode,
                eligible,
                collar_breaking_bid,
                participation_today,
                held,
                protective_open,
                side,
                origin,
                tif,
                extended_hours,
                owner_bid,
                fee,
                after_hours,
                fractional,
            },
        )
}

fn varied_context(varied: &Varied) -> AdvisoryGateContext {
    let asset = mandate_risk::AssetId::new(ASSET).unwrap();
    let mut context = allowing();
    context.order.origin = varied.origin;
    context.order.tif = varied.tif;
    context.order.extended_hours = varied.extended_hours;
    context.order.owner_confirmed_bid = varied.owner_bid.then(|| Price::parse("99.95").unwrap());
    context.order.fee_reservation = mandate_num::Usd::parse(varied.fee).unwrap();
    if varied.after_hours {
        context.now = UtcNanos::parse("2026-09-21T21:30:00.000000000Z").unwrap();
    }
    context.instrument.fractionable = varied.fractional;
    context.pass = varied.pass;
    context.risk.agent_mode = varied.risk_mode;
    context.account.state = varied.account_state;
    context.agent.mode = varied.agent_mode;
    context.instrument.status_active = varied.eligible;
    if varied.collar_breaking_bid {
        context.market.quote = Some(mandate_risk::SaneQuote {
            bid: Price::parse("80").unwrap(),
            ask: Price::parse("80.1").unwrap(),
            at: UtcNanos::parse("2026-09-21T14:59:59.000000000Z").unwrap(),
        });
    }
    if varied.participation_today > 0 {
        context.conduct.participation_today.insert(
            asset.clone(),
            Qty::parse(&varied.participation_today.to_string()).unwrap(),
        );
    }
    if varied.held > 0 {
        context
            .agent
            .positions
            .insert(asset.clone(), Qty::parse(&varied.held.to_string()).unwrap());
    }
    if varied.protective_open > 0 {
        let id = mandate_risk::ClientOrderId(9);
        context.agent.working_orders.insert(id);
        context.account.working_orders.insert(
            id,
            mandate_risk::WorkingOrder {
                agent: context.agent.agent,
                instrument: asset,
                side: Side::Sell,
                max_cost: mandate_num::Usd::ZERO,
                open_qty: Qty::parse(&varied.protective_open.to_string()).unwrap(),
                protective: true,
                opening: false,
                submitted_on: mandate_time::Date::parse("2026-09-21").unwrap(),
            },
        );
    }
    context
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

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

    /// The port hands the gate every input as its host gave it: over varied passes, account and
    /// agent modes, account states, eligibility, quotes, the day's participation, positions,
    /// protective orders, sides, origins, times in force, extended hours, owner-confirmed bids, fee
    /// reservations, the session and fractional quantities, the shared evaluation and the gate
    /// called directly agree.
    #[test]
    fn the_port_answers_as_the_gate_over_every_input(varied in varied(), units in 1_u32..12) {
        let qty = if varied.fractional { format!("{units}.5") } else { units.to_string() };
        let mut proposal = proposal(&qty);
        proposal.side = varied.side;
        proposal.limit = Price::parse("100").unwrap();
        let context = varied_context(&varied);
        let theirs = direct(&proposal, &context);
        match evaluate(&proposal, &context) {
            Ok(decision) => prop_assert_eq!(format!("Ok({decision:?})"), theirs),
            Err(PortError::Gate(error)) => prop_assert_eq!(format!("Err({error:?})"), theirs),
            Err(other) => prop_assert!(false, "{other}"),
        }
    }

    /// The advisory port never changes the advisory pass's answer: over every input and every
    /// set of deferred checks, the shared evaluation and `mandate-risk`'s called directly agree.
    #[test]
    fn the_port_answers_as_the_advisory_pass_over_every_deferral(
        varied in varied(),
        units in 1_u32..12,
        deferred in proptest::collection::btree_set(
            prop_oneof![
                Just(Deferred::Eligibility),
                Just(Deferred::LiquidityFloor),
                Just(Deferred::Participation),
                Just(Deferred::DayTradeBudget),
            ],
            0..=4,
        ),
    ) {
        let qty = if varied.fractional { format!("{units}.5") } else { units.to_string() };
        let mut proposal = proposal(&qty);
        proposal.side = varied.side;
        proposal.limit = Price::parse("100").unwrap();
        let context = varied_context(&varied);
        let theirs = direct_deferring(&proposal, &context, &deferred);
        match evaluate_deferring(&proposal, &context, &deferred) {
            Ok(advisory) => prop_assert_eq!(format!("Ok({advisory:?})"), theirs),
            Err(PortError::Gate(error)) => prop_assert_eq!(format!("Err({error:?})"), theirs),
            Err(other) => prop_assert!(false, "{other}"),
        }
    }
}
