//! The binding gate's input for one deployed agent's order (DEC-2380 item 2): the reviewed
//! deployment's limits, rule set and fee schedule from `mandate-gate-inputs`, the account, mark and
//! clock from `mandate-gate-context`'s fold of the journal, and the facts the journal does not hold
//! (the asset record, the account's day-trade rules, and liquidity) from the broker or the cell's
//! fixtures.
//!
//! The gate evaluates eligibility, the liquidity floor, participation and the day-trade regime on
//! these facts, so a fact that is missing is no input at all: the source answers `None`, and the
//! core holds the order (`AGENTS.md` rule 3). Nothing here computes a money or quantity figure
//! (DEC-138 item 3): the fee reservation is `mandate-accounting`'s and every other figure is a
//! fact's.

use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::{AccountType, InstrumentId};
use mandate_executor::{AgentId, BindingGateConfigRefs, BindingGateInput, BindingGateRequest};
use mandate_gate_context::Facts;
use mandate_gate_inputs::{InputError, Reviewed, gate_mandate, reservation};
use mandate_num::{Price, Qty, Ratio, Usd};
use mandate_risk::{
    AccountSnapshot, AccountState, AgentId as GateAgentId, AgentMode, AgentSnapshot, AssetId,
    ClientOrderId, ConductState, DayTradeLedger, DayTradeRegime, EtpClass, Exchange, GateConfig,
    InstrumentSnapshot, MarketSnapshot, QuoteCurrency, RiskSnapshot, SaneQuote, ValidatedMandate,
    WorkingUniverse,
};
use mandate_time::UtcNanos;

/// The broker's record of the instrument: the eligibility facts of trading spec §9.4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssetRecord {
    pub active: bool,
    pub tradable: bool,
    pub fractionable: bool,
    pub ipo: bool,
    pub ptp_no_exception: bool,
}

/// The account's day-trade rules and the prior close's equity the legacy pattern-day-trader check
/// reads (trading spec §9.2), from the connector's declared rules and the broker's account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccountRules {
    pub account_type: AccountType,
    pub regime: DayTradeRegime,
    pub prior_close_equity: Usd,
}

/// The liquidity facts of trading spec §9.4 and §9.5: the prior close, the 20-session median
/// dollar volume and average daily volume from completed daily bars, and the trailing five
/// minutes' volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Liquidity {
    pub prior_close: Price,
    pub median_dollar_volume_20d: Usd,
    pub adv_20d: Qty,
    pub trailing_5m_volume: Qty,
}

/// Every fact one opening's binding gate reads that is not the deployment's: the journal's account,
/// mark and clock, the sane quote the order is judged against, the facts the journal does not
/// hold, each `None` until it is read, and whether the account is flat: it holds no position and
/// works no order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderFacts {
    pub journaled: Facts,
    pub quote: SaneQuote,
    pub asset: Option<AssetRecord>,
    pub rules: Option<AccountRules>,
    pub liquidity: Option<Liquidity>,
    pub flat: bool,
}

/// One reviewed deployment as the binding gate reads it.
#[derive(Debug, Clone)]
pub struct DeployedGate {
    agent: AgentId,
    asset: AssetId,
    exchange: Exchange,
    etp: EtpClass,
    etp_classified_at: UtcNanos,
    increment: Qty,
    allocation: Usd,
    config: GateConfig,
    mandate: ValidatedMandate,
    config_refs: BindingGateConfigRefs,
    fees: mandate_accounting::Config,
    feed: String,
}

impl DeployedGate {
    /// `agent`'s deployment as the gate reads it, quoted from `feed`.
    ///
    /// # Errors
    /// [`InputError`] when the mandate's limits or allocation cannot be read, or the instrument's
    /// asset id is not one the gate admits.
    pub fn new(agent: &str, reviewed: &Reviewed, feed: &str) -> Result<Self, InputError> {
        let asset = AssetId::new(reviewed.instrument.asset_id.as_str()).map_err(|_| {
            InputError::Missing {
                what: "the reviewed instrument id",
            }
        })?;
        Ok(Self {
            agent: AgentId(agent.to_owned()),
            asset,
            exchange: reviewed.instrument.gate_exchange,
            etp: reviewed.instrument.etp,
            etp_classified_at: reviewed.instrument.etp_classified_at,
            increment: reviewed.instrument.increment_qty,
            allocation: Usd::parse(reviewed.mandate.capital.allocation_usd.as_str())?,
            config: reviewed.gate_config.clone(),
            mandate: gate_mandate(&reviewed.mandate)?,
            config_refs: reviewed.config_refs.clone(),
            fees: reviewed.fees.clone(),
            feed: feed.to_owned(),
        })
    }

    /// The binding gate's input for the opening `request` on `facts`, or `None` when the request
    /// is not the deployed agent's opening on its instrument, the account is not flat, active and
    /// unblocked, or any fact the gate reads is missing.
    ///
    /// An order that reduces risk is answered `None` too: the core then keeps its own decision,
    /// which a missing snapshot never turns into a denial (`AGENTS.md` rule 13, DEC-2034 item 2).
    /// The risk snapshot is the allocation's, as the paper shell's first order reads it (DEC-470
    /// item 1), until the account risk view supplies it, so an opening is answered only from a
    /// flat account, where the allocation is the agent's whole equity. The day's day trades are
    /// not yet folded from the journal, so an opening is answered only where none of them could
    /// bind: under the legacy pattern-day-trader regime with a prior close's equity at or above
    /// the rule set's threshold (trading spec §9.2). Any other regime holds it.
    pub fn input(
        &self,
        facts: &OrderFacts,
        request: &BindingGateRequest<'_>,
    ) -> Option<BindingGateInput> {
        if !request.purpose.adds_risk()
            || !facts.flat
            || *request.agent != self.agent
            || request.instrument.as_str() != self.asset.as_str()
        {
            return None;
        }
        let asset = facts.asset?;
        let rules = facts.rules?;
        let liquidity = facts.liquidity?;
        if rules.regime != DayTradeRegime::LegacyPdt
            || rules.prior_close_equity < self.config.legacy_pdt_equity_threshold
        {
            return None;
        }
        let journaled = &facts.journaled;
        let account = &journaled.account;
        if account.status != "ACTIVE"
            || account.trading_blocked
            || account.account_blocked
            || account.trade_suspended_by_user
        {
            return None;
        }
        let now = journaled.now;
        let fee_reservation = reservation(
            &self.fees,
            &InstrumentId::new(request.instrument.as_str()).ok()?,
            request.side,
            request.qty,
            request.limit,
            now,
        )
        .ok()?;
        let gate_agent = GateAgentId(1);
        let allocation = self.allocation;
        Some(BindingGateInput {
            config_refs: self.config_refs.clone(),
            now,
            config: self.config.clone(),
            mandate: self.mandate.clone(),
            risk: RiskSnapshot {
                agent_equity: allocation,
                high_water_mark: allocation,
                day_start_equity: allocation,
                capital_base: allocation,
                inherited_loss: Usd::ZERO,
                latched: BTreeSet::new(),
                active_rungs: BTreeMap::new(),
                size_factor: Ratio::parse("1").ok()?,
                agent_mode: AgentMode::Normal,
            },
            account: AccountSnapshot {
                account_type: rules.account_type,
                state: AccountState::Active,
                crypto_active: account.crypto_status == "ACTIVE",
                regime: rules.regime,
                equity: account.equity,
                prior_close_equity: rules.prior_close_equity,
                model_buying_power: account.cash.min(account.non_marginable_buying_power),
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
            instrument: InstrumentSnapshot {
                instrument: self.asset.clone(),
                asset_class: mandate_risk::AssetClass::UsEquity,
                exchange: Some(self.exchange),
                status_active: asset.active,
                tradable: asset.tradable,
                fractionable: asset.fractionable,
                ipo: asset.ipo,
                ptp_no_exception: asset.ptp_no_exception,
                etp: self.etp,
                etp_classified_at: Some(self.etp_classified_at),
                quote_currency: Some(QuoteCurrency::Usd),
                prior_close: Some(liquidity.prior_close),
                median_dollar_volume_20d: Some(liquidity.median_dollar_volume_20d),
                median_dollar_volume_30d: None,
                min_order_size: self.increment,
                qty_increment: self.increment,
                halted: false,
                status_feed_current: false,
            },
            market: MarketSnapshot {
                quote: Some(facts.quote),
                last_trade: None,
                trailing_5m_volume: Some(liquidity.trailing_5m_volume),
                adv_20d: Some(liquidity.adv_20d),
            },
            conduct: ConductState::default(),
            universe: WorkingUniverse::Known {
                instruments: BTreeSet::from([self.asset.clone()]),
                pinned: true,
            },
            asset: self.asset.clone(),
            gate_agent,
            gate_client_order_id: ClientOrderId(1),
            owner_confirmed_bid: None,
            fee_reservation,
            data_profile: "cell-paper".to_owned(),
            feed: self.feed.clone(),
        })
    }
}
