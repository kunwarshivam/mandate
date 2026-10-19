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

use mandate_canon::{Object, Value, parse};
use mandate_domain::{AgentMode, MarketSession};
use mandate_journal::StoredEvent;
use mandate_spec::SpecError;
use mandate_spec::risk::{
    Input, Opening, Outcome, RiskEvent, RiskState, SessionClock, Snapshot, Step, risk_day,
};
use mandate_spec::validate::ValidatedMandate;
use mandate_time::UtcNanos;

use crate::ExecutorError;
use crate::payload::{object, text};
use crate::tripwire::TripwireState;

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
}

/// The account stream's catalogue (journal spec §9): every event type `mandate-journal`'s
/// `stream_event_types` lists for an account stream, and how the fold reads each. A table rather
/// than a `match`, so no type can fall through to another type's reading.
const ACCOUNT_STREAM: [(&str, Reading); 51] = [
    ("StreamOpened", Reading::Record),
    ("TradingDayStarted", Reading::Record),
    ("ClockAdvanced", Reading::Tick),
    ("KillSwitchActivated", Reading::Record),
    ("OwnerAcknowledged", Reading::RiskInput),
    ("OwnerAcknowledgmentRequested", Reading::Record),
    ("OwnerCommandRefused", Reading::Record),
    ("IntentReceived", Reading::Record),
    ("GateDecided", Reading::Record),
    ("FillApplied", Reading::RiskInput),
    ("LateFillApplied", Reading::RiskInput),
    ("FeesCharged", Reading::RiskInput),
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
    ("OrderSubmitted", Reading::Record),
    ("OrderRequestRecorded", Reading::Record),
    ("OrderStateChanged", Reading::Record),
    ("OrderAbandoned", Reading::Record),
    ("BrokerExchangeRecorded", Reading::Record),
    ("MarkUpdated", Reading::Mark),
    ("ProtectionChanged", Reading::Record),
    ("OwnerConfirmationReceived", Reading::Record),
    ("BrokerPositionObserved", Reading::Record),
    ("ReconciliationRun", Reading::Record),
    ("CompensatingEvent", Reading::RiskInput),
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

/// A refusal to decide, which the fold fails closed on (DEC-2304 item 1).
fn refused(_: SpecError) -> UnreadableCause {
    UnreadableCause::RefusedToDecide
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

/// The account stream's risk state, folded row by row.
pub struct AccountRiskFold<'c> {
    stream: String,
    agent: String,
    risk: RiskState<'c>,
    calendar: &'c dyn RiskCalendar,
    #[expect(dead_code, reason = "the fold steps the tripwires from E6-15 B2b on")]
    tripwires: TripwireState,
    view: RiskView,
    /// The risk clock the risk state last stepped at, from which a tick crosses midnight.
    clock: UtcNanos,
}

impl<'c> AccountRiskFold<'c> {
    /// Opens the fold for `agent` on `stream` after the row at `head`, with the risk state at
    /// `opening` (mandate spec §5.2) and the tripwire state folded so far.
    ///
    /// # Errors
    /// Whatever `RiskState::open` refuses.
    pub fn open(
        stream: &str,
        head: u64,
        agent: &str,
        mandate: &ValidatedMandate,
        opening: &Opening,
        calendar: &'c dyn RiskCalendar,
        tripwires: TripwireState,
    ) -> Result<Self, SpecError> {
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
            clock: opening.at,
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
    /// changes nothing. [`ExecutorError::Unimplemented`] for a version-2 `MarkUpdated` until E6-15
    /// slice B2b reads it, which changes nothing either.
    pub fn fold(&mut self, row: &StoredEvent) -> Result<Vec<RiskAppend>, ExecutorError> {
        if row.stream_id != self.stream {
            return Err(ExecutorError::ForeignStream {
                stream: row.stream_id.clone(),
            });
        }
        let RiskView::Readable { head, .. } = &self.view else {
            return Ok(Vec::new());
        };
        if head.checked_add(1) != Some(row.seq) {
            self.unreadable(UnreadableCause::SeqGap);
            return Ok(Vec::new());
        }
        if (row.event_type.as_str(), row.schema_version) == ("MarkUpdated", 2) {
            return Self::mark(row);
        }
        match self.read(row) {
            Ok(appends) => {
                self.view = RiskView::Readable {
                    snapshot: Box::new(self.risk.snapshot().clone()),
                    head: row.seq,
                };
                Ok(appends)
            }
            Err(cause) => {
                self.unreadable(cause);
                Ok(Vec::new())
            }
        }
    }

    /// Reads the row after the head: its body, its type and version, and, for a tick, the risk
    /// state's steps.
    fn read(&mut self, row: &StoredEvent) -> Result<Vec<RiskAppend>, UnreadableCause> {
        let body = parse(&row.body).map_err(|_| UnreadableCause::Undecodable)?;
        let payload = body
            .get("payload")
            .and_then(Value::as_object)
            .ok_or(UnreadableCause::Undecodable)?;
        match (reading(&row.event_type), row.schema_version) {
            (None, _) => Err(UnreadableCause::UnfoldedEventType),
            (Some(Reading::Record), _) | (Some(Reading::Mark), 1) => Ok(Vec::new()),
            (Some(Reading::Tick), 1) => self.tick(row, payload),
            (Some(_), _) => Err(UnreadableCause::UnknownSchemaVersion),
        }
    }

    /// Steps a `MarkUpdated` at version 2 (journal spec §9.30, DEC-2305): its `bid`, `sane`, and
    /// `session` become `Input::Mark` at its `risk_clock`, as mandate spec §5.2 reads a mark.
    fn mark(row: &StoredEvent) -> Result<Vec<RiskAppend>, ExecutorError> {
        let _ = row;
        Err(ExecutorError::Unimplemented { story: "E6-15" })
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
        let clock = payload
            .get("clock")
            .and_then(Value::as_str)
            .and_then(|clock| UtcNanos::parse(clock).ok())
            .filter(|clock| clock.nanos() == 0)
            .ok_or(UnreadableCause::Undecodable)?;
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

    /// The records a step journaled, as §9.27 closes them. B2a writes `RiskDayStarted` only, so any
    /// other record is one it cannot append, and the view fails closed rather than hold a change it
    /// has not journaled (`AGENTS.md` rule 5).
    fn records(
        &self,
        row: &StoredEvent,
        clock: UtcNanos,
        outcome: &Outcome,
    ) -> Result<Vec<RiskAppend>, UnreadableCause> {
        outcome
            .journal
            .iter()
            .map(|event| match event {
                RiskEvent::RiskDayStarted { day_start_equity } => Ok(RiskAppend {
                    event_type: "RiskDayStarted",
                    schema_version: 1,
                    causation_id: Some(row.event_id.clone()),
                    payload: object(vec![
                        ("agent_id", text(self.agent.as_str())),
                        (
                            "day",
                            text(risk_day(clock).map_err(refused)?.day.to_string()),
                        ),
                        ("day_start_equity", text(day_start_equity.to_string())),
                        ("risk_clock", text(clock.to_string())),
                    ])
                    .map_err(|_| UnreadableCause::AppendFailed)?,
                }),
                _ => Err(UnreadableCause::AppendFailed),
            })
            .collect()
    }

    /// Records that the records the fold last returned could not be appended, which makes the
    /// view unreadable for good (DEC-2304).
    pub fn append_failed(&mut self) {
        self.unreadable(UnreadableCause::AppendFailed);
    }

    /// Makes the view unreadable at its head, keeping the first cause. Its mode is at least
    /// `exits_only`, and the last readable view's when that held more (DEC-2304).
    fn unreadable(&mut self, cause: UnreadableCause) {
        if let RiskView::Readable { snapshot, head } = &self.view {
            self.view = RiskView::Unreadable {
                cause,
                head: *head,
                mode: snapshot.agent_mode.max(AgentMode::ExitsOnly),
            };
        }
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
