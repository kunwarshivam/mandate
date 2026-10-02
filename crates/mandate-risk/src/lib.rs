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
//! E6-3 implements [`evaluate`] — the eight checks in order, purpose assignment, check 1 whole
//! (the account's state, then the mode rule), the working universe, the mandate limits of mandate
//! spec §5.3 with the account's own 1× bound, and §5.3 rules 3 and 9 — and [`agent_flatten`].
//! E6-9 adds check 3's halt and, at check 4, §4.4's "no market orders" under a presumed halt: a
//! market opening is denied and a market exit is re-priced as a marketable limit (DEC-129 items 24
//! and 28). E6-7 adds check 2's eligibility floor (§3.2, items 1 to 7 in list order), which makes
//! checks 1 and 2 whole. E6-6 adds [`session_at`] and check 3's session rules, the rest of check 4
//! (§5.3 rules 2 and 4 to 8, and §5.1's limit-only openings), check 7's buying power with the fee
//! reservation, and check 8's `legacy_pdt` budget, which makes checks 3, 4, 7 and 8 whole, and
//! its second slice [`fold_day_trades`], the budget's ledger folded account-wide (DEC-259). E6-8
//! adds check 5 (mark freshness and the collar), check 6's market-conduct controls, the pacing an
//! allowed exit is sent with, [`evaluate_cancel`]'s minimum resting time and the [`surveillance`]
//! report (§9.6, DEC-163). E6-10 adds §3.2 item 7's "USD pairs only" at check 2 (DEC-254,
//! DEC-255), which makes check 2 whole for crypto and so every check whole for both asset classes:
//! DEC-129 item 29's fail-closed refusal of an opening a missing check might have denied has
//! nothing left to refuse, and [`evaluate`] decides every proposal itself. E6-4 adds
//! [`trim_proposals`], mandate spec §5.5's `trim_to_target` (DEC-65, DEC-399), the last entry point
//! #136 stubbed. E6-6's last row refuses a proposal of zero quantity as
//! [`GateError::ZeroQuantity`] before any check (DEC-401). [`GateError::Unimplemented`] stays for
//! DEC-129 item 27's refusal of an opening that breaks a §5.3 rule with no registered reason code,
//! and, until DEC-423's implementation PR, for a trim that sells everything left to sell below the
//! instrument's minimum order size.

use core::fmt::Display;
use std::collections::{BTreeMap, BTreeSet};

use mandate_num::{Fraction, Price, Qty, Usd};
use mandate_time::{Date, UtcNanos};
use thiserror::Error;

mod account_rules;
mod conduct;
mod daytrades;
mod flatten;
mod floor;
mod gate;
mod limits;
mod session;
#[doc(hidden)]
pub mod spec_types;
mod surveillance;
mod trim;

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
/// registered the gate denies without minting one. `CryptoPairNotUsd` is §3.2 item 7's "USD pairs
/// only" (DEC-255).
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
    CryptoPairNotUsd,
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
    pub const ALL: [Self; 39] = [
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
        Self::CryptoPairNotUsd,
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
            Self::CryptoPairNotUsd => "crypto_pair_not_usd",
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
    #[error("a fill sells more of an instrument than the account held")]
    DayTradeLedgerInconsistent,
    /// A proposal of zero quantity is no order: it reduces and adds nothing, the broker refuses
    /// it, and the gate refuses to decide it rather than allow it (DEC-401).
    #[error("the proposal is for a quantity of zero, which is no order")]
    ZeroQuantity,
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
            Self::DayTradeLedgerInconsistent => "day_trade_ledger_inconsistent",
            Self::ZeroQuantity => "zero_quantity",
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

/// How an allowed order must be sent when the gate constrains its form but never denies it: §9.6's
/// pacing of a discretionary or owner exit (the collar, the participation caps, the close window),
/// and §4.4 and §5.6's re-pricing of a market-order exit as a marketable limit under a real or
/// presumed halt, which applies to every reducing purpose, risk exits, protective legs and
/// kill-switch exits included (DEC-129 items 28 and 31).
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

/// What a pair is quoted in, for §3.2 item 7's "USD pairs only" (DEC-254).
///
/// `Usd` is the only value that can admit a crypto opening, and nothing produces it by default:
/// the type has no `Default`, [`InstrumentSnapshot::quote_currency`] is `None` when the instrument
/// master stated no quote currency, and every stated code but exactly `USD` is `Other` — a
/// stablecoin (USDT, USDC), a fiat other than USD, a crypto asset, or a code the loader does not
/// recognise. A USD-pegged stablecoin is not USD: the floor's dollar figures were written for USD.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteCurrency {
    Usd,
    Other,
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
    /// §9.6's self-trade input: per instrument, every side on which an order rests in any account
    /// of the owner's related-accounts group (by default every account in the workspace), this
    /// account included, whichever agent placed it, the deciding agent's own included, and of any
    /// purpose, protective included. The gate filters nothing: any listed side opposite an opening
    /// denies it. In this account a non-protective order already denies an opening at check 4
    /// (§5.3 rule 6), so here the set adds the resting protective orders (DEC-163 item 11).
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
    /// The quote half of the pair (the `USD` of `BTC/USD`), which §3.2 item 7 reads for crypto
    /// only; `None` when the instrument master did not state it, which admits no crypto opening. A
    /// US equity is quoted in USD and the gate never reads the field for one.
    pub quote_currency: Option<QuoteCurrency>,
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

/// What the executor folds from the account streams for §9.6 (DEC-163 item 11). "Today" is the
/// risk day, 00:00 to 00:00 America/New_York (mandate §5.3), which holds an equity's whole trading
/// day; a per-day field starts from empty at its first instant.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConductState {
    /// Per instrument, the deciding agent's orders that filled at least in part today, each order
    /// once however many fills it took, so partial fills cannot dilute the ratio. Only this agent's
    /// orders, in the account it trades (§9.6: "per agent per instrument per day"); reset daily.
    pub filled_today: BTreeMap<AssetId, u32>,
    /// Per instrument, the deciding agent's orders submitted today, of any purpose, each
    /// `client_order_id` once, rejected ones included, less those an exit sequence or a kill switch
    /// canceled (§9.6); the proposal being decided is not among them. Only this agent's orders, in
    /// the account it trades; reset daily.
    pub orders_today_per_instrument: BTreeMap<AssetId, u32>,
    /// Per instrument and side, the latest fill on that side by any agent in any account of the
    /// related-accounts group. Never reset: a fill older than `opposite_fill_interval_s` simply no
    /// longer blocks, across midnight too.
    pub last_opposite_fill_at: BTreeMap<(AssetId, RestingSide), UtcNanos>,
    /// Per instrument, the quantity filled today on both sides by every agent in every account of
    /// the related-accounts group, plus the open quantity of their working orders in it; the
    /// proposal being decided is not included. Reset daily.
    pub participation_today: BTreeMap<AssetId, Qty>,
    /// Per working order in this account, the instant the broker accepted it, kept until the order
    /// is done. [`evaluate`] does not read it; the executor passes the canceled order's instant to
    /// [`evaluate_cancel`] as [`CancelInput::resting_since`].
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
    /// The risk clock's latest tick, as for [`GateInput::now`].
    pub now: UtcNanos,
    pub config: &'a GateConfig,
    /// The working order to cancel, as it stands in [`AccountSnapshot::working_orders`].
    pub order: &'a WorkingOrder,
    /// The instant the broker accepted `order`, from [`ConductState::resting_since`].
    pub resting_since: UtcNanos,
    /// Whether the cancel is a step toward a risk-reducing order: an exit sequence, a kill switch,
    /// or clearing the way for a risk exit, a protective order or an owner exit. §9.6 exempts it,
    /// and it is the only way an exit sequence's or kill switch's cancel is exempt, since the
    /// executor sets it and this input has no cause field (DEC-163 item 7).
    pub precedes_risk_reducing_order: bool,
    /// Whether `order`'s limit trades against the current quote (a buy at or above the ask, a sell
    /// at or below the bid). With no usable quote it is `false`, so an order not known to be
    /// marketable is held to the resting time.
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

/// A day trade on one trading day in one instrument: one of the broker's reported day trades
/// before today, or one [`fold_day_trades`] counted today.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayTrade {
    pub date: Date,
    pub instrument: AssetId,
}

/// One fill on the account, by any agent: the fold's view of an execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountFill {
    pub at: UtcNanos,
    pub instrument: AssetId,
    pub asset_class: AssetClass,
    pub side: Side,
    pub qty: Qty,
}

/// What §9.2's `legacy_pdt` ledger is folded from. Every field is the **account's**, across every
/// agent trading it, because the budget is the account's (DEC-150 item 7): a ledger folded from one
/// agent's fills would undercount it.
#[derive(Debug, Clone)]
pub struct DayTradeInput<'a> {
    /// The risk clock's latest tick; today is its trading day (§2.2's trade date).
    pub now: UtcNanos,
    /// Day trades on trading days before today, as the broker reported them or an earlier fold
    /// counted them. Only those inside the window count; one dated today or later is an error.
    pub earlier: &'a [DayTrade],
    /// The account's US-equity quantity per instrument at the start of today: the shares held
    /// overnight, which a sell takes first.
    pub held_overnight: &'a BTreeMap<AssetId, Qty>,
    /// Today's fills on the account, in execution order, none after `now`. Crypto fills are read
    /// for their order only, since crypto never counts.
    pub fills_today: &'a [AccountFill],
    /// The broker's pattern-day-trader flag, passed through to the ledger.
    pub flagged_pattern_day_trader: bool,
}

/// The ledger check 8 reads, and today's day trades for the executor to journal and to pass back
/// as [`DayTradeInput::earlier`] once the day is over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayTradeFold {
    pub ledger: DayTradeLedger,
    pub today: Vec<DayTrade>,
}

/// §9.2's `legacy_pdt` ledger, folded account-wide: the window is today plus the four prior trading
/// days of the committed calendar, a sell takes shares held overnight first, each same-day
/// open-then-close counts once, a repurchase after a same-day sale of overnight shares is not a day
/// trade (DEC-269), fractional day trades count, and crypto never does (DEC-129 item 6, DEC-259).
///
/// # Errors
/// [`GateError::DayTradeLedgerOutOfOrder`] for fills out of execution order, after `now`, or
/// (for an equity) outside today's trading day, and for an earlier day trade dated today or
/// later; [`GateError::DayTradeLedgerInconsistent`] for a sell of more than the account held;
/// [`GateError::ConfigOutOfRange`] for a date outside the committed calendar. Every error is a
/// refusal to decide an opening, never a smaller count, and **never more than that**: the caller
/// (E7-3's executor) must treat a fold error as refusing openings only, and still route exits and
/// protective orders, which check 8 never reads (`AGENTS.md` rule 13).
pub fn fold_day_trades(input: &DayTradeInput<'_>) -> Result<DayTradeFold, GateError> {
    daytrades::fold(input)
}

/// The whole gate: §9.1's eight checks in order, stopping at the first failure.
///
/// # Errors
/// Returns [`GateError`] when an input makes the decision impossible rather than negative — an
/// unread working universe above all, which is never an allow.
pub fn evaluate(input: &GateInput<'_>) -> Result<Decision, GateError> {
    gate::evaluate(input)
}

/// §9.6's minimum resting time, and §5.3 rule 5's cancels before a risk-reducing order.
///
/// # Errors
/// Returns [`GateError`] when the input cannot be evaluated.
pub fn evaluate_cancel(input: &CancelInput<'_>) -> Result<Decision, GateError> {
    conduct::evaluate_cancel(input)
}

/// The purpose §9.1 assigns to a proposal, from its origin, side and the agent's position. A sell
/// above the position is typed [`Purpose::Open`]: it would open a short, and check 4 denies it
/// `would_cross_zero`.
///
/// # Errors
/// None today: every origin, side and position has a row of the table. The `Result` is the
/// signature #136 fixed, and [`GateError::PurposeUnassignable`] stays for a row a later origin
/// might lack.
pub fn assign_purpose(
    origin: Origin,
    side: Side,
    qty: Qty,
    agent_position: Qty,
) -> Result<Purpose, GateError> {
    Ok(gate::assign_purpose(origin, side, qty, agent_position))
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
    session::derive(now, config, asset_class)
}

/// One `trim_to_target` sell per position that is far enough above `factor × cap` (§5.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrimProposal {
    pub instrument: AssetId,
    pub qty: Qty,
    pub purpose: Purpose,
}

/// §5.5's `trim_to_target` proposals, rounded **up** to the increment so a trim never leaves the
/// position above its target, net of the agent's own open non-protective sells in the instrument
/// in `account`, so a trim already working is never proposed again. The guards of DEC-65 and the
/// readings of DEC-399 are in the `trim` module's doc.
///
/// # Errors
/// Returns [`GateError`] when a figure cannot be computed exactly, when the calendar does not cover
/// `now` for an equity, and [`GateError::InstrumentUnknown`] for a position to trim with no
/// instrument snapshot.
pub fn trim_proposals(
    now: UtcNanos,
    config: &GateConfig,
    mandate: &ValidatedMandate,
    risk: &RiskSnapshot,
    agent: &AgentSnapshot,
    account: &AccountSnapshot,
    instruments: &BTreeMap<AssetId, InstrumentSnapshot>,
) -> Result<Vec<TrimProposal>, GateError> {
    trim::proposals(now, config, mandate, risk, agent, account, instruments)
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
    flatten::agent_flatten(input)
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
    /// Not raised in v1: §3.3 supplies no concentration threshold, and one the platform chose
    /// would be compliance-visible, so the report states concentration as a figure only (DEC-163
    /// item 8).
    Concentration,
}

/// The §9.6 daily surveillance report: the day's figures, each threshold crossed flagged, and no
/// judgement.
///
/// # Errors
/// Returns [`GateError`] when a ratio cannot be computed exactly.
pub fn surveillance(
    day: Date,
    config: &GateConfig,
    input: &SurveillanceInput,
) -> Result<SurveillanceReport, GateError> {
    surveillance::report(day, config, input)
}
