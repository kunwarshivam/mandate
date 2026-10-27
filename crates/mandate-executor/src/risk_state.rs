//! The account stream's risk state, folded from its records (E6-15 slice B; DEC-2301, DEC-2304).
//!
//! [`AccountRiskFold`] reads the account stream's rows in `seq` order and folds the risk inputs
//! among them into `mandate_spec`'s [`RiskState`] and the executor's tripwire fold. It returns the
//! records those folds would append (journal spec §9.27 and §9.3) and keeps a [`RiskView`] the gate
//! can read. It is pure: it reads no clock, journals nothing itself, and sends nothing.
//!
//! Nothing calls it yet. Slice C wires it into the shell's gate once R0 closes (DEC-967 item 2),
//! and until then the executor's own fold still refuses the risk records as `E6-4`'s.
//!
//! # Fail closed
//!
//! A row the fold cannot read makes the view [`RiskView::Unreadable`] for good (DEC-2301 item 3,
//! DEC-2304). An unreadable view holds new openings as `exits_only` does and never holds an exit,
//! a protective order, a risk exit, or a kill switch on its own account (`AGENTS.md` rules 3 and
//! 13). It keeps `paused` or `stopped` only when the last readable view already was.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::mem;

use mandate_canon::{Object, Value, parse};
use mandate_domain::{AgentMode, AssetClass, AssetId, MarketSession, Side};
use mandate_journal::StoredEvent;
use mandate_num::{Price, Qty, Usd};
use mandate_spec::SpecError;
use mandate_spec::document::{LadderAction, OnComplete};
use mandate_spec::risk::{
    GoalReason, Input, LiftReason, LimitKey, Opening, Outcome, RemovalReason, RestrictionReason,
    RiskEvent, RiskState, SessionClock, Snapshot, Step, ThenAction, TriggerReason, risk_day,
};
use mandate_spec::validate::ValidatedMandate;
use mandate_time::{ExchangeCalendar, Session, UtcNanos};

use crate::ExecutorError;
use crate::payload::{int, object, text};
use crate::tripwire::TripwireState;

/// The order states that end an order (trading-domain spec §5.7), as the executor's fold ends one.
const TERMINAL: [&str; 6] = [
    "filled",
    "canceled",
    "rejected",
    "expired",
    "replaced",
    "abandoned",
];

/// How the fold reads an account-stream event type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reading {
    /// Not a risk input (mandate spec §5.2), or a record the fold itself writes, which slice B3's
    /// replay compares: it advances the head and steps nothing.
    Record,
    /// The executor's copy of `ClockAdvanced`, read at version 1 (DEC-2304 item 7).
    Tick,
    /// `MarkUpdated`: version 1 is no verdict and steps nothing (DEC-2305 item 3), version 2 is the
    /// mark with its verdict (journal spec §9.30), and a later version is one the fold does not know.
    Mark,
    /// A risk input the fold reads at no version yet, so every version is one it does not know and
    /// the view fails closed rather than skip the input (DEC-2304 item 1).
    RiskInput,
    /// A record that says whose an order is (DEC-2393 item 1): read for that, and stepping nothing.
    Order,
    /// `CompensatingEvent`, read at version 1 beside the `OrderStateChanged` it corrects, and
    /// stepping nothing (DEC-2829).
    Compensation,
    /// A fill, read at version 1: the agent's own steps the risk state (DEC-2393 item 2).
    Fill,
    /// `FeesCharged`, read at version 1: one charged to the agent's own order steps a fee (DEC-2393
    /// item 3).
    Fee,
}

/// The account stream's catalogue (journal spec §9): every event type `mandate-journal`'s
/// `stream_event_types` lists for an account stream, and how the fold reads each. A table rather
/// than a `match`, so no type can fall through to another type's reading.
const ACCOUNT_STREAM: [(&str, Reading); 52] = [
    ("StreamOpened", Reading::Record),
    ("TradingDayStarted", Reading::Record),
    ("ClockAdvanced", Reading::Tick),
    ("KillSwitchActivated", Reading::Record),
    ("OwnerAcknowledged", Reading::RiskInput),
    ("OwnerAcknowledgmentRequested", Reading::Record),
    ("OwnerCommandRefused", Reading::Record),
    ("IntentReceived", Reading::Record),
    ("GateDecided", Reading::Record),
    ("FillApplied", Reading::Fill),
    ("LateFillApplied", Reading::Fill),
    ("FeesCharged", Reading::Fee),
    ("SettlementPosted", Reading::Record),
    ("DividendPaid", Reading::RiskInput),
    ("CashInLieuPosted", Reading::RiskInput),
    ("CorporateActionPrepared", Reading::Record),
    ("CorporateActionApplied", Reading::RiskInput),
    ("ConductBreachDetected", Reading::Record),
    ("MandateVersionApplied", Reading::RiskInput),
    ("RiskDayStarted", Reading::Record),
    ("RiskLimitTriggered", Reading::Record),
    ("RiskLimitLifted", Reading::Record),
    ("HighWaterMarkReset", Reading::Record),
    ("PositionReleased", Reading::Record),
    ("InstrumentRestrictionChanged", Reading::Record),
    ("GoalCompleted", Reading::Record),
    ("UniverseChanged", Reading::RiskInput),
    ("OrderSubmitted", Reading::Order),
    ("OrderRequestRecorded", Reading::Order),
    ("OrderStateChanged", Reading::Order),
    ("OrderAbandoned", Reading::Record),
    ("BrokerExchangeRecorded", Reading::Record),
    ("MarkUpdated", Reading::Mark),
    ("ProtectionChanged", Reading::Record),
    ("OwnerConfirmationReceived", Reading::Record),
    ("AccountCloseRequested", Reading::Record),
    ("BrokerPositionObserved", Reading::Record),
    ("ReconciliationRun", Reading::Record),
    ("CompensatingEvent", Reading::Compensation),
    ("AccountSnapshotRecorded", Reading::Record),
    ("AccountStateObserved", Reading::Record),
    ("RejectObserved", Reading::Record),
    ("AccountRestrictionChanged", Reading::Record),
    ("ExternalActivityIngested", Reading::Record),
    ("RelatedAccountsCoordination", Reading::Record),
    ("AgentModeApplied", Reading::Record),
    ("ConnectionEstablished", Reading::Record),
    ("ConnectionCredentialRotated", Reading::Record),
    ("ConnectionChecked", Reading::Record),
    ("ConnectionStateChanged", Reading::Record),
    ("ConnectionCredentialRefreshed", Reading::Record),
    ("OwnerAlertSent", Reading::Record),
];

fn reading(event_type: &str) -> Option<Reading> {
    ACCOUNT_STREAM
        .iter()
        .find(|(name, _)| *name == event_type)
        .map(|(_, reading)| *reading)
}

/// The journal's spelling of each agent mode (journal spec §9.6's `to`).
const MODES: [(AgentMode, &str); 4] = [
    (AgentMode::Normal, "normal"),
    (AgentMode::ExitsOnly, "exits_only"),
    (AgentMode::Paused, "paused"),
    (AgentMode::Stopped, "stopped"),
];

/// Why a limit triggered, as §9.27's `RiskLimitTriggered.reason` spells it.
const TRIGGER_REASONS: [(TriggerReason, &str); 5] = [
    (TriggerReason::HardTrigger, "hard_trigger"),
    (TriggerReason::HardBreachPending, "hard_breach_pending"),
    (TriggerReason::ResolvedAtRollover, "resolved_at_rollover"),
    (TriggerReason::NewDayBreach, "new_day_breach"),
    (TriggerReason::AfterReset, "after_reset"),
];

/// Why a limit lifted, as §9.27's `RiskLimitLifted.reason` spells it.
const LIFT_REASONS: [(LiftReason, &str); 3] = [
    (LiftReason::OwnerAcknowledged, "owner_acknowledged"),
    (LiftReason::VersionLoosened, "version_loosened"),
    (LiftReason::HardBreachCleared, "hard_breach_cleared"),
];

/// Why an instrument restriction changed, as §9.27's `InstrumentRestrictionChanged.reason` spells
/// it: a mark's verdict, or §9.3's `UniverseChanged` reason.
const RESTRICTION_REASONS: [(RestrictionReason, &str); 9] = [
    (RestrictionReason::NoSaneMark, "no_sane_mark"),
    (RestrictionReason::SaneMark, "sane_mark"),
    (
        RestrictionReason::Removal(RemovalReason::ThesisAdmitted),
        "thesis_admitted",
    ),
    (
        RestrictionReason::Removal(RemovalReason::ThesisExpired),
        "thesis_expired",
    ),
    (
        RestrictionReason::Removal(RemovalReason::ThesisInvalidated),
        "thesis_invalidated",
    ),
    (
        RestrictionReason::Removal(RemovalReason::LineageRetired),
        "lineage_retired",
    ),
    (
        RestrictionReason::Removal(RemovalReason::EligibilityLost),
        "eligibility_lost",
    ),
    (
        RestrictionReason::Removal(RemovalReason::OperatorHalt),
        "operator_halt",
    ),
    (
        RestrictionReason::Removal(RemovalReason::VersionApplied),
        "version_applied",
    ),
];

/// The journal's spelling of `value` in `table`. Every table above is total over its type, so a
/// value it lacks is a record the fold cannot write.
fn spelled<T: PartialEq + Copy>(
    table: &[(T, &'static str)],
    value: T,
) -> Result<&'static str, UnreadableCause> {
    table
        .iter()
        .find(|(known, _)| *known == value)
        .map(|(_, name)| *name)
        .ok_or(UnreadableCause::AppendFailed)
}

/// An optional spelling: the text, or `null`.
fn nullable(name: Option<&str>) -> Value {
    name.map_or(Value::Null, text)
}

/// A refusal to decide, which the fold fails closed on (DEC-2304 item 1).
fn refused(_: SpecError) -> UnreadableCause {
    UnreadableCause::RefusedToDecide
}

/// Every session a version-2 mark may carry (journal spec §9.30).
const SESSIONS: [MarketSession; 5] = [
    MarketSession::Overnight,
    MarketSession::PreMarket,
    MarketSession::Regular,
    MarketSession::AfterHours,
    MarketSession::Crypto,
];

/// A payload's whole-second instant at `member`: the scheduler's `clock`, or a risk input's
/// `risk_clock` (mandate spec §5.2). Anything else is undecodable.
fn whole_second(payload: &Object, member: &str) -> Result<UtcNanos, UnreadableCause> {
    payload
        .get(member)
        .and_then(Value::as_str)
        .and_then(|instant| UtcNanos::parse(instant).ok())
        .filter(|instant| instant.nanos() == 0)
        .ok_or(UnreadableCause::Undecodable)
}

/// The bid a mark that is not sane steps with. Mandate spec §5.2 reads a bid from a sane mark only,
/// so this price is never read; `Input::Mark` takes a price, and a not-sane mark may carry none.
fn unread_bid() -> Result<Price, UnreadableCause> {
    Price::parse("1").map_err(|_| UnreadableCause::Undecodable)
}

/// Whether `row` is the record `owed` (DEC-2301 item 2): the same type, version, cause and payload.
fn reproduces(owed: &RiskAppend, row: &StoredEvent, body: &Value) -> bool {
    let cause = body.get("causation_id").and_then(Value::as_str);
    row.event_type == owed.event_type
        && row.schema_version == owed.schema_version
        && cause == owed.causation_id.as_deref()
        && body.get("payload") == Some(&owed.payload)
}

/// The versions of the order records the fold reads for ownership (journal spec §9.5, §9.16):
/// `OrderSubmitted` and `OrderStateChanged` at 1 and 2, `OrderRequestRecorded` at 1. Any other is
/// one it does not know, and the view fails closed rather than miss whose an order is.
fn order_version(event_type: &str, version: u64) -> bool {
    match event_type {
        "OrderRequestRecorded" => version == 1,
        _ => matches!(version, 1 | 2),
    }
}

/// The versions of the fill records the fold reads (journal spec §9.5): `FillApplied` at 1 and 2,
/// whose `client_order_id` is `null` for a fill that names no order (DEC-2812), and
/// `LateFillApplied` at 1. Any other is one it does not know, and the view fails closed.
fn fill_version(event_type: &str, version: u64) -> bool {
    match event_type {
        "FillApplied" => matches!(version, 1 | 2),
        _ => version == 1,
    }
}

/// Whether the version-1 `CompensatingEvent` `row` corrects `corrects`, the row right before it
/// (DEC-2829): its `difference` is `order_state`, its `from` is text, and `corrects` is an
/// `OrderStateChanged` in the same append (the same `recorded_at` its writer gave it), named by the
/// compensation's `causation_id` and its one corrected event id, whose order is the compensation's
/// `subject` and whose state is its `to`. Such a compensation moves no figure: the order's state is
/// that record's, read for ownership already, and its fill is its own `FillApplied`. Any other is
/// an input the fold does not read, and the view fails closed (DEC-2304 item 1); a paired one
/// whose `risk_clock` is not a whole-second timestamp is undecodable.
fn compensates(
    row: &StoredEvent,
    body: &Value,
    payload: &Object,
    corrects: Option<StateChange>,
) -> bool {
    let named = |member: &str| payload.get(member).and_then(Value::as_str);
    let corrected = match payload.get("corrected_event_ids") {
        Some(Value::Array(ids)) => match ids.as_slice() {
            [id] => id.as_str(),
            _ => None,
        },
        _ => None,
    };
    corrects.is_some_and(|corrects| {
        named("difference") == Some("order_state")
            && named("from").is_some()
            && corrects.recorded_at == row.recorded_at
            && body.get("causation_id").and_then(Value::as_str) == Some(corrects.event_id.as_str())
            && corrected == Some(corrects.event_id.as_str())
            && named("subject") == Some(corrects.client_order_id.as_str())
            && named("to") == Some(corrects.state.as_str())
    })
}

/// The sum of a fill's `fees` charged in `USD` (DEC-2393 item 3). A fee in any other asset moves
/// the quantity received by a rule the fold does not apply, so it is a refusal to decide.
fn usd_fees(payload: &Object) -> Result<Usd, UnreadableCause> {
    let Some(Value::Array(fees)) = payload.get("fees") else {
        return Err(UnreadableCause::Undecodable);
    };
    fees.iter().try_fold(Usd::ZERO, |sum, fee| {
        let member = |name: &str| fee.get(name).and_then(Value::as_str);
        if member("asset").ok_or(UnreadableCause::Undecodable)? != "USD" {
            return Err(UnreadableCause::RefusedToDecide);
        }
        let amount = member("amount")
            .and_then(|amount| Usd::parse(amount).ok())
            .filter(|amount| !amount.is_negative())
            .ok_or(UnreadableCause::Undecodable)?;
        sum.checked_add(amount)
            .map_err(|_| UnreadableCause::Undecodable)
    })
}

/// The trading calendar the fold steps the risk state on: §5.5's session seconds, and the session
/// each input arrived in (mandate spec §5.2). The caller supplies it, since this crate holds no
/// calendar.
pub trait RiskCalendar: SessionClock {
    /// The market session at `at`.
    ///
    /// # Errors
    /// Whatever the calendar cannot answer, which the fold reads as a refusal to decide.
    fn session(&self, at: UtcNanos) -> Result<MarketSession, SpecError>;
}

/// Why the view stopped being readable (DEC-2301 item 3, DEC-2304).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UnreadableCause {
    /// A row whose body is not the event it names.
    Undecodable,
    /// A `schema_version` the fold does not know for its event type.
    UnknownSchemaVersion,
    /// A row whose `seq` is not one above the last folded.
    SeqGap,
    /// An event type the account stream's catalogue does not hold.
    UnfoldedEventType,
    /// A journaled risk record the fold would not have produced.
    NotReproduced,
    /// A record the fold produced that could not be appended.
    AppendFailed,
    /// The risk state refused to decide an input (mandate spec §5.2; `RiskState::step`).
    RefusedToDecide,
    /// No fold is open for the agent (DEC-2804 items 2 and 8): its deployment, version, equity or
    /// instrument does not read, it is redeployed on another version, another agent that has not
    /// stopped is followed on its account, or its universe may hold a second instrument.
    NotOpened,
    /// The fold, opened flat, read an own fill before any counted sane mark of its instrument, so E
    /// is unknown (DEC-2834). The one cause the fold leaves again: its first counted sane mark
    /// prices the inputs it holds, and the view reads.
    Unpriced,
}

/// What the gate reads of the account's risk state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RiskView {
    /// The fold read every row up to `head`, and `snapshot` is the risk state after it.
    Readable { snapshot: Box<Snapshot>, head: u64 },
    /// The fold could not read the row after `head`. `mode` is `exits_only`, or the last readable
    /// view's mode when that held more.
    Unreadable {
        cause: UnreadableCause,
        head: u64,
        mode: AgentMode,
    },
}

/// A path an order or a command can take, as far as the risk view is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OrderPath {
    /// A new opening or an increase.
    Opening,
    /// The owner's own exit.
    OwnerExit,
    /// An exit a signal or a goal asks for.
    DiscretionaryExit,
    /// An exit a risk limit forces.
    RiskExit,
    /// A protective order's placement, re-placement, or cancel.
    ProtectiveOrder,
    /// A kill switch on this account.
    KillSwitch,
}

impl RiskView {
    /// The mode the gate applies.
    pub fn mode(&self) -> AgentMode {
        match self {
            Self::Readable { snapshot, .. } => snapshot.agent_mode,
            Self::Unreadable { mode, .. } => *mode,
        }
    }

    /// Whether the view holds `path`. A kill switch is never held. Only `paused` or `stopped` holds
    /// an exit or a protective order, so `exits_only`, an unreadable view's floor, holds none of
    /// them. Anything but a readable `normal` view holds an opening (`AGENTS.md` rule 13).
    pub fn holds(&self, path: OrderPath) -> bool {
        let mode = self.mode();
        match path {
            OrderPath::KillSwitch => false,
            OrderPath::Opening => {
                !(matches!(self, Self::Readable { .. }) && mode == AgentMode::Normal)
            }
            OrderPath::OwnerExit
            | OrderPath::DiscretionaryExit
            | OrderPath::RiskExit
            | OrderPath::ProtectiveOrder => matches!(mode, AgentMode::Paused | AgentMode::Stopped),
        }
    }
}

/// A record the fold produced, for the caller to give an event id and append.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskAppend {
    pub event_type: &'static str,
    pub schema_version: u64,
    /// The row the record follows from, when it copies or answers one.
    pub causation_id: Option<String>,
    pub payload: Value,
}

/// The account stream's risk state, folded row by row. A clone steps a row the live fold has not
/// journaled, to see whether it would return a record (DEC-2804 item 5).
#[derive(Clone)]
pub struct AccountRiskFold<'c> {
    stream: String,
    agent: String,
    risk: RiskState<'c>,
    calendar: &'c dyn RiskCalendar,
    #[expect(dead_code, reason = "the fold steps the tripwires from E6-15 B2b on")]
    tripwires: TripwireState,
    view: RiskView,
    /// The opening instant: a row whose clock precedes it is from before the agent was deployed,
    /// and is not the agent's (DEC-2804 item 3).
    opened_at: UtcNanos,
    /// The last tick's clock, or the opening instant before any tick, from which the next tick
    /// crosses midnight. A mark does not move it (§5.4 starts the day at a tick).
    clock: UtcNanos,
    /// The one instrument the risk state's position is (DEC-2310 item 1): the opening's, or the first
    /// a mark names when the opening is flat. `RiskState` holds a single position, so a row naming
    /// another instrument is one it cannot step without moving this one's E on the other's price,
    /// and the view becomes unreadable. Ids are compared exactly, as the journal's canonical asset
    /// ids.
    instrument: Option<String>,
    /// Each `OrderRequestRecorded` not yet named by its `OrderSubmitted`, by event id, with the
    /// agent it names (journal spec rule 45).
    requests: BTreeMap<String, String>,
    /// The agent's own orders by client order id (DEC-2393 item 1).
    own: BTreeSet<String>,
    /// The `fill_id`s of the agent's own fills already stepped, each once, as the executor's fold
    /// applies a fill once (DEC-2393 item 2).
    fills: BTreeSet<String>,
    /// The records the fold returned and the stream has not yet carried back, in order (DEC-2301
    /// item 2, DEC-2399 items 1 and 2): the next risk records on the stream must be exactly these.
    /// Other rows may come before them (the rest of their cause's batch), and the caller folds
    /// every record it appends back in, or the next risk record is `not_reproduced`.
    owed: VecDeque<RiskAppend>,
    /// The row right before the next, when it is an `OrderStateChanged`: the only row a
    /// compensation may correct (DEC-2829).
    state_change: Option<StateChange>,
    /// The position's asset class, which says which marks count (mandate spec §5.2).
    asset_class: AssetClass,
    /// Whether the risk state's mark is one a counted sane mark set: a fold opened with a position
    /// is priced at its cost, and one opened flat is not until such a mark (DEC-2834 item 1).
    priced: bool,
    /// While unpriced, the inputs held since the own fill that found no price (DEC-2834 item 2).
    held: Option<Held>,
}

/// The inputs an unpriced fold holds, from the own fill that found no price on, in journal order.
#[derive(Clone)]
struct Held {
    /// The first held input's instant, at which the pricing mark's bid is taken.
    since: UtcNanos,
    /// The mode of the view the fill found, which the unpriced view keeps.
    mode: AgentMode,
    rows: Vec<(StoredEvent, Stepped)>,
}

/// Which step a held row is.
#[derive(Clone, Copy)]
enum Stepped {
    Tick,
    Mark,
    Fill,
    Fee,
}

/// An `OrderStateChanged` as a compensation right after it must name it (DEC-2829).
#[derive(Clone)]
struct StateChange {
    event_id: String,
    recorded_at: String,
    client_order_id: String,
    state: String,
}

impl StateChange {
    /// `row` when it is an `OrderStateChanged` naming its order and state, else `None`.
    fn of(row: &StoredEvent, payload: &Object) -> Option<Self> {
        if row.event_type != "OrderStateChanged" {
            return None;
        }
        let named = |member: &str| payload.get(member).and_then(Value::as_str);
        Some(Self {
            event_id: row.event_id.clone(),
            recorded_at: row.recorded_at.clone(),
            client_order_id: named("client_order_id")?.to_owned(),
            state: named("state")?.to_owned(),
        })
    }
}

/// The records the fold returns, which it must find on the stream exactly as it returned them.
const FOLD_RECORDS: [&str; 8] = [
    "RiskDayStarted",
    "RiskLimitTriggered",
    "RiskLimitLifted",
    "HighWaterMarkReset",
    "PositionReleased",
    "InstrumentRestrictionChanged",
    "GoalCompleted",
    "AgentModeApplied",
];

impl<'c> AccountRiskFold<'c> {
    /// Opens the fold for `agent` on `stream` after the row at `head`, with the risk state at
    /// `opening` (mandate spec §5.2) and the tripwire state folded so far. `instrument` is the
    /// position's instrument, which the fold binds at once (DEC-2310 item 1): a held opening must
    /// name it, so no first mark can name another and move E, and so must a fold opened past seq 1,
    /// since a row before `head` may have bound it.
    ///
    /// # Errors
    /// Whatever `RiskState::open` refuses. `invalid_input` (`what: "instrument"`) for a held
    /// opening, or an opening after seq 1, that names no instrument (DEC-2310 item 1), and for any
    /// opening that names one that is not a canonical asset id (DEC-2313 item 1).
    #[expect(
        clippy::too_many_arguments,
        reason = "each is one of the fold's independent inputs, none derivable from another"
    )]
    pub fn open(
        stream: &str,
        head: u64,
        agent: &str,
        mandate: &ValidatedMandate,
        opening: &Opening,
        instrument: Option<&str>,
        calendar: &'c dyn RiskCalendar,
        tripwires: TripwireState,
    ) -> Result<Self, SpecError> {
        if instrument.is_none() && (!opening.position_qty.is_zero() || head > 1) {
            return Err(SpecError::InvalidInput { what: "instrument" });
        }
        if instrument.is_some_and(|named| AssetId::parse(named).is_err()) {
            return Err(SpecError::InvalidInput { what: "instrument" });
        }
        let risk = RiskState::open(mandate, opening, calendar)?;
        let view = RiskView::Readable {
            snapshot: Box::new(risk.snapshot().clone()),
            head,
        };
        Ok(Self {
            stream: stream.to_owned(),
            agent: agent.to_owned(),
            risk,
            calendar,
            tripwires,
            view,
            opened_at: opening.at,
            clock: opening.at,
            instrument: instrument.map(str::to_owned),
            requests: BTreeMap::new(),
            own: BTreeSet::new(),
            fills: BTreeSet::new(),
            owed: VecDeque::new(),
            state_change: None,
            asset_class: opening.asset_class,
            priced: !opening.position_qty.is_zero(),
            held: None,
        })
    }

    pub fn view(&self) -> &RiskView {
        &self.view
    }

    /// Folds the next row and returns the records the risk state and the tripwires produce for it,
    /// in the order they are appended (mandate spec §5.2). A row the fold cannot read returns no
    /// records and makes the view unreadable.
    ///
    /// # Errors
    /// [`ExecutorError::ForeignStream`] for a row on another stream: a caller's mistake, which
    /// changes nothing.
    pub fn fold(&mut self, row: &StoredEvent) -> Result<Vec<RiskAppend>, ExecutorError> {
        if row.stream_id != self.stream {
            return Err(ExecutorError::ForeignStream {
                stream: row.stream_id.clone(),
            });
        }
        let head = match &self.view {
            RiskView::Readable { head, .. }
            | RiskView::Unreadable {
                cause: UnreadableCause::Unpriced,
                head,
                ..
            } => *head,
            RiskView::Unreadable { .. } => return Ok(Vec::new()),
        };
        if head.checked_add(1) != Some(row.seq) {
            self.unreadable(UnreadableCause::SeqGap);
            return Ok(Vec::new());
        }
        match self.read(row) {
            Ok(appends) => {
                self.owed.extend(appends.iter().cloned());
                self.view = match &self.held {
                    Some(held) => RiskView::Unreadable {
                        cause: UnreadableCause::Unpriced,
                        head: row.seq,
                        mode: held.mode,
                    },
                    None => RiskView::Readable {
                        snapshot: Box::new(self.risk.snapshot().clone()),
                        head: row.seq,
                    },
                };
                Ok(appends)
            }
            Err(cause) => {
                self.unreadable(cause);
                Ok(Vec::new())
            }
        }
    }

    /// Reads the row after the head: its body, its type and version, and, for a tick or a
    /// version-2 mark, the risk state's steps.
    fn read(&mut self, row: &StoredEvent) -> Result<Vec<RiskAppend>, UnreadableCause> {
        let body = parse(&row.body).map_err(|_| UnreadableCause::Undecodable)?;
        let payload = body
            .get("payload")
            .and_then(Value::as_object)
            .ok_or(UnreadableCause::Undecodable)?;
        let corrects = mem::replace(&mut self.state_change, StateChange::of(row, payload));
        let cause = body.get("causation_id").and_then(Value::as_str);
        let owed_cause = cause.is_some_and(|cause| {
            self.owed
                .iter()
                .any(|owed| owed.causation_id.as_deref() == Some(cause))
        });
        let others_mode = row.event_type == "AgentModeApplied" && !owed_cause;
        if FOLD_RECORDS.contains(&row.event_type.as_str()) && !others_mode {
            return match self.owed.front() {
                Some(owed) if reproduces(owed, row, &body) => {
                    self.owed.pop_front();
                    Ok(Vec::new())
                }
                _ => Err(UnreadableCause::NotReproduced),
            };
        }
        match (reading(&row.event_type), row.schema_version) {
            (None, _) => Err(UnreadableCause::UnfoldedEventType),
            (Some(Reading::Record), _) | (Some(Reading::Mark), 1) => Ok(Vec::new()),
            (Some(Reading::Tick), 1) => self.stepped(row, payload, Stepped::Tick),
            (Some(Reading::Mark), 2) => self.stepped(row, payload, Stepped::Mark),
            (Some(Reading::Order), version) if order_version(&row.event_type, version) => {
                self.order(row, &body, payload)?;
                Ok(Vec::new())
            }
            (Some(Reading::Fill), version) if fill_version(&row.event_type, version) => {
                self.stepped(row, payload, Stepped::Fill)
            }
            (Some(Reading::Fee), 1) => self.stepped(row, payload, Stepped::Fee),
            (Some(Reading::Compensation), 1) if compensates(row, &body, payload, corrects) => {
                whole_second(payload, "risk_clock")?;
                Ok(Vec::new())
            }
            (Some(_), _) => Err(UnreadableCause::UnknownSchemaVersion),
        }
    }

    /// Binds the fold to the first instrument a row names, and refuses to decide a row naming any
    /// other (DEC-2308 item 4), or a first one that is not a canonical asset id (DEC-2314): openings
    /// wait, exits go on (`AGENTS.md` rule 13).
    fn bind(&mut self, instrument: &str) -> Result<(), UnreadableCause> {
        match &self.instrument {
            Some(bound) if bound != instrument => Err(UnreadableCause::RefusedToDecide),
            Some(_) => Ok(()),
            None if AssetId::parse(instrument).is_err() => Err(UnreadableCause::RefusedToDecide),
            None => {
                self.instrument = Some(instrument.to_owned());
                Ok(())
            }
        }
    }

    /// Whose an order is (DEC-2393 item 1), as the executor's own fold attributes it: an
    /// `OrderSubmitted`'s companion, the `OrderRequestRecorded` its `causation_id` names, else the
    /// submission's own `agent_id`; and an order that replaces one of the agent's, or that one of
    /// its terminal states names as `replaced_by`.
    fn order(
        &mut self,
        row: &StoredEvent,
        body: &Value,
        payload: &Object,
    ) -> Result<(), UnreadableCause> {
        let named = |member: &str| payload.get(member).and_then(Value::as_str);
        match row.event_type.as_str() {
            "OrderRequestRecorded" => {
                let agent = named("agent_id")
                    .or_else(|| named("agent"))
                    .ok_or(UnreadableCause::Undecodable)?;
                self.requests.insert(row.event_id.clone(), agent.to_owned());
            }
            "OrderSubmitted" => {
                let order = named("client_order_id").ok_or(UnreadableCause::Undecodable)?;
                let companion = body
                    .get("causation_id")
                    .and_then(Value::as_str)
                    .and_then(|cause| self.requests.remove(cause));
                let agent = companion
                    .or_else(|| {
                        named("agent_id")
                            .or_else(|| named("agent"))
                            .map(str::to_owned)
                    })
                    .ok_or(UnreadableCause::Undecodable)?;
                if agent == self.agent {
                    self.own.insert(order.to_owned());
                }
            }
            _ => {
                let order = named("client_order_id").ok_or(UnreadableCause::Undecodable)?;
                if named("replaces").is_some_and(|old| self.own.contains(old)) {
                    self.own.insert(order.to_owned());
                }
                let terminal = named("state").is_some_and(|state| TERMINAL.contains(&state));
                let ignored = payload.get("ignored") == Some(&Value::Bool(true));
                if let Some(successor) = named("replaced_by")
                    && terminal
                    && !ignored
                    && self.own.contains(order)
                {
                    self.own.insert(successor.to_owned());
                }
            }
        }
        Ok(())
    }

    /// Steps a fill of the agent's own order (DEC-2393 item 2): `Input::Fill` at its `risk_clock`,
    /// in the session the calendar places it in, then the `USD` fees it carries as one
    /// `Input::Fee` at the same instant (item 3). A fill of any other order steps nothing; a fee in
    /// any other asset is a refusal to decide.
    fn fill(
        &mut self,
        row: &StoredEvent,
        payload: &Object,
    ) -> Result<Vec<RiskAppend>, UnreadableCause> {
        if !self.ours(payload) {
            return Ok(Vec::new());
        }
        let fill_id = payload
            .get("fill_id")
            .and_then(Value::as_str)
            .ok_or(UnreadableCause::Undecodable)?;
        if self.fills.contains(fill_id) {
            return Ok(Vec::new());
        }
        let instrument = payload
            .get("instrument_id")
            .and_then(Value::as_str)
            .ok_or(UnreadableCause::Undecodable)?;
        let side = match payload.get("side").and_then(Value::as_str) {
            Some("buy") => Side::Buy,
            Some("sell") => Side::Sell,
            _ => return Err(UnreadableCause::Undecodable),
        };
        let qty = payload
            .get("qty_gross")
            .and_then(Value::as_str)
            .and_then(|qty| Qty::parse(qty).ok())
            .filter(|qty| !qty.is_zero())
            .ok_or(UnreadableCause::Undecodable)?;
        let price = payload
            .get("price")
            .and_then(Value::as_str)
            .and_then(|price| Price::parse(price).ok())
            .ok_or(UnreadableCause::Undecodable)?;
        let fees = usd_fees(payload)?;
        let clock = whole_second(payload, "risk_clock")?;
        if clock < self.opened_at {
            return Err(UnreadableCause::RefusedToDecide);
        }
        self.bind(instrument)?;
        if !self.priced {
            let mode = match &self.view {
                RiskView::Readable { snapshot, .. } => snapshot.agent_mode,
                RiskView::Unreadable { mode, .. } => *mode,
            };
            self.held = Some(Held {
                since: clock,
                mode: mode.max(AgentMode::ExitsOnly),
                rows: vec![(row.clone(), Stepped::Fill)],
            });
            return Ok(Vec::new());
        }
        self.fills.insert(fill_id.to_owned());
        let session = self.calendar.session(clock).map_err(refused)?;
        let outcome = self.step(clock, session, Input::Fill { side, qty, price })?;
        let mut appends = self.records(row, clock, &outcome)?;
        if !fees.is_zero() {
            let outcome = self.step(clock, session, Input::Fee { usd: fees })?;
            appends.extend(self.records(row, clock, &outcome)?);
        }
        Ok(appends)
    }

    /// Steps a `FeesCharged` of the agent's own order (DEC-2393 item 3): the regulatory fee the
    /// paper executor simulates for its fill (`simulated`, family `equities`) as `Input::Fee` of its
    /// `accrued`, the fee trading spec §8.2 takes from equity and §10 puts in P&L; the record
    /// writes `charged` 0, since no broker charges it. Any other shape naming an own order is one
    /// no slice reads yet, a refusal to decide. One of any other order, or of none, steps nothing.
    fn fee(
        &mut self,
        row: &StoredEvent,
        payload: &Object,
    ) -> Result<Vec<RiskAppend>, UnreadableCause> {
        if !self.ours(payload) {
            return Ok(Vec::new());
        }
        let simulated = payload.get("simulated") == Some(&Value::Bool(true))
            && payload.get("family").and_then(Value::as_str) == Some("equities");
        if !simulated {
            return Err(UnreadableCause::RefusedToDecide);
        }
        let charged = payload
            .get("accrued")
            .and_then(Value::as_str)
            .and_then(|accrued| Usd::parse(accrued).ok())
            .filter(|accrued| !accrued.is_negative())
            .ok_or(UnreadableCause::Undecodable)?;
        if charged.is_zero() {
            return Ok(Vec::new());
        }
        let clock = whole_second(payload, "risk_clock")?;
        if clock < self.opened_at {
            return Err(UnreadableCause::RefusedToDecide);
        }
        let session = self.calendar.session(clock).map_err(refused)?;
        let outcome = self.step(clock, session, Input::Fee { usd: charged })?;
        self.records(row, clock, &outcome)
    }

    /// Steps a `MarkUpdated` at version 2 (journal spec §9.30, DEC-2305): its `bid` and `sane`
    /// become `Input::Mark` at its `risk_clock`, in the `session` the record carries and never one
    /// the calendar derives (DEC-2305 item 1), as mandate spec §5.2 reads a mark.
    ///
    /// A sane mark's `bid` is a positive price (rule 244). A mark that is not sane may carry any bid
    /// or none, and §5.2 never reads one, so its step carries [`unread_bid`] instead.
    ///
    /// A mark never starts the risk day, and never moves the instant the next tick crosses midnight
    /// from: §5.4 starts the day at the copied `ClockAdvanced` that crosses New York midnight, as the
    /// reference cases do (MC-R13 steps a pre-market mark on the next day with E₀ unchanged and no
    /// `RiskDayStarted`). So a mark past midnight is stepped against the closing day's E₀, and the
    /// next tick still starts the new day.
    fn mark(
        &mut self,
        row: &StoredEvent,
        payload: &Object,
    ) -> Result<Vec<RiskAppend>, UnreadableCause> {
        let instrument = payload
            .get("instrument_id")
            .and_then(Value::as_str)
            .ok_or(UnreadableCause::Undecodable)?;
        let clock = whole_second(payload, "risk_clock")?;
        if clock < self.opened_at || self.others(instrument) {
            return Ok(Vec::new());
        }
        self.bind(instrument)?;
        let sane = match payload.get("sane") {
            Some(Value::Bool(sane)) => *sane,
            _ => return Err(UnreadableCause::Undecodable),
        };
        let bid = payload
            .get("bid")
            .filter(|bid| matches!(bid, Value::Str(_) | Value::Null))
            .ok_or(UnreadableCause::Undecodable)?;
        let bid = if sane {
            bid.as_str()
                .and_then(|bid| Price::parse(bid).ok())
                .ok_or(UnreadableCause::Undecodable)?
        } else {
            unread_bid()?
        };
        let session = payload
            .get("session")
            .and_then(Value::as_str)
            .and_then(|name| {
                SESSIONS
                    .into_iter()
                    .find(|session| session.as_str() == name)
            })
            .ok_or(UnreadableCause::Undecodable)?;
        let outcome = self.step(clock, session, Input::Mark { bid, sane })?;
        self.priced |= sane && self.counts(session);
        self.records(row, clock, &outcome)
    }

    /// Whether the fill or fee `payload` names one of the agent's own orders (DEC-2393 item 1).
    fn ours(&self, payload: &Object) -> bool {
        payload
            .get("client_order_id")
            .and_then(Value::as_str)
            .is_some_and(|order| self.own.contains(order))
    }

    /// Whether `instrument` is another than the one the fold is bound to: its mark steps nothing,
    /// since the agent's lots hold none of it (DEC-2849 item 2, DEC-2804 item 9). An unbound fold
    /// binds as before (DEC-2310 item 1).
    fn others(&self, instrument: &str) -> bool {
        self.instrument
            .as_deref()
            .is_some_and(|bound| bound != instrument)
    }

    /// Whether a mark in `session` counts (mandate spec §5.2): any for crypto, the regular session
    /// for equities.
    fn counts(&self, session: MarketSession) -> bool {
        self.asset_class == AssetClass::Crypto || session == MarketSession::Regular
    }

    /// Steps `row`, a risk input of kind `kind`, or holds it while the fold is unpriced (DEC-2834
    /// item 2). Whose a fill or fee is, and whether a mark is from before the opening instant, is
    /// decided where the row is, as a priced fold decides it: another's fill or fee and an earlier
    /// mark step nothing and are not held. A counted sane mark of the bound instrument prices the
    /// held inputs (item 3); a mark naming another instrument steps nothing (DEC-2849 item 2).
    fn stepped(
        &mut self,
        row: &StoredEvent,
        payload: &Object,
        kind: Stepped,
    ) -> Result<Vec<RiskAppend>, UnreadableCause> {
        if self.held.is_none() {
            return self.step_input(row, payload, kind);
        }
        if matches!(kind, Stepped::Fill | Stepped::Fee) && !self.ours(payload) {
            return Ok(Vec::new());
        }
        if let Stepped::Mark = kind {
            let instrument = payload
                .get("instrument_id")
                .and_then(Value::as_str)
                .ok_or(UnreadableCause::Undecodable)?;
            if whole_second(payload, "risk_clock")? < self.opened_at {
                return Ok(Vec::new());
            }
            if self.others(instrument) {
                return Ok(Vec::new());
            }
            let session = payload
                .get("session")
                .and_then(Value::as_str)
                .and_then(|name| {
                    SESSIONS
                        .into_iter()
                        .find(|session| session.as_str() == name)
                })
                .ok_or(UnreadableCause::Undecodable)?;
            if payload.get("sane") == Some(&Value::Bool(true)) && self.counts(session) {
                return self.price(row, payload, session);
            }
        }
        if let Some(held) = &mut self.held {
            held.rows.push((row.clone(), kind));
        }
        Ok(Vec::new())
    }

    /// Steps `row` as a risk input of kind `kind`.
    fn step_input(
        &mut self,
        row: &StoredEvent,
        payload: &Object,
        kind: Stepped,
    ) -> Result<Vec<RiskAppend>, UnreadableCause> {
        match kind {
            Stepped::Tick => self.tick(row, payload),
            Stepped::Mark => self.mark(row, payload),
            Stepped::Fill => self.fill(row, payload),
            Stepped::Fee => self.fee(row, payload),
        }
    }

    /// Prices the held inputs with `row`, the first counted sane mark (DEC-2834 item 3): its bid at
    /// the first held input's instant in the mark's `session`, then each held input in journal
    /// order, then the mark itself. Each record is owed after the mark, with its own cause.
    fn price(
        &mut self,
        row: &StoredEvent,
        payload: &Object,
        session: MarketSession,
    ) -> Result<Vec<RiskAppend>, UnreadableCause> {
        let Some(held) = self.held.take() else {
            return Err(UnreadableCause::Unpriced);
        };
        let bid = payload
            .get("bid")
            .and_then(Value::as_str)
            .and_then(|bid| Price::parse(bid).ok())
            .ok_or(UnreadableCause::Undecodable)?;
        let outcome = self.step(held.since, session, Input::Mark { bid, sane: true })?;
        let mut appends = self.records(row, held.since, &outcome)?;
        self.priced = true;
        for (held_row, kind) in &held.rows {
            let body = parse(&held_row.body).map_err(|_| UnreadableCause::Undecodable)?;
            let held_payload = body
                .get("payload")
                .and_then(Value::as_object)
                .ok_or(UnreadableCause::Undecodable)?;
            appends.extend(self.step_input(held_row, held_payload, *kind)?);
        }
        appends.extend(self.mark(row, payload)?);
        Ok(appends)
    }

    /// Steps a `ClockAdvanced` tick at its `clock` (DEC-2304 item 7). A tick that reaches the next
    /// New York midnight is first the `RiskDayStarted` input at that tick (mandate spec §5.4): its
    /// step settles time up to the tick under the closing day's state and then sets E₀ (§5.2's
    /// order), as the reference cases step a rollover at its tick. The tick's `Clock` input follows
    /// at the same instant, with no time passing.
    fn tick(
        &mut self,
        row: &StoredEvent,
        payload: &Object,
    ) -> Result<Vec<RiskAppend>, UnreadableCause> {
        let clock = whole_second(payload, "clock")?;
        if clock < self.opened_at {
            return Ok(Vec::new());
        }
        let session = self.calendar.session(clock).map_err(refused)?;
        let mut appends = Vec::new();
        if clock >= risk_day(self.clock).map_err(refused)?.ends_at {
            let outcome = self.step(clock, session, Input::RiskDayStarted)?;
            appends.extend(self.records(row, clock, &outcome)?);
        }
        let outcome = self.step(clock, session, Input::Clock)?;
        appends.extend(self.records(row, clock, &outcome)?);
        self.clock = clock;
        Ok(appends)
    }

    fn step(
        &mut self,
        at: UtcNanos,
        session: MarketSession,
        input: Input,
    ) -> Result<Outcome, UnreadableCause> {
        self.risk
            .step(&Step { at, session, input })
            .map_err(refused)
    }

    /// The records a step journaled, in the step's §5.2 order, each with the payload journal spec
    /// §9.27 or §9.6 closes for it and the row as its cause. A record whose payload the fold cannot
    /// build makes the view unreadable (`append_failed`) rather than hold a change it has not
    /// journaled (`AGENTS.md` rule 5):
    ///
    /// - `KillSwitchActivated` is never returned. The fold never acts; the cancels and closes a
    ///   `flatten_and_pause` rung asks for are slice C5's (DEC-2307 item 3).
    /// - `AgentModeApplied` is the executor's own, and names as its restriction the action of the
    ///   step's last `RiskLimitTriggered` before it (DEC-2308 item 3). With no trigger before it in
    ///   the step, it names the step's last `RiskLimitLifted` (its ladder action, or its limit's
    ///   journal name when it carries none), or `goal_completed` after `GoalCompleted` (DEC-2331
    ///   item 1); any other mode change with no cause in its step fails closed.
    /// - `InstrumentRestrictionChanged` names the bound instrument (DEC-2307 item 4, DEC-2308 item 2),
    ///   which `open` and `bind` only ever bind to a canonical asset id or to one a mark named; a
    ///   restriction on an instrument that is not a canonical asset id fails closed as a refusal to
    ///   decide.
    /// - `HighWaterMarkReset` and `PositionReleased` carry the acknowledging user, step-up evidence,
    ///   and the canceled protective orders, which only the owner's input rows hold. The fold reads
    ///   none of those inputs yet, so they fail closed, as do `MandateVersionApplied`,
    ///   `UniverseChanged`, and `AgentStopped`, which the account stream's own writers journal.
    fn records(
        &self,
        row: &StoredEvent,
        clock: UtcNanos,
        outcome: &Outcome,
    ) -> Result<Vec<RiskAppend>, UnreadableCause> {
        let snapshot = &outcome.snapshot;
        let figures = || {
            vec![
                ("agent_equity", text(snapshot.agent_equity.to_string())),
                (
                    "high_water_mark",
                    text(snapshot.high_water_mark.to_string()),
                ),
                ("drawdown", text(snapshot.drawdown.to_string())),
                (
                    "day_start_equity",
                    text(snapshot.day_start_equity.to_string()),
                ),
                ("capital_base", text(snapshot.capital_base.to_string())),
                ("inherited_loss", text(snapshot.inherited_loss.to_string())),
                (
                    "net_contributed",
                    text(snapshot.net_contributed.to_string()),
                ),
            ]
        };
        let mut appends = Vec::new();
        let mut last_action: Option<String> = None;
        let mut cause: Option<String> = None;
        for (index, event) in outcome.journal.iter().enumerate() {
            let (event_type, agent, mut members) = match event {
                RiskEvent::RiskDayStarted { day_start_equity } => (
                    "RiskDayStarted",
                    "agent_id",
                    vec![
                        (
                            "day",
                            text(risk_day(clock).map_err(refused)?.day.to_string()),
                        ),
                        ("day_start_equity", text(day_start_equity.to_string())),
                    ],
                ),
                RiskEvent::RiskLimitTriggered {
                    limit,
                    action,
                    reason,
                } => {
                    last_action = Some(action.as_str().to_owned());
                    let breach_time_s = outcome
                        .breach_time_s(index)
                        .ok()
                        .flatten()
                        .ok_or(UnreadableCause::AppendFailed)?;
                    let reason = reason.map(|r| spelled(&TRIGGER_REASONS, r)).transpose()?;
                    let mut members = figures();
                    members.extend([
                        ("limit", text(limit.journal_name())),
                        ("action", text(action.as_str())),
                        ("reason", nullable(reason)),
                        (
                            "breach_time_s",
                            int(breach_time_s).map_err(|_| UnreadableCause::AppendFailed)?,
                        ),
                        ("metric", Value::Null),
                        ("threshold", Value::Null),
                        ("value", Value::Null),
                    ]);
                    ("RiskLimitTriggered", "agent_id", members)
                }
                RiskEvent::RiskLimitLifted {
                    limit,
                    action,
                    reason,
                } => {
                    let reason = reason.map(|r| spelled(&LIFT_REASONS, r)).transpose()?;
                    cause = Some(
                        action.map_or_else(|| limit.journal_name(), |a| a.as_str().to_owned()),
                    );
                    let mut members = figures();
                    members.extend([
                        ("limit", text(limit.journal_name())),
                        ("action", nullable(action.map(LadderAction::as_str))),
                        ("reason", nullable(reason)),
                    ]);
                    ("RiskLimitLifted", "agent_id", members)
                }
                RiskEvent::AgentModeApplied { to, .. } => {
                    let restriction = last_action
                        .clone()
                        .or_else(|| cause.clone())
                        .ok_or(UnreadableCause::AppendFailed)?;
                    (
                        "AgentModeApplied",
                        "agent",
                        vec![
                            ("to", text(spelled(&MODES, *to)?)),
                            ("restriction", text(restriction)),
                            ("originated", Value::Bool(true)),
                        ],
                    )
                }
                RiskEvent::InstrumentRestrictionChanged {
                    restriction,
                    reason,
                    active,
                } => {
                    let instrument = self
                        .instrument
                        .as_deref()
                        .filter(|named| AssetId::parse(named).is_ok())
                        .ok_or(UnreadableCause::RefusedToDecide)?;
                    (
                        "InstrumentRestrictionChanged",
                        "agent_id",
                        vec![
                            ("instrument", text(instrument)),
                            ("restriction", text(restriction.as_str())),
                            ("reason", text(spelled(&RESTRICTION_REASONS, *reason)?)),
                            ("active", Value::Bool(*active)),
                        ],
                    )
                }
                RiskEvent::GoalCompleted {
                    reason,
                    then,
                    on_complete,
                } => {
                    cause = Some("goal_completed".to_owned());
                    (
                        "GoalCompleted",
                        "agent_id",
                        vec![
                            ("reason", nullable(reason.map(GoalReason::as_str))),
                            ("then", nullable(then.map(ThenAction::as_str))),
                            ("on_complete", nullable(on_complete.map(OnComplete::as_str))),
                        ],
                    )
                }
                RiskEvent::KillSwitchActivated { .. } => continue,
                RiskEvent::HighWaterMarkReset { .. }
                | RiskEvent::PositionReleased { .. }
                | RiskEvent::MandateVersionApplied { .. }
                | RiskEvent::UniverseChanged { .. }
                | RiskEvent::AgentStopped { .. } => return Err(UnreadableCause::AppendFailed),
            };
            members.extend([
                (agent, text(self.agent.as_str())),
                ("risk_clock", text(clock.to_string())),
            ]);
            appends.push(RiskAppend {
                event_type,
                schema_version: 1,
                causation_id: Some(row.event_id.clone()),
                payload: object(members).map_err(|_| UnreadableCause::AppendFailed)?,
            });
        }
        Ok(appends)
    }

    /// The records the fold returned that the stream has not carried back yet, in the order they
    /// are owed. At the stream's head after a restart these are the records a crash kept from
    /// being appended after their cause, which the executor appends next (DEC-2399 item 3).
    /// Nothing is owed by an unreadable view: what it would owe is no longer the journal's.
    pub fn owed(&self) -> impl Iterator<Item = &RiskAppend> {
        self.owed.iter()
    }

    /// Records that the records the fold last returned could not be appended, which makes the
    /// view unreadable for good (DEC-2304).
    pub fn append_failed(&mut self) {
        self.unreadable(UnreadableCause::AppendFailed);
    }

    /// Makes the view unreadable at its head, keeping the first cause. Its mode is at least
    /// `exits_only`, and the last readable view's when that held more (DEC-2304).
    /// An unreadable view owes nothing (DEC-2399 item 3).
    fn unreadable(&mut self, cause: UnreadableCause) {
        self.owed.clear();
        self.held = None;
        let (head, mode) = match &self.view {
            RiskView::Readable { snapshot, head } => {
                (*head, snapshot.agent_mode.max(AgentMode::ExitsOnly))
            }
            RiskView::Unreadable {
                cause: UnreadableCause::Unpriced,
                head,
                mode,
            } => (*head, *mode),
            RiskView::Unreadable { .. } => return,
        };
        self.view = RiskView::Unreadable { cause, head, mode };
    }
}

/// The binding gate's risk snapshot of `view` (DEC-2804 item 7): the risk state's E, H, E₀, C, L,
/// latched limits, active rungs, size factor and mode, with the mode beside it, for a readable
/// view; `None` for an unreadable one, which holds the opening and never an exit (`AGENTS.md`
/// rules 3 and 13).
pub fn risk_snapshot(
    view: &RiskView,
) -> Option<(mandate_risk::RiskSnapshot, mandate_risk::AgentMode)> {
    let RiskView::Readable { snapshot, .. } = view else {
        return None;
    };
    let mode = match snapshot.agent_mode {
        AgentMode::Normal => mandate_risk::AgentMode::Normal,
        AgentMode::ExitsOnly => mandate_risk::AgentMode::ExitsOnly,
        AgentMode::Paused => mandate_risk::AgentMode::Paused,
        AgentMode::Stopped => mandate_risk::AgentMode::Stopped,
    };
    let latched = snapshot
        .latched
        .iter()
        .map(|limit| match limit {
            LimitKey::MaxDailyLoss => mandate_risk::spec_types::LimitKey::MaxDailyLoss,
            LimitKey::DrawdownRung(rung) => mandate_risk::spec_types::LimitKey::DrawdownRung(*rung),
            LimitKey::LifetimeFloor => mandate_risk::spec_types::LimitKey::LifetimeFloor,
            LimitKey::ProfitStop => mandate_risk::spec_types::LimitKey::ProfitStop,
        })
        .collect();
    let risk = mandate_risk::RiskSnapshot {
        agent_equity: snapshot.agent_equity,
        high_water_mark: snapshot.high_water_mark,
        day_start_equity: snapshot.day_start_equity,
        capital_base: snapshot.capital_base,
        inherited_loss: snapshot.inherited_loss,
        latched,
        active_rungs: snapshot.active_rungs.clone(),
        size_factor: snapshot.size_factor,
        agent_mode: mode,
    };
    Some((risk, mode))
}

/// The calendar the deployed fold steps on (DEC-2804 item 6), from `mandate-time`'s US-equities
/// exchange calendar. For an equity it counts regular-session seconds (mandate spec §5.2,
/// DEC-2305 item 8) and answers the session an instant is in; while the market is closed it
/// answers `overnight`. Only a mark reads its step's session, and a mark carries its own (DEC-2305
/// item 1), so that answer moves no figure. For crypto it counts every second and answers `crypto`.
pub struct ExchangeRiskCalendar {
    calendar: ExchangeCalendar,
    crypto: bool,
}

impl ExchangeRiskCalendar {
    /// The calendar for a position of `class`.
    ///
    /// # Errors
    /// Whatever `mandate-time` refuses when its calendar cannot load.
    pub fn new(class: AssetClass) -> Result<Self, mandate_time::CalendarDataError> {
        Ok(Self {
            calendar: ExchangeCalendar::us_equities()?,
            crypto: class == AssetClass::Crypto,
        })
    }
}

/// The whole seconds `[start, end)` and `[from, to)` share; none when they do not meet, which the
/// difference shows as negative.
fn overlap(start: UtcNanos, end: UtcNanos, from: UtcNanos, to: UtcNanos) -> u64 {
    let shared = end.min(to).secs().saturating_sub(start.max(from).secs());
    u64::try_from(shared).unwrap_or(0)
}

fn whole_seconds(from: UtcNanos, to: UtcNanos) -> Result<u64, SpecError> {
    to.secs()
        .checked_sub(from.secs())
        .and_then(|seconds| u64::try_from(seconds).ok())
        .ok_or(SpecError::ClockWentBackwards)
}

impl SessionClock for ExchangeRiskCalendar {
    fn seconds_between(&self, from: UtcNanos, to: UtcNanos) -> Result<u64, SpecError> {
        let every_second = whole_seconds(from, to)?;
        if self.crypto {
            return Ok(every_second);
        }
        let mut total: u64 = 0;
        let mut date = from.date();
        while date <= to.date() {
            for span in self.calendar.sessions(date)? {
                if span.session() == Session::Regular {
                    total = total.saturating_add(overlap(span.start(), span.end(), from, to));
                }
            }
            date = date.next()?;
        }
        Ok(total)
    }
}

impl RiskCalendar for ExchangeRiskCalendar {
    fn session(&self, at: UtcNanos) -> Result<MarketSession, SpecError> {
        if self.crypto {
            return Ok(MarketSession::Crypto);
        }
        Ok(match self.calendar.session_at(at)? {
            Some(Session::PreMarket) => MarketSession::PreMarket,
            Some(Session::Regular) => MarketSession::Regular,
            Some(Session::AfterHours) => MarketSession::AfterHours,
            Some(Session::Overnight) | None => MarketSession::Overnight,
        })
    }
}

/// [`ACCOUNT_STREAM`] reads exactly the types `mandate-journal` admits on an account stream, so a
/// type the catalogue adds there is never left to fail as `unfolded_event_type`.
#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use mandate_journal::{StreamType, stream_event_types};

    use super::{ACCOUNT_STREAM, reading};

    #[test]
    fn the_fold_reads_every_type_the_journal_admits_on_an_account_stream_and_no_other() {
        let catalogue: BTreeSet<&str> = stream_event_types(StreamType::Account).collect();
        let table: BTreeSet<&str> = ACCOUNT_STREAM.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            table.len(),
            ACCOUNT_STREAM.len(),
            "the table repeats a type"
        );
        assert_eq!(table, catalogue);
        for event_type in catalogue {
            assert!(reading(event_type).is_some(), "{event_type} has no reading");
        }
    }
}
