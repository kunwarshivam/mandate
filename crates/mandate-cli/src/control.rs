//! What M7's owner commands share ([task brief](../../../docs/project/tasks/M7-escalation-v0.md),
//! "The CLI control surface"; DEC-155 item 5): the journal they read and append to, who is
//! running them, and how one control-stream event is committed.
//!
//! The CLI is an **untrusted surface** and a one-shot writer of the workspace control stream. Every
//! command that changes anything commits exactly one control-stream event and nothing else; it
//! never writes an agent or account stream and never talks to a runtime or a broker (`AGENTS.md`
//! rule 12). Its local checks are a convenience: the runtime makes every one of them again.

use std::fmt;

use mandate_canon::{Digest, Int, Key, Object, Value, parse, to_canonical};
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

/// How many times a command re-reads and appends its one draft before it gives up. Each retry
/// follows a fence, a head that moved, or a lost answer (DEC-257 item 16); one that keeps meeting
/// them is reported rather than retried for ever, and a report commits nothing twice either way,
/// because every attempt carries the same event id.
const ATTEMPTS: usize = 8;

/// One stored event's envelope, parsed from its canonical bytes.
///
/// # Errors
/// [`ControlError::Journal`] for bytes that are not a canonical object.
pub(crate) fn envelope(row: &StoredEvent) -> Result<Value, ControlError> {
    parse(&row.body).map_err(|e| ControlError::Journal(format!("a stored event: {e:?}")))
}

/// Every stored envelope of `stream`, in `seq` order.
///
/// # Errors
/// As [`ControlJournal::rows`] and [`envelope`].
pub(crate) fn envelopes(
    journal: &dyn ControlJournal,
    stream: &StreamId,
) -> Result<Vec<Value>, ControlError> {
    journal.rows(stream)?.iter().map(envelope).collect()
}

pub(crate) fn text(value: &str) -> Value {
    Value::Str(value.to_owned())
}

pub(crate) fn object(members: Vec<(&'static str, Value)>) -> Result<Value, ControlError> {
    let mut map = Object::new();
    for (name, value) in members {
        let key = Key::new(name).map_err(|_| ControlError::Journal(format!("key {name}")))?;
        map.insert(key, value);
    }
    Ok(Value::Object(map))
}

/// A risk-clock second as the canonical integer the control-stream payloads carry (DEC-257 item 5).
///
/// # Errors
/// [`ControlError::Journal`] for a second the canonical integer cannot hold.
pub(crate) fn seconds(secs: i64) -> Result<Value, ControlError> {
    u64::try_from(secs)
        .ok()
        .and_then(Int::new)
        .map(Value::Int)
        .ok_or_else(|| ControlError::Journal(format!("the second {secs}")))
}

/// `cli_confirm` evidence for one gesture: a fresh assertion id, authenticated when the owner ran
/// the command (DEC-155 item 4). Minted only once every local check has passed, so a refused
/// command uses no assertion.
///
/// # Errors
/// As [`seconds`].
pub(crate) fn step_up(ids: &mut dyn Ids, now: Now) -> Result<Value, ControlError> {
    object(vec![
        ("assertion_id", text(&ids.assertion_id())),
        ("authenticated_at", seconds(now.secs)?),
        ("method", text("cli_confirm")),
    ])
}

/// The first eight hex digits of the SHA-256 of `confirmed`'s canonical bytes: the shape every
/// code the owner re-types takes (DEC-155 item 3, DEC-257 item 13).
pub(crate) fn code_of(confirmed: &Value) -> String {
    let hex = Digest::of(&to_canonical(confirmed)).to_hex();
    hex.chars().take(CODE_LEN).collect()
}

const CODE_LEN: usize = 8;

/// The build the envelope's actor names: a content reference over the crate's name and version.
fn build_ref() -> String {
    let digest = Digest::of(concat!("mandate-cli/", env!("CARGO_PKG_VERSION")).as_bytes());
    format!("sha256:{}", digest.to_hex())
}

/// The canonical bytes of one control-stream draft, written by the owner as a `user` (journal spec
/// §3; EI-10 admits nothing else).
fn draft(
    owner: &Owner,
    stream: &StreamId,
    event_id: &str,
    event_type: &str,
    payload: Value,
    now: Now,
) -> Result<Vec<u8>, ControlError> {
    let one = seconds(1)?;
    let fields = object(vec![
        (
            "actor",
            object(vec![
                ("build", text(&build_ref())),
                ("id", text(&owner.user)),
                ("kind", text("user")),
                ("version", text(env!("CARGO_PKG_VERSION"))),
            ])?,
        ),
        ("artifact_refs", Value::Array(Vec::new())),
        ("causation_id", Value::Null),
        ("clock_source", text("local")),
        ("config_refs", Value::Object(Object::new())),
        ("correlation_id", Value::Null),
        ("environment", text(owner.environment.as_str())),
        ("envelope_version", one.clone()),
        ("event_id", text(event_id)),
        ("event_time", text(&now.at.to_string())),
        ("event_type", text(event_type)),
        ("payload", payload),
        ("pii_refs", Value::Array(Vec::new())),
        ("schema_version", one),
        ("stream_id", text(stream.as_str())),
    ])?;
    Ok(to_canonical(&fields))
}

/// Commits exactly one control-stream event (DEC-155 items 2 and 5, DEC-257 item 16). The event id
/// is minted once; every attempt takes a new writer epoch, reads the head afresh, and appends the
/// same bytes, so a fence, a moved head, or a lost answer is retried, and a retry of an append that
/// did commit answers `AlreadyCommitted` with the stored event rather than storing a second.
///
/// # Errors
/// [`ControlError::Journal`] when the journal refuses the draft, reports a conflicting event under
/// the id, or keeps answering with a retry after [`ATTEMPTS`].
pub(crate) fn commit(
    journal: &mut dyn ControlJournal,
    ids: &mut dyn Ids,
    owner: &Owner,
    event_type: &str,
    payload: Value,
    now: Now,
) -> Result<Submitted, ControlError> {
    let stream = control_stream(owner)?;
    let event_id = ids.event_id();
    let bytes = draft(owner, &stream, &event_id, event_type, payload, now)?;
    for _ in 0..ATTEMPTS {
        let epoch = journal.take_ownership(&stream)?;
        let head = journal.head(&stream)?.seq;
        match journal.append(&stream, head, epoch, now.at, &[&bytes])? {
            AppendOutcome::Committed(rows) | AppendOutcome::AlreadyCommitted(rows) => {
                let stored = rows
                    .iter()
                    .find(|row| row.event_id == event_id)
                    .ok_or_else(|| ControlError::Journal("the stored event is missing".into()))?;
                return Ok(Submitted {
                    event_id,
                    seq: stored.seq,
                });
            }
            AppendOutcome::Fenced { .. }
            | AppendOutcome::HeadMismatch { .. }
            | AppendOutcome::Ambiguous
            | AppendOutcome::Unavailable => {}
            AppendOutcome::IdempotencyConflict { stored_seq } => {
                return Err(ControlError::Journal(format!(
                    "another event is stored under this id at seq {stored_seq}"
                )));
            }
            AppendOutcome::Invalid { error, .. } => {
                return Err(ControlError::Journal(format!("refused: {error:?}")));
            }
        }
    }
    Err(ControlError::Journal(
        "the control stream did not settle; the command was committed at most once".to_owned(),
    ))
}
