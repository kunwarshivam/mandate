//! `mandate agent status`, `pause`, `resume`, `stop`, `kill`, `exit`, and `acknowledge` (the M7 brief's "The CLI
//! control surface"; mandate spec §6.1; DEC-155 item 4, DEC-158 option (c), DEC-257 item 5).
//!
//! Every command commits exactly one control-stream event: `OwnerCommandIssued`, or
//! `OwnerAcknowledged` for an acknowledgment. Pause
//! needs no code (PX-4). Resume, Stop, and acknowledge need the code [`code`] prints, and a wrong
//! one is refused
//! locally with nothing committed, so pause stays the brake. A kill switch and an owner exit are
//! never refused here: with a wrong or missing code they are committed without step-up evidence,
//! and the runtime still stops and flattens, or routes the exit for the regular session, without
//! the owner-exit privilege (rule 13). Each code is derived on the owner's host from what it
//! confirms and the control stream's head, with no network, runtime, or model state.

use mandate_approval::KillScope;
use mandate_canon::Value;

use crate::approvals;
use crate::control::{
    ControlError, ControlJournal, Ids, Now, Owner, Submitted, agent_stream, code_of, commit,
    control_stream, envelopes, object, seconds, step_up, text,
};

/// An owner command addressed to one agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Pause,
    Resume,
    Stop,
    /// The owner's exit of one instrument, with the displayed bid, bid size, and floor the owner
    /// confirmed, when they did (DEC-58, DEC-66).
    Exit {
        instrument: String,
        bid: Option<Confirmed>,
    },
    /// The owner's acknowledgment of an account-stream event (mandate spec §5.8, trading spec
    /// §11), committed as `OwnerAcknowledged`, which the executor copies.
    Acknowledge {
        event: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirmed {
    pub bid: String,
    pub bid_size: String,
    pub floor: String,
}

/// What a kill switch reaches, as the owner typed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    Agent(String),
    Connection(String),
    Workspace,
}

/// The code the owner re-types for `command`, bound to the agent, the command, and the control
/// stream's head now; `None` for pause, which needs none.
///
/// # Errors
/// [`ControlError::Journal`] when the journal cannot be read.
pub fn code(
    journal: &dyn ControlJournal,
    owner: &Owner,
    agent: &str,
    command: &Command,
) -> Result<Option<String>, ControlError> {
    let head = journal.head(&control_stream(owner)?)?.seq;
    let bid = |field: fn(&Confirmed) -> &String, bid: &Option<Confirmed>| {
        bid.as_ref().map_or(Value::Null, |b| text(field(b)))
    };
    let (name, instrument, confirmed, event) = match command {
        Command::Pause => return Ok(None),
        Command::Resume => ("resume", None, &None, None),
        Command::Stop => ("stop", None, &None, None),
        Command::Exit { instrument, bid } => ("owner_exit", Some(instrument), bid, None),
        Command::Acknowledge { event } => ("acknowledge", None, &None, Some(event)),
    };
    let bound = object(vec![
        ("confirms", text(name)),
        ("agent", text(agent)),
        ("control_head", text(&head.to_string())),
        ("instrument", instrument.map_or(Value::Null, |i| text(i))),
        ("bid", bid(|b| &b.bid, confirmed)),
        ("bid_size", bid(|b| &b.bid_size, confirmed)),
        ("floor", bid(|b| &b.floor, confirmed)),
        ("event", event.map_or(Value::Null, |e| text(e))),
    ])?;
    Ok(Some(code_of(&bound)))
}

/// The code for a kill switch of `scope`: `mandate_approval::kill_switch_code` of the scope and the
/// control stream's head.
///
/// # Errors
/// [`ControlError::Journal`] when the journal cannot be read.
pub fn kill_code(
    journal: &dyn ControlJournal,
    owner: &Owner,
    scope: &Scope,
) -> Result<String, ControlError> {
    let head = journal.head(&control_stream(owner)?)?.seq;
    let scope = match scope {
        Scope::Agent(agent) => KillScope::Agent(agent.clone()),
        Scope::Connection(connection) => KillScope::Connection(connection.clone()),
        Scope::Workspace => KillScope::Workspace,
    };
    mandate_approval::kill_switch_code(&scope, head)
        .map(|code| code.0)
        .map_err(|e| ControlError::Journal(e.to_string()))
}

/// Commits `command` for `agent`.
///
/// # Errors
/// [`ControlError::Refused`] with `step_up_missing` for a resume, Stop, or acknowledgment without
/// the right code,
/// committing nothing; [`ControlError::Journal`] when the append fails.
pub fn command(
    journal: &mut dyn ControlJournal,
    ids: &mut dyn Ids,
    owner: &Owner,
    agent: &str,
    command: &Command,
    typed: Option<&str>,
    now: Now,
) -> Result<Submitted, ControlError> {
    let right = code(journal, owner, agent, command)?;
    let confirmed = matches!((&right, typed), (Some(right), Some(typed)) if right == typed);
    let needs_code = matches!(
        command,
        Command::Resume | Command::Stop | Command::Acknowledge { .. }
    );
    if needs_code && !confirmed {
        return Err(ControlError::Refused {
            reason: "step_up_missing",
        });
    }
    let evidence = if confirmed {
        step_up(ids, now)?
    } else {
        Value::Null
    };
    if let Command::Acknowledge { event } = command {
        let payload = object(vec![
            ("agent", text(agent)),
            ("event", text(event)),
            ("risk_clock", seconds(now.secs)?),
            ("submitted_at", seconds(now.secs)?),
            ("step_up", evidence),
            ("user", text(&owner.user)),
        ])?;
        return commit(journal, ids, owner, "OwnerAcknowledged", payload, now);
    }
    let (name, scope, subject, bid) = match command {
        Command::Pause => ("pause", "agent", agent, None),
        Command::Resume => ("resume", "agent", agent, None),
        Command::Stop => ("stop", "agent", agent, None),
        Command::Exit { instrument, bid } => (
            "owner_exit",
            "instrument",
            instrument.as_str(),
            bid.as_ref(),
        ),
        Command::Acknowledge { .. } => ("acknowledge", "agent", agent, None),
    };
    let payload = issued(
        owner,
        Some(agent),
        name,
        (scope, subject),
        bid,
        evidence,
        now,
    )?;
    commit(journal, ids, owner, "OwnerCommandIssued", payload, now)
}

/// Commits a kill switch of `scope`, with step-up evidence only when `typed` is the scope's code.
///
/// # Errors
/// [`ControlError::Journal`] when the append fails; never a refusal (DEC-158 option (c)).
pub fn kill(
    journal: &mut dyn ControlJournal,
    ids: &mut dyn Ids,
    owner: &Owner,
    scope: &Scope,
    typed: Option<&str>,
    now: Now,
) -> Result<Submitted, ControlError> {
    let confirmed = typed
        .is_some_and(|typed| kill_code(journal, owner, scope).is_ok_and(|right| right == typed));
    let evidence = if confirmed {
        step_up(ids, now)?
    } else {
        Value::Null
    };
    let (agent, kind, subject) = match scope {
        Scope::Agent(agent) => (Some(agent.as_str()), "agent", agent.as_str()),
        Scope::Connection(connection) => (None, "connection", connection.as_str()),
        Scope::Workspace => (None, "workspace", owner.workspace.as_str()),
    };
    let payload = issued(
        owner,
        agent,
        "kill_switch",
        (kind, subject),
        None,
        evidence,
        now,
    )?;
    commit(journal, ids, owner, "OwnerCommandIssued", payload, now)
}

/// What `status` shows, from the agent stream alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// The effective mode the last `AgentModeChanged` recorded: `normal` before any.
    pub mode: String,
    /// Whether that change was the startup hold (`awaiting_reconciliation`).
    pub startup_hold: bool,
    pub pending_approvals: usize,
}

/// # Errors
/// [`ControlError::Journal`] when the journal cannot be read.
pub fn status(
    journal: &dyn ControlJournal,
    owner: &Owner,
    agent: &str,
) -> Result<Status, ControlError> {
    let mut mode = "normal".to_owned();
    let mut startup_hold = false;
    for event in envelopes(journal, &agent_stream(owner, agent)?)? {
        if event.get("event_type").and_then(Value::as_str) == Some("AgentModeChanged") {
            let payload = event.get("payload");
            let field = |k: &str| payload.and_then(|p| p.get(k)).and_then(Value::as_str);
            mode = field("to").unwrap_or("normal").to_owned();
            startup_hold = field("reason") == Some("awaiting_reconciliation");
        }
    }
    let pending_approvals = approvals::pending_count(journal, owner, agent)?;
    Ok(Status {
        mode,
        startup_hold,
        pending_approvals,
    })
}

/// One `OwnerCommandIssued` payload (DEC-257 item 5): the agent, or `null` for a kill switch of a
/// connection or a workspace; the scope and its subject; the confirmed bid, bid size and floor or
/// `null`; when the owner ran it; the step-up evidence or `null`; and the owner's opaque id.
fn issued(
    owner: &Owner,
    agent: Option<&str>,
    command: &str,
    (scope, subject): (&str, &str),
    bid: Option<&Confirmed>,
    evidence: Value,
    now: Now,
) -> Result<Value, ControlError> {
    let given = |field: fn(&Confirmed) -> &String| bid.map_or(Value::Null, |b| text(field(b)));
    object(vec![
        ("agent", agent.map_or(Value::Null, text)),
        ("command", text(command)),
        ("scope", text(scope)),
        ("subject", text(subject)),
        ("bid", given(|b| &b.bid)),
        ("bid_size", given(|b| &b.bid_size)),
        ("floor", given(|b| &b.floor)),
        ("submitted_at", seconds(now.secs)?),
        ("step_up", evidence),
        ("user", text(&owner.user)),
    ])
}
