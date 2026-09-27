#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]
//! The independent risk gate ([trading-domain spec §9](../../../docs/specs/trading-domain.md#9-risk-gate)):
//! a mandate, a risk state, an account snapshot, a working universe, a market context, a conduct
//! state and one proposed order in; one [`Decision`] out.
//!
//! [`evaluate`] is pure. It reads only its arguments, keeps no state between calls, touches no
//! clock, no filesystem and no randomness, and returns the same decision for the same inputs, so a
//! replay reproduces every verdict (ADR-0001 ES-21). It never submits, cancels, reserves or
//! journals anything: the executor does all four, and [`Decision::checks`] is the payload it
//! journals.
//!
//! That shape is the point. `AGENTS.md` rule 1 says no code path may let an agent act outside its
//! mandate, and that is only true if the enforcement lives in a function the agent does not call
//! and cannot reach. Every §9.1 check is here — the working universe, the eligibility floor,
//! concentration, sessions, order constraints, the collar, the conduct controls, buying power, the
//! day-trade budget — rather than spread across the components that propose orders, because two
//! gates would eventually disagree and one of them would be the one that allowed the order.
//!
//! The order of the checks is the crate's whole contract, fixed by §9.1 and reproduced by
//! [`evaluate`]: it stops at the first failure and reports that check's stable reason code.
//! [`Decision::checks`] still lists every check reached, in order, with the rest marked
//! [`CheckOutcome::NotReached`], so a reader of the journal can never mistake "we did not look" for
//! "it passed".
//!
//! Reducing risk is never denied by a limit (MI-1): see [`Purpose`] for what each exit type is
//! exempt from and [`Verdict::Hold`] for the only four things that may hold one. The gate assigns
//! the purpose itself from [`Origin`], the side and the position, so a proposer cannot claim an
//! exemption by describing its own order.
//!
//! Stubs only: every entry point returns [`GateError::Unimplemented`] until the story named in its
//! doc comment lands (DEC-77, DEC-83).

use core::fmt::Display;
use std::collections::{BTreeMap, BTreeSet};

use mandate_num::{Fraction, Price, Qty, Usd};
use mandate_time::{Date, UtcNanos};
use thiserror::Error;

#[doc(hidden)]
pub mod spec_types;

pub use mandate_accounting::{AccountType, AssetClass, Side};
pub use spec_types::{
    AgentMode, AssetId, InstrumentRestriction, RiskSnapshot, ValidatedMandate, WorkingUniverse,
};

/// A stable reason code (ADR-0001 ES-09), registered in
/// [the trading-domain reference cases](../../../docs/specs/reference-cases/trading-domain.yaml).
///
/// Every variant is a code that file's `reason_codes` list registers. §5.3 rule 2's minimum size
/// and increment have no registered code, which is why no variant names them: DEC-129 item 27
/// proposes `below_min_order_size` and `quantity_off_increment` to the founder, and until they are
/// registered the gate denies without minting one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReasonCode {
    AccountTradingBlocked,
    AccountRestricted,
    CryptoAccountInactive,
    AgentExitsOnly,
    AgentPaused,
    AgentStopped,
    NotInWorkingUniverse,
    IneligibleExchange,
    IpoNotTradable,
    BelowPriceFloor,
    BelowLiquidityFloor,
    LeveragedEtpNotEnabled,
    ConcentrationLimit,
    MaxOrderSize,
    ReentryCooldown,
    SessionNotAllowed,
    ExtendedHoursOpeningNotAllowed,
    AuctionWindow,
    InstrumentHalted,
    WouldCrossZero,
    SellExceedsAvailable,
    WorkingOrderLimit,
    AddBlockedByProtectiveOrder,
    UnknownOrderInFlight,
    MarketOrderNotAllowed,
    StaleMark,
    PriceOutsideCollar,
    MinRestingTime,
    OppositeFillInterval,
    ConductLimitBreached,
    MaxOrdersPerDay,
    CloseWindow,
    DiscretionaryExitRegularSessionOnly,
    OwnerConfirmationRequired,
    GrossExposureLimit,
    InsufficientBuyingPower,
    InsufficientSettledBuyingPower,
    LegacyPdtDayTradeBudget,
}

impl ReasonCode {
    /// Every variant, so a caller that needs the whole set cannot hand-list a stale subset — the
    /// mistake that would have made `MC-G07` unpassable. A new variant that is not added here is
    /// caught by `hand::every_reason_code_is_registered_in_the_case_file`, which counts the
    /// variants the enum declares by reading this file rather than by reading this array: an array
    /// compared against itself can catch a duplicate but never an omission. `as_str`'s exhaustive
    /// match forces a new variant to be named; only that count forces it in here.
    pub const ALL: [Self; 38] = [
        Self::AccountTradingBlocked,
        Self::AccountRestricted,
        Self::CryptoAccountInactive,
        Self::AgentExitsOnly,
        Self::AgentPaused,
        Self::AgentStopped,
        Self::NotInWorkingUniverse,
        Self::IneligibleExchange,
        Self::IpoNotTradable,
        Self::BelowPriceFloor,
        Self::BelowLiquidityFloor,
        Self::LeveragedEtpNotEnabled,
        Self::ConcentrationLimit,
        Self::MaxOrderSize,
        Self::ReentryCooldown,
        Self::SessionNotAllowed,
        Self::ExtendedHoursOpeningNotAllowed,
        Self::AuctionWindow,
        Self::InstrumentHalted,
        Self::WouldCrossZero,
        Self::SellExceedsAvailable,
        Self::WorkingOrderLimit,
        Self::AddBlockedByProtectiveOrder,
        Self::UnknownOrderInFlight,
        Self::MarketOrderNotAllowed,
        Self::StaleMark,
        Self::PriceOutsideCollar,
        Self::MinRestingTime,
        Self::OppositeFillInterval,
        Self::ConductLimitBreached,
        Self::MaxOrdersPerDay,
        Self::CloseWindow,
        Self::DiscretionaryExitRegularSessionOnly,
        Self::OwnerConfirmationRequired,
        Self::GrossExposureLimit,
        Self::InsufficientBuyingPower,
        Self::InsufficientSettledBuyingPower,
        Self::LegacyPdtDayTradeBudget,
    ];

    /// The registered spelling, which is what the reference cases compare against.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AccountTradingBlocked => "account_trading_blocked",
            Self::AccountRestricted => "account_restricted",
            Self::CryptoAccountInactive => "crypto_account_inactive",
            Self::AgentExitsOnly => "agent_exits_only",
            Self::AgentPaused => "agent_paused",
            Self::AgentStopped => "agent_stopped",
            Self::NotInWorkingUniverse => "not_in_working_universe",
            Self::IneligibleExchange => "ineligible_exchange",
            Self::IpoNotTradable => "ipo_not_tradable",
            Self::BelowPriceFloor => "below_price_floor",
            Self::BelowLiquidityFloor => "below_liquidity_floor",
            Self::LeveragedEtpNotEnabled => "leveraged_etp_not_enabled",
            Self::ConcentrationLimit => "concentration_limit",
            Self::MaxOrderSize => "max_order_size",
            Self::ReentryCooldown => "reentry_cooldown",
            Self::SessionNotAllowed => "session_not_allowed",
            Self::ExtendedHoursOpeningNotAllowed => "extended_hours_opening_not_allowed",
            Self::AuctionWindow => "auction_window",
            Self::InstrumentHalted => "instrument_halted",
            Self::WouldCrossZero => "would_cross_zero",
            Self::SellExceedsAvailable => "sell_exceeds_available",
            Self::WorkingOrderLimit => "working_order_limit",
            Self::AddBlockedByProtectiveOrder => "add_blocked_by_protective_order",
            Self::UnknownOrderInFlight => "unknown_order_in_flight",
            Self::MarketOrderNotAllowed => "market_order_not_allowed",
            Self::StaleMark => "stale_mark",
            Self::PriceOutsideCollar => "price_outside_collar",
            Self::MinRestingTime => "min_resting_time",
            Self::OppositeFillInterval => "opposite_fill_interval",
            Self::ConductLimitBreached => "conduct_limit_breached",
            Self::MaxOrdersPerDay => "max_orders_per_day",
            Self::CloseWindow => "close_window",
            Self::DiscretionaryExitRegularSessionOnly => "discretionary_exit_regular_session_only",
            Self::OwnerConfirmationRequired => "owner_confirmation_required",
            Self::GrossExposureLimit => "gross_exposure_limit",
            Self::InsufficientBuyingPower => "insufficient_buying_power",
            Self::InsufficientSettledBuyingPower => "insufficient_settled_buying_power",
            Self::LegacyPdtDayTradeBudget => "legacy_pdt_day_trade_budget",
        }
    }
}

impl Display for ReasonCode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A refusal to decide, never a verdict (ADR-0001 ES-09).
///
/// An error is neither an allow nor a deny: the executor treats it as `AGENTS.md` rule 3's safe
/// default, adding no risk while leaving the exit paths that do not need the failing input.
#[derive(Debug, Error)]
pub enum GateError {
    #[error("the working universe has not been read; the gate cannot decide an opening")]
    WorkingUniverseUnavailable,
    #[error("the risk state is older than the proposal's risk clock")]
    RiskStateStale,
    #[error("no purpose follows from this origin, side and position")]
    PurposeUnassignable,
    #[error("the quote is not sane, so no collar or risk mark can be computed")]
    QuoteUnsane,
    #[error("a configured value is out of range, or the date is outside the committed calendar")]
    ConfigOutOfRange,
    #[error("the proposed instrument has no snapshot")]
    InstrumentUnknown,
    #[error("a day-trade ledger entry precedes one already folded")]
    DayTradeLedgerOutOfOrder,
    #[error("{0} is not implemented yet (pending {1})")]
    Unimplemented(&'static str, &'static str),
    #[error(transparent)]
    Num(#[from] mandate_num::NumError),
    #[error(transparent)]
    Time(#[from] mandate_time::TimeError),
}

impl GateError {
    /// Stable reason code (ADR-0001 ES-09).
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::WorkingUniverseUnavailable => "working_universe_unavailable",
            Self::RiskStateStale => "risk_state_stale",
            Self::PurposeUnassignable => "purpose_unassignable",
            Self::QuoteUnsane => "quote_unsane",
            Self::ConfigOutOfRange => "config_out_of_range",
            Self::InstrumentUnknown => "instrument_unknown",
            Self::DayTradeLedgerOutOfOrder => "day_trade_ledger_out_of_order",
            Self::Unimplemented(_, _) => "unimplemented",
            Self::Num(e) => e.code(),
            Self::Time(e) => e.code(),
        }
    }
}

/// What a decision can be. `Deny` and `Hold` are different things (DEC-129 item 12): a hold is not
/// a denial by a limit, which is what keeps MI-1 true in the code as well as in the spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Deny,
    /// Discretionary exits only, and never converted to a `Deny` (§9.1).
    Defer,
    /// An exit the agent's mode, an `Unknown` order, or the broker stops for now (MI-1).
    Hold,
}

/// Assigned by the gate from [`Origin`], the side and the position; never taken from the proposer
/// (§9.1). See the task brief's purpose table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Open,
    Increase,
    DiscretionaryExit,
    RiskExit,
    OwnerExit,
    Protective,
}

/// The component that proposed the order, and nothing else: no verdict and no purpose.
///
/// A proposer that misreports its origin can only make its own exit stricter (a
/// [`Purpose::DiscretionaryExit`] is paced where a [`Purpose::RiskExit`] is not), or claim a risk
/// exit its own component identity contradicts. The kill-switch origins are separate variants
/// because §5.5 exempts a kill switch's orders from the agent's mode and an owner's ordinary close
/// is not a kill switch (DEC-129 item 17).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    OrderBuilder,
    GoalCompletion,
    RemovedInstrument,
    RiskEngine,
    TrimToTarget,
    StopWatchdog,
    AutomatedKillSwitch,
    OwnerClose,
    OwnerKillSwitch,
    ProtectiveLeg,
}

impl Origin {
    /// Whether §5.5's "kill-switch orders are exempt from the agent's mode" covers this origin.
    #[must_use]
    pub fn is_kill_switch(self) -> bool {
        matches!(self, Self::AutomatedKillSwitch | Self::OwnerKillSwitch)
    }
}

/// A side, orderable so it can key a set. `mandate_accounting::Side` is the same two values but
/// is not `Ord`, and ES-21 forbids a `HashSet`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RestingSide {
    Buy,
    Sell,
}

impl From<Side> for RestingSide {
    fn from(side: Side) -> Self {
        match side {
            Side::Buy => Self::Buy,
            Side::Sell => Self::Sell,
        }
    }
}

/// Which pass of §5.3 this is: the first gate decision excludes the agent's own protective and
/// resting opening orders from rules 4 to 6, because the executor cancels them first; the re-run
/// immediately before submission applies every rule in full.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatePass {
    First,
    BeforeSubmission,
}

/// One §9.1 check and what it decided, for the journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckOutcome {
    Passed(Check),
    Failed(Check, ReasonCode),
    /// The gate stopped before this check, so it says nothing about it.
    NotReached(Check),
}

/// The eight checks of §9.1, in evaluation order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Check {
    AccountAndMode,
    UniverseAndLimits,
    SessionAndHalt,
    OrderConstraints,
    MarkAndCollar,
    ConductControls,
    BuyingPowerAndExposure,
    DayTradeBudget,
}

/// What §9.6 did to a discretionary or owner exit that it may pace but never deny.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pacing {
    pub qty: Qty,
    pub limit_price: Price,
    pub marketable_limit_required: bool,
    pub applied: BTreeSet<PacingControl>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PacingControl {
    Collar,
    OrderSizeParticipation,
    DailyParticipation,
    CloseWindow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub verdict: Verdict,
    pub reason: Option<ReasonCode>,
    pub purpose: Purpose,
    pub pacing: Option<Pacing>,
    pub checks: Vec<CheckOutcome>,
    /// The figures the deciding check compared, which the reference cases pin and which §9.1
    /// journals with the decision. Reporting the verdict without them would let a gate reach the
    /// right answer from the wrong arithmetic: `MC-G05` allows and denies the same order depending
    /// only on whether the cap is 1500 or 1425.
    pub computed: Computed,
}

/// The `computed` block of a `kind: gate` reference case: every figure a check compared, `None`
/// where the gate stopped before computing it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Computed {
    pub instrument_total: Option<Usd>,
    pub cap: Option<Usd>,
    pub order_usd: Option<Usd>,
    pub gross: Option<Usd>,
    pub gross_limit: Option<Usd>,
    pub orders_today: Option<u32>,
    pub last_exit_fill_at: Option<UtcNanos>,
    pub instrument: Option<AssetId>,
}

impl Computed {
    /// One figure by the name the reference cases use, as the canonical text they compare against.
    /// An unknown key is `None`, which the harness reports rather than skipping (DEC-85).
    #[must_use]
    pub fn get(&self, key: &str) -> Option<String> {
        match key {
            "instrument_total" => self.instrument_total.map(|v| v.to_string()),
            "cap" => self.cap.map(|v| v.to_string()),
            "order_usd" => self.order_usd.map(|v| v.to_string()),
            "gross" => self.gross.map(|v| v.to_string()),
            "gross_limit" => self.gross_limit.map(|v| v.to_string()),
            "orders_today" => self.orders_today.map(|v| v.to_string()),
            "last_exit_fill_at" => self.last_exit_fill_at.map(|v| v.to_string()),
            "instrument" => self.instrument.as_ref().map(|v| v.as_str().to_owned()),
            _ => None,
        }
    }
}

/// The organization's settings. `test_default` in the reference cases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateConfig {
    pub price_floor: Usd,
    pub liquidity_floor_usd: Usd,
    pub crypto_liquidity_floor_usd: Usd,
    pub collar_liquid_threshold_usd: Usd,
    pub collar_liquid_x: Fraction,
    pub collar_other_x: Fraction,
    pub collar_crypto_x: Fraction,
    pub collar_passive_band: Fraction,
    pub opposite_fill_interval_s: u32,
    pub min_resting_time_s: u32,
    pub order_to_fill_max: u32,
    pub order_to_fill_min_orders: u32,
    pub order_size_participation: Fraction,
    pub daily_participation: Fraction,
    pub close_window_minutes: u32,
    pub legacy_pdt_equity_threshold: Usd,
    /// §3.2 item 6's "configured age", which `test_default` does not carry (DEC-129 items 10, 26).
    pub etp_classification_max_age_s: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountState {
    Active,
    ClosingOnly,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayTradeRegime {
    IntradayMargin { maintenance_excess: Usd },
    LegacyPdt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exchange {
    Nasdaq,
    Nyse,
    Arca,
    Amex,
    Bats,
    Otc,
    Other,
}

impl Exchange {
    /// §3.1: `NASDAQ`, `NYSE`, `ARCA`, `AMEX` and `BATS` are eligible; `OTC` and anything else are
    /// not.
    #[must_use]
    pub fn is_eligible(self) -> bool {
        matches!(
            self,
            Self::Nasdaq | Self::Nyse | Self::Arca | Self::Amex | Self::Bats
        )
    }
}

/// §3.2 item 6. An ETP the classification source has not classified is treated as complex, so the
/// rule fails closed (DEC-129 item 10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EtpClass {
    Plain,
    Complex,
    Unclassified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaneQuote {
    pub bid: Price,
    pub ask: Price,
    pub at: UtcNanos,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ClientOrderId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct GroupId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct AgentId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkingOrder {
    pub agent: AgentId,
    pub instrument: AssetId,
    pub side: Side,
    pub max_cost: Usd,
    pub open_qty: Qty,
    pub protective: bool,
    pub opening: bool,
    pub submitted_on: Date,
}

/// Derived inside the gate from `now` and `mandate-time`'s committed calendar, never a caller's
/// label: a check the caller can defeat is not an independent gate (DEC-129 item 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionAt {
    pub session: Session,
    pub start: UtcNanos,
    pub end: UtcNanos,
    pub opening_auction: bool,
    pub close_window: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Session {
    Overnight,
    PreMarket,
    Regular,
    AfterHours,
    Continuous,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountSnapshot {
    pub account_type: AccountType,
    pub state: AccountState,
    pub crypto_active: bool,
    pub regime: DayTradeRegime,
    pub equity: Usd,
    pub prior_close_equity: Usd,
    /// `Account::buying_power(reservations)`: already net of reservations and pending charges, so
    /// check 7 subtracts neither again (DEC-129 item 8).
    pub model_buying_power: Usd,
    pub broker_buying_power: Usd,
    pub broker_non_marginable_buying_power: Usd,
    pub positions: BTreeMap<AssetId, Qty>,
    pub market_values: BTreeMap<AssetId, Usd>,
    pub working_orders: BTreeMap<ClientOrderId, WorkingOrder>,
    pub unknown_orders: BTreeSet<AssetId>,
    pub related_account_resting: BTreeMap<AssetId, BTreeSet<RestingSide>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSnapshot {
    pub agent: AgentId,
    pub mode: AgentMode,
    pub instrument_restrictions: BTreeMap<AssetId, BTreeSet<InstrumentRestriction>>,
    pub positions: BTreeMap<AssetId, Qty>,
    pub market_values: BTreeMap<AssetId, Usd>,
    pub working_orders: BTreeSet<ClientOrderId>,
    pub instrument_groups: BTreeMap<AssetId, GroupId>,
    pub last_exit_fill_at: BTreeMap<AssetId, UtcNanos>,
    pub orders_today: u32,
    pub day_trades: DayTradeLedger,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstrumentSnapshot {
    pub instrument: AssetId,
    pub asset_class: AssetClass,
    pub exchange: Option<Exchange>,
    pub status_active: bool,
    pub tradable: bool,
    pub fractionable: bool,
    pub ipo: bool,
    pub ptp_no_exception: bool,
    pub etp: EtpClass,
    pub etp_classified_at: Option<UtcNanos>,
    pub prior_close: Option<Price>,
    pub median_dollar_volume_20d: Option<Usd>,
    pub median_dollar_volume_30d: Option<Usd>,
    pub min_order_size: Qty,
    pub halted: bool,
    pub status_feed_current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketSnapshot {
    pub quote: Option<SaneQuote>,
    pub last_trade: Option<(Price, UtcNanos)>,
    pub trailing_5m_volume: Option<Qty>,
    pub adv_20d: Option<Qty>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConductState {
    pub filled_today: BTreeMap<AssetId, u32>,
    pub orders_today_per_instrument: BTreeMap<AssetId, u32>,
    pub last_opposite_fill_at: BTreeMap<(AssetId, RestingSide), UtcNanos>,
    pub participation_today: BTreeMap<AssetId, Qty>,
    pub resting_since: BTreeMap<ClientOrderId, UtcNanos>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProposedKind {
    Plain,
    Bracket {
        take_profit: Price,
        stop: Price,
    },
    Ioc,
    /// Reducing orders only, and only in the regular session outside an auction window with current
    /// status data (§5.1, §4.3). The variant exists so the gate can refuse one where those
    /// conditions do not hold, rather than the type making the refusal untestable.
    Market,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeInForce {
    Day,
    Gtc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposedOrder {
    pub instrument: AssetId,
    pub side: Side,
    pub qty: Qty,
    pub limit_price: Price,
    pub kind: ProposedKind,
    pub tif: TimeInForce,
    pub extended_hours: bool,
    pub origin: Origin,
    pub owner_confirmed_bid: Option<Price>,
    pub client_order_id: ClientOrderId,
    /// `round(estimated fees, 2, ceiling)` from the pinned fee configuration; 0 for a crypto buy
    /// (§9.5, DEC-129 item 16).
    pub fee_reservation: Usd,
}

#[derive(Debug, Clone)]
pub struct GateInput<'a> {
    /// The risk clock's latest tick. The gate reads no wall clock (ES-21).
    pub now: UtcNanos,
    pub pass: GatePass,
    pub config: &'a GateConfig,
    pub mandate: &'a ValidatedMandate,
    pub risk: &'a RiskSnapshot,
    pub account: &'a AccountSnapshot,
    pub agent: &'a AgentSnapshot,
    pub instrument: &'a InstrumentSnapshot,
    pub market: &'a MarketSnapshot,
    pub conduct: &'a ConductState,
    pub universe: &'a WorkingUniverse,
    pub proposed: &'a ProposedOrder,
}

/// §9.6's minimum resting time is a rule about cancelling, and its exemption for a cancel that
/// precedes a risk-reducing order is a risk judgement, so the gate decides it too (DEC-129
/// item 20).
#[derive(Debug, Clone)]
pub struct CancelInput<'a> {
    pub now: UtcNanos,
    pub config: &'a GateConfig,
    pub order: &'a WorkingOrder,
    pub resting_since: UtcNanos,
    pub precedes_risk_reducing_order: bool,
    pub marketable: bool,
}

/// §9.2's window, count and `required`, folded here because no other crate has a reason to know the
/// rule (DEC-129 item 6).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DayTradeLedger {
    pub window_count: u32,
    pub flagged_pattern_day_trader: bool,
    pub sold_earlier_today: BTreeSet<AssetId>,
    pub open_same_day_positions: BTreeSet<AssetId>,
}

/// The whole gate: §9.1's eight checks in order, stopping at the first failure.
///
/// # Errors
/// Returns [`GateError`] when an input makes the decision impossible rather than negative — an
/// unread working universe above all, which is never an allow.
pub fn evaluate(input: &GateInput<'_>) -> Result<Decision, GateError> {
    let _ = input;
    Err(GateError::Unimplemented("evaluate", "E6-3"))
}

/// §9.6's minimum resting time, and §5.3 rule 5's cancels before a risk-reducing order.
///
/// # Errors
/// Returns [`GateError`] when the input cannot be evaluated.
pub fn evaluate_cancel(input: &CancelInput<'_>) -> Result<Decision, GateError> {
    let _ = input;
    Err(GateError::Unimplemented("evaluate_cancel", "E6-8"))
}

/// The purpose §9.1 assigns to a proposal, from its origin, side and the agent's position.
///
/// # Errors
/// Returns [`GateError::PurposeUnassignable`] when no row of the table applies.
pub fn assign_purpose(
    origin: Origin,
    side: Side,
    qty: Qty,
    agent_position: Qty,
) -> Result<Purpose, GateError> {
    let _ = (origin, side, qty, agent_position);
    Err(GateError::Unimplemented("assign_purpose", "E6-3"))
}

/// The session, auction windows and close window at `now`, from the committed calendar.
///
/// # Errors
/// Returns [`GateError::ConfigOutOfRange`] for a date the calendar does not cover.
pub fn session_at(
    now: UtcNanos,
    config: &GateConfig,
    asset_class: AssetClass,
) -> Result<SessionAt, GateError> {
    let _ = (now, config, asset_class);
    Err(GateError::Unimplemented("session_at", "E6-6"))
}

/// One `trim_to_target` sell per position that is far enough above `factor × cap` (§5.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrimProposal {
    pub instrument: AssetId,
    pub qty: Qty,
    pub purpose: Purpose,
}

/// §5.5's `trim_to_target` proposals, rounded **up** to the increment so a trim never leaves the
/// position above its target.
///
/// # Errors
/// Returns [`GateError`] when a figure cannot be computed exactly.
pub fn trim_proposals(
    now: UtcNanos,
    config: &GateConfig,
    mandate: &ValidatedMandate,
    risk: &RiskSnapshot,
    agent: &AgentSnapshot,
    instruments: &BTreeMap<AssetId, InstrumentSnapshot>,
) -> Result<Vec<TrimProposal>, GateError> {
    let _ = (now, config, mandate, risk, agent, instruments);
    Err(GateError::Unimplemented("trim_proposals", "E6-4"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlattenInitiator {
    RiskLimit,
    Owner,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentPosition {
    pub agent: AgentId,
    pub instrument: AssetId,
    pub asset_class: AssetClass,
    pub qty: Qty,
}

#[derive(Debug, Clone)]
pub struct FlattenInput<'a> {
    pub agent: AgentId,
    pub open_orders: &'a BTreeMap<ClientOrderId, WorkingOrder>,
    pub agent_positions: &'a [AgentPosition],
    /// The broker's own quantities, which the plan must **ignore**: it sells exactly the agent's
    /// sub-ledger, never the broker's position (§5.5). `MC-F01` holds 15 at the broker against a
    /// sub-ledger of 10, and the field is here so a test can prove the difference is untouched.
    pub broker_positions: &'a BTreeMap<AssetId, Qty>,
    pub session: Session,
    pub initiator: FlattenInitiator,
    pub owner_confirmed_bid: Option<Price>,
    pub max_exit_offset: Fraction,
    /// The owner's explicit floor, which `ref.py` takes ahead of the computed one.
    pub owner_floor_price: Option<Price>,
}

/// How a flatten's sell is priced: by the session alone (DEC-129 item 21).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlattenPricing {
    MarketOrLadder,
    ExitPriceLadder,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlattenSell {
    pub instrument: AssetId,
    pub qty: Qty,
    pub pricing: FlattenPricing,
    pub floor_price: Option<Price>,
    pub rests_at_floor_then_waits_for_open: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeferredSell {
    pub instrument: AssetId,
    pub qty: Qty,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlattenPlan {
    pub mode_applied_first: AgentMode,
    pub purpose: Purpose,
    pub cancel_client_order_ids: Vec<ClientOrderId>,
    /// Always false for an agent scope: never the broker's cancel-all (§5.5).
    pub cancel_all_endpoint: bool,
    /// Always false for an agent scope: never the broker's close-position (§5.5).
    pub close_position_endpoint: bool,
    pub sells: Vec<FlattenSell>,
    pub deferred_sells: Vec<DeferredSell>,
}

/// The agent-scoped kill switch's plan (§5.5): mode first, this agent's orders only, the agent's
/// sub-ledger quantity, never the account-wide endpoints.
///
/// # Errors
/// Returns [`GateError`] when a floor price cannot be computed exactly.
pub fn agent_flatten(input: &FlattenInput<'_>) -> Result<FlattenPlan, GateError> {
    let _ = input;
    Err(GateError::Unimplemented("agent_flatten", "E6-3"))
}

/// The day's surveillance figures per workspace (§9.6). It states figures and flags thresholds; it
/// makes no judgement, because the platform does not supervise users' trading.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SurveillanceInput {
    pub orders: BTreeMap<(AgentId, AssetId), OrderCounts>,
    pub opposite_side_rests: BTreeMap<AssetId, BTreeSet<AgentId>>,
    pub close_window_orders: BTreeMap<(AgentId, AssetId), u32>,
    pub end_of_day_market_values: BTreeMap<(AgentId, AssetId), Usd>,
    pub agent_equity: BTreeMap<AgentId, Usd>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OrderCounts {
    pub submitted: u32,
    pub filled: u32,
    pub cancels_excluded: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurveillanceReport {
    pub day: Date,
    pub order_to_fill: BTreeMap<(AgentId, AssetId), u32>,
    pub self_trade_instruments: BTreeSet<AssetId>,
    pub close_window_orders: BTreeMap<(AgentId, AssetId), u32>,
    pub concentration: BTreeMap<(AgentId, AssetId), Fraction>,
    pub breaches: BTreeSet<SurveillanceBreach>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SurveillanceBreach {
    OrderToFill,
    SelfTrade,
    CloseWindow,
    Concentration,
}

/// The §9.6 daily surveillance report.
///
/// # Errors
/// Returns [`GateError`] when a ratio cannot be computed exactly.
pub fn surveillance(
    day: Date,
    config: &GateConfig,
    input: &SurveillanceInput,
) -> Result<SurveillanceReport, GateError> {
    let _ = (day, config, input);
    Err(GateError::Unimplemented("surveillance", "E6-8"))
}
