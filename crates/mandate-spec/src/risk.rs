//! The risk state ([mandate spec §5](../../../docs/specs/mandate.md#5-risk-state-and-limits)): a
//! transition function over the account stream's inputs, in `seq` order.
//!
//! The account sub-ledger fold stays in `mandate-accounting` and the executor wires the two together
//! (§5: "the executor computes the risk state from the account ledger"). What this crate owns is the
//! transition: how equity, the high-water mark, the ladder, the daily loss, the lifetime floor, the
//! restrictions, and the effective mode move when an input arrives, and which events that emits.
//!
//! Time arrives with the input and is never read (ES-21). Durations are integer seconds, credited over
//! the interval between inputs by the state at the interval's **start** (§5.2), and for an equity the
//! clocks that matter run in regular-session time, which the caller's calendar supplies.

use std::collections::{BTreeMap, BTreeSet};

use mandate_domain::{AgentMode, AssetClass, AssetId, MarketSession, Side};
use mandate_num::{Price, Qty, Ratio, Usd};
use mandate_time::{Date, UtcNanos};

use crate::document::{LimitAction, OnComplete};
use crate::validate::ValidatedMandate;
use crate::{SchemaDec, SpecError};

/// The hard-trigger multiple of §5.6: a loss at 1.25 times a limit's level escalates at once and
/// latches only on a second sane quote (DEC-63).
pub const HARD_TRIGGER_MULTIPLE: &str = "1.25";

/// The longest §5.6 lets a limit confirm for, which the schema also caps.
pub const MAX_BREACH_CONFIRM_S: u32 = 300;

/// Which limit an event is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LimitKey {
    MaxDailyLoss,
    /// A rung of the ladder, by its index in `risk.drawdown_ladder`, which is how §5.10 names it
    /// (`drawdown_ladder[i]`).
    DrawdownRung(u8),
    LifetimeFloor,
    /// Not a limit but confirmed the same way (§3.1, §5.6).
    ProfitStop,
}

impl LimitKey {
    /// The name §5.10 journals, `drawdown_ladder[i]` included.
    pub fn journal_name(self) -> String {
        match self {
            Self::MaxDailyLoss => "max_daily_loss".to_owned(),
            Self::DrawdownRung(i) => format!("drawdown_ladder[{i}]"),
            Self::LifetimeFloor => "lifetime_floor".to_owned(),
            Self::ProfitStop => "profit_stop".to_owned(),
        }
    }
}

/// The four latches of §5.8, which lift only by their defined path (MI-3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Latch {
    DailyLoss,
    DrawdownLadder,
    LifetimeFloor,
}

/// An agent-level restriction (§5.9). Each lifts independently, and the effective mode is the
/// strictest, which is a maximum over [`AgentMode`] (MI-6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Restriction {
    DailyLoss,
    DrawdownExitsOnly,
    DrawdownFlatten,
    LifetimeFloor,
    /// Applied at once on a 1.25x loss and cleared by a sane quote below it, so one bad print never
    /// latches anything (§5.6, DEC-63).
    HardBreach,
    GoalComplete,
    Retired,
}

impl Restriction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DailyLoss => "daily_loss",
            Self::DrawdownExitsOnly => "drawdown_exits_only",
            Self::DrawdownFlatten => "drawdown_flatten",
            Self::LifetimeFloor => "lifetime_floor",
            Self::HardBreach => "hard_breach",
            Self::GoalComplete => "goal_complete",
            Self::Retired => "retired",
        }
    }

    /// The mode this restriction asks for (§5.4, §5.5, §5.7, §5.9, §3.1).
    pub fn mode(self) -> AgentMode {
        match self {
            Self::DailyLoss | Self::DrawdownExitsOnly | Self::HardBreach | Self::GoalComplete => {
                AgentMode::ExitsOnly
            }
            Self::DrawdownFlatten | Self::LifetimeFloor => AgentMode::Paused,
            Self::Retired => AgentMode::Stopped,
        }
    }
}

/// A restriction on one instrument, which blocks opening and increasing in that instrument only
/// (§5.9). It never restricts the agent, and MI-1 still holds for the instrument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InstrumentRestriction {
    StaleMark,
    RemovedInstrument,
}

impl InstrumentRestriction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StaleMark => "stale_mark",
            Self::RemovedInstrument => "removed_instrument",
        }
    }
}

/// What the agent's state was when a step finished.
///
/// This is the type `mandate-risk` and `mandate-builder` read. It carries `latched`, `active_rungs`,
/// and `inherited_loss` as well as the reported figures, because the gate's `trim_to_target` guard
/// needs rung activation, the floor and acknowledgment paths need the latches, and the independent
/// oracle rebuilds exactly those three from the journal — so a limit that latches without journalling
/// is caught once here rather than per consumer (DEC-128 item 21).
///
/// The field is `day_start_equity`, the name every `risk_state` reference case uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub agent_equity: Usd,
    pub high_water_mark: Usd,
    pub drawdown: Ratio,
    pub day_start_equity: Usd,
    pub daily_pnl: Usd,
    pub daily_pnl_fraction: Ratio,
    pub capital_base: Usd,
    pub inherited_loss: Usd,
    pub size_factor: Ratio,
    pub latched: BTreeSet<LimitKey>,
    /// The active `scale_sizes` rungs, each with how long it has been active, in the session seconds
    /// §5.5 counts (regular-session for an equity, all time for crypto).
    ///
    /// A key present means the rung is active; the value is what §5.5's `trim_to_target` guard needs,
    /// since a trim waits until the rung has been active for `breach_confirm_s`. **This crate folds
    /// it** rather than leaving the gate to accumulate a duration of its own: the risk state is the
    /// only thing that steps the risk clock, and a second timekeeper could disagree with it about
    /// when a rung became active (the coordinator's ruling on #136).
    pub active_rungs: BTreeMap<u8, u64>,
    pub restrictions: BTreeSet<Restriction>,
    pub agent_mode: AgentMode,
    pub instrument_restrictions: BTreeSet<InstrumentRestriction>,
    pub net_contributed: Usd,
}

/// What the agent held when the state opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opening {
    pub position_qty: Qty,
    pub avg_cost: Price,
    pub asset_class: AssetClass,
    pub at: UtcNanos,
    /// The connection's loss carry, which a new agent starts with so that retiring and redeploying
    /// cannot reset the floor (§5.7, MI-14).
    pub inherited_loss_usd: Usd,
    /// How long a held instrument may go without a sane mark before `stale_mark` (§5.2).
    pub mark_max_age_s: u32,
}

/// One risk input (§5.2), always with the risk clock and the session it arrived in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub at: UtcNanos,
    pub session: MarketSession,
    pub input: Input,
}

/// The account-stream events the risk state folds (§5.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// A risk mark. For an equity only a regular-session sane mark updates equity; crypto uses sane
    /// marks at all times (§5.2, trading spec §8.2).
    Mark {
        bid: Price,
        sane: bool,
    },
    Fill {
        side: Side,
        qty: Qty,
        price: Price,
    },
    /// 00:00 America/New_York: E0 becomes E (§5.4).
    RiskDayStarted,
    OwnerAcknowledged {
        restriction: Latch,
    },
    /// Applied when its version applies, after time is settled at that instant, with no time passing
    /// (§5.1).
    AllocationChange {
        delta_usd: Usd,
    },
    /// A copied `ClockAdvanced`: confirmation continues when no marks arrive (§5.6).
    Clock,
    UniverseChanged {
        instrument: AssetId,
        change: UniverseChange,
        reason: RemovalReason,
    },
    /// A version raising `max_loss_from_allocation`, the only path that lifts the floor (§5.7).
    FloorLoosened {
        new_max_loss_from_allocation: SchemaDec,
        confirmed_at: UtcNanos,
        independent_approval: bool,
    },
    AgentStopped {
        reason: StopReason,
    },
    /// The goal completed, so the mandate's `on_complete` applies (§3.1).
    GoalComplete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UniverseChange {
    Admitted,
    Removed,
}

/// Why an instrument left or rejoined the working universe (§5.10, §8.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RemovalReason {
    ThesisAdmitted,
    ThesisExpired,
    ThesisInvalidated,
    LineageRetired,
    EligibilityLost,
    OperatorHalt,
    VersionApplied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StopReason {
    OwnerStop,
    ProfitStopReached,
    EndDate,
    GoalComplete,
}

/// Why a goal ended (§3.1, §5.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GoalReason {
    ProfitStopReached,
    TargetQty,
    MaxSpend,
    EndDate,
}

/// What follows a completed goal: the `on_complete` the owner chose, or the one outcome §3.1 gives a
/// `profit_stop`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ThenAction {
    Applied(OnComplete),
    DiscretionaryExitAllThenRetire,
}

/// Why a limit triggered, when it was not plain confirmation (§5.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TriggerReason {
    HardTrigger,
    /// The 1.25x level seen on one sane quote: `exits_only` now, nothing latched yet (§5.6).
    HardBreachPending,
    ResolvedAtRollover,
    NewDayBreach,
    AfterReset,
}

/// Why a limit or a rung lifted (§5.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LiftReason {
    OwnerAcknowledged,
    VersionLoosened,
    /// A sane quote below the hard level. Carried **instead of** an action, which is how MC-R19 step 3
    /// distinguishes it from a scale rung lifting.
    HardBreachCleared,
}

/// Why an instrument restriction changed (§5.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RestrictionReason {
    NoSaneMark,
    SaneMark,
    Removal(RemovalReason),
}

/// An account-stream event the risk state emits (§5.10).
///
/// The shapes are the reference cases': a `scale_sizes` rung carries an action and no reason, a latch
/// carries a reason only when it is not plain confirmation, a hard-breach lift carries
/// [`LiftReason::HardBreachCleared`] and no action, and `GoalCompleted` comes in two forms — the
/// `on_complete` one for the goal-complete input and the `{reason, then}` one for a confirmed
/// `profit_stop` (DEC-128 item 15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RiskEvent {
    MandateVersionApplied {
        result: ApplyResult,
    },
    RiskDayStarted {
        day_start_equity: Usd,
    },
    RiskLimitTriggered {
        limit: LimitKey,
        action: LimitAction,
        reason: Option<TriggerReason>,
    },
    RiskLimitLifted {
        limit: LimitKey,
        action: Option<LimitAction>,
        reason: Option<LiftReason>,
    },
    HighWaterMarkReset {
        from: Usd,
        to: Usd,
    },
    AgentModeApplied {
        from: AgentMode,
        to: AgentMode,
    },
    KillSwitchActivated {
        scope: KillScope,
        initiator: LimitKey,
    },
    UniverseChanged {
        instrument: AssetId,
        change: UniverseChange,
        reason: RemovalReason,
    },
    InstrumentRestrictionChanged {
        restriction: InstrumentRestriction,
        reason: RestrictionReason,
        active: bool,
    },
    GoalCompleted {
        reason: Option<GoalReason>,
        then: Option<ThenAction>,
        on_complete: Option<OnComplete>,
    },
    PositionReleased {
        qty: Qty,
    },
    AgentStopped {
        reason: StopReason,
        /// The net dollar loss the connection carries for 90 days: max(0, net contributed − E)
        /// (§5.7, MI-14).
        loss_carry_usd: Usd,
    },
}

/// Whether a version or an allocation change took effect (§5.10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyResult {
    Applied { allocation_change: Option<Usd> },
    Rejected { reason: Rejection },
}

/// The scope a kill switch touches. An agent-scoped flatten touches only that agent, and never the
/// broker's cancel-all or close-position (§5.5, trading spec §5.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KillScope {
    Agent,
}

/// Why an input was refused (§5.1, §5.7, §5.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Rejection {
    #[error("an allocation increase is blocked while a limit is latched")]
    IncreaseBlockedWhileLatched,
    #[error("equity after the change would not cover the agent's exposure")]
    EquityBelowExposure,
    #[error("the change would make a limit condition newly true")]
    WouldTriggerLimit,
    #[error("the lifetime floor cannot be acknowledged")]
    NotAcknowledgeable,
    #[error("nothing is latched that this acknowledgment would lift")]
    NothingToAcknowledge,
    #[error("the agent is not flat, so the flatten has not finished")]
    FlattenInProgress,
    #[error("the new version does not loosen the floor")]
    NotLoosening,
    #[error("a full risk day has not passed since confirmation")]
    WaitingPeriod,
    #[error("equity is still at or below the new floor")]
    StillBelowNewFloor,
}

impl Rejection {
    /// Stable reason code (ADR-0001 ES-09), as the reference cases spell it.
    pub fn code(self) -> &'static str {
        match self {
            Self::IncreaseBlockedWhileLatched => "increase_blocked_while_latched",
            Self::EquityBelowExposure => "equity_below_exposure",
            Self::WouldTriggerLimit => "would_trigger_limit",
            Self::NotAcknowledgeable => "not_acknowledgeable",
            Self::NothingToAcknowledge => "nothing_to_acknowledge",
            Self::FlattenInProgress => "flatten_in_progress",
            Self::NotLoosening => "not_loosening",
            Self::WaitingPeriod => "waiting_period",
            Self::StillBelowNewFloor => "still_below_new_floor",
        }
    }
}

/// What one step produced.
///
/// A refused input still produces an outcome, with `rejection` set and the refusal journalled, so a
/// caller cannot lose it by matching on `Ok` (DEC-128 item 16).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub snapshot: Snapshot,
    pub journal: Vec<RiskEvent>,
    /// Limits with breach time accumulating, shown in the agent's state (§5.6).
    pub pending: BTreeSet<LimitKey>,
    pub rejection: Option<Rejection>,
}

/// How many regular-session seconds lie between two instants, which is the clock §5.2 and §5.5 use for
/// an equity's staleness and scale-lift timers.
///
/// Supplied by the caller because it comes from the trading calendar: this crate holds no calendar and
/// reads no clock. Crypto passes wall-clock seconds, since crypto trades continuously.
pub trait SessionClock {
    fn seconds_between(&self, from: UtcNanos, to: UtcNanos) -> Result<u64, SpecError>;
}

/// The agent's risk state (§5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskState {
    snapshot: Snapshot,
}

impl RiskState {
    /// Opens the state at `opening.at` with E = H = E0 = C = the allocation and mode `normal` (§5.2).
    pub fn open(
        mandate: &ValidatedMandate,
        opening: &Opening,
        clock: &dyn SessionClock,
    ) -> Result<Self, SpecError> {
        let _ = (mandate, opening, clock);
        Err(SpecError::Unimplemented)
    }

    /// Applies one input in the §5.2 order: settle time, apply the input, update E then H, the ladder
    /// rungs in ascending `at`, the daily loss, the lifetime floor, then the effective mode. The
    /// journal follows that order.
    ///
    /// A step at or before the previous step's time is [`SpecError::ClockWentBackwards`].
    pub fn step(&mut self, step: &Step) -> Result<Outcome, SpecError> {
        let _ = step;
        Err(SpecError::Unimplemented)
    }

    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }
}

/// A risk day: 00:00 to 00:00 America/New_York, which is 23 or 25 hours on a daylight-saving change
/// day (§5.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskDay {
    pub day: Date,
    pub starts_at: UtcNanos,
    pub ends_at: UtcNanos,
    pub length_s: u32,
}

/// The risk day containing an instant, and its bounds.
pub fn risk_day(at: UtcNanos) -> Result<RiskDay, SpecError> {
    let _ = at;
    Err(SpecError::Unimplemented)
}

/// Breach-time confirmation (§5.6), kept as its own type because three limits and one goal condition
/// share it and because the independent oracle re-derives it from the input list.
///
/// Breach time accumulates over every interval that **starts** with the condition true, and resets only
/// after the condition has been false continuously for `breach_confirm_s` — so a brief bounce does not
/// restart confirmation, which is the rule a careless implementation gets wrong (planted bug 1).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Confirmation {
    accumulated_s: u64,
    false_run_s: u64,
    breached_at_last_input: bool,
}

impl Confirmation {
    /// Credits `elapsed_s` by the condition at the interval's start, then takes this input's
    /// condition. True when the limit triggers here.
    pub fn update(&mut self, breached: bool, elapsed_s: u64, need_s: u32) -> bool {
        let _ = (breached, elapsed_s, need_s);
        false
    }

    /// True while breach time is accumulating, which is what a step reports as `pending`.
    pub fn is_pending(&self) -> bool {
        self.breached_at_last_input || self.accumulated_s > 0
    }

    pub fn accumulated_s(&self) -> u64 {
        self.accumulated_s
    }
}

/// How long a hard breach waits for its second quote: min(`breach_confirm_s`, 10) seconds (§5.6).
pub fn hard_wait_s(breach_confirm_s: u32) -> u32 {
    breach_confirm_s.min(10)
}

/// The size factor: the product of the active `scale_sizes` rungs' factors (§5.5).
///
/// Only the keys of `active` matter here; the durations beside them are §5.5's trim guard, which is
/// the gate's. The factor lives on [`Snapshot`] because it is folded risk state, and `mandate-risk`
/// reads it there rather than recomputing it (the coordinator's ruling on #136).
pub fn size_factor(
    ladder: &[crate::document::LadderRung],
    active: &BTreeMap<u8, u64>,
) -> Result<Ratio, SpecError> {
    let _ = (ladder, active);
    Err(SpecError::Unimplemented)
}

/// The order §5.8 lifts scale rungs in after a reset: highest `at` first, each after
/// `scale_lift_after_s` of session time, so sizes return in steps rather than at once.
pub fn reset_lift_order(ladder: &[crate::document::LadderRung]) -> Result<Vec<u8>, SpecError> {
    let _ = ladder;
    Err(SpecError::Unimplemented)
}

/// Every limit's condition at a state, and whether its 1.25x hard level is reached (§5.2, §5.6).
pub fn conditions(
    mandate: &ValidatedMandate,
    snapshot: &Snapshot,
) -> Result<BTreeMap<LimitKey, (bool, bool)>, SpecError> {
    let _ = (mandate, snapshot);
    Err(SpecError::Unimplemented)
}
