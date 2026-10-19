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

use mandate_canon::Value;
use mandate_domain::{AgentMode, MarketSession};
use mandate_journal::StoredEvent;
use mandate_spec::SpecError;
use mandate_spec::risk::{Opening, RiskState, SessionClock, Snapshot};
use mandate_spec::validate::ValidatedMandate;
use mandate_time::UtcNanos;

use crate::ExecutorError;
use crate::tripwire::TripwireState;

const STORY: &str = "E6-15";

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
    #[expect(dead_code, reason = "the fold reads it from E6-15 B2 on")]
    agent: String,
    #[expect(dead_code, reason = "the fold reads it from E6-15 B2 on")]
    risk: RiskState<'c>,
    #[expect(dead_code, reason = "the fold reads it from E6-15 B2 on")]
    calendar: &'c dyn RiskCalendar,
    #[expect(dead_code, reason = "the fold reads it from E6-15 B2 on")]
    tripwires: TripwireState,
    view: RiskView,
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
        Err(ExecutorError::Unimplemented { story: STORY })
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
