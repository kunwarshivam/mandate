//! The injected collaborators. Every one is pure, so [`crate::handle`] stays deterministic.
//!
//! [`BindingGateSource`] supplies trusted facts, never a verdict. The executor derives the order
//! and calls `mandate-risk` itself, so neither this port nor stream I's advisory `GateDryRun` can
//! inject an allow (`AGENTS.md` rule 1; journal spec §2).

#[cfg(test)]
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;

#[cfg(test)]
use mandate_accounting::AccountType;
use mandate_accounting::{AssetClass, InstrumentId};
use mandate_domain::{CapabilityProfile, ProfileError};
use mandate_num::{Fraction, ShareIncrement};
#[cfg(test)]
use mandate_num::{Price, Qty, Ratio, Usd};
#[cfg(test)]
use mandate_risk::spec_types::{GoalState, RiskLimits};
use mandate_risk::{
    AccountSnapshot, AgentId as GateAgentId, AgentSnapshot, AssetId, ClientOrderId as GateOrderId,
    ConductState, GateConfig, InstrumentSnapshot as GateInstrumentSnapshot, MarketSnapshot,
    RiskSnapshot, ValidatedMandate, WorkingUniverse,
};
#[cfg(test)]
use mandate_risk::{
    AccountState, AgentMode, DayTradeLedger, DayTradeRegime, EtpClass, Exchange, QuoteCurrency,
    SaneQuote,
};
use mandate_time::UtcNanos;

use crate::types::{
    AgentId, BrokerOutcome, BrokerRequest, BrokerUnknown, EventId, ExecutorConfig, ExitTier,
    MandateVersion, ProtectionPrices, Purpose, Seq, TimeInForce, WriterEpoch,
};

/// Deterministic event identity (ADR-0001 ES-06, ES-21, DEC-131 item 6). The id is derived from
/// `(epoch, head, ordinal)` rather than generated, so a retry after `Unavailable` or `Ambiguous`
/// re-derives it and the append answers `AlreadyCommitted` (journal spec §5.1). The epoch is in
/// the derivation so a fenced writer's retry cannot collide with the new writer's id at the same
/// head.
pub trait IdGen {
    fn event_id(&self, epoch: WriterEpoch, head: Seq, ordinal: u32) -> EventId;
}

/// The narrow read-only view of stream F's mandate that this crate needs.
///
/// `mandate-spec` is layer 3 and will own the full type; until it lands each layer-6 crate
/// declares the view it reads (stream I already does), and the two converge on F's type in the
/// implementation PR rather than one same-layer crate importing the other's.
pub trait MandateView {
    /// The agent's confirmed mandate version, which every gated draft carries as its `man`
    /// configuration reference (journal spec §9).
    fn version(&self, agent: &AgentId) -> Option<MandateVersion>;
    /// The fraction below the stop at which a crypto stop-limit's limit sits
    /// (trading-domain spec §5.4, DEC-36).
    fn crypto_stop_limit_offset(&self, agent: &AgentId) -> Option<Fraction>;
    /// Whether the agent's working universe holds the instrument at all.
    fn covers(&self, agent: &AgentId, instrument: &InstrumentId) -> bool;
}

/// Stream G's effective-dated instrument snapshot (`ins`), read-only.
pub trait InstrumentSnapshot {
    fn asset_class(&self, instrument: &InstrumentId) -> Option<AssetClass>;
    /// Whole or fractional shares. Only the whole-share part of a fractional position can be
    /// protected (trading-domain spec §5.4).
    fn increment(&self, instrument: &InstrumentId) -> Option<ShareIncrement>;
    /// Which of trading-domain spec §5.6's three tiers the instrument prices its exits from.
    fn exit_tier(&self, instrument: &InstrumentId) -> Option<ExitTier>;
}

/// The trusted values outside the account-stream fold that complete one §9.1 [`mandate_risk::GateInput`].
///
/// The proposed order and gate pass are deliberately absent: the executor derives both, then calls
/// [`mandate_risk::evaluate`] directly. A caller can supply facts, never a verdict.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BindingGateConfigRefs {
    pub fee_config: Option<String>,
    pub trading_calendar: Option<String>,
    pub instrument_snapshot: Option<String>,
    pub rule_set: Option<String>,
    pub mandate_version: Option<String>,
}

impl BindingGateConfigRefs {
    pub fn complete(
        fee_config: impl Into<String>,
        trading_calendar: impl Into<String>,
        instrument_snapshot: impl Into<String>,
        rule_set: impl Into<String>,
        mandate_version: impl Into<String>,
    ) -> Self {
        Self {
            fee_config: Some(fee_config.into()),
            trading_calendar: Some(trading_calendar.into()),
            instrument_snapshot: Some(instrument_snapshot.into()),
            rule_set: Some(rule_set.into()),
            mandate_version: Some(mandate_version.into()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BindingGateInput {
    pub config_refs: BindingGateConfigRefs,
    pub now: UtcNanos,
    pub config: GateConfig,
    pub mandate: ValidatedMandate,
    pub risk: RiskSnapshot,
    pub account: AccountSnapshot,
    pub agent: AgentSnapshot,
    pub instrument: GateInstrumentSnapshot,
    pub market: MarketSnapshot,
    pub conduct: ConductState,
    pub universe: WorkingUniverse,
    pub asset: AssetId,
    pub gate_agent: GateAgentId,
    pub gate_client_order_id: GateOrderId,
    pub owner_confirmed_bid: Option<mandate_num::Price>,
    pub fee_reservation: mandate_num::Usd,
    pub data_profile: String,
    pub feed: String,
}

/// The executor-owned proposal fields used to select trusted snapshots.
///
/// These are lookup keys, not a verdict and not the [`mandate_risk::ProposedOrder`]; the executor
/// constructs that type itself after the lookup.
pub struct BindingGateRequest<'a> {
    pub agent: &'a AgentId,
    pub instrument: &'a InstrumentId,
    pub side: mandate_accounting::Side,
    pub qty: mandate_num::Qty,
    pub limit: mandate_num::Price,
    pub purpose: Purpose,
    pub tif: TimeInForce,
    pub protection: Option<ProtectionPrices>,
}

/// Pure lookup of the trusted snapshots for one executor agent and instrument.
///
/// Returning `None` is an unavailable input, never an allow. This port cannot return a gate
/// decision; the binding verdict is therefore not injectable.
pub trait BindingGateSource {
    fn input(&self, request: &BindingGateRequest<'_>) -> Option<BindingGateInput>;
}

#[cfg(test)]
pub(crate) struct AllowingBindingGate;

#[cfg(test)]
impl BindingGateSource for AllowingBindingGate {
    fn input(&self, request: &BindingGateRequest<'_>) -> Option<BindingGateInput> {
        if !request.purpose.adds_risk() {
            return None;
        }
        let asset = AssetId::new(request.instrument.as_str()).ok()?;
        let at = UtcNanos::parse_rfc3339("2026-09-21T15:00:00Z").ok()?;
        let quote_at = UtcNanos::parse_rfc3339("2026-09-21T14:59:59Z").ok()?;
        let held = Qty::parse("1000000").ok()?;
        let equity = Usd::parse("1000000000").ok()?;
        let mut positions = BTreeMap::new();
        positions.insert(asset.clone(), held);
        let gate_agent = GateAgentId(1);
        Some(BindingGateInput {
            config_refs: BindingGateConfigRefs::complete(
                format!("sha256:{}", "1".repeat(64)),
                format!("sha256:{}", "2".repeat(64)),
                format!("sha256:{}", "3".repeat(64)),
                format!("sha256:{}", "4".repeat(64)),
                format!("sha256:{}", "5".repeat(64)),
            ),
            now: at,
            config: GateConfig {
                price_floor: Usd::parse("0.01").ok()?,
                liquidity_floor_usd: Usd::ZERO,
                crypto_liquidity_floor_usd: Usd::ZERO,
                collar_liquid_threshold_usd: Usd::ZERO,
                collar_liquid_x: Fraction::parse("0.5").ok()?,
                collar_other_x: Fraction::parse("0.5").ok()?,
                collar_crypto_x: Fraction::parse("0.5").ok()?,
                collar_passive_band: Fraction::parse("0.5").ok()?,
                opposite_fill_interval_s: 0,
                min_resting_time_s: 0,
                order_to_fill_max: u32::MAX,
                order_to_fill_min_orders: u32::MAX,
                order_size_participation: Fraction::parse("1").ok()?,
                daily_participation: Fraction::parse("1").ok()?,
                close_window_minutes: 0,
                legacy_pdt_equity_threshold: Usd::ZERO,
                etp_classification_max_age_s: u32::MAX,
            },
            mandate: ValidatedMandate::from_validated_parts(
                RiskLimits {
                    max_position_usd: equity,
                    max_position_fraction: Fraction::parse("1").ok()?,
                    max_order_usd: equity,
                    max_gross_exposure_usd: equity,
                    max_orders_per_day: u32::MAX,
                    reentry_cooldown_s: 0,
                    rebalance_band: Fraction::parse("1").ok()?,
                    breach_confirm_s: 0,
                    drawdown_ladder: Vec::new(),
                },
                GoalState::Running,
                true,
                true,
            ),
            risk: RiskSnapshot {
                agent_equity: equity,
                high_water_mark: equity,
                day_start_equity: equity,
                capital_base: equity,
                inherited_loss: Usd::ZERO,
                latched: BTreeSet::new(),
                active_rungs: BTreeMap::new(),
                size_factor: Ratio::parse("1").ok()?,
                agent_mode: AgentMode::Normal,
            },
            account: AccountSnapshot {
                account_type: AccountType::Margin,
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
                asset_class: AssetClass::UsEquity,
                exchange: Some(Exchange::Nasdaq),
                status_active: true,
                tradable: true,
                fractionable: true,
                ipo: false,
                ptp_no_exception: false,
                etp: EtpClass::Plain,
                etp_classified_at: Some(at),
                quote_currency: Some(QuoteCurrency::Usd),
                prior_close: Some(Price::parse("150").ok()?),
                median_dollar_volume_20d: Some(equity),
                median_dollar_volume_30d: Some(equity),
                min_order_size: Qty::parse("0.000000001").ok()?,
                qty_increment: Qty::parse("0.000000001").ok()?,
                halted: false,
                status_feed_current: true,
            },
            market: MarketSnapshot {
                quote: Some(SaneQuote {
                    bid: Price::parse("150").ok()?,
                    ask: Price::parse("150").ok()?,
                    at: quote_at,
                }),
                last_trade: Some((Price::parse("150").ok()?, quote_at)),
                trailing_5m_volume: Some(held),
                adv_20d: Some(held),
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
            data_profile: "test".to_owned(),
            feed: "test".to_owned(),
        })
    }
}

#[cfg(test)]
pub(crate) static ALLOWING_BINDING_GATE: AllowingBindingGate = AllowingBindingGate;

/// The pure ports a step reads. Each is a function of its arguments, so `handle` stays
/// deterministic and a test injects fixed implementations.
pub struct Ports<'a> {
    pub ids: &'a dyn IdGen,
    pub mandates: &'a dyn MandateView,
    pub instruments: &'a dyn InstrumentSnapshot,
    /// The effective-dated parameters of trading-domain spec §5.4, §5.6, and §5.7. Configuration,
    /// not constants, so changing one is a configuration change with a content hash in
    /// `config_refs` (interpretation 22).
    pub config: &'a ExecutorConfig,
    /// The effective-dated fee configuration (`fee` in `config_refs`, trading-domain spec §6.2,
    /// §6.3), from which the executor computes paper's simulated regulatory fees with
    /// `mandate-accounting`'s own fee rules (§10, DEC-133).
    pub fees: &'a mandate_accounting::Config,
}

/// Why one broker round trip produced no broker fact.
///
/// Only [`Self::Unknown`] is handed to the executor, as `Input::Broker(Err(..))`: it means the
/// request may have reached the broker and nobody knows what it did, so recovery queries on the
/// client order id and never resubmits (task brief interpretation 10). The other two are **not**
/// unknown outcomes, and folding either as one would make the executor keep querying on an answer
/// it could never read, or on a request that never left the process. The shell stops the executor
/// and alerts on them instead (DEC-85: an uninterpreted input fails loudly, never a guess), and
/// neither is ever a rejection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ConnectorError {
    /// The outcome is unknown: a timeout, a dropped connection, or a 5xx after the request left.
    #[error(transparent)]
    Unknown(#[from] BrokerUnknown),
    /// The broker answered and the connector could not read the answer: a body that is not the
    /// wire type, a number that has been through a float, a status outside §5.7's table. `code`
    /// is the connector's own stable reason.
    #[error("the broker's answer could not be read ({code})")]
    Unreadable { code: &'static str },
    /// The request never left the process: its method and path are not an endpoint the connector
    /// allows, or the connector does not implement it yet. `code` is the connector's own reason.
    #[error("the request was not sent ({code})")]
    NotSent { code: &'static str },
}

impl ConnectorError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(self) -> &'static str {
        match self {
            Self::Unknown(_) => "unknown_outcome",
            Self::Unreadable { .. } => "unreadable",
            Self::NotSent { .. } => "not_sent",
        }
    }

    /// The unknown outcome to fold as `Input::Broker(Err(..))`, or `None` for a failure the shell
    /// must stop on rather than hand to the executor.
    pub fn as_unknown(self) -> Option<BrokerUnknown> {
        match self {
            Self::Unknown(unknown) => Some(unknown),
            Self::Unreadable { .. } | Self::NotSent { .. } => None,
        }
    }
}

/// One broker round trip. Implemented by `mandate-alpaca` and by the tests' fake connector.
///
/// The core never calls it: it only describes the request as an [`crate::Effect::Broker`], which
/// is what keeps `handle` pure and every HTTP type out of the core. An `Err` is **never** a
/// rejection: a rejection is an `Ok` answer the broker gave. [`ConnectorError::Unknown`] means
/// the outcome is unknown and recovery must query (task brief interpretation 10); the other two
/// variants are failures of this process that the shell stops on.
pub trait BrokerConnector {
    /// The broker's capability profile, declared by the connector from its published contract
    /// alone (DEC-531 item 1). Shared code reads what the broker accepts from here, never from
    /// the broker's name or the asset class. The answer never depends on a broker call.
    fn profile(&self) -> Result<CapabilityProfile, ProfileError>;

    fn call(
        &mut self,
        request: &BrokerRequest,
    ) -> impl Future<Output = Result<BrokerOutcome, ConnectorError>>;
}
