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

use crate::control::{ControlError, ControlJournal, Ids, Now, Owner, Submitted};

const STORY: &str = "E8-3";

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
    let _ = (journal, owner, agent, command);
    Err(ControlError::Unimplemented { story: STORY })
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
    let _ = (journal, owner, scope);
    Err(ControlError::Unimplemented { story: STORY })
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
    let _ = (journal, ids, owner, agent, command, typed, now);
    Err(ControlError::Unimplemented { story: STORY })
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
    let _ = (journal, ids, owner, scope, typed, now);
    Err(ControlError::Unimplemented { story: STORY })
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
    let _ = (journal, owner, agent);
    Err(ControlError::Unimplemented { story: STORY })
}
