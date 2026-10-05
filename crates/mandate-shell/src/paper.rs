//! Trusted, paper-only assembly for E7-7's one AAPL migration run (DEC-466).
//!
//! The values are deliberately narrower than a general deployment configuration. The loader
//! accepts only the reviewed E7-7 artifact shapes, binds their exact bytes into executor config
//! references, and refuses every other instrument, model, environment, or ruleset.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::rc::Rc;

use mandate_accounting::{AssetClass, InstrumentId};
use mandate_builder::{
    AccountSnapshot as BuilderAccountSnapshot, ActionContext, Market as BuilderMarket, RequestedBy,
    RiskContext as BuilderRiskContext,
};
use mandate_canon::{Digest, Value};
use mandate_domain::{AssetClass as DomainAssetClass, AssetId, MarketSession, Purpose};
use mandate_executor::{
    BindingGateConfigRefs, BindingGateInput, BindingGateRequest, BindingGateSource, ExecutorConfig,
    InstrumentSnapshot, MandateVersion, MandateView as ExecutorMandateView,
};
use mandate_num::{
    CostBasis, FeeRate, Fraction, MarkPrice, Price, Qty, Ratio, ShareIncrement, Signed,
    SizeFraction, Unit, Usd,
};
use mandate_risk::spec_types::{GoalState, RiskLimits};
use mandate_risk::{
    AccountSnapshot, AccountState, AgentId as GateAgentId, AgentMode, AgentSnapshot,
    AssetId as GateAssetId, ClientOrderId as GateOrderId, ConductState, DayTradeLedger,
    DayTradeRegime, EtpClass, Exchange, GateConfig, GatePass,
    InstrumentSnapshot as GateInstrumentSnapshot, MarketSnapshot, Origin, ProposedKind,
    QuoteCurrency, RiskSnapshot, SaneQuote, TimeInForce as GateTimeInForce,
    ValidatedMandate as GateMandate, WorkingUniverse,
};
use mandate_runtime::{AgentId, OrderExecution, ProtectionPrices, TimeInForce};
use mandate_spec::ValidationContext;
use mandate_spec::document::ProvenanceMap;
use mandate_time::{Date, TradingCalendar, UtcNanos};

use crate::adapters::{
    AdvisoryGateContext, AdvisoryOrderFacts, BuilderContext, DecisionContext, ExecutorContext,
    RunContext,
};
use crate::envelope::{IdSpace, Ids};
use crate::error::Cause;

const INSTRUMENT_ID: &str = "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415";
const MODEL_ID: &str = "quant.ma_crossover";
const MODEL_VERSION: &str = "1.0.0";
const BID: &str = "255.1";
const ASK: &str = "255.2";
const STOP: &str = "242.44";
const TAKE_PROFIT: &str = "280.72";

/// The contexts the shipping binary must supply before it can read the broker or decide.
pub struct Contexts {
    pub executor: ExecutorContext,
    pub run: RunContext,
}

/// Loads and verifies the E7-7 effective-dated artifacts, then assembles the two trusted contexts.
///
/// This is intentionally one-run migration assembly, not a general configuration loader. DEC-466
/// keeps the constants here identical to the independently reviewed E7-7 fixture while every
/// artifact byte is bound into the executor's journal references.
pub fn load_contexts(
    mandate_path: &Path,
    config_dir: &Path,
    now: UtcNanos,
    agent: &AgentId,
) -> Result<Contexts, Cause> {
    let mandate_bytes = fs::read(mandate_path).map_err(|_| absent("the mandate artifact"))?;
    let mandate_value =
        mandate_canon::parse(&mandate_bytes).map_err(|_| absent("a canonical mandate artifact"))?;
    let mandate = mandate_spec::Mandate::parse(&mandate_value)
        .map_err(|_| absent("a parsed mandate artifact"))?;
    if mandate.environment != mandate_domain::Environment::Paper {
        return Err(absent("a paper mandate artifact"));
    }
    let [pinned] = mandate.universe.pinned_instruments.as_slice() else {
        return Err(absent("the one AAPL instrument"));
    };
    if pinned.asset_id.as_str() != INSTRUMENT_ID
        || pinned.symbol != "AAPL"
        || pinned.asset_class != DomainAssetClass::UsEquity
    {
        return Err(absent("the reviewed AAPL instrument"));
    }
    let [model] = mandate.behavior.signal_models.as_slice() else {
        return Err(absent("the one reviewed signal model"));
    };
    let model_bytes = fs::read(config_dir.join("model-artifact.json"))
        .map_err(|_| absent("the model artifact"))?;
    if model.id.as_str() != MODEL_ID
        || model.version != MODEL_VERSION
        || model.content_hash != Digest::of(&model_bytes)
    {
        return Err(absent("the reviewed model artifact"));
    }

    let fee = artifact(config_dir, "fee-config.json")?;
    require_text(&fee, "environment", "paper")?;
    require_text(&fee, "schedule", "conservative_v1")?;
    let effective_from = required_text(&fee, "effective_from")?;

    let calendar_value = artifact(config_dir, "trading-calendar.json")?;
    let first = Date::parse(required_text(&calendar_value, "first")?)
        .map_err(|_| absent("the trading calendar's first date"))?;
    let last = Date::parse(required_text(&calendar_value, "last")?)
        .map_err(|_| absent("the trading calendar's last date"))?;
    require_empty_array(&calendar_value, "holidays")?;
    require_empty_array(&calendar_value, "special_sessions")?;
    let calendar = TradingCalendar::new(first, last, [], [])
        .map_err(|_| absent("the effective trading calendar"))?;
    let fees = mandate_executor::paper_only_fee_config("paper", calendar, effective_from)
        .map_err(Cause::Executor)?;

    let instrument = artifact(config_dir, "instrument-snapshot.json")?;
    require_text(&instrument, "instrument_id", INSTRUMENT_ID)?;
    require_text(&instrument, "symbol", "AAPL")?;
    require_text(&instrument, "asset_class", "us_equity")?;
    require_text(&instrument, "exchange", "nasdaq")?;
    require_text(&instrument, "increment", "whole")?;

    let rules = artifact(config_dir, "rule-set.json")?;
    require_text(&rules, "gate", "trading-domain-9.1")?;
    require_text(&rules, "ruleset_version", "v1")?;
    require_text(&rules, "story", "E7-7")?;

    let canonical_mandate = mandate
        .canonical_bytes()
        .map_err(|_| absent("the mandate's canonical artifact"))?;
    let config_refs = config_refs(config_dir, canonical_mandate)?;
    let source = Rc::new(TrustedPaperContext {
        config_refs: config_refs.clone(),
        now,
        agent: agent.0.clone(),
    });
    let executor = ExecutorContext::new(
        Rc::new(Ids {
            space: IdSpace::Account,
        }),
        source.clone(),
        source.clone(),
        source,
        ExecutorConfig::PROPOSED,
        fees,
    );
    Ok(Contexts {
        executor,
        run: run_context(now, model.content_hash, config_refs)?,
    })
}

fn artifact(config_dir: &Path, name: &str) -> Result<Value, Cause> {
    let bytes =
        fs::read(config_dir.join(name)).map_err(|_| absent("a required config artifact"))?;
    mandate_canon::parse(&bytes).map_err(|_| absent("a canonical config artifact"))
}

fn required_text<'a>(value: &'a Value, name: &'static str) -> Result<&'a str, Cause> {
    value
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| absent(name))
}

fn require_text(value: &Value, name: &'static str, expected: &str) -> Result<(), Cause> {
    if required_text(value, name)? == expected {
        Ok(())
    } else {
        Err(absent(name))
    }
}

fn require_empty_array(value: &Value, name: &'static str) -> Result<(), Cause> {
    match value.get(name).and_then(Value::as_array) {
        Some([]) => Ok(()),
        Some(_) | None => Err(absent(name)),
    }
}

fn config_refs(config_dir: &Path, mandate_bytes: Vec<u8>) -> Result<BindingGateConfigRefs, Cause> {
    let read = |name: &'static str| {
        fs::read(config_dir.join(name)).map_err(|_| absent("a required config artifact"))
    };
    let reference = |bytes: &[u8]| format!("sha256:{}", Digest::of(bytes).to_hex());
    let fee = read("fee-config.json")?;
    let calendar = read("trading-calendar.json")?;
    let instrument = read("instrument-snapshot.json")?;
    let rules = read("rule-set.json")?;
    Ok(BindingGateConfigRefs::complete(
        reference(&fee),
        reference(&calendar),
        reference(&instrument),
        reference(&rules),
        reference(&mandate_bytes),
    ))
}

fn run_context(
    now: UtcNanos,
    model_hash: Digest,
    config_refs: BindingGateConfigRefs,
) -> Result<RunContext, Cause> {
    let instrument = AssetId::parse(INSTRUMENT_ID).map_err(|_| absent("the AAPL asset id"))?;
    let equity = usd("100000")?;
    let order = usd(ASK)?;
    let builder = BuilderContext {
        account: BuilderAccountSnapshot {
            agent_equity: usd("1000")?,
            position_qty: Qty::ZERO,
            cost_basis: CostBasis::ZERO,
            risk_mark: MarkPrice::parse(BID)?,
            gross_usd: Usd::ZERO,
            working_opening_cost: Usd::ZERO,
            goal_spent_usd: Usd::ZERO,
        },
        market: BuilderMarket {
            instrument: instrument.clone(),
            asset_class: DomainAssetClass::UsEquity,
            session: MarketSession::Regular,
            in_close_window: false,
            bid: Price::parse(BID)?,
            ask: Price::parse(ASK)?,
            increment: Qty::parse("1")?,
            min_order_usd: usd("1")?,
            fee_rate_cash: FeeRate::parse("0")?,
            fee_rate_asset: FeeRate::parse("0")?,
        },
        risk: BuilderRiskContext {
            size_factor: SizeFraction::ONE,
            drawdown: Unit::ZERO,
            daily_pnl_fraction: Signed::ZERO,
            position_pnl_fraction: Signed::ZERO,
            bought_today_usd: Usd::ZERO,
            has_prior_fill: false,
            new_instrument: false,
            thesis_confidence: Unit::ZERO,
            risk_day: now.date(),
        },
        action: ActionContext {
            purpose: Purpose::Open,
            order_usd: order,
            combined_score: Unit::ONE,
            instrument,
            asset_class: DomainAssetClass::UsEquity,
            session: MarketSession::Regular,
            first_trade_in_instrument: true,
            new_instrument: false,
            thesis_confidence: Unit::ZERO,
            drawdown: Unit::ZERO,
            daily_pnl_fraction: Signed::ZERO,
            position_usd_after: order,
            gross_usd_after: order,
            bought_today_usd: order,
            position_pnl_fraction: Signed::ZERO,
            requested_by: RequestedBy::Agent,
            risk_day: now.date(),
        },
        model_content_hashes: BTreeMap::from([(
            (MODEL_ID.to_owned(), MODEL_VERSION.to_owned()),
            model_hash,
        )]),
        execution: OrderExecution {
            asset_class: DomainAssetClass::UsEquity,
            tif: TimeInForce::Day,
            protection_required: true,
            protection: Some(ProtectionPrices {
                stop: Price::parse(STOP)?,
                take_profit: Some(Price::parse(TAKE_PROFIT)?),
            }),
        },
    };
    Ok(RunContext {
        validation: ValidationContext {
            account_equity_usd: equity,
            other_allocations_usd: Usd::ZERO,
            validation_date: now.date(),
            registry: None,
            provenance: ProvenanceMap::default(),
            workspace_users: 1,
            approver_users: 1,
            independent_approval_required: false,
            disclosures_accepted: BTreeSet::new(),
            instrument_groups: BTreeMap::new(),
            claimed_by_other_agents: BTreeSet::new(),
            connection_environment: Some(mandate_domain::Environment::Paper),
            connection_loss_carry_usd: Usd::ZERO,
            eligibility_failures: BTreeSet::new(),
            previous_version: None,
            current_mandate_version: None,
        },
        policies: Vec::new(),
        author: "founder".to_owned(),
        restricted_instruments: BTreeSet::new(),
        decision: Some(DecisionContext {
            builder: Some(builder),
            gate: Some(advisory_gate_context(now, config_refs)?),
        }),
    })
}

struct TrustedPaperContext {
    config_refs: BindingGateConfigRefs,
    now: UtcNanos,
    agent: String,
}

impl ExecutorMandateView for TrustedPaperContext {
    fn version(&self, _agent: &mandate_executor::AgentId) -> Option<MandateVersion> {
        self.config_refs.mandate_version.clone().map(MandateVersion)
    }

    fn crypto_stop_limit_offset(&self, _agent: &mandate_executor::AgentId) -> Option<Fraction> {
        None
    }

    fn covers(&self, agent: &mandate_executor::AgentId, instrument: &InstrumentId) -> bool {
        agent.0 == self.agent && instrument.as_str() == INSTRUMENT_ID
    }
}

impl InstrumentSnapshot for TrustedPaperContext {
    fn asset_class(&self, instrument: &InstrumentId) -> Option<AssetClass> {
        (instrument.as_str() == INSTRUMENT_ID).then_some(AssetClass::UsEquity)
    }

    fn increment(&self, instrument: &InstrumentId) -> Option<ShareIncrement> {
        (instrument.as_str() == INSTRUMENT_ID).then_some(ShareIncrement::Whole)
    }

    fn exit_tier(&self, _instrument: &InstrumentId) -> Option<mandate_executor::ExitTier> {
        None
    }
}

impl BindingGateSource for TrustedPaperContext {
    fn input(&self, request: &BindingGateRequest<'_>) -> Option<BindingGateInput> {
        match gate_input(self.now, self.config_refs.clone(), request) {
            Ok(input) => Some(input),
            Err(_) => None,
        }
    }
}

fn advisory_gate_context(
    now: UtcNanos,
    config_refs: BindingGateConfigRefs,
) -> Result<AdvisoryGateContext, Cause> {
    let agent = mandate_executor::AgentId("tracer-aapl".to_owned());
    let instrument = InstrumentId::new(INSTRUMENT_ID).map_err(|_| absent("the AAPL instrument"))?;
    let request = BindingGateRequest {
        agent: &agent,
        instrument: &instrument,
        side: mandate_accounting::Side::Buy,
        qty: Qty::parse("1")?,
        limit: Price::parse(ASK)?,
        purpose: mandate_executor::Purpose::Open,
        tif: mandate_executor::TimeInForce::Day,
        protection: None,
    };
    let trusted = gate_input(now, config_refs, &request)?;
    Ok(AdvisoryGateContext {
        now: trusted.now,
        pass: GatePass::First,
        config: trusted.config,
        mandate: trusted.mandate,
        risk: trusted.risk,
        account: trusted.account,
        agent: trusted.agent,
        instrument: trusted.instrument,
        market: trusted.market,
        conduct: trusted.conduct,
        universe: trusted.universe,
        order: AdvisoryOrderFacts {
            kind: ProposedKind::Plain,
            tif: GateTimeInForce::Day,
            extended_hours: false,
            origin: Origin::OrderBuilder,
            owner_confirmed_bid: None,
            client_order_id: GateOrderId(1),
            fee_reservation: Usd::ZERO,
        },
    })
}

fn gate_input(
    now: UtcNanos,
    config_refs: BindingGateConfigRefs,
    request: &BindingGateRequest<'_>,
) -> Result<BindingGateInput, Cause> {
    let asset = GateAssetId::new(request.instrument.as_str())
        .map_err(|_| absent("the risk gate's AAPL instrument"))?;
    let equity = usd("100000")?;
    let liquidity = usd("1000000")?;
    let volume = Qty::parse("1000000")?;
    let positions = BTreeMap::new();
    let gate_agent = GateAgentId(1);
    Ok(BindingGateInput {
        config_refs,
        now,
        config: GateConfig {
            price_floor: usd("0.01")?,
            liquidity_floor_usd: Usd::ZERO,
            crypto_liquidity_floor_usd: Usd::ZERO,
            collar_liquid_threshold_usd: Usd::ZERO,
            collar_liquid_x: Fraction::parse("0.5")?,
            collar_other_x: Fraction::parse("0.5")?,
            collar_crypto_x: Fraction::parse("0.5")?,
            collar_passive_band: Fraction::parse("0.5")?,
            opposite_fill_interval_s: 0,
            min_resting_time_s: 0,
            order_to_fill_max: u32::MAX,
            order_to_fill_min_orders: u32::MAX,
            order_size_participation: Fraction::ONE,
            daily_participation: Fraction::ONE,
            close_window_minutes: 0,
            legacy_pdt_equity_threshold: Usd::ZERO,
            etp_classification_max_age_s: u32::MAX,
        },
        mandate: GateMandate::from_validated_parts(
            RiskLimits {
                max_position_usd: usd("1000")?,
                max_position_fraction: Fraction::ONE,
                max_order_usd: usd("300")?,
                max_gross_exposure_usd: usd("1000")?,
                max_orders_per_day: 50,
                reentry_cooldown_s: 3600,
                rebalance_band: Fraction::parse("0.05")?,
                breach_confirm_s: 60,
                drawdown_ladder: Vec::new(),
            },
            GoalState::Running,
            true,
            true,
        ),
        risk: RiskSnapshot {
            agent_equity: usd("1000")?,
            high_water_mark: usd("1000")?,
            day_start_equity: usd("1000")?,
            capital_base: usd("1000")?,
            inherited_loss: Usd::ZERO,
            latched: BTreeSet::new(),
            active_rungs: BTreeMap::new(),
            size_factor: Ratio::parse("1")?,
            agent_mode: AgentMode::Normal,
        },
        account: AccountSnapshot {
            account_type: mandate_accounting::AccountType::Margin,
            state: AccountState::Active,
            crypto_active: true,
            regime: DayTradeRegime::IntradayMargin {
                maintenance_excess: equity,
            },
            equity,
            prior_close_equity: equity,
            model_buying_power: equity,
            broker_buying_power: equity,
            broker_non_marginable_buying_power: equity,
            positions: positions.clone(),
            market_values: BTreeMap::new(),
            working_orders: BTreeMap::new(),
            unknown_orders: BTreeSet::new(),
            related_account_resting: BTreeMap::new(),
        },
        agent: AgentSnapshot {
            agent: gate_agent,
            mode: AgentMode::Normal,
            instrument_restrictions: BTreeMap::new(),
            positions,
            market_values: BTreeMap::new(),
            working_orders: BTreeSet::new(),
            instrument_groups: BTreeMap::new(),
            last_exit_fill_at: BTreeMap::new(),
            orders_today: 0,
            day_trades: DayTradeLedger::default(),
        },
        instrument: GateInstrumentSnapshot {
            instrument: asset.clone(),
            asset_class: mandate_risk::AssetClass::UsEquity,
            exchange: Some(Exchange::Nasdaq),
            status_active: true,
            tradable: true,
            fractionable: true,
            ipo: false,
            ptp_no_exception: false,
            etp: EtpClass::Plain,
            etp_classified_at: Some(now),
            quote_currency: Some(QuoteCurrency::Usd),
            prior_close: Some(request.limit),
            median_dollar_volume_20d: Some(liquidity),
            median_dollar_volume_30d: Some(liquidity),
            min_order_size: Qty::parse("1")?,
            qty_increment: Qty::parse("1")?,
            halted: false,
            status_feed_current: true,
        },
        market: MarketSnapshot {
            quote: Some(SaneQuote {
                bid: Price::parse(BID)?,
                ask: Price::parse(ASK)?,
                at: now,
            }),
            last_trade: Some((request.limit, now)),
            trailing_5m_volume: Some(volume),
            adv_20d: Some(volume),
        },
        conduct: ConductState::default(),
        universe: WorkingUniverse::Known {
            instruments: BTreeSet::from([asset.clone()]),
            pinned: true,
        },
        asset,
        gate_agent,
        gate_client_order_id: GateOrderId(1),
        owner_confirmed_bid: None,
        fee_reservation: Usd::ZERO,
        data_profile: "tracer-paper".to_owned(),
        feed: "iex".to_owned(),
    })
}

fn usd(text: &str) -> Result<Usd, Cause> {
    Usd::parse(text).map_err(Cause::Num)
}

fn absent(what: &'static str) -> Cause {
    Cause::Absent { what }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use mandate_executor::BindingGateConfigRefs;
    use mandate_num::Usd;
    use mandate_runtime::AgentId;
    use mandate_time::UtcNanos;

    use super::{advisory_gate_context, load_contexts};
    use crate::Cause;

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tracer")
    }

    #[test]
    fn the_reviewed_artifacts_assemble_both_trusted_contexts() {
        let loaded = load_contexts(
            &fixtures().join("mandate.json"),
            &fixtures().join("config"),
            UtcNanos::parse("2026-10-05T17:00:00.000000000Z").unwrap(),
            &AgentId("tracer-aapl".to_owned()),
        );
        assert!(
            loaded.is_ok(),
            "the reviewed artifacts must assemble both contexts"
        );
    }

    #[test]
    fn a_missing_effective_dated_artifact_refuses_the_assembly() {
        let loaded = load_contexts(
            &fixtures().join("mandate.json"),
            &fixtures().join("no-config"),
            UtcNanos::parse("2026-10-05T17:00:00.000000000Z").unwrap(),
            &AgentId("tracer-aapl".to_owned()),
        );
        assert!(
            matches!(loaded, Err(Cause::Absent { .. })),
            "a missing artifact must refuse"
        );
    }

    #[test]
    fn the_reviewed_aapl_snapshot_meets_the_platform_liquidity_floor() {
        let context = advisory_gate_context(
            UtcNanos::parse("2026-10-05T17:00:00.000000000Z").unwrap(),
            BindingGateConfigRefs::complete("fee", "calendar", "instrument", "rules", "mandate"),
        )
        .unwrap();
        let platform_floor = Usd::parse("1000000").unwrap();
        assert_eq!(
            context.instrument.median_dollar_volume_20d,
            Some(platform_floor)
        );
        assert_eq!(
            context.instrument.median_dollar_volume_30d,
            Some(platform_floor)
        );
    }
}
