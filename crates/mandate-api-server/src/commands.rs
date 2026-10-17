//! Pause and the kill switch as `OwnerCommandIssued` on the workspace's control stream
//! (§3.11 item 10, §4.2, §5.4, DEC-1007): the members journal spec §9.11 lists, as the CLI writes
//! them, so the runtime reads the API's commands exactly as it reads the CLI's.
//!
//! The event id is derived from the client's key (§3.4), so a retry, a lost answer, or a fence
//! commits nothing twice: each attempt takes a new writer epoch, reads the head, and appends the
//! same bytes, and a retry of an append that landed answers `AlreadyCommitted` (journal spec §5.1).

use std::time::Duration;

use mandate_authn::Request;
use mandate_canon::{Int, Key, Object, Value, parse, to_canonical};
use mandate_identity::Permission;
use mandate_journal::{AppendOutcome, StoredEvent};
use mandate_time::UtcNanos;

use crate::journal::Journal;
use crate::reads::deployed;
use crate::routes::BUILD;

/// How many appends a command makes before it answers that the journal did not settle.
pub const ATTEMPTS: usize = 5;

/// The pause after an attempt that did not settle.
pub const BACKOFF: Duration = Duration::from_millis(20);

/// What a kill switch stops (§5.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KillScope {
    Agent(String),
    Connection(String),
    Workspace,
}

/// One owner command the API records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Pause the agent.
    Pause(String),
    /// Engage the kill switch at a scope.
    KillSwitch(KillScope),
    /// Skip an approval (§5.2): its agent, its `ApprovalRequested` id, and the content hash the
    /// client rendered.
    Skip {
        agent: String,
        approval: String,
        content_hash: String,
    },
}

impl Command {
    /// §3.4's operation name, which is also the journaled `command`'s.
    pub fn operation(&self) -> &'static str {
        match self {
            Self::Pause(_) => "pause",
            Self::KillSwitch(_) => "kill_switch",
            Self::Skip { .. } => "approval_response",
        }
    }

    /// The matrix row that authorizes it (identity spec §4.2).
    pub fn permission(&self) -> Permission {
        match self {
            Self::Pause(_) => Permission::Pause,
            Self::KillSwitch(KillScope::Agent(_)) => Permission::KillSwitchAgent,
            Self::KillSwitch(_) => Permission::KillSwitchConnectionOrWorkspace,
            Self::Skip { .. } => Permission::Skip,
        }
    }

    /// What it is to the session (identity spec §6.4): pause and the kill switch survive an
    /// outage; a Skip is not one of a reduction-only session's rows.
    pub fn request(&self) -> Request {
        match self {
            Self::Pause(_) => Request::Pause,
            Self::KillSwitch(_) => Request::KillSwitch,
            Self::Skip { .. } => Request::Other,
        }
    }

    /// The member of the `202` that names the event: `command_id`, or a Skip's `response_id`.
    pub fn id_member(&self) -> &'static str {
        match self {
            Self::Skip { .. } => "response_id",
            Self::Pause(_) | Self::KillSwitch(_) => "command_id",
        }
    }

    /// The event it is journaled as.
    fn event_type(&self) -> &'static str {
        match self {
            Self::Skip { .. } => "ApprovalResponseSubmitted",
            Self::Pause(_) | Self::KillSwitch(_) => "OwnerCommandIssued",
        }
    }

    /// The digest references its payload names, which the envelope's `artifact_refs` lists
    /// (journal spec §3): a Skip's content hash, and none for an owner command.
    fn artifact_refs(&self) -> Vec<Value> {
        match self {
            Self::Skip { content_hash, .. } => vec![text(content_hash)],
            Self::Pause(_) | Self::KillSwitch(_) => Vec::new(),
        }
    }

    /// Its payload: journal spec §9.11's members for an owner command, §9.7's for a Skip, as the
    /// CLI writes each.
    fn payload(&self, workspace: &str, principal: &str, second: Int) -> Option<Value> {
        if let Self::Skip {
            agent,
            approval,
            content_hash,
        } = self
        {
            return object(vec![
                ("agent", text(agent)),
                ("approval", text(approval)),
                ("verdict", text("skipped")),
                ("content_hash", text(content_hash)),
                ("submitted_at", Value::Int(second)),
                ("step_up", Value::Null),
                ("responder", text(principal)),
                ("role", text("approver")),
            ]);
        }
        let (agent, scope, subject) = self.named(workspace);
        object(vec![
            ("agent", agent.map_or(Value::Null, text)),
            ("command", text(self.operation())),
            ("scope", text(scope)),
            ("subject", text(subject)),
            ("release", Value::Null),
            ("warning_shown", Value::Null),
            ("bid", Value::Null),
            ("bid_size", Value::Null),
            ("floor", Value::Null),
            ("user", text(principal)),
            ("submitted_at", Value::Int(second)),
            ("step_up", Value::Null),
        ])
    }

    /// The payload's `agent`, `scope`, and `subject` (journal spec §9.11, rule 91): the agent for
    /// an agent's command, and for a kill switch the scope it stops.
    fn named<'a>(&'a self, workspace: &'a str) -> (Option<&'a str>, &'static str, &'a str) {
        match self {
            Self::Pause(agent) | Self::KillSwitch(KillScope::Agent(agent)) => {
                (Some(agent), "agent", agent)
            }
            Self::KillSwitch(KillScope::Connection(connection)) => (None, "connection", connection),
            Self::KillSwitch(KillScope::Workspace) => (None, "workspace", workspace),
            Self::Skip { agent, .. } => (Some(agent), "agent", agent),
        }
    }
}

/// An object of `members`, each name a valid key.
fn object(members: Vec<(&str, Value)>) -> Option<Value> {
    let mut object = Object::new();
    for (name, value) in members {
        object.insert(Key::new(name).ok()?, value);
    }
    Some(Value::Object(object))
}

fn text(value: &str) -> Value {
    Value::Str(value.to_owned())
}

/// Who issued a command, where, and when.
#[derive(Debug, Clone, Copy)]
pub struct Issued<'a> {
    /// The workspace's ULID.
    pub workspace: &'a str,
    /// The principal's ULID, the actor and the payload's `user`.
    pub principal: &'a str,
    /// The event id §3.4 derives.
    pub event_id: &'a str,
    /// The control stream's environment.
    pub environment: &'a str,
    /// The instant of receipt.
    pub now: UtcNanos,
    pub command: &'a Command,
}

/// The canonical draft of `issued`'s event; `None` only for a value the canonical form cannot
/// hold.
pub fn draft(issued: &Issued<'_>) -> Option<Vec<u8>> {
    let second = u64::try_from(issued.now.secs()).ok().and_then(Int::new)?;
    let payload = issued
        .command
        .payload(issued.workspace, issued.principal, second)?;
    let event = Event {
        event_type: issued.command.event_type(),
        payload,
        artifact_refs: issued.command.artifact_refs(),
        config_refs: Vec::new(),
    };
    envelope(&Writer::of(issued), &event)
}

/// Who writes an event, where, under which id, and when.
#[derive(Debug, Clone, Copy)]
pub struct Writer<'a> {
    pub workspace: &'a str,
    pub principal: &'a str,
    pub event_id: &'a str,
    pub environment: &'a str,
    pub now: UtcNanos,
}

impl<'a> Writer<'a> {
    fn of(issued: &Issued<'a>) -> Self {
        Self {
            workspace: issued.workspace,
            principal: issued.principal,
            event_id: issued.event_id,
            environment: issued.environment,
            now: issued.now,
        }
    }
}

/// One control-stream event's own members: its type, payload, the digest references the payload
/// names (journal spec §3), and its config references.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub event_type: &'static str,
    pub payload: Value,
    pub artifact_refs: Vec<Value>,
    pub config_refs: Vec<(&'static str, Value)>,
}

/// The canonical draft of `event`, written by the user `writer` names on the workspace's control
/// stream; `None` only for a value the canonical form cannot hold.
pub fn envelope(writer: &Writer<'_>, event: &Event) -> Option<Vec<u8>> {
    let actor = object(vec![
        ("build", text(BUILD)),
        ("id", text(writer.principal)),
        ("kind", text("user")),
        ("version", text(env!("CARGO_PKG_VERSION"))),
    ])?;
    let one = Value::Int(Int::new(1)?);
    let envelope = object(vec![
        ("actor", actor),
        ("artifact_refs", Value::Array(event.artifact_refs.clone())),
        ("causation_id", Value::Null),
        ("clock_source", text("local")),
        ("config_refs", object(event.config_refs.clone())?),
        ("correlation_id", Value::Null),
        ("environment", text(writer.environment)),
        ("envelope_version", one.clone()),
        ("event_id", text(writer.event_id)),
        ("event_time", text(&writer.now.to_string())),
        ("event_type", text(event.event_type)),
        ("payload", event.payload.clone()),
        ("pii_refs", Value::Array(Vec::new())),
        ("schema_version", one),
        ("stream_id", text(&format!("ctl:{}", writer.workspace))),
    ])?;
    Some(to_canonical(&envelope))
}

/// What a repeat must match: the event type, the stream, the actor's kind and id, and the payload
/// without `submitted_at`.
fn journaled(body: &[u8]) -> Option<[Value; 5]> {
    let event = parse(body).ok()?;
    let member = |path: &[&str]| path.iter().try_fold(&event, |v, k| v.get(k)).cloned();
    let Value::Object(mut payload) = member(&["payload"])? else {
        return None;
    };
    payload.remove("submitted_at");
    Some([
        member(&["event_type"])?,
        member(&["stream_id"])?,
        member(&["actor", "kind"])?,
        member(&["actor", "id"])?,
        Value::Object(payload),
    ])
}

/// Whether `stored` records the command `draft` does: the same event type, stream, actor, and
/// payload, but the second it was submitted at (§3.4 step 1, DEC-1007 item 4).
pub fn same(stored: &StoredEvent, draft: &[u8]) -> bool {
    match (journaled(&stored.body), journaled(draft)) {
        (Some(stored), Some(draft)) => stored == draft,
        _ => false,
    }
}

/// Whether the control stream holds `command`'s scope (DEC-1007 item 5): an agent an
/// `AgentDeployed` names, a connection an event names, or, for the workspace, any row at all.
pub fn in_workspace(control: &[StoredEvent], command: &Command) -> bool {
    let agent = |id: &str| {
        deployed(control)
            .iter()
            .any(|deployment| deployment.0 == id)
    };
    let connection = |id: &str| {
        control.iter().any(|row| {
            parse(&row.body).ok().is_some_and(|event| {
                event
                    .get("payload")
                    .and_then(|payload| payload.get("connection_id"))
                    .and_then(Value::as_str)
                    == Some(id)
            })
        })
    };
    match command {
        Command::Pause(id) | Command::KillSwitch(KillScope::Agent(id)) => agent(id),
        Command::Skip { agent: id, .. } => agent(id),
        Command::KillSwitch(KillScope::Connection(id)) => connection(id),
        Command::KillSwitch(KillScope::Workspace) => !control.is_empty(),
    }
}

/// How an append ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Settled {
    /// The event is stored: committed now or before.
    Stored(Box<StoredEvent>),
    /// Another event of other content is stored under the id.
    Conflict,
    /// An attempt's commit was ambiguous and none since confirmed it.
    Unknown,
    /// Nothing was stored.
    Unavailable,
}

/// Appends `draft`, whose id is `event_id`, to `stream`, in up to [`ATTEMPTS`] attempts; a draft
/// the journal refuses ends them, since the same bytes are refused again (DEC-1007 item 6).
pub async fn settle(
    journal: &dyn Journal,
    stream: &str,
    event_id: &str,
    draft: &[u8],
    now: UtcNanos,
) -> Settled {
    let mut ambiguous = false;
    for _ in 0..ATTEMPTS {
        match attempt(journal, stream, draft, now).await {
            Some(AppendOutcome::Committed(rows) | AppendOutcome::AlreadyCommitted(rows)) => {
                match rows.into_iter().find(|row| row.event_id == event_id) {
                    Some(row) => return Settled::Stored(Box::new(row)),
                    None => ambiguous = true,
                }
            }
            Some(AppendOutcome::IdempotencyConflict { .. }) => return Settled::Conflict,
            Some(AppendOutcome::Invalid { .. }) => break,
            Some(AppendOutcome::Ambiguous) => ambiguous = true,
            Some(_) | None => {}
        }
        tokio::time::sleep(BACKOFF).await;
    }
    if ambiguous {
        Settled::Unknown
    } else {
        Settled::Unavailable
    }
}

/// One attempt: a new writer epoch, the head now, and the append; `None` when the journal did not
/// answer.
async fn attempt(
    journal: &dyn Journal,
    stream: &str,
    draft: &[u8],
    now: UtcNanos,
) -> Option<AppendOutcome> {
    let epoch = journal.take_ownership(stream).await.ok()?;
    let head = journal.head(stream).await.ok()?;
    journal.append(stream, head, epoch, now, draft).await.ok()
}
