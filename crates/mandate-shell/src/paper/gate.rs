//! The binding gate's input and its advisory copy, mapped from the snapshot, the mandate, and the
//! trading-domain spec's defaults. The 1× buying power is the executor's and the fee reservation is
//! `mandate-accounting`'s; nothing here computes a money or quantity figure (DEC-138 item 3).

use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::{
    AccountingError, AssetClass, InstrumentId, ProspectiveOrder, Side, fee_reservation,
};
use mandate_alpaca::{AccountRules, DeclaredRegime};
use mandate_executor::{BindingGateInput, BrokerAccount};
use mandate_num::{Fraction, Price, Qty, Ratio, Usd};
use mandate_risk::spec_types::{GoalState, RiskLimits, Rung, RungAction, ScaleAction};
use mandate_risk::{
    AccountSnapshot, AccountState, AgentId as GateAgentId, AgentMode, AgentSnapshot,
    AssetId as GateAssetId, ClientOrderId as GateOrderId, ConductState, DayTradeLedger,
    DayTradeRegime, GateConfig, GatePass, InstrumentSnapshot as GateInstrumentSnapshot,
    MarketSnapshot, Origin, ProposedKind, QuoteCurrency, RiskSnapshot, SaneQuote,
    TimeInForce as GateTimeInForce, ValidatedMandate as GateMandate, WorkingUniverse,
};
use mandate_time::UtcNanos;

use super::artifacts::Artifacts;
use super::facts::PaperFacts;
use super::{absent, usd};
use crate::adapters::{AdvisoryGateContext, AdvisoryOrderFacts};
use crate::error::Cause;

/// The binding gate's input, the same for every request the reviewed agent makes on the reviewed
/// instrument: the gate itself judges the proposal against it.
pub(super) fn gate_template(
    artifacts: &Artifacts,
    facts: &PaperFacts,
    now: UtcNanos,
    config: GateConfig,
) -> Result<BindingGateInput, Cause> {
    let account = &facts.broker.account;
    let quote = &facts.broker.quote;
    let asset_record = &facts.broker.asset.asset;
    let liquidity = &facts.liquidity;
    let allocation = usd(artifacts.mandate.capital.allocation_usd.as_str())?;
    let asset = GateAssetId::new(artifacts.instrument.asset_id.as_str())
        .map_err(|_| absent("the reviewed instrument id"))?;
    let gate_agent = GateAgentId(1);
    let planned_reservation = reservation(
        &artifacts.fees,
        &artifacts.instrument.symbol,
        Side::Buy,
        artifacts.instrument.increment_qty,
        quote.ask,
        now,
    )
    .map_err(|_| absent("the reviewed fee schedule's price of the order"))?;
    Ok(BindingGateInput {
        config_refs: artifacts.config_refs.clone(),
        now,
        config,
        mandate: gate_mandate(&artifacts.mandate)?,
        risk: RiskSnapshot {
            agent_equity: allocation,
            high_water_mark: allocation,
            day_start_equity: allocation,
            capital_base: allocation,
            inherited_loss: Usd::ZERO,
            latched: BTreeSet::new(),
            active_rungs: BTreeMap::new(),
            size_factor: Ratio::parse("1")?,
            agent_mode: AgentMode::Normal,
        },
        account: AccountSnapshot {
            account_type: facts.broker.account_rules.account_type,
            state: AccountState::Active,
            crypto_active: account.crypto_status == "ACTIVE",
            regime: opening_regime(facts.broker.account_rules, account)?,
            equity: account.equity,
            prior_close_equity: account.last_equity,
            model_buying_power: account.one_x_buying_power(),
            broker_buying_power: account.buying_power,
            broker_non_marginable_buying_power: account.non_marginable_buying_power,
            positions: BTreeMap::new(),
            market_values: BTreeMap::new(),
            working_orders: BTreeMap::new(),
            unknown_orders: BTreeSet::new(),
            related_account_resting: BTreeMap::new(),
        },
        agent: AgentSnapshot {
            agent: gate_agent,
            mode: AgentMode::Normal,
            instrument_restrictions: BTreeMap::new(),
            positions: BTreeMap::new(),
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
            exchange: Some(artifacts.instrument.gate_exchange),
            status_active: asset_record.active,
            tradable: asset_record.tradable,
            fractionable: asset_record.fractionable,
            ipo: asset_record.ipo,
            ptp_no_exception: asset_record.ptp_no_exception,
            etp: artifacts.instrument.etp,
            etp_classified_at: Some(artifacts.instrument.etp_classified_at),
            quote_currency: Some(QuoteCurrency::Usd),
            prior_close: Some(liquidity.prior_close),
            median_dollar_volume_20d: Some(liquidity.median_dollar_volume_20d),
            median_dollar_volume_30d: None,
            min_order_size: artifacts.instrument.increment_qty,
            qty_increment: artifacts.instrument.increment_qty,
            halted: false,
            status_feed_current: false,
        },
        market: MarketSnapshot {
            quote: Some(SaneQuote {
                bid: quote.bid,
                ask: quote.ask,
                at: quote.at,
            }),
            last_trade: None,
            trailing_5m_volume: Some(liquidity.trailing_5m_volume),
            adv_20d: Some(liquidity.adv_20d),
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
        fee_reservation: planned_reservation,
        data_profile: "tracer-paper".to_owned(),
        feed: "iex".to_owned(),
    })
}

/// The gate's day-trading regime for an opening run, from the connector's declared regime and the
/// broker's account answer (DEC-840 item 5). Under `intraday_margin` the excess is the executor's
/// [`BrokerAccount::maintenance_excess`]; a maintenance figure it refuses is refused here, never read
/// as no requirement, and a reported deficit refuses the opening, since nothing yet turns it into
/// §9.2's `exits_only` (DEC-840 item 4). A zero excess is no deficit. Only the assembly of an opening
/// run may call this: no exit, protective-order or recovery path is gated by it (`AGENTS.md` rule
/// 13).
fn opening_regime(rules: AccountRules, account: &BrokerAccount) -> Result<DayTradeRegime, Cause> {
    match rules.regime {
        DeclaredRegime::LegacyPdt => Ok(DayTradeRegime::LegacyPdt),
        DeclaredRegime::IntradayMargin => {
            let maintenance_excess = account
                .maintenance_excess()
                .map_err(|_| absent("the broker's maintenance excess"))?;
            if maintenance_excess.is_negative() {
                return Err(absent("an account with no maintenance deficit"));
            }
            Ok(DayTradeRegime::IntradayMargin { maintenance_excess })
        }
    }
}

/// `mandate-accounting`'s fee reservation for one equity order on the reviewed instrument.
pub(super) fn reservation(
    fees: &mandate_accounting::Config,
    instrument: &InstrumentId,
    side: Side,
    qty: Qty,
    limit: Price,
    at: UtcNanos,
) -> Result<Usd, AccountingError> {
    fee_reservation(
        &ProspectiveOrder {
            instrument: instrument.clone(),
            asset_class: AssetClass::UsEquity,
            side,
            qty,
            limit,
            at,
        },
        fees,
    )
}

pub(super) fn gate_mandate(document: &mandate_spec::Mandate) -> Result<GateMandate, Cause> {
    let risk = &document.risk;
    let scale_action = match risk.scale_action {
        mandate_spec::document::ScaleAction::LimitBuys => ScaleAction::LimitBuys,
        mandate_spec::document::ScaleAction::TrimToTarget => ScaleAction::TrimToTarget,
    };
    let drawdown_ladder = risk
        .drawdown_ladder
        .iter()
        .enumerate()
        .map(|(index, rung)| {
            Ok(Rung {
                index: u8::try_from(index).map_err(|_| absent("the mandate's drawdown ladder"))?,
                at: Fraction::parse(rung.at.as_str())?,
                action: match rung.action {
                    mandate_spec::document::LadderAction::ScaleSizes => RungAction::ScaleSizes,
                    mandate_spec::document::LadderAction::ExitsOnly => RungAction::ExitsOnly,
                    mandate_spec::document::LadderAction::FlattenAndPause => {
                        RungAction::FlattenAndPause
                    }
                },
                factor: rung
                    .factor
                    .as_ref()
                    .map(|factor| Fraction::parse(factor.as_str()))
                    .transpose()?,
                scale_action: Some(scale_action),
            })
        })
        .collect::<Result<Vec<_>, Cause>>()?;
    Ok(GateMandate::from_validated_parts(
        RiskLimits {
            max_position_usd: usd(document.risk.max_position_usd.as_str())?,
            max_position_fraction: Fraction::parse(document.risk.max_position_fraction.as_str())?,
            max_order_usd: usd(document.risk.max_order_usd.as_str())?,
            max_gross_exposure_usd: usd(document.risk.max_gross_exposure_usd.as_str())?,
            max_orders_per_day: document.risk.max_orders_per_day,
            reentry_cooldown_s: document.risk.reentry_cooldown_s,
            rebalance_band: Fraction::parse(document.behavior.sizing.rebalance_band.as_str())?,
            breach_confirm_s: document.risk.breach_confirm_s,
            drawdown_ladder,
        },
        GoalState::Running,
        document.universe.leveraged_etps_enabled,
        document.universe.leveraged_etp_disclosure_version.is_some(),
    ))
}

pub(super) fn advisory_gate_context(template: BindingGateInput) -> AdvisoryGateContext {
    AdvisoryGateContext {
        now: template.now,
        pass: GatePass::First,
        config: template.config,
        mandate: template.mandate,
        risk: template.risk,
        account: template.account,
        agent: template.agent,
        instrument: template.instrument,
        market: template.market,
        conduct: template.conduct,
        universe: template.universe,
        order: AdvisoryOrderFacts {
            kind: ProposedKind::Plain,
            tif: GateTimeInForce::Day,
            extended_hours: false,
            origin: Origin::OrderBuilder,
            owner_confirmed_bid: None,
            client_order_id: template.gate_client_order_id,
            fee_reservation: template.fee_reservation,
        },
    }
}
