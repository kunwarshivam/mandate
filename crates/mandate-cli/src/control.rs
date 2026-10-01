//! What M7's owner commands share ([task brief](../../../docs/project/tasks/M7-escalation-v0.md),
//! "The CLI control surface"; DEC-155 item 5): the journal they read and append to, who is
//! running them, and how one control-stream event is committed.
//!
//! The CLI is an **untrusted surface** and a one-shot writer of the workspace control stream. Every
//! command that changes anything commits exactly one control-stream event and nothing else; it
//! never writes an agent or account stream and never talks to a runtime or a broker (`AGENTS.md`
//! rule 12). Its local checks are a convenience: the runtime makes every one of them again.

use std::fmt;

use mandate_journal::{AppendOutcome, Environment, Head, StoredEvent, StreamId};
use mandate_time::UtcNanos;

/// The reads and the one append a command makes, with journal spec §5.1's append. The
/// implementation gives it a `mandate-journal-pg` adapter for a workspace deployment; the tests
/// answer it with their own in-memory journal in `tests/common`, because `mandate-journal`'s
/// `MemoryJournal` validates payloads it has no schema for yet (DEC-257 item 17).
///
/// A command mints its control-stream event id once and retries the same draft until the journal
/// answers `Committed` or `AlreadyCommitted`: the id is the idempotency key (DEC-155 item 2), so a
/// retry after a fence, a head that moved, or a lost answer commits nothing twice (DEC-257 item
/// 16).
pub trait ControlJournal {
    /// Every committed event of `stream`, in `seq` order.
    ///
    /// # Errors
    /// [`ControlError::Journal`] when the journal cannot be read.
    fn rows(&self, stream: &StreamId) -> Result<Vec<StoredEvent>, ControlError>;
    /// # Errors
    /// [`ControlError::Journal`] when the journal cannot be read.
    fn head(&self, stream: &StreamId) -> Result<Head, ControlError>;
    /// Takes a new writer epoch for `stream`, fencing any other writer (journal spec §5.1).
    ///
    /// # Errors
    /// [`ControlError::Journal`] when the journal cannot be written.
    fn take_ownership(&mut self, stream: &StreamId) -> Result<u64, ControlError>;
    /// # Errors
    /// [`ControlError::Journal`] when the journal cannot be written.
    fn append(
        &mut self,
        stream: &StreamId,
        expected_head: u64,
        writer_epoch: u64,
        recorded_at: UtcNanos,
        drafts: &[&[u8]],
    ) -> Result<AppendOutcome, ControlError>;
}

/// Who runs a command, from the CLI's workspace configuration: opaque ids only, no personal data
/// (journal spec §6.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owner {
    pub workspace: String,
    /// The owner's opaque user id, which the event's `actor.id` and the payload carry.
    pub user: String,
    pub environment: Environment,
}

/// The ids a command needs, injected so a test sees them: the control-stream event's ULID, minted
/// once per command and reused on every retry, and a fresh step-up assertion id per gesture.
pub trait Ids {
    fn event_id(&mut self) -> String;
    fn assertion_id(&mut self) -> String;
}

/// The moment the owner ran the command: the instant the event records and its risk-clock second.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Now {
    pub at: UtcNanos,
    pub secs: i64,
}

/// A committed control-stream event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submitted {
    pub event_id: String,
    pub seq: u64,
}

/// Why a command did not commit, or could not answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlError {
    /// The body of every stub in the tests PR (DEC-77).
    Unimplemented { story: &'static str },
    /// A local check refused, with the runtime's own reason code; nothing was committed.
    Refused { reason: &'static str },
    /// The journal could not be read or written, or refused the append.
    Journal(String),
}

impl fmt::Display for ControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unimplemented { story } => write!(f, "{story} has not been implemented yet"),
            Self::Refused { reason } => write!(
                f,
                "refused ({reason}); nothing was committed, and if you do nothing, this action is \
                 skipped"
            ),
            Self::Journal(why) => write!(f, "the journal: {why}"),
        }
    }
}

impl std::error::Error for ControlError {}

/// The workspace control stream, `ctl:{workspace}` (journal spec §2).
///
/// # Errors
/// [`ControlError::Journal`] for a workspace id that is not a stream id's.
pub fn control_stream(owner: &Owner) -> Result<StreamId, ControlError> {
    StreamId::parse(&format!("ctl:{}", owner.workspace))
        .ok_or_else(|| ControlError::Journal("not a workspace id".to_owned()))
}

/// An agent's stream, `agent:{workspace}:{agent}` (journal spec §2).
///
/// # Errors
/// [`ControlError::Journal`] for an id that is not a stream id's.
pub fn agent_stream(owner: &Owner, agent: &str) -> Result<StreamId, ControlError> {
    StreamId::parse(&format!("agent:{}:{agent}", owner.workspace))
        .ok_or_else(|| ControlError::Journal("not an agent id".to_owned()))
}
