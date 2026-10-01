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
//! confirms and the control stream's head, with no network, runtime, or model state. A Stop that
//! releases the agent's positions to the owner records the choice and the warning shown (DEC-136).

use mandate_approval::KillScope;
use mandate_canon::Value;

use crate::approvals;
use crate::control::{
    Confirmation, ControlError, ControlJournal, Decision, Ids, Now, Owner, Submitted,
    account_stream, agent_stream, code_of, commit, control_stream, decide, envelopes, object, text,
};

/// An owner command addressed to one agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Pause,
    Resume,
    /// The owner's Stop (DEC-136): with `release`, the agent's positions become the owner's and
    /// unprotected, and the command records that [`RELEASE_WARNING`] was shown. Whether the agent
    /// must be flat without it is the runtime's precondition, not the CLI's.
    Stop {
        release: bool,
    },
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

/// The warning `mandate agent stop --release` shows before the owner types the code (DEC-136,
/// PX-13 (a)). The command records the content reference of exactly this text as
/// `warning_shown`, so the journal says which warning the owner saw.
pub const RELEASE_WARNING: &str = "The positions become yours and unprotected: this agent's \
     protective orders are canceled and nothing watches them. This warning is recorded with your \
     confirmation.";

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
    code_at(agent, command, head)
}

/// The code for `command` at the control stream's head `head` (DEC-279 item 1).
fn code_at(agent: &str, command: &Command, head: u64) -> Result<Option<String>, ControlError> {
    let bid = |field: fn(&Confirmed) -> &String, bid: &Option<Confirmed>| {
        bid.as_ref().map_or(Value::Null, |b| text(field(b)))
    };
    let (name, instrument, confirmed, event) = match command {
        Command::Pause => return Ok(None),
        Command::Resume => ("resume", None, &None, None),
        Command::Stop { release: false } => ("stop", None, &None, None),
        Command::Stop { release: true } => {
            return Err(ControlError::Unimplemented { story: "E8-3" });
        }
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
    kill_code_at(scope, head)
}

fn kill_code_at(scope: &Scope, head: u64) -> Result<String, ControlError> {
    let scope = match scope {
        Scope::Agent(agent) => KillScope::Agent(agent.clone()),
        Scope::Connection(connection) => KillScope::Connection(connection.clone()),
        Scope::Workspace => KillScope::Workspace,
    };
    mandate_approval::kill_switch_code(&scope, head)
        .map(|code| code.0)
        .map_err(|e| ControlError::Journal(e.to_string()))
}

/// Commits `command` for `agent`. A re-run of a command already committed as the control stream's
/// last event, and not yet recorded by the runtime, reports that event and commits nothing
/// (DEC-290).
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
    let confirmed = |head: u64| {
        Ok(match (code_at(agent, command, head)?, typed) {
            (Some(right), Some(typed)) => right == typed,
            _ => false,
        })
    };
    let needs_code = matches!(
        command,
        Command::Resume | Command::Stop { .. } | Command::Acknowledge { .. }
    );
    let (event_type, key, timed): (_, _, &[&'static str]) = match command {
        Command::Acknowledge { event } => (
            "OwnerAcknowledged",
            vec![
                ("agent", text(agent)),
                ("event", text(event)),
                ("user", text(&owner.user)),
            ],
            &["risk_clock", "submitted_at"],
        ),
        Command::Pause => (
            "OwnerCommandIssued",
            issued(owner, Some(agent), "pause", ("agent", agent), None, None)?,
            &["submitted_at"],
        ),
        Command::Resume => (
            "OwnerCommandIssued",
            issued(owner, Some(agent), "resume", ("agent", agent), None, None)?,
            &["submitted_at"],
        ),
        Command::Stop { release } => (
            "OwnerCommandIssued",
            issued(
                owner,
                Some(agent),
                "stop",
                ("agent", agent),
                Some(*release),
                None,
            )?,
            &["submitted_at"],
        ),
        Command::Exit { instrument, bid } => (
            "OwnerCommandIssued",
            issued(
                owner,
                Some(agent),
                "owner_exit",
                ("instrument", instrument.as_str()),
                None,
                bid.as_ref(),
            )?,
            &["submitted_at"],
        ),
    };
    match decide(
        journal,
        owner,
        event_type,
        key,
        Confirmation::AtHead(&confirmed),
    )? {
        Decision::Committed(submitted) => Ok(submitted),
        Decision::Fresh(decided) if needs_code && !decided.stepped_up => {
            Err(ControlError::Refused {
                reason: "step_up_missing",
            })
        }
        Decision::Fresh(decided) => commit(journal, ids, owner, decided, timed, now),
    }
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
    let confirmed = |head: u64| {
        Ok(typed.is_some_and(|typed| kill_code_at(scope, head).is_ok_and(|right| right == typed)))
    };
    let (agent, kind, subject) = match scope {
        Scope::Agent(agent) => (Some(agent.as_str()), "agent", agent.as_str()),
        Scope::Connection(connection) => (None, "connection", connection.as_str()),
        Scope::Workspace => (None, "workspace", owner.workspace.as_str()),
    };
    let key = issued(owner, agent, "kill_switch", (kind, subject), None, None)?;
    match decide(
        journal,
        owner,
        "OwnerCommandIssued",
        key,
        Confirmation::AtHead(&confirmed),
    )? {
        Decision::Committed(submitted) => Ok(submitted),
        Decision::Fresh(decided) => commit(journal, ids, owner, decided, &["submitted_at"], now),
    }
}

/// What `status` shows: the mode and hold from the agent stream, the approvals still pending, and
/// the restrictions the agent's account stream holds for it (the M7 brief's command table).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// The effective mode the last `AgentModeChanged` recorded: `normal` before any.
    pub mode: String,
    /// Whether that change was the startup hold (`awaiting_reconciliation`).
    pub startup_hold: bool,
    pub pending_approvals: usize,
    /// The restrictions in force, agent restrictions first, each group in name order.
    pub restrictions: Vec<Restriction>,
}

/// A restriction in force on the agent (mandate spec §5.9), as the account stream records it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Restriction {
    /// An agent restriction and the mode it holds, from the latest `AgentModeApplied` naming it;
    /// one whose latest names `normal` has lifted.
    Agent { restriction: String, mode: String },
    /// An instrument restriction (`stale_mark`, `removed_instrument`) whose latest
    /// `InstrumentRestrictionChanged` is active.
    Instrument {
        instrument: String,
        restriction: String,
    },
}

/// # Errors
/// [`ControlError::Journal`] when the journal cannot be read.
pub fn status(
    journal: &dyn ControlJournal,
    owner: &Owner,
    agent: &str,
    account: &str,
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
    let restrictions = restrictions(journal, owner, agent, account)?;
    Ok(Status {
        mode,
        startup_hold,
        pending_approvals,
        restrictions,
    })
}

/// The restrictions `account`'s stream holds for `agent`.
///
/// # Errors
/// [`ControlError::Journal`] when the journal cannot be read; [`ControlError::Unimplemented`] for
/// an account stream with events, in the tests PR (DEC-77).
fn restrictions(
    journal: &dyn ControlJournal,
    owner: &Owner,
    agent: &str,
    account: &str,
) -> Result<Vec<Restriction>, ControlError> {
    if envelopes(journal, &account_stream(owner, account)?)?.is_empty() {
        return Ok(Vec::new());
    }
    let _ = agent;
    Err(ControlError::Unimplemented { story: "E8-3" })
}

/// What the owner chose in one `OwnerCommandIssued` (DEC-257 item 5, DEC-290): the agent, or `null`
/// for a kill switch of a connection or a workspace; the scope and its subject; for a Stop, whether
/// it releases the positions, and the content reference of the warning shown when it does; the
/// confirmed bid, bid size and floor or `null`; and the owner's opaque id. [`commit`] adds
/// `submitted_at` and the step-up evidence or `null`.
fn issued(
    owner: &Owner,
    agent: Option<&str>,
    command: &str,
    (scope, subject): (&str, &str),
    release: Option<bool>,
    bid: Option<&Confirmed>,
) -> Result<Vec<(&'static str, Value)>, ControlError> {
    let given = |field: fn(&Confirmed) -> &String| bid.map_or(Value::Null, |b| text(field(b)));
    let warning_shown = match release {
        Some(true) => return Err(ControlError::Unimplemented { story: "E8-3" }),
        Some(false) | None => Value::Null,
    };
    Ok(vec![
        ("agent", agent.map_or(Value::Null, text)),
        ("command", text(command)),
        ("scope", text(scope)),
        ("subject", text(subject)),
        ("release", release.map_or(Value::Null, Value::Bool)),
        ("warning_shown", warning_shown),
        ("bid", given(|b| &b.bid)),
        ("bid_size", given(|b| &b.bid_size)),
        ("floor", given(|b| &b.floor)),
        ("user", text(&owner.user)),
    ])
}
