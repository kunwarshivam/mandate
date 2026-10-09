//! What M7's owner commands share ([task brief](../../../docs/project/tasks/M7-escalation-v0.md),
//! "The CLI control surface"; DEC-155 item 5): the journal they read and append to, who is
//! running them, and how one control-stream event is committed.
//!
//! The CLI is an **untrusted surface** and a one-shot writer of the workspace control stream. Every
//! command that changes anything commits exactly one control-stream event and nothing else; it
//! never writes an agent or account stream and never talks to a runtime or a broker (`AGENTS.md`
//! rule 12). Its local checks are a convenience: the runtime makes every one of them again.

use std::collections::BTreeSet;
use std::fmt;
use std::time::Duration;

use mandate_canon::{Digest, Int, Key, Object, Value, parse, to_canonical};
use mandate_journal::{AppendOutcome, ArtifactRef, Environment, Head, StoredEvent, StreamId};
use mandate_time::UtcNanos;

/// The reads and the one append a command makes, with journal spec §5.1's append. The
/// implementation gives it a `mandate-journal-pg` adapter for a workspace deployment; the tests
/// answer it with their own in-memory journal in `tests/common`, because `mandate-journal`'s
/// `MemoryJournal` validates payloads it has no schema for yet (DEC-257 item 17).
///
/// A command derives its control-stream event id from what the owner chose and the head it was
/// decided at, and retries the same draft until the journal answers `Committed` or
/// `AlreadyCommitted`: the id is the idempotency key (DEC-155 item 2), so a retry after a fence, a
/// head that moved, or a lost answer commits nothing twice (DEC-257 item 16), and a re-run of a
/// command that did commit finds it (DEC-290).
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
    /// The pause before the next attempt, after one the journal answered with a retry. The
    /// deployment's adapter sleeps; a test records it.
    fn wait(&mut self, delay: Duration);
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

/// The ids a command mints, injected so a test sees them: a fresh step-up assertion id per gesture.
/// The control-stream event id is not minted but derived (DEC-290).
pub trait Ids {
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

/// An account's stream, `acct:{workspace}:{account}` (journal spec §2), which `status` reads for
/// the agent's restrictions.
///
/// # Errors
/// [`ControlError::Journal`] for an id that is not a stream id's.
pub fn account_stream(owner: &Owner, account: &str) -> Result<StreamId, ControlError> {
    StreamId::parse(&format!("acct:{}:{account}", owner.workspace))
        .ok_or_else(|| ControlError::Journal("not an account id".to_owned()))
}

/// How many times a command re-reads and appends its one draft before it gives up. Each retry
/// follows a fence, a head that moved, or a lost answer (DEC-257 item 16), after a [`backoff`];
/// one that keeps meeting them is reported rather than retried for ever, and a report commits
/// nothing twice either way, because every attempt carries the same event id.
const ATTEMPTS: u32 = 8;

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
fn step_up(ids: &mut dyn Ids, now: Now) -> Result<Value, ControlError> {
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

/// What an envelope says of its event besides its type and payload: the schema version, the
/// content references the payload names, sorted, which the envelope must list (journal spec §3),
/// and the configuration it binds, by kind (§9's required `config_refs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Shape {
    pub(crate) schema_version: u64,
    pub(crate) artifact_refs: Vec<String>,
    pub(crate) config_refs: Vec<(&'static str, Value)>,
}

/// Who writes a control-stream draft. Private to this module, so no other module can name the
/// system writer, and [`draft`] refuses it for anything but the stream's opening (#725 review).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Writer {
    /// The owner, as a `user` (EI-10).
    Owner,
    /// The vectors' `control_services` opener, as a `system`, for `StreamOpened` alone (DEC-527
    /// item 7).
    Opener,
}

const OPENER_ID: &str = "control_services";

/// Commits `ctl:{workspace}`'s `StreamOpened` in the owner's environment, as the vectors'
/// `control_services` opener (DEC-527 item 7), its id derived at head 0 (DEC-290).
///
/// # Errors
/// As [`commit`].
pub(crate) fn open_control_stream(
    journal: &mut dyn ControlJournal,
    owner: &Owner,
    now: Now,
) -> Result<Submitted, ControlError> {
    let stream = control_stream(owner)?;
    let payload = object(vec![
        ("stream_type", text("control")),
        ("workspace_id", text(&owner.workspace)),
    ])?;
    let event_id = derive(&stream, "StreamOpened", &payload, false, 0)?;
    let shape = Shape {
        schema_version: 1,
        artifact_refs: Vec::new(),
        config_refs: Vec::new(),
    };
    let bytes = draft(
        Writer::Opener,
        owner,
        &stream,
        &event_id,
        "StreamOpened",
        shape,
        payload,
        now,
    )?;
    settle(
        journal,
        &stream,
        event_id,
        Repeat::FindsEarlier,
        &bytes,
        now,
    )
}

/// A schema version as the canonical integer the envelope carries.
///
/// # Errors
/// [`ControlError::Journal`] for a version the canonical integer cannot hold.
fn version(schema_version: u64) -> Result<Value, ControlError> {
    Int::new(schema_version)
        .map(Value::Int)
        .ok_or_else(|| ControlError::Journal(format!("the schema version {schema_version}")))
}

/// The canonical bytes of one control-stream draft, written by the owner as a `user` (journal spec
/// §3; EI-10 admits nothing else), or, for the stream's `StreamOpened` alone, by the opener.
///
/// # Errors
/// [`ControlError::Journal`] for the opener writing any other event type.
#[allow(
    clippy::too_many_arguments,
    reason = "each is one envelope member's source; bundling them would name a type for one caller"
)]
fn draft(
    writer: Writer,
    owner: &Owner,
    stream: &StreamId,
    event_id: &str,
    event_type: &str,
    shape: Shape,
    payload: Value,
    now: Now,
) -> Result<Vec<u8>, ControlError> {
    let ids = (event_id, None);
    draft_caused(writer, owner, stream, ids, event_type, shape, payload, now)
}

/// [`draft`] for an event that names its cause, `ids.1`, as its `causation_id` (journal spec §3).
///
/// # Errors
/// As [`draft`].
#[allow(
    clippy::too_many_arguments,
    reason = "each is one envelope member's source; bundling them would name a type for one caller"
)]
fn draft_caused(
    writer: Writer,
    owner: &Owner,
    stream: &StreamId,
    (event_id, causation): (&str, Option<&str>),
    event_type: &str,
    shape: Shape,
    payload: Value,
    now: Now,
) -> Result<Vec<u8>, ControlError> {
    let (kind, id) = match writer {
        Writer::Owner => ("user", owner.user.as_str()),
        Writer::Opener if event_type == "StreamOpened" => ("system", OPENER_ID),
        Writer::Opener => {
            return Err(ControlError::Journal(format!(
                "only the stream's opening is written as {OPENER_ID}, not {event_type}"
            )));
        }
    };
    let one = seconds(1)?;
    let fields = object(vec![
        (
            "actor",
            object(vec![
                ("build", text(&build_ref())),
                ("id", text(id)),
                ("kind", text(kind)),
                ("version", text(env!("CARGO_PKG_VERSION"))),
            ])?,
        ),
        (
            "artifact_refs",
            Value::Array(shape.artifact_refs.iter().map(|r| text(r)).collect()),
        ),
        ("causation_id", causation.map_or(Value::Null, text)),
        ("clock_source", text("local")),
        ("config_refs", object(shape.config_refs)?),
        ("correlation_id", Value::Null),
        ("environment", text(owner.environment.as_str())),
        ("envelope_version", one.clone()),
        ("event_id", text(event_id)),
        ("event_time", text(&now.at.to_string())),
        ("event_type", text(event_type)),
        ("payload", payload),
        ("pii_refs", Value::Array(Vec::new())),
        ("schema_version", version(shape.schema_version)?),
        ("stream_id", text(stream.as_str())),
    ])?;
    Ok(to_canonical(&fields))
}

/// A command the owner decided, before it is committed: what the owner chose (`key`, the payload's
/// members other than the owner's second and the step-up evidence), whether the owner's
/// confirmation counts at the control stream's head it was decided at, and the event id derived
/// from all three (DEC-290).
#[derive(Debug, Clone)]
pub(crate) struct Decided {
    stream: StreamId,
    event_type: &'static str,
    key: Vec<(&'static str, Value)>,
    /// Whether the command carries step-up evidence: the owner typed the code it needs at `head`.
    pub(crate) stepped_up: bool,
    event_id: String,
    /// Whether a re-run finds this command or commits another, which the exhausted report says.
    repeat: Repeat,
}

/// What [`decide`] found: a command to commit, or the same command already committed as the
/// control stream's last event and not yet recorded by its runtime, which a re-run reports rather
/// than committing twice (DEC-290).
#[derive(Debug)]
pub(crate) enum Decision {
    Fresh(Decided),
    Committed(Submitted),
}

/// Whether a re-run of a command may be reported as the same command already committed, or is
/// always committed (DEC-290 item 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Repeat {
    FindsEarlier,
    /// A kill switch: never reported as an earlier one (rule 13).
    AlwaysCommits,
}

/// Whether a command carries step-up evidence.
#[derive(Clone, Copy)]
pub(crate) enum Confirmation<'a> {
    /// The owner typed a code bound to the control stream's head: the function answers whether it
    /// is the code the command needs at a given head.
    AtHead(&'a dyn Fn(u64) -> Result<bool, ControlError>),
    /// Whether it does, whatever the head: a grant's code is bound to the request's content.
    Fixed(bool),
}

impl Confirmation<'_> {
    fn at(self, head: u64) -> Result<bool, ControlError> {
        match self {
            Self::AtHead(confirmed) => confirmed(head),
            Self::Fixed(confirmed) => Ok(confirmed),
        }
    }
}

/// Reads the control stream's head and derives the command's event id from `key`, whether it is
/// confirmed at that head, and the head, so the same choice confirmed the same way at the same
/// head is the same event across retries and invocations (DEC-290; the #397 review, minor 4).
///
/// # Errors
/// [`ControlError::Journal`] when the journal cannot be read; whatever `confirmed` answers.
pub(crate) fn decide(
    journal: &dyn ControlJournal,
    owner: &Owner,
    event_type: &'static str,
    key: Vec<(&'static str, Value)>,
    confirmed: Confirmation<'_>,
    repeat: Repeat,
) -> Result<Decision, ControlError> {
    let stream = control_stream(owner)?;
    let head = journal.head(&stream)?.seq;
    let chosen = object(key.clone())?;
    if repeat == Repeat::FindsEarlier
        && let Some(earlier) = earlier(journal, &stream, event_type, &chosen, confirmed, head)?
    {
        return Ok(Decision::Committed(earlier));
    }
    let stepped_up = confirmed.at(head)?;
    let event_id = derive(&stream, event_type, &chosen, stepped_up, head)?;
    Ok(Decision::Fresh(Decided {
        stream,
        event_type,
        key,
        stepped_up,
        event_id,
        repeat,
    }))
}

/// The same command, committed as the control stream's last event when it was decided at any
/// head before that event, and not yet recorded by the runtime that copies it. Any earlier head
/// counts, because an event that landed between the command's read of the head and its append
/// leaves the command last but decided further back. Only the last event is read, so a command
/// repeated after any other command is committed again; and a code that confirms at the head now
/// is a new gesture, never a re-run, whose code could confirm only at a head before its own event
/// (DEC-290 item 5).
///
/// # Errors
/// [`ControlError::Journal`] when the journal cannot be read.
fn earlier(
    journal: &dyn ControlJournal,
    stream: &StreamId,
    event_type: &str,
    chosen: &Value,
    confirmed: Confirmation<'_>,
    head: u64,
) -> Result<Option<Submitted>, ControlError> {
    if matches!(confirmed, Confirmation::AtHead(_)) && confirmed.at(head)? {
        return Ok(None);
    }
    let Some(last) = journal.rows(stream)?.pop() else {
        return Ok(None);
    };
    for before in (0..last.seq).rev() {
        if derive(stream, event_type, chosen, confirmed.at(before)?, before)? == last.event_id {
            if recorded(journal, stream, &last)? {
                return Ok(None);
            }
            return Ok(Some(Submitted {
                event_id: last.event_id,
                seq: last.seq,
            }));
        }
    }
    Ok(None)
}

/// Whether the runtime of the agent `row` addresses has recorded it: an event on that agent's
/// stream whose `causation_id` is `row`'s id, as every copy the runtime makes is (journal spec §2).
/// A command addressed to no agent has no agent stream to read, and reads as not recorded.
fn recorded(
    journal: &dyn ControlJournal,
    stream: &StreamId,
    row: &StoredEvent,
) -> Result<bool, ControlError> {
    let Some(agent) = envelope(row)?
        .get("payload")
        .and_then(|p| p.get("agent"))
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return Ok(false);
    };
    let workspace = stream
        .as_str()
        .strip_prefix("ctl:")
        .ok_or_else(|| ControlError::Journal("not a control stream".to_owned()))?;
    let agent_stream = StreamId::parse(&format!("agent:{workspace}:{agent}"))
        .ok_or_else(|| ControlError::Journal("not an agent id".to_owned()))?;
    Ok(envelopes(journal, &agent_stream)?
        .iter()
        .any(|e| e.get("causation_id").and_then(Value::as_str) == Some(row.event_id.as_str())))
}

/// The ULID-shaped event id of `chosen` decided at `head`: the first 128 bits of the SHA-256 of a
/// canonical object naming the stream, the event type, the choice, whether it is stepped up, and
/// the head, in ULID's 26 Crockford base-32 digits. The ULID's time component carries no meaning
/// (journal spec §3).
fn derive(
    stream: &StreamId,
    event_type: &str,
    chosen: &Value,
    stepped_up: bool,
    head: u64,
) -> Result<String, ControlError> {
    let bound = object(vec![
        ("control_head", text(&head.to_string())),
        ("event_type", text(event_type)),
        ("key", chosen.clone()),
        ("stepped_up", Value::Bool(stepped_up)),
        ("stream", text(stream.as_str())),
    ])?;
    let digest = Digest::of(&to_canonical(&bound));
    let mut high = [0_u8; 16];
    high.copy_from_slice(&digest.as_bytes()[..16]);
    Ok(ulid(u128::from_be_bytes(high)))
}

/// `n` in ULID's 26 Crockford base-32 digits, most significant first. The fallbacks are dead: a
/// digit's shift is at most 125 bits, which neither overflows `checked_mul` nor reaches 128 for
/// `checked_shr`, and a five-bit index always names one of the 32 letters; they stand in for the
/// panics the lint header forbids.
fn ulid(n: u128) -> String {
    const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    (0..ULID_LEN)
        .rev()
        .map(|digit| {
            let shifted = digit
                .checked_mul(5)
                .and_then(|bits| n.checked_shr(bits))
                .unwrap_or_default();
            let index = usize::try_from(shifted & 0x1f).unwrap_or_default();
            ALPHABET.get(index).copied().map_or('0', char::from)
        })
        .collect()
}

const ULID_LEN: u32 = 26;

/// The pause before retry `retry` (1 for the first retry) of the command whose id is `event_id`:
/// doubling from [`FIRST_BACKOFF`] to at most [`LAST_BACKOFF`], plus a jitter below that base drawn
/// from the id, so two invocations racing for the stream stop fencing each other (DEC-290; the
/// #397 review, minor 4). Public so its tests can judge it directly.
///
/// # Errors
/// None: every retry has a pause. The `Result` is the stub API's shape.
pub fn backoff(retry: u32, event_id: &str) -> Result<Duration, ControlError> {
    let doubled =
        FIRST_BACKOFF.saturating_mul(1_u32 << retry.saturating_sub(1).min(BACKOFF_DOUBLINGS));
    let base = doubled.min(LAST_BACKOFF);
    let drawn = Digest::of_parts(&[event_id.as_bytes(), &retry.to_be_bytes()]);
    let mut high = [0_u8; 8];
    high.copy_from_slice(&drawn.as_bytes()[..8]);
    let base_nanos = u64::try_from(base.as_nanos()).unwrap_or(u64::MAX).max(1);
    let jitter = Duration::from_nanos(
        u64::from_be_bytes(high)
            .checked_rem(base_nanos)
            .unwrap_or_default(),
    );
    Ok(base.saturating_add(jitter))
}

/// The doublings after which the base would pass [`LAST_BACKOFF`] anyway, so the shift never
/// overflows.
const BACKOFF_DOUBLINGS: u32 = 16;

/// What one attempt at an append found.
enum Attempted {
    /// The journal holds the draft, at this `seq`.
    Stored(u64),
    /// The stream's head was not the one the append expected.
    Moved,
    /// A fence, a lost answer, or an unavailable journal: the same append is tried again.
    Retry,
}

/// One attempt at appending `bytes`, the draft of `event_id`, at `pinned` when the caller decided
/// at that head, or at the head read now.
fn attempt(
    journal: &mut dyn ControlJournal,
    stream: &StreamId,
    (event_id, bytes): (&str, &[u8]),
    pinned: Option<u64>,
    now: Now,
) -> Result<Attempted, ControlError> {
    let epoch = journal.take_ownership(stream)?;
    let head = match pinned {
        Some(head) => head,
        None => journal.head(stream)?.seq,
    };
    match journal.append(stream, head, epoch, now.at, &[bytes])? {
        AppendOutcome::Committed(rows) | AppendOutcome::AlreadyCommitted(rows) => rows
            .iter()
            .find(|row| row.event_id == event_id)
            .map(|row| Attempted::Stored(row.seq))
            .ok_or_else(|| ControlError::Journal("the stored event is missing".into())),
        AppendOutcome::HeadMismatch { .. } => Ok(Attempted::Moved),
        AppendOutcome::Fenced { .. } | AppendOutcome::Ambiguous | AppendOutcome::Unavailable => {
            Ok(Attempted::Retry)
        }
        AppendOutcome::IdempotencyConflict { stored_seq } => Err(ControlError::Journal(format!(
            "another event is stored under this id at seq {stored_seq}"
        ))),
        AppendOutcome::Invalid { error, .. } => {
            Err(ControlError::Journal(format!("refused: {error:?}")))
        }
    }
}

const FIRST_BACKOFF: Duration = Duration::from_millis(100);
const LAST_BACKOFF: Duration = Duration::from_secs(2);

/// Commits exactly one control-stream event (DEC-155 items 2 and 5, DEC-257 item 16, DEC-290): the
/// owner's choice, the second the owner ran the command at under each of `timed`, and, when the
/// command is stepped up, fresh `cli_confirm` evidence, minted here so a command [`decide`] found
/// committed spends none. Every attempt takes a new writer epoch, reads the head afresh, and appends the same
/// bytes after a [`backoff`], so a fence, a moved head, or a lost answer is retried, and a retry of
/// an append that did commit answers `AlreadyCommitted` with the stored event rather than storing a
/// second.
///
/// # Errors
/// [`ControlError::Journal`] when the journal refuses the draft, reports a conflicting event under
/// the id, or keeps answering with a retry after [`ATTEMPTS`], naming the event id, which a re-run
/// finds only if nothing else has been committed since: the id is derived at the head the command
/// was decided at, and an event that landed between that read and the append moves the head a
/// re-run decides at (DEC-290 item 5).
pub(crate) fn commit(
    journal: &mut dyn ControlJournal,
    ids: &mut dyn Ids,
    owner: &Owner,
    decided: Decided,
    timed: &[&'static str],
    now: Now,
) -> Result<Submitted, ControlError> {
    let Decided {
        stream,
        event_type,
        mut key,
        stepped_up,
        event_id,
        repeat,
    } = decided;
    for name in timed {
        key.push((name, seconds(now.secs)?));
    }
    let evidence = if stepped_up {
        step_up(ids, now)?
    } else {
        Value::Null
    };
    key.push(("step_up", evidence));
    let payload = object(key)?;
    let mut referenced = BTreeSet::new();
    digest_refs(&payload, &mut referenced);
    let shape = Shape {
        schema_version: 1,
        artifact_refs: referenced.into_iter().collect(),
        config_refs: Vec::new(),
    };
    let bytes = draft(
        Writer::Owner,
        owner,
        &stream,
        &event_id,
        event_type,
        shape,
        payload,
        now,
    )?;
    settle(journal, &stream, event_id, repeat, &bytes, now)
}

/// A control-stream event the owner confirms that names another event as its cause: its type and
/// schema version, the cause's event id, and the payload's members other than the step-up evidence.
#[derive(Debug, Clone)]
pub(crate) struct Caused {
    /// The control stream's head the owner's command read the rows it checked at.
    pub(crate) head: u64,
    pub(crate) event_type: &'static str,
    pub(crate) schema_version: u64,
    pub(crate) causation_id: String,
    pub(crate) key: Vec<(&'static str, Value)>,
}

/// Commits `caused` with fresh `cli_confirm` evidence whose `authenticated_at` is the instant the
/// owner ran the command, a timestamp as journal spec §9.8 types it (DEC-802 item 8), appended at
/// `caused.head` and nowhere else: what the command checked against the stream's rows holds at the
/// append (#907 review). Its event id is derived from the key, the cause and that head (DEC-290), so
/// a retry commits nothing twice. `None` when the stream has moved past that head: the command
/// reads the rows again and decides again.
///
/// # Errors
/// As [`commit`].
pub(crate) fn commit_caused(
    journal: &mut dyn ControlJournal,
    ids: &mut dyn Ids,
    owner: &Owner,
    caused: Caused,
    now: Now,
) -> Result<Option<Submitted>, ControlError> {
    let Caused {
        head,
        event_type,
        schema_version,
        causation_id,
        mut key,
    } = caused;
    let stream = control_stream(owner)?;
    let mut bound = key.clone();
    bound.push(("causation_id", text(&causation_id)));
    let event_id = derive(&stream, event_type, &object(bound)?, true, head)?;
    let evidence = object(vec![
        ("assertion_id", text(&ids.assertion_id())),
        ("authenticated_at", text(&now.at.to_string())),
        ("method", text("cli_confirm")),
    ])?;
    key.push(("step_up", evidence));
    let shape = Shape {
        schema_version,
        artifact_refs: Vec::new(),
        config_refs: Vec::new(),
    };
    let named = (event_id.as_str(), Some(causation_id.as_str()));
    let payload = object(key)?;
    let bytes = draft_caused(
        Writer::Owner,
        owner,
        &stream,
        named,
        event_type,
        shape,
        payload,
        now,
    )?;
    let mut retries = 1..ATTEMPTS;
    loop {
        match attempt(journal, &stream, (&event_id, &bytes), Some(head), now)? {
            Attempted::Stored(seq) => return Ok(Some(Submitted { event_id, seq })),
            Attempted::Moved => return Ok(None),
            Attempted::Retry => {}
        }
        let Some(retry) = retries.next() else {
            return Err(unsettled(&event_id, Repeat::FindsEarlier));
        };
        journal.wait(backoff(retry, &event_id)?);
    }
}

/// Every digest reference `value` holds, at any depth: what the envelope's `artifact_refs` lists,
/// sorted and each once (journal spec §3), such as an answer's `content_hash` (DEC-533 item 6).
fn digest_refs(value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::Str(s) if ArtifactRef::parse(s).is_some() => {
            out.insert(s.clone());
        }
        Value::Array(items) => items.iter().for_each(|v| digest_refs(v, out)),
        Value::Object(members) => members.values().for_each(|v| digest_refs(v, out)),
        _ => {}
    }
}

/// [`commit`] for an event whose payload is exactly the owner's choice, with no second and no
/// step-up member, in `shape`: a configuration registration (journal spec §9.2).
///
/// # Errors
/// As [`commit`].
pub(crate) fn commit_choice(
    journal: &mut dyn ControlJournal,
    owner: &Owner,
    decided: Decided,
    shape: Shape,
    now: Now,
) -> Result<Submitted, ControlError> {
    let Decided {
        stream,
        event_type,
        key,
        event_id,
        repeat,
        ..
    } = decided;
    let payload = object(key)?;
    let bytes = draft(
        Writer::Owner,
        owner,
        &stream,
        &event_id,
        event_type,
        shape,
        payload,
        now,
    )?;
    settle(journal, &stream, event_id, repeat, &bytes, now)
}

/// Appends `bytes`, the draft of `event_id`, until the journal stores it or [`ATTEMPTS`] run out.
///
/// # Errors
/// As [`commit`].
fn settle(
    journal: &mut dyn ControlJournal,
    stream: &StreamId,
    event_id: String,
    repeat: Repeat,
    bytes: &[u8],
    now: Now,
) -> Result<Submitted, ControlError> {
    let mut retries = 1..ATTEMPTS;
    loop {
        if let Attempted::Stored(seq) = attempt(journal, stream, (&event_id, bytes), None, now)? {
            return Ok(Submitted { event_id, seq });
        }
        let Some(retry) = retries.next() else {
            return Err(unsettled(&event_id, repeat));
        };
        journal.wait(backoff(retry, &event_id)?);
    }
}

/// Why a command gave up after [`ATTEMPTS`], naming the event it may have committed.
fn unsettled(event_id: &str, repeat: Repeat) -> ControlError {
    let again = match repeat {
        Repeat::FindsEarlier => {
            "if nothing else has been committed since, running it again finds it rather than \
             committing it twice"
        }
        Repeat::AlwaysCommits => "running it again commits another",
    };
    ControlError::Journal(format!(
        "the control stream did not settle after {ATTEMPTS} attempts; the command may have been \
         committed once, as event {event_id}; {again}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The actor's build is a content reference: `sha256:` and 64 lowercase hex digits, which the
    /// envelope's `build` member admits (journal spec §3).
    #[test]
    fn the_build_is_a_digest_reference() {
        let build = build_ref();
        let hex = build.strip_prefix("sha256:").unwrap_or_default();
        assert_eq!(hex.len(), 64, "{build}");
        assert!(
            hex.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "{build}"
        );
    }

    /// The opener writes the stream's `StreamOpened` as the `control_services` system and nothing
    /// else; the owner writes as a `user` (#725 review).
    #[test]
    fn only_the_opening_is_written_as_the_system() -> Result<(), ControlError> {
        let owner = Owner {
            workspace: "ws1".to_owned(),
            user: "u1".to_owned(),
            environment: Environment::Paper,
        };
        let now = Now {
            at: UtcNanos::from_parts(1_790_000_000, 0)
                .map_err(|e| ControlError::Journal(format!("{e:?}")))?,
            secs: 1_790_000_000,
        };
        let stream = control_stream(&owner)?;
        let shape = || Shape {
            schema_version: 1,
            artifact_refs: Vec::new(),
            config_refs: Vec::new(),
        };
        let actor = |writer, event_type| -> Result<(String, String), ControlError> {
            let bytes = draft(
                writer,
                &owner,
                &stream,
                "01J8ZA00000000000000000001",
                event_type,
                shape(),
                Value::Object(Object::new()),
                now,
            )?;
            let body = parse(&bytes).map_err(|e| ControlError::Journal(format!("{e:?}")))?;
            let member = |name: &str| {
                let actor = body.get("actor").and_then(|a| a.get(name));
                actor.and_then(Value::as_str).unwrap_or_default().to_owned()
            };
            Ok((member("kind"), member("id")))
        };
        let pair = |kind: &str, id: &str| (kind.to_owned(), id.to_owned());
        assert_eq!(
            actor(Writer::Opener, "StreamOpened")?,
            pair("system", "control_services")
        );
        assert_eq!(
            actor(Writer::Owner, "ApprovalResponseSubmitted")?,
            pair("user", "u1")
        );
        for other in ["ConfigSnapshotRegistered", "ApprovalResponseSubmitted"] {
            assert!(
                matches!(actor(Writer::Opener, other), Err(ControlError::Journal(_))),
                "{other}"
            );
        }
        Ok(())
    }

    /// The references an envelope lists are every digest reference in the payload, at any depth,
    /// inside arrays as inside objects, sorted and each once; a lookalike is not one (journal spec
    /// §3, DEC-533 item 6).
    #[test]
    fn the_listed_references_are_every_digest_reference_at_any_depth() {
        let [a, b, c] = ["a", "b", "c"].map(|d| format!("sha256:{}", d.repeat(64)));
        let payload = parse(
            format!(
                r#"{{"top":"{c}","nested":{{"list":["{a}",{{"deep":"{b}"}},"{c}"]}},
                "upper":"sha256:{}","short":"sha256:abc","plain":"cli-0123","n":1}}"#,
                "A".repeat(64)
            )
            .as_bytes(),
        )
        .unwrap_or(Value::Null);
        let mut listed = BTreeSet::new();
        digest_refs(&payload, &mut listed);
        assert_eq!(listed.into_iter().collect::<Vec<_>>(), [a, b, c]);
    }
}
