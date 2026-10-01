//! `mandate approvals list`, `show`, `approve`, and `skip` (backlog E8-1 to E8-3; the M7 brief's
//! "The CLI control surface"; mandate spec §6.4; DEC-155 item 5, DEC-257 item 5).
//!
//! The inbox is the owner's own journal: `list` and `show` read an agent stream's approval events
//! and commit nothing. `approve` and `skip` each commit exactly one `ApprovalResponseSubmitted` to
//! the workspace control stream, one approval per command, with no batch form. `approve` needs the
//! confirmation code `show` printed, bound to the request's content hash, and records `cli_confirm`
//! step-up evidence authenticated when the owner ran it; `skip` takes the same arguments without
//! the code and needs no step-up (PX-7, PX-10). Whatever the CLI checks first, the runtime checks
//! again (DEC-155 item 5).

use mandate_canon::Value;

use crate::control::{ControlError, ControlJournal, Ids, Now, Owner, Submitted};

/// The story every stub here belongs to.
const STORY: &str = "E8-3";

/// One approval as `list` shows it: opaque ids and times, never the content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub approval: String,
    pub agent: String,
    pub state: State,
    /// The deadline, as the request's risk-clock second.
    pub deadline_s: i64,
    /// Seconds left before the deadline at the moment `list` ran; 0 once it has passed.
    pub remaining_s: i64,
}

/// Where an approval stands, from the agent stream's events alone (the M7 brief's lifecycle).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum State {
    Pending,
    /// An admitted grant whose re-validation acted: the executor's binding gate still decides.
    Acted,
    /// Skipped by the owner, by re-validation, or by the deadline, or cancelled.
    Skipped(Ended),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Ended {
    ByOwner,
    OnRevalidation,
    TimedOut,
    Canceled,
}

/// Every approval of `agents`, pending ones by deadline and then resolved ones, as the D5 inbox
/// orders them.
///
/// # Errors
/// [`ControlError::Journal`] when the journal cannot be read.
pub fn list(
    journal: &dyn ControlJournal,
    owner: &Owner,
    agents: &[&str],
    now: Now,
) -> Result<Vec<Listed>, ControlError> {
    let _ = (journal, owner, agents, now);
    Err(ControlError::Unimplemented { story: STORY })
}

/// What `show` renders: the request's content object exactly as committed, and the code the owner
/// re-types to approve it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub approval: String,
    pub content: Value,
    pub content_hash: String,
    /// `mandate_approval::confirmation_code` of the content hash.
    pub code: String,
    pub deadline_s: i64,
}

/// # Errors
/// [`ControlError::Refused`] with `not_pending` for an approval the agent stream does not hold;
/// [`ControlError::Journal`] when the journal cannot be read.
pub fn show(
    journal: &dyn ControlJournal,
    owner: &Owner,
    agent: &str,
    approval: &str,
) -> Result<Shown, ControlError> {
    let _ = (journal, owner, agent, approval);
    Err(ControlError::Unimplemented { story: STORY })
}

/// Commits the owner's grant. Refused locally, committing nothing, when the approval is not
/// pending (`not_pending`), its deadline has passed at `now` (`late`), or `code` is not the one for
/// its content hash (`content_mismatch`).
///
/// # Errors
/// [`ControlError::Refused`] as above; [`ControlError::Journal`] when the append fails.
pub fn approve(
    journal: &mut dyn ControlJournal,
    ids: &mut dyn Ids,
    owner: &Owner,
    agent: &str,
    approval: &str,
    code: &str,
    now: Now,
) -> Result<Submitted, ControlError> {
    let _ = (journal, ids, owner, agent, approval, code, now);
    Err(ControlError::Unimplemented { story: STORY })
}

/// Commits the owner's skip. Refused locally, committing nothing, when the approval is not pending
/// or its deadline has passed, since the approval is then already skipped.
///
/// # Errors
/// As [`approve`], less the code.
pub fn skip(
    journal: &mut dyn ControlJournal,
    ids: &mut dyn Ids,
    owner: &Owner,
    agent: &str,
    approval: &str,
    now: Now,
) -> Result<Submitted, ControlError> {
    let _ = (journal, ids, owner, agent, approval, now);
    Err(ControlError::Unimplemented { story: STORY })
}

/// What the runtime recorded for a submitted answer: the `ApprovalResponded` whose `causation_id`
/// is it, and the `ApprovalRevalidated` that followed an admitted grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing recorded yet. Never reported as approved (rule 3).
    NotRecorded,
    Admitted {
        revalidation: Option<Revalidated>,
    },
    Counted,
    Refused {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Revalidated {
    Act,
    Skip { reason: String },
}

/// # Errors
/// [`ControlError::Journal`] when the journal cannot be read.
pub fn outcome(
    journal: &dyn ControlJournal,
    owner: &Owner,
    agent: &str,
    submitted: &Submitted,
) -> Result<Outcome, ControlError> {
    let _ = (journal, owner, agent, submitted);
    Err(ControlError::Unimplemented { story: STORY })
}

/// The line `approve` prints for an outcome. Only an admitted grant whose re-validation acted says
/// it was sent, and nothing the runtime has not recorded is ever called approved.
///
/// # Errors
/// [`ControlError::Unimplemented`] until the implementation PR.
pub fn message(outcome: &Outcome) -> Result<String, ControlError> {
    let _ = outcome;
    Err(ControlError::Unimplemented { story: STORY })
}
