//! Step-up evidence and the owner commands that need it (DEC-155 item 4, DEC-156 item 8, DEC-158
//! option (c)).

use std::collections::BTreeSet;

use crate::{ApprovalError, RiskClock};

/// How long step-up evidence stays fresh, in seconds (mandate spec §6.4: "within the 5 minutes").
pub const STEP_UP_WINDOW_S: i64 = 300;

/// A step-up gesture's id: usable once per workspace (EI-11).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssertionId(pub String);

/// The only v0 method. The owner re-types the code the CLI printed, which proves the owner read
/// this content and does not authenticate against an identity provider, so it is accepted on a
/// `paper` stream only (DEC-155 item 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepUpMethod {
    CliConfirm,
}

/// The stream's trading environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    Paper,
    Live,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepUp {
    pub assertion: AssertionId,
    pub authenticated_at: RiskClock,
    pub method: StepUpMethod,
}

/// Why step-up evidence does not count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StepUpRefusal {
    Missing,
    Stale,
    Reused,
    Method,
}

/// The owner commands that are judged here. The kill switch is not one of them: it has its own
/// entry point, [`kill_switch`], which cannot refuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerCommandKind {
    /// Needs no step-up (PX-4): always applies.
    Pause,
    /// Judged when the runtime processes it.
    Resume,
    Stop,
    Acknowledge,
    /// Judged when the owner committed it (DEC-156 item 8).
    OwnerExit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandAuthority {
    Apply,
    Refused(StepUpRefusal),
}

/// What a kill switch may do. There is no refusing variant (DEC-158 option (c)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KillSwitchAuthority {
    /// Stop and flatten as an automated flatten does, equities waiting for the regular session
    /// (MC-F04), because the evidence did not count for the reason given.
    AutomatedFlatten(StepUpRefusal),
    /// Valid evidence as the owner committed it: the owner-exit privileges apply as well (selling
    /// equities outside the regular session at a confirmed bid, DEC-58, DEC-66).
    OwnerExitPrivileges,
}

/// Judges an owner command's step-up evidence. `committed_at` is the command's `submitted_at` on
/// the control stream; `processed_at` is the runtime's folded clock when it reads it.
///
/// # Errors
/// [`ApprovalError::Unimplemented`] until E8-3.
pub fn owner_command(
    kind: OwnerCommandKind,
    evidence: Option<&StepUp>,
    committed_at: RiskClock,
    processed_at: RiskClock,
    environment: Environment,
    used: &BTreeSet<AssertionId>,
) -> Result<CommandAuthority, ApprovalError> {
    let _ = (
        kind,
        evidence,
        committed_at,
        processed_at,
        environment,
        used,
    );
    Err(ApprovalError::Unimplemented { story: "E8-3" })
}

/// Judges a kill switch's evidence at the moment the owner committed it. Never refuses.
///
/// # Errors
/// [`ApprovalError::Unimplemented`] until E8-3.
pub fn kill_switch(
    evidence: Option<&StepUp>,
    committed_at: RiskClock,
    environment: Environment,
    used: &BTreeSet<AssertionId>,
) -> Result<KillSwitchAuthority, ApprovalError> {
    let _ = (evidence, committed_at, environment, used);
    Err(ApprovalError::Unimplemented { story: "E8-3" })
}
