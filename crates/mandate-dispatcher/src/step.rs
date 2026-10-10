//! The dispatcher's step over the journal, its outbox (notifications spec §5.1, NT-3, NT-8; E8-10
//! slice D2, DEC-701 item 3, DEC-704). A [`step`] reads the committed `OwnerAlertSent` events of
//! its subject streams, journals one `NoticeIssued` per cause before any send, sends each notice
//! once to each recipient's push channel, and journals each outcome as `NoticeAttempted`. Every
//! append goes through a [`NoticeWriter`], which holds only an `ntf:` stream id, so the
//! dispatcher cannot name an agent, account, or control stream. An append that does not commit
//! stops the step before its next send.

use std::collections::BTreeSet;

use mandate_canon::{Int, Key, Object, Value, encode_ulid, parse, to_canonical};
use mandate_journal::{AppendOutcome, Environment, StoredEvent, StreamId, StreamType};
use mandate_notify::{
    AddressHandle, Alert, IdempotencyKey, NoticeId, NoticeKind, Notification, NotifyError, Origin,
    Outcome, Provider, PushChannel, Recipient, SecureRandom, notice_keys,
};
use mandate_time::UtcNanos;

use crate::DispatchError;

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
    fn committed(&self, stream: &StreamId) -> Vec<StoredEvent>;
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
    /// The workspace's agent, account, and control streams, whose alerts are causes.
    pub subjects: &'a [StreamId],
    /// The push channels, by opaque recipient id, every notice goes to until roles exist (E9-2;
    /// spec §3.3, DEC-704 item 4).
    pub audience: &'a [(&'a str, PushChannel)],
    pub origin: &'a Origin,
    pub environment: Environment,
    /// This binary's digest, `sha256:<64 hex>`, for each draft's actor.
    pub build: &'a str,
}

/// A committed `OwnerAlertSent` as the step reads it: its event id and stream, its kind, its
/// owner command, and whether it is on a control stream (DEC-705).
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
struct Issued {
    notice: String,
    kind: String,
    cause: String,
    recipients: Vec<String>,
}

/// One send as its `NoticeAttempted` names it: notice id, recipient, and channel.
type Send = (String, String, String);

/// The notice stream as one read finds it: its head, its notices in `seq` order, and every send
/// already attempted.
#[derive(Default)]
struct Notices {
    head: u64,
    issued: Vec<Issued>,
    attempted: BTreeSet<Send>,
}

/// One pass of the dispatcher at `now` (spec §5.1 steps 1 to 4, DEC-704 items 2 and 3, DEC-705).
/// Every new cause's `NoticeIssued` is one batch, opened by `StreamOpened` on an empty stream, that
/// commits before any send; then each send not yet attempted is made once, and its outcome is
/// journaled before the next send.
///
/// # Errors
/// [`DispatchError::Fenced`] when a newer dispatcher owns the notice stream and
/// [`DispatchError::NotCommitted`] for any other append that does not commit, each before the
/// next send; [`DispatchError::Notify`] when the random source fails.
pub fn step(
    config: &Config<'_>,
    journal: &mut dyn Journal,
    random: &mut dyn SecureRandom,
    provider: &mut dyn Provider,
    now: UtcNanos,
) -> Result<(), DispatchError> {
    let stream = config.writer.stream()?;
    let before = notices(journal, stream);
    let alerts = causes(journal, config.subjects);
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
            drafts.push(draft(config, random, now, "StreamOpened", None, opened)?);
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
            drafts.push(draft(config, random, now, "NoticeIssued", caused, payload)?);
        }
        append(journal, config.writer, before.head, now, &drafts)?;
    }
    let notices = notices(journal, stream);
    let audience: BTreeSet<(&str, PushChannel)> = config.audience.iter().copied().collect();
    let mut head = notices.head;
    for issued in &notices.issued {
        let text_key = match kind(&issued.kind) {
            Some(kind) => kind.text_key()?,
            None => None,
        };
        let Some(text_key) = text_key else { continue };
        let notice = NoticeId::parse(&issued.notice)?;
        let notification = Notification {
            notice,
            text: text_key,
        };
        for recipient in &issued.recipients {
            for &(_, channel) in audience.iter().filter(|(r, _)| r == recipient) {
                let send = (
                    issued.notice.clone(),
                    recipient.clone(),
                    channel.key()?.to_owned(),
                );
                if notices.attempted.contains(&send) {
                    continue;
                }
                let address = AddressHandle {
                    recipient: Recipient::parse(recipient)?,
                    channel,
                };
                let key = IdempotencyKey::of(&notice, &address)?;
                let outcome = provider.send(config.origin, &notification, &address, &key)?;
                let payload = attempt(&send, &outcome)?;
                let attempted = draft(config, random, now, "NoticeAttempted", None, payload)?;
                head = append(journal, config.writer, head, now, &[attempted])?;
            }
        }
    }
    Ok(())
}

/// The `NoticeAttempted` payload of a first attempt's `outcome` (spec §5.5, DEC-704 item 2):
/// `accepted` is `delivered` with the provider's message id, and `retryable` and `permanent` are
/// `failed` with the reason.
fn attempt(send: &Send, outcome: &Outcome) -> Result<Value, DispatchError> {
    let (status, reason, message) = match outcome {
        Outcome::Accepted {
            provider_message_id,
        } => ("delivered", None, Some(provider_message_id.as_str())),
        Outcome::Retryable { reason } | Outcome::Permanent { reason } => {
            ("failed", Some(reason.key()?), None)
        }
    };
    let (notice, recipient, channel) = send;
    let first = Int::new(1).ok_or(NotifyError::Unrepresentable { what: "attempt" })?;
    object([
        ("notice", text(notice)),
        ("recipient", text(recipient)),
        ("channel", text(channel)),
        ("attempt", Value::Int(first)),
        ("status", text(status)),
        ("reason", reason.map_or(Value::Null, text)),
        ("provider_message_id", message.map_or(Value::Null, text)),
        ("coalesced_into", Value::Null),
    ])
}

/// The committed `OwnerAlertSent` of the subject streams, the streams in the order given and each
/// in `seq` order. A batch the journal refused, a fenced one included, has no row, so it is no
/// cause (spec §5.1 step 1, NT-8).
fn causes(journal: &dyn Journal, subjects: &[StreamId]) -> Vec<Cause> {
    let mut alerts = Vec::new();
    for stream in subjects {
        for row in journal.committed(stream) {
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
    alerts
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

/// The notice stream as it stands: its head, its `NoticeIssued`, and its `NoticeAttempted`.
fn notices(journal: &dyn Journal, stream: &StreamId) -> Notices {
    let rows = journal.committed(stream);
    let mut read = Notices {
        head: rows.last().map_or(0, |r| r.seq),
        ..Notices::default()
    };
    for row in &rows {
        let Some(payload) = payload(row) else {
            continue;
        };
        let at = |name: &str| member(&payload, name).to_owned();
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
            "NoticeAttempted" => {
                read.attempted
                    .insert((at("notice"), at("recipient"), at("channel")));
            }
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

/// One notice-stream draft, its event id the ULID text of its own draw from `random` and its
/// actor the dispatcher (DEC-704 items 5 and 6).
fn draft(
    config: &Config<'_>,
    random: &mut dyn SecureRandom,
    now: UtcNanos,
    event_type: &str,
    causation: Option<&str>,
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
    let envelope = object([
        ("envelope_version", Value::Int(one)),
        ("environment", text(config.environment.as_str())),
        ("event_id", text(&encode_ulid(u128::from_be_bytes(bits)))),
        ("stream_id", text(config.writer.stream()?.as_str())),
        ("event_type", text(event_type)),
        ("schema_version", Value::Int(one)),
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
fn kind(key: &str) -> Option<NoticeKind> {
    NoticeKind::ALL
        .into_iter()
        .find(|kind| kind.key() == Ok(key))
}

#[cfg(test)]
mod tests;
