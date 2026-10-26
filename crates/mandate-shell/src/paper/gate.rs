//! The binding gate's input and its advisory copy, mapped from the snapshot, the mandate, and the
//! trading-domain spec's defaults. The 1× buying power is the executor's and the fee reservation is
//! `mandate-accounting`'s; nothing here computes a money or quantity figure (DEC-138 item 3).

use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::Side;
use mandate_alpaca::{AccountRules, DeclaredRegime};
use mandate_executor::{BindingGateInput, BrokerAccount};
use mandate_num::{Ratio, Usd};
use mandate_risk::{
    AccountSnapshot, AccountState, AgentId as GateAgentId, AgentMode, AgentSnapshot,
    AssetId as GateAssetId, ClientOrderId as GateOrderId, ConductState, DayTradeLedger,
    DayTradeRegime, GateConfig, GatePass, InstrumentSnapshot as GateInstrumentSnapshot,
    MarketSnapshot, Origin, ProposedKind, QuoteCurrency, RiskSnapshot, SaneQuote,
    TimeInForce as GateTimeInForce, WorkingUniverse,
};
use mandate_time::UtcNanos;

use super::artifacts::Artifacts;
use super::facts::PaperFacts;
use super::{absent, usd};
use crate::adapters::{AdvisoryGateContext, AdvisoryOrderFacts};
use crate::error::Cause;

pub(super) use mandate_gate_inputs::{gate_mandate, reservation};

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

/// The advisory gate's context from the binding gate's `template`, with the account risk view and
/// the agent's mode named by the caller: the gate reads both only from here, so whoever builds a
/// context says where its risk state came from (lane TW's E6-15 slice C supplies them from the
/// account risk view).
pub(super) fn advisory_gate_context(
    template: BindingGateInput,
    risk: RiskSnapshot,
    mode: AgentMode,
) -> AdvisoryGateContext {
    let mut agent = template.agent;
    agent.mode = mode;
    AdvisoryGateContext {
        now: template.now,
        pass: GatePass::First,
        config: template.config,
        mandate: template.mandate,
        risk,
        account: template.account,
        agent,
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
