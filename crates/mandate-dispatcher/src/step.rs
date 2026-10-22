//! The dispatcher's step over the journal, its outbox (notifications spec §5.1, NT-3, NT-8; E8-10
//! slice D2, DEC-701 item 3, DEC-704). A [`step`] reads the committed `OwnerAlertSent` events of
//! its subject streams and the `ApprovalRequested` events of their agent streams (slice D7a,
//! DEC-2501), journals one `NoticeIssued` per cause before any send, sends each notice
//! to each recipient's push channel and again on §5.3's schedule while it fails `retryable`
//! ([`crate::due`], slice D4, DEC-707), and journals each outcome, and each send abandoned at its
//! window's end, as `NoticeAttempted`. Every
//! append goes through a [`NoticeWriter`], which holds only an `ntf:` stream id, so the
//! dispatcher cannot name an agent, account, or control stream. An append that does not commit
//! stops the step before its next send. Every `NoticeAttempted` is journaled at schema version 3,
//! its `verdict` the outcome's own variant and never a reading of its reason (DEC-706), and its
//! `address_ref` `null`, since no web-push send is made until the per-address fan-out exists
//! (DEC-2504 item 5, DEC-2505).

use std::collections::BTreeSet;

use mandate_canon::{Int, Key, Object, Value, encode_ulid, parse, to_canonical};
use mandate_journal::{AppendOutcome, Environment, StoredEvent, StreamId, StreamType};
use mandate_notify::{
    AddressHandle, Alert, NoticeId, NoticeKind, Notification, NotifyError, Origin, Outcome,
    Provider, PushChannel, SecureRandom, notice_keys,
};
use mandate_time::UtcNanos;

use crate::due::{Pending, pending};
use crate::{DispatchError, Next, WebPushSwitch};

/// The `NoticeAttempted` schema version `append` admits (journal spec v0.51 §9.15, DEC-706,
/// DEC-2505): every attempt is journaled at version 3, its `verdict` and then its `address_ref`
/// last, so no writer leaves either out.
const ATTEMPTED_VERSION: u64 = 3;

/// The one handle the dispatcher appends through: a notice stream `ntf:{workspace_id}` and the
/// writer epoch the dispatcher took for it (journal spec §5.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoticeWriter {
    stream: StreamId,
    epoch: u64,
}

impl NoticeWriter {
    /// A writer for `stream` at `epoch`, and the only way to make one (DEC-704 item 1).
    ///
    /// # Errors
    /// [`DispatchError::NotANoticeStream`] for any stream that is not a notice stream.
    pub fn new(stream: StreamId, epoch: u64) -> Result<Self, DispatchError> {
        if stream.stream_type() == StreamType::Notice {
            Ok(Self { stream, epoch })
        } else {
            Err(DispatchError::NotANoticeStream)
        }
    }

    /// The notice stream this writer appends to.
    ///
    /// # Errors
    /// Never.
    pub fn stream(&self) -> Result<&StreamId, DispatchError> {
        Ok(&self.stream)
    }

    /// The writer epoch this writer appends at.
    ///
    /// # Errors
    /// Never.
    pub fn epoch(&self) -> Result<u64, DispatchError> {
        Ok(self.epoch)
    }
}

/// What the dispatcher reads and writes: the committed rows of a stream in `seq` order, and one
/// all-or-nothing append to a [`NoticeWriter`]'s stream at its epoch.
pub trait Journal {
    /// The committed rows of `stream` in `seq` order (DEC-2500 item 3).
    ///
    /// # Errors
    /// [`DispatchError::JournalUnavailable`] when the store cannot answer. A failed read is never
    /// an empty stream: read as one, it would re-open the notice stream and re-issue every cause.
    fn committed(&self, stream: &StreamId) -> Result<Vec<StoredEvent>, DispatchError>;
    fn append_notices(
        &mut self,
        writer: &NoticeWriter,
        expected_head: u64,
        recorded_at: UtcNanos,
        drafts: &[&[u8]],
    ) -> AppendOutcome;
}

/// One step's inputs besides the journal, the random source, and the provider.
#[derive(Debug, Clone, Copy)]
pub struct Config<'a> {
    pub writer: &'a NoticeWriter,
    /// The workspace's agent, account, and control streams, whose alerts, and the agent streams'
    /// approval requests, are causes (DEC-2501 item 1).
    pub subjects: &'a [StreamId],
    /// The push channels, by opaque recipient id, every notice goes to until roles exist (E9-2;
    /// spec §3.3, DEC-704 item 4).
    pub audience: &'a [(&'a str, PushChannel)],
    pub origin: &'a Origin,
    pub environment: Environment,
    /// This binary's digest, `sha256:<64 hex>`, for each draft's actor.
    pub build: &'a str,
}

/// A committed `OwnerAlertSent` or `ApprovalRequested` as the step reads it: its event id and
/// stream, its kind, its owner command (none for a request), and whether it is on a control stream
/// (DEC-705, DEC-2501).
struct Cause {
    event: String,
    stream: String,
    kind: String,
    command: Option<String>,
    control: bool,
}

impl Cause {
    /// The notice key: the owner command when the alert names one, its own event id otherwise
    /// (spec §3.4, DEC-725 item 7).
    fn key(&self) -> &str {
        self.command.as_deref().unwrap_or(&self.event)
    }
}

/// A `NoticeIssued` as the notice stream holds it.
pub(crate) struct Issued {
    pub(crate) notice: String,
    pub(crate) kind: String,
    cause: String,
    pub(crate) recipients: Vec<String>,
}

/// A `NoticeAttempted` as the notice stream holds it: the send it names (notice id, recipient,
/// and channel), its number, status, and verdict (`None` when `null` or, at version 1, absent),
/// and its record's own `event_time`.
pub(crate) struct Attempted {
    pub(crate) notice: String,
    pub(crate) recipient: String,
    pub(crate) channel: String,
    pub(crate) attempt: Option<u64>,
    pub(crate) status: String,
    pub(crate) verdict: Option<String>,
    pub(crate) event_time: String,
}

/// The notice stream as one read finds it: its head, its notices in `seq` order, and every
/// attempt in `seq` order.
#[derive(Default)]
pub(crate) struct Notices {
    head: u64,
    pub(crate) issued: Vec<Issued>,
    pub(crate) attempted: Vec<Attempted>,
}

/// One pass of the dispatcher at `now` (spec §5.1 steps 1 to 4, DEC-704 items 2 and 3, DEC-705).
/// Every new cause's `NoticeIssued` is one batch, opened by `StreamOpened` on an empty stream, that
/// commits before any send. Every recipient of the audience is listed on each notice, but no
/// web-push send is made or journaled: a version-3 web-push attempt names the one address it went
/// to, and the fan-out over a recipient's active addresses is not built yet (journal spec §9.15,
/// DEC-2504 items 4 and 5, DEC-2506's owed fan-out), so web push stays off whatever the process's
/// [`WebPushSwitch`] says. Then each send [`crate::due`] lists gets at most one record in this
/// step (DEC-707 item 5): a due attempt is made under the send's one key and its outcome journaled
/// as that attempt, before the next send; a send whose window has ended is journaled `abandoned`
/// with `retry_window_ended` and a `null` verdict, with no send; a waiting send gets nothing.
///
/// # Errors
/// [`DispatchError::Fenced`] when a newer dispatcher owns the notice stream and
/// [`DispatchError::NotCommitted`] for any other append that does not commit, each before the
/// next send; [`DispatchError::Notify`] when the random source fails; and
/// [`DispatchError::JournalUnavailable`] when a read fails, which ends the step before it issues,
/// sends, or appends anything more (DEC-2500 item 3).
pub fn step(
    config: &Config<'_>,
    journal: &mut dyn Journal,
    random: &mut dyn SecureRandom,
    provider: &mut dyn Provider,
    now: UtcNanos,
) -> Result<(), DispatchError> {
    let stream = config.writer.stream()?;
    let before = notices_of(&read(journal, stream)?);
    let alerts = causes(journal, config.subjects)?;
    let fresh = fresh(&alerts, &before.issued)?;
    if !fresh.is_empty() {
        let recipients: BTreeSet<&str> = config.audience.iter().map(|&(r, _)| r).collect();
        let listed: Vec<Value> = recipients.into_iter().map(text).collect();
        let mut drafts = Vec::new();
        if before.head == 0 {
            let workspace = stream.as_str().strip_prefix("ntf:").unwrap_or_default();
            let opened = object([
                ("stream_type", text("notice")),
                ("workspace_id", text(workspace)),
            ])?;
            drafts.push(draft(config, random, now, "StreamOpened", None, 1, opened)?);
        }
        for cause in fresh {
            let kind = kind(&cause.kind).ok_or(NotifyError::Unrepresentable { what: "kind" })?;
            let payload = object([
                ("notice", text(&NoticeId::mint(random)?.hex()?)),
                ("kind", text(&cause.kind)),
                ("class", text(kind.class()?.key()?)),
                ("cause", text(&cause.event)),
                ("cause_stream", text(&cause.stream)),
                ("recipients", Value::Array(listed.clone())),
            ])?;
            let caused = Some(cause.event.as_str());
            drafts.push(draft(
                config,
                random,
                now,
                "NoticeIssued",
                caused,
                1,
                payload,
            )?);
        }
        append(journal, config.writer, before.head, now, &drafts)?;
    }
    let rows = read(journal, stream)?;
    let mut head = notices_of(&rows).head;
    for pending in pending(&rows, &WebPushSwitch::Off.sends(config.audience)?, now)? {
        let due = &pending.due;
        let record = match &due.next {
            Next::Attempt { attempt, key } => {
                let notification = Notification {
                    notice: due.notice,
                    text: pending.text,
                };
                let address = AddressHandle {
                    recipient: due.recipient.clone(),
                    channel: due.channel,
                };
                let outcome = provider.send(config.origin, &notification, &address, key)?;
                Record::of(*attempt, outcome)?
            }
            Next::Abandon { attempt } => Record {
                attempt: *attempt,
                status: "abandoned",
                reason: Some("retry_window_ended"),
                message: None,
                verdict: None,
            },
            Next::Wait { .. } => continue,
        };
        let payload = record.payload(&pending)?;
        let attempted = draft(
            config,
            random,
            now,
            "NoticeAttempted",
            None,
            ATTEMPTED_VERSION,
            payload,
        )?;
        head = append(journal, config.writer, head, now, &[attempted])?;
    }
    Ok(())
}

/// One `NoticeAttempted` the step journals for a send: its attempt number, status, reason,
/// provider message id, and verdict.
struct Record {
    attempt: u32,
    status: &'static str,
    reason: Option<&'static str>,
    message: Option<String>,
    verdict: Option<&'static str>,
}

impl Record {
    /// Attempt `attempt`'s `outcome` (spec §5.5, DEC-704 item 2, DEC-706): `accepted` is
    /// `delivered` with the provider's message id and no verdict, and `retryable` and `permanent`
    /// are `failed` with the reason and the verdict of their own variant, never a reading of the
    /// reason, which rule 134 holds to §5.2 to §5.4.
    fn of(attempt: u32, outcome: Outcome) -> Result<Self, DispatchError> {
        let (status, reason, message, verdict) = match outcome {
            Outcome::Accepted {
                provider_message_id,
            } => ("delivered", None, Some(provider_message_id), None),
            Outcome::Retryable { reason } => {
                ("failed", Some(reason.key()?), None, Some("retryable"))
            }
            Outcome::Permanent { reason } => {
                ("failed", Some(reason.key()?), None, Some("permanent"))
            }
        };
        Ok(Self {
            attempt,
            status,
            reason,
            message,
            verdict,
        })
    }

    /// The record's payload for `pending`'s send.
    fn payload(&self, pending: &Pending) -> Result<Value, DispatchError> {
        let due = &pending.due;
        let attempt = Int::new(u64::from(self.attempt))
            .ok_or(NotifyError::Unrepresentable { what: "attempt" })?;
        object([
            ("notice", text(&due.notice.hex()?)),
            ("recipient", text(&pending.recipient)),
            ("channel", text(due.channel.key()?)),
            ("attempt", Value::Int(attempt)),
            ("status", text(self.status)),
            ("reason", self.reason.map_or(Value::Null, text)),
            (
                "provider_message_id",
                self.message.as_deref().map_or(Value::Null, text),
            ),
            ("coalesced_into", Value::Null),
            ("verdict", self.verdict.map_or(Value::Null, text)),
            ("address_ref", Value::Null),
        ])
    }
}

/// The committed causes of the subject streams, the streams in the order given and each in `seq`
/// order: every `OwnerAlertSent`, and every `ApprovalRequested` on an agent stream, read by its
/// envelope alone and never its payload (DEC-2501 items 1 and 5). An `ApprovalRequested` on any
/// other stream is no cause, so rule 125 never refuses the batch it would join. A batch the
/// journal refused, a fenced one included, has no row, so it is no cause (spec §5.1 step 1, NT-8).
/// A subject stream that cannot be read ends the step.
fn causes(journal: &dyn Journal, subjects: &[StreamId]) -> Result<Vec<Cause>, DispatchError> {
    let mut alerts = Vec::new();
    for stream in subjects {
        for row in read(journal, stream)? {
            if row.event_type == "ApprovalRequested" && stream.stream_type() == StreamType::Agent {
                alerts.push(Cause {
                    event: row.event_id,
                    stream: stream.as_str().to_owned(),
                    kind: "approval_requested".to_owned(),
                    command: None,
                    control: false,
                });
                continue;
            }
            let alert = payload(&row).filter(|_| row.event_type == "OwnerAlertSent");
            let Some(alert) = alert else { continue };
            alerts.push(Cause {
                event: row.event_id,
                stream: stream.as_str().to_owned(),
                kind: member(&alert, "kind").to_owned(),
                command: alert
                    .get("owner_command")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                control: stream.stream_type() == StreamType::Control,
            });
        }
    }
    Ok(alerts)
}

/// One read of `stream`'s committed rows; a read that fails ends the step with the store's closed
/// [`DispatchError::JournalUnavailable`], so nothing more is issued, sent, or appended, and the
/// failure is never read as an empty stream (DEC-2500 item 3).
fn read(journal: &dyn Journal, stream: &StreamId) -> Result<Vec<StoredEvent>, DispatchError> {
    journal.committed(stream)
}

/// One cause per notice key that has no `NoticeIssued` yet, in first-read order: the key's
/// control-stream alert when it has one, its first alert otherwise (DEC-704 item 2, DEC-705).
fn fresh<'a>(alerts: &'a [Cause], issued: &[Issued]) -> Result<Vec<&'a Cause>, DispatchError> {
    let read: Vec<Alert<&str>> = alerts
        .iter()
        .map(|a| Alert {
            event: a.event.as_str(),
            owner_command: a.command.as_deref(),
        })
        .collect();
    let mut fresh = Vec::new();
    for key in notice_keys(&read)? {
        let group: Vec<&Cause> = alerts.iter().filter(|a| a.key() == key).collect();
        if group
            .iter()
            .any(|a| issued.iter().any(|n| n.cause == a.event))
        {
            continue;
        }
        fresh.extend(group.iter().find(|a| a.control).or(group.first()).copied());
    }
    Ok(fresh)
}

/// The notice stream as its committed `rows` stand: its head, its `NoticeIssued`, and its
/// `NoticeAttempted`.
pub(crate) fn notices_of(rows: &[StoredEvent]) -> Notices {
    let mut read = Notices {
        head: rows.last().map_or(0, |r| r.seq),
        ..Notices::default()
    };
    for row in rows {
        let Some(body) = parse(&row.body).ok() else {
            continue;
        };
        let Some(payload) = body.get("payload") else {
            continue;
        };
        let at = |name: &str| member(payload, name).to_owned();
        match row.event_type.as_str() {
            "NoticeIssued" => read.issued.push(Issued {
                notice: at("notice"),
                kind: at("kind"),
                cause: at("cause"),
                recipients: payload
                    .get("recipients")
                    .and_then(Value::as_array)
                    .unwrap_or_default()
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect(),
            }),
            "NoticeAttempted" => read.attempted.push(Attempted {
                notice: at("notice"),
                recipient: at("recipient"),
                channel: at("channel"),
                attempt: payload.get("attempt").and_then(Value::as_int),
                status: at("status"),
                verdict: payload
                    .get("verdict")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                event_time: member(&body, "event_time").to_owned(),
            }),
            _ => {}
        }
    }
    read
}

/// Appends `drafts` through `writer` and returns the new head. Anything but `Committed` stops the
/// step before its next send (DEC-704 item 3).
fn append(
    journal: &mut dyn Journal,
    writer: &NoticeWriter,
    head: u64,
    now: UtcNanos,
    drafts: &[Vec<u8>],
) -> Result<u64, DispatchError> {
    let drafts: Vec<&[u8]> = drafts.iter().map(Vec::as_slice).collect();
    match journal.append_notices(writer, head, now, &drafts) {
        AppendOutcome::Committed(rows) => Ok(rows.last().map_or(head, |r| r.seq)),
        AppendOutcome::Fenced { current_epoch } => Err(DispatchError::Fenced { current_epoch }),
        refused => Err(DispatchError::NotCommitted {
            outcome: refused.name(),
        }),
    }
}

/// One notice-stream draft at `schema_version`, its event id the ULID text of its own draw from
/// `random` and its actor the dispatcher (DEC-704 items 5 and 6).
fn draft(
    config: &Config<'_>,
    random: &mut dyn SecureRandom,
    now: UtcNanos,
    event_type: &str,
    causation: Option<&str>,
    schema_version: u64,
    payload: Value,
) -> Result<Vec<u8>, DispatchError> {
    let mut bits = [0u8; 16];
    random.fill(&mut bits)?;
    let actor = object([
        ("kind", text("system")),
        ("id", text("dispatcher")),
        ("version", text(env!("CARGO_PKG_VERSION"))),
        ("build", text(config.build)),
    ])?;
    let one = Int::new(1).ok_or(NotifyError::Unrepresentable { what: "draft" })?;
    let version = Int::new(schema_version).ok_or(NotifyError::Unrepresentable {
        what: "schema_version",
    })?;
    let envelope = object([
        ("envelope_version", Value::Int(one)),
        ("environment", text(config.environment.as_str())),
        ("event_id", text(&encode_ulid(u128::from_be_bytes(bits)))),
        ("stream_id", text(config.writer.stream()?.as_str())),
        ("event_type", text(event_type)),
        ("schema_version", Value::Int(version)),
        ("event_time", text(&now.to_string())),
        ("clock_source", text("local")),
        ("causation_id", causation.map_or(Value::Null, text)),
        ("correlation_id", Value::Null),
        ("actor", actor),
        ("config_refs", Value::Object(Object::new())),
        ("payload", payload),
        ("artifact_refs", Value::Array(Vec::new())),
        ("pii_refs", Value::Array(Vec::new())),
    ])?;
    Ok(to_canonical(&envelope))
}

/// A canonical object of `members`, each name a fixed key.
fn object<const N: usize>(members: [(&str, Value); N]) -> Result<Value, DispatchError> {
    let mut object = Object::new();
    for (name, value) in members {
        let key = Key::new(name).map_err(|_| NotifyError::Unrepresentable { what: "draft" })?;
        object.insert(key, value);
    }
    Ok(Value::Object(object))
}

fn text(value: &str) -> Value {
    Value::Str(value.to_owned())
}

/// A committed row's payload.
fn payload(row: &StoredEvent) -> Option<Value> {
    parse(&row.body).ok()?.get("payload").cloned()
}

/// A payload's string member, or empty when it has none.
fn member<'a>(payload: &'a Value, name: &str) -> &'a str {
    payload
        .get(name)
        .and_then(Value::as_str)
        .unwrap_or_default()
}

/// The kind a record names, by its key.
pub(crate) fn kind(key: &str) -> Option<NoticeKind> {
    NoticeKind::ALL
        .into_iter()
        .find(|kind| kind.key() == Ok(key))
}

#[cfg(test)]
mod tests;
