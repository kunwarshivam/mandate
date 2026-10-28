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
use mandate_gate_inputs::{InputError, Reviewed, reservation};
use mandate_num::{Price, Qty, Usd};
use mandate_ports::{PortError, gate_mandate};
use mandate_risk::{
    AccountSnapshot, AccountState, AgentId as GateAgentId, AgentSnapshot, AssetId, ClientOrderId,
    ConductState, DayTradeLedger, DayTradeRegime, EtpClass, Exchange, GateConfig,
    InstrumentRestriction, InstrumentSnapshot, MarketSnapshot, QuoteCurrency, RiskSnapshot,
    SaneQuote, ValidatedMandate, WorkingUniverse,
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

/// The agent's own risk as the account risk view folds it from the journal (trading spec §9.6 to
/// §9.8): its equity against its high-water mark and day start, its latched limits and ladder
/// rungs, its mode, the instrument restrictions on it, its exits' last fills, its orders today,
/// its day trades and its conduct counters. The gate reads every limit on these, so none is ever
/// assumed: an opening without them is held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRisk {
    pub risk: RiskSnapshot,
    pub conduct: ConductState,
    pub instrument_restrictions: BTreeMap<AssetId, BTreeSet<InstrumentRestriction>>,
    pub last_exit_fill_at: BTreeMap<AssetId, UtcNanos>,
    pub orders_today: u32,
    pub day_trades: DayTradeLedger,
}

/// Every fact one opening's binding gate reads that is not the deployment's: the journal's account,
/// mark and clock, the sane quote the order is judged against, the facts the journal does not
/// hold, the agent's journaled risk and whether the instrument is halted, each `None` until it is
/// read, and whether the account is flat: it holds no position and works no order, as the caller
/// reads the account stream's fold and the broker's open orders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderFacts {
    pub journaled: Facts,
    pub quote: SaneQuote,
    pub asset: Option<AssetRecord>,
    pub rules: Option<AccountRules>,
    pub liquidity: Option<Liquidity>,
    pub agent: Option<AgentRisk>,
    pub halted: Option<bool>,
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
    config: GateConfig,
    mandate: ValidatedMandate,
    config_refs: BindingGateConfigRefs,
    fees: mandate_accounting::Config,
    feed: String,
}

/// `gate_mandate`'s refusal as the gate inputs' (DEC-2388 item 1 moved the reading to
/// `mandate-ports`): an absent fact keeps its name and an amount its error; anything else is the
/// mandate's limits, unread, so the opening is refused (`AGENTS.md` rule 3).
fn limits(error: PortError) -> InputError {
    match error {
        PortError::Absent { what } => InputError::Missing { what },
        PortError::Num(error) => InputError::Num(error),
        PortError::Gate(_) | PortError::Builder(_) => InputError::Missing {
            what: "the mandate's gate limits",
        },
    }
}

impl DeployedGate {
    /// The configuration this deployment's gate decisions are journaled under (journal spec §9).
    #[must_use]
    pub fn config_refs(&self) -> &BindingGateConfigRefs {
        &self.config_refs
    }

    /// `agent`'s deployment as the gate reads it, quoted from `feed`.
    ///
    /// # Errors
    /// [`InputError`] when the mandate's limits cannot be read, the instrument's
    /// asset id is not one the gate admits, or the instrument is not a US equity, the only class
    /// this input judges.
    pub fn new(agent: &str, reviewed: &Reviewed, feed: &str) -> Result<Self, InputError> {
        if reviewed.instrument.asset_class != mandate_domain::AssetClass::UsEquity {
            return Err(InputError::Missing {
                what: "a US-equity instrument",
            });
        }
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
            config: reviewed.gate_config.clone(),
            mandate: gate_mandate(&reviewed.mandate).map_err(limits)?,
            config_refs: reviewed.config_refs.clone(),
            fees: reviewed.fees.clone(),
            feed: feed.to_owned(),
        })
    }

    /// The binding gate's input for the opening `request` on `facts`, or `None` when the request
    /// is not the deployed agent's opening on its instrument, the account is not flat, active and
    /// unblocked, or any fact the gate reads is missing, the agent's journaled risk and the halt
    /// status among them, so no limit is judged on an assumed figure (`AGENTS.md` rules 1 and 3).
    ///
    /// An order that reduces risk is answered `None` too: the core then keeps its own decision,
    /// which a missing snapshot never turns into a denial (`AGENTS.md` rule 13, DEC-2034 item 2).
    /// The account's positions and working orders are not yet read, so an opening is answered
    /// only from a flat account, and only where the account's day trades could not bind (trading
    /// spec §9.2): under `intraday_margin`, which counts none, while the broker reports no
    /// maintenance deficit (a deficit puts the account's agents in exits only), and under the
    /// legacy pattern-day-trader regime with a prior close's equity at or above the rule set's
    /// threshold.
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
        let agent = facts.agent.clone()?;
        let halted = facts.halted?;
        let day_trades_cannot_bind = match rules.regime {
            DayTradeRegime::IntradayMargin { maintenance_excess } => {
                maintenance_excess >= Usd::ZERO
            }
            DayTradeRegime::LegacyPdt => {
                rules.prior_close_equity >= self.config.legacy_pdt_equity_threshold
            }
        };
        if !day_trades_cannot_bind {
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
        Some(BindingGateInput {
            config_refs: self.config_refs.clone(),
            now,
            config: self.config.clone(),
            mandate: self.mandate.clone(),
            risk: agent.risk.clone(),
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
                mode: agent.risk.agent_mode,
                instrument_restrictions: agent.instrument_restrictions,
                positions: BTreeMap::new(),
                market_values: BTreeMap::new(),
                working_orders: BTreeSet::new(),
                instrument_groups: BTreeMap::new(),
                last_exit_fill_at: agent.last_exit_fill_at,
                orders_today: agent.orders_today,
                day_trades: agent.day_trades,
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
                halted,
                status_feed_current: false,
            },
            market: MarketSnapshot {
                quote: Some(facts.quote),
                last_trade: None,
                trailing_5m_volume: Some(liquidity.trailing_5m_volume),
                adv_20d: Some(liquidity.adv_20d),
            },
            conduct: agent.conduct,
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
