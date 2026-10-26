//! DEC-2380 item 2: the executor's binding gate reads the deployment's reviewed limits, rule set
//! and fees, the journal's account, mark and clock, and the asset record, the account's day-trade
//! rules and liquidity, which the advisory gate defers to it (DEC-2378 option b). A missing fact,
//! an account that is not flat or not active, a regime whose day trades could bind, another agent
//! or instrument, or an order that reduces risk is answered with no input, so an opening is held
//! and an exit keeps the core's own decision; and the eligibility, liquidity and participation
//! facts the gate reads each refuse the opening when they say so.

use std::time::Duration;

use mandate_accounting::{AccountType, InstrumentId, Side};
use mandate_canon::Digest;
use mandate_domain::{AssetClass as DomainAssetClass, AssetId as DomainAssetId};
use mandate_executor::{
    AgentId, BindingGateConfigRefs, BindingGateInput, BindingGateRequest, ExecutorConfig, Purpose,
    TimeInForce,
};
use mandate_executord::gate::{AccountRules, AssetRecord, DeployedGate, Liquidity, OrderFacts};
use mandate_gate_context::{AccountFacts, Facts, Mark};
use mandate_gate_inputs::control::SnapshotExchange;
use mandate_gate_inputs::{Reviewed, ReviewedInstrument};
use mandate_num::{Fraction, Price, Qty, ShareIncrement, Usd};
use mandate_risk::{
    DayTradeRegime, EtpClass, Exchange, GateConfig, GateInput, GatePass, Origin, ProposedKind,
    ProposedOrder, ReasonCode, SaneQuote, TimeInForce as GateTimeInForce, Verdict, evaluate,
};
use mandate_time::{Date, TradingCalendar, UtcNanos};

type Error = Box<dyn std::error::Error>;

const MANDATE: &str = include_str!("../../mandate-shell/tests/fixtures/tracer/mandate.json");
const ASSET: &str = "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415";
const AGENT: &str = "agent_spy";
const NOW: &str = "2026-09-28T17:00:00Z";
const VERSION: &str = "sha256:5555555555555555555555555555555555555555555555555555555555555555";

fn usd(text: &str) -> Result<Usd, Error> {
    Ok(Usd::parse(text)?)
}

fn price(text: &str) -> Result<Price, Error> {
    Ok(Price::parse(text)?)
}

fn qty(text: &str) -> Result<Qty, Error> {
    Ok(Qty::parse(text)?)
}

fn fraction(text: &str) -> Result<Fraction, Error> {
    Ok(Fraction::parse(text)?)
}

fn at(text: &str) -> Result<UtcNanos, Error> {
    Ok(UtcNanos::parse_rfc3339(text)?)
}

/// The tracer's reviewed rule set (`rule-set.json`'s `gate_config`).
fn gate_config() -> Result<GateConfig, Error> {
    Ok(GateConfig {
        price_floor: usd("5")?,
        liquidity_floor_usd: usd("1000000")?,
        crypto_liquidity_floor_usd: usd("1000000")?,
        collar_liquid_threshold_usd: usd("50000000")?,
        collar_liquid_x: fraction("0.01")?,
        collar_other_x: fraction("0.02")?,
        collar_crypto_x: fraction("0.02")?,
        collar_passive_band: fraction("0.2")?,
        opposite_fill_interval_s: 60,
        min_resting_time_s: 2,
        order_to_fill_max: 10,
        order_to_fill_min_orders: 20,
        order_size_participation: fraction("0.05")?,
        daily_participation: fraction("0.05")?,
        close_window_minutes: 10,
        legacy_pdt_equity_threshold: usd("25000")?,
        etp_classification_max_age_s: 604_800,
    })
}

/// The tracer's paper deployment of its pinned instrument, reviewed.
fn reviewed() -> Result<Reviewed, Error> {
    let value = mandate_canon::parse(MANDATE.trim().as_bytes())?;
    let mandate = mandate_spec::Mandate::parse(&value).map_err(|e| format!("{e:?}"))?;
    let calendar = TradingCalendar::new(
        Date::parse("2026-01-01")?,
        Date::parse("2026-12-31")?,
        [],
        [],
    )?;
    Ok(Reviewed {
        mandate,
        model_id: "quant.ma_crossover".to_owned(),
        model_version: "1.0.0".to_owned(),
        model_hash: Digest::of(b"model"),
        fees: mandate_executor::paper_only_fee_config("paper", calendar, "2026-01-01")?,
        instrument: ReviewedInstrument {
            asset_id: DomainAssetId::parse(ASSET).map_err(|e| format!("{e:?}"))?,
            symbol: InstrumentId::new("AAPL")?,
            asset_class: DomainAssetClass::UsEquity,
            exchange: SnapshotExchange::Nasdaq,
            gate_exchange: Exchange::Nasdaq,
            increment: ShareIncrement::Whole,
            increment_qty: qty("1")?,
            etp: EtpClass::Plain,
            etp_classified_at: at("2026-09-27T00:00:00Z")?,
        },
        quote_max_age: Duration::from_secs(10),
        config_refs: BindingGateConfigRefs::complete(
            "sha256:fee",
            "sha256:cal",
            "sha256:ins",
            "sha256:set",
            VERSION,
        ),
        gate_config: gate_config()?,
        executor_config: ExecutorConfig {
            max_intent_age_s: 120,
            unknown_absent_lookups: 3,
            unknown_absent_window_s: 15,
            protective_replace_buffer_trading_days: 5,
            restriction_403_threshold: 3,
            bracket_partial_fill_timeout_s: 60,
            max_unprotected_s: 60,
            stop_watchdog_s: 60,
            exit_step_s: 5,
            gtc_expiry_days: 90,
        },
    })
}

fn gate() -> Result<DeployedGate, Error> {
    Ok(DeployedGate::new(AGENT, &reviewed()?, "fixture")?)
}

/// An active, unblocked paper account at noon, flat, quoted 99.95 by 100.05, with every fact the
/// gate reads: an eligible asset, the legacy regime above its threshold, and deep liquidity.
fn facts() -> Result<OrderFacts, Error> {
    let now = at(NOW)?;
    Ok(OrderFacts {
        journaled: Facts {
            now,
            mandate_version: VERSION.to_owned(),
            account: AccountFacts {
                status: "ACTIVE".to_owned(),
                crypto_status: "INACTIVE".to_owned(),
                trading_blocked: false,
                account_blocked: false,
                trade_suspended_by_user: false,
                multiplier: 2,
                equity: usd("100000")?,
                cash: usd("100000")?,
                buying_power: usd("200000")?,
                non_marginable_buying_power: usd("100000")?,
                at: now,
            },
            mark: Mark {
                bid: price("99.95")?,
                at: now,
            },
        },
        quote: SaneQuote {
            bid: price("99.95")?,
            ask: price("100.05")?,
            at: at("2026-09-28T16:59:59Z")?,
        },
        asset: Some(AssetRecord {
            active: true,
            tradable: true,
            fractionable: true,
            ipo: false,
            ptp_no_exception: false,
        }),
        rules: Some(AccountRules {
            account_type: AccountType::Margin,
            regime: DayTradeRegime::LegacyPdt,
            prior_close_equity: usd("100000")?,
        }),
        liquidity: Some(Liquidity {
            prior_close: price("100")?,
            median_dollar_volume_20d: usd("5000000000")?,
            adv_20d: qty("50000000")?,
            trailing_5m_volume: qty("1000000")?,
        }),
        flat: true,
    })
}

/// The binding gate's input for `agent`'s buy of two shares of `instrument` at 100.05 for
/// `purpose`.
fn input_for(
    gate: &DeployedGate,
    facts: &OrderFacts,
    agent: &str,
    instrument: &str,
    purpose: Purpose,
) -> Result<Option<BindingGateInput>, Error> {
    let agent = AgentId(agent.to_owned());
    let instrument = InstrumentId::new(instrument)?;
    Ok(gate.input(
        facts,
        &BindingGateRequest {
            agent: &agent,
            instrument: &instrument,
            side: Side::Buy,
            qty: qty("2")?,
            limit: price("100.05")?,
            purpose,
            tif: TimeInForce::Day,
            protection: None,
        },
    ))
}

fn opening(facts: &OrderFacts) -> Result<Option<BindingGateInput>, Error> {
    input_for(&gate()?, facts, AGENT, ASSET, Purpose::Open)
}

/// The gate's verdict and reason on the opening, as the executor evaluates its snapshot.
fn verdict(snapshot: &BindingGateInput) -> Result<(Verdict, Option<ReasonCode>), Error> {
    let proposed = ProposedOrder {
        instrument: snapshot.asset.clone(),
        side: Side::Buy,
        qty: qty("2")?,
        limit_price: price("100.05")?,
        kind: ProposedKind::Plain,
        tif: GateTimeInForce::Day,
        extended_hours: false,
        origin: Origin::OrderBuilder,
        owner_confirmed_bid: None,
        client_order_id: snapshot.gate_client_order_id,
        fee_reservation: snapshot.fee_reservation,
    };
    let decision = evaluate(&GateInput {
        now: snapshot.now,
        pass: GatePass::First,
        config: &snapshot.config,
        mandate: &snapshot.mandate,
        risk: &snapshot.risk,
        account: &snapshot.account,
        agent: &snapshot.agent,
        instrument: &snapshot.instrument,
        market: &snapshot.market,
        conduct: &snapshot.conduct,
        universe: &snapshot.universe,
        proposed: &proposed,
    })?;
    Ok((decision.verdict, decision.reason))
}

#[test]
fn an_opening_on_every_fact_is_allowed() -> Result<(), Error> {
    let snapshot = opening(&facts()?)?.ok_or("an input")?;
    assert_eq!(snapshot.now, at(NOW)?);
    assert_eq!(snapshot.asset.as_str(), ASSET);
    assert_eq!(
        snapshot.config_refs.mandate_version.as_deref(),
        Some(VERSION)
    );
    assert_eq!(snapshot.account.equity, usd("100000")?);
    assert_eq!(snapshot.account.prior_close_equity, usd("100000")?);
    assert_eq!(snapshot.account.model_buying_power, usd("100000")?);
    assert_eq!(snapshot.feed, "fixture");
    assert!(!snapshot.account.crypto_active);
    let mut crypto = facts()?;
    crypto.journaled.account.crypto_status = "ACTIVE".to_owned();
    assert!(opening(&crypto)?.ok_or("an input")?.account.crypto_active);
    assert!(snapshot.fee_reservation > Usd::ZERO);
    assert_eq!(verdict(&snapshot)?, (Verdict::Allow, None));
    Ok(())
}

#[test]
fn a_missing_fact_holds_the_opening() -> Result<(), Error> {
    let base = facts()?;
    let mut no_asset = base.clone();
    no_asset.asset = None;
    let mut no_rules = base.clone();
    no_rules.rules = None;
    let mut no_liquidity = base;
    no_liquidity.liquidity = None;
    for (case, facts) in [
        ("asset", no_asset),
        ("rules", no_rules),
        ("liquidity", no_liquidity),
    ] {
        assert!(opening(&facts)?.is_none(), "{case}");
    }
    Ok(())
}

#[test]
fn an_account_not_flat_or_not_active_holds_the_opening() -> Result<(), Error> {
    let base = facts()?;
    let mut holding = base.clone();
    holding.flat = false;
    assert!(opening(&holding)?.is_none(), "not flat");
    for case in 0..4 {
        let mut changed = base.clone();
        let account = &mut changed.journaled.account;
        match case {
            0 => account.status = "ACCOUNT_UPDATED".to_owned(),
            1 => account.trading_blocked = true,
            2 => account.account_blocked = true,
            _ => account.trade_suspended_by_user = true,
        }
        assert!(opening(&changed)?.is_none(), "{case}");
    }
    Ok(())
}

#[test]
fn a_regime_whose_day_trades_could_bind_holds_the_opening() -> Result<(), Error> {
    let base = facts()?;
    let mut at_threshold = base.clone();
    if let Some(rules) = at_threshold.rules.as_mut() {
        rules.prior_close_equity = usd("25000")?;
    }
    assert!(
        opening(&at_threshold)?.is_some(),
        "the threshold itself does not bind"
    );
    let mut below = base.clone();
    if let Some(rules) = below.rules.as_mut() {
        rules.prior_close_equity = usd("24999.99")?;
    }
    assert!(opening(&below)?.is_none(), "below the threshold");
    let mut intraday = base;
    if let Some(rules) = intraday.rules.as_mut() {
        rules.regime = DayTradeRegime::IntradayMargin {
            maintenance_excess: usd("50000")?,
        };
    }
    assert!(opening(&intraday)?.is_none(), "intraday margin");
    Ok(())
}

#[test]
fn only_the_deployed_agents_opening_on_its_instrument_is_answered() -> Result<(), Error> {
    let gate = gate()?;
    let facts = facts()?;
    assert!(input_for(&gate, &facts, AGENT, ASSET, Purpose::Increase)?.is_some());
    assert!(input_for(&gate, &facts, "agent_other", ASSET, Purpose::Open)?.is_none());
    assert!(
        input_for(
            &gate,
            &facts,
            AGENT,
            "b0b6dd9d-8b9b-48a9-ba46-b9d54906e416",
            Purpose::Open
        )?
        .is_none()
    );
    for purpose in [
        Purpose::RiskExit,
        Purpose::OwnerExit,
        Purpose::DiscretionaryExit,
        Purpose::Protective,
        Purpose::Flatten,
    ] {
        assert!(
            input_for(&gate, &facts, AGENT, ASSET, purpose)?.is_none(),
            "{purpose:?}"
        );
    }
    Ok(())
}

#[test]
fn each_eligibility_fact_refuses_the_opening() -> Result<(), Error> {
    for case in 0..2 {
        let mut changed = facts()?;
        if let Some(asset) = changed.asset.as_mut() {
            match case {
                0 => asset.active = false,
                _ => asset.tradable = false,
            }
        }
        let snapshot = opening(&changed)?.ok_or("an input")?;
        assert_eq!(
            verdict(&snapshot)?,
            (Verdict::Deny, Some(ReasonCode::NotInWorkingUniverse)),
            "{case}"
        );
    }
    Ok(())
}

#[test]
fn the_liquidity_floor_and_participation_refuse_the_opening() -> Result<(), Error> {
    let mut thin = facts()?;
    if let Some(liquidity) = thin.liquidity.as_mut() {
        liquidity.median_dollar_volume_20d = usd("999999")?;
    }
    let snapshot = opening(&thin)?.ok_or("an input")?;
    assert_eq!(
        verdict(&snapshot)?,
        (Verdict::Deny, Some(ReasonCode::BelowLiquidityFloor))
    );
    let mut quiet = facts()?;
    if let Some(liquidity) = quiet.liquidity.as_mut() {
        liquidity.trailing_5m_volume = qty("39")?;
    }
    let snapshot = opening(&quiet)?.ok_or("an input")?;
    assert_eq!(
        verdict(&snapshot)?,
        (Verdict::Deny, Some(ReasonCode::ConductLimitBreached)),
        "two shares exceed 5 % of 39"
    );
    let mut cheap = facts()?;
    if let Some(liquidity) = cheap.liquidity.as_mut() {
        liquidity.prior_close = price("4.99")?;
    }
    let snapshot = opening(&cheap)?.ok_or("an input")?;
    assert_eq!(
        verdict(&snapshot)?,
        (Verdict::Deny, Some(ReasonCode::BelowPriceFloor))
    );
    Ok(())
}
