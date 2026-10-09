//! The connection manager's start of a connect (connections spec §5.2 step 1, §9.1; journal spec
//! §9.8 rule 131; DEC-694, DEC-699). [`plan_start`] returns a typed plan: the pending record
//! `ConnectionRequested`, which commits with the step-up challenge marked used (identity spec §7.2
//! step 5), or a refusal, never both (DEC-697). `mandate-api-server` maps each [`ManagerEffect`]
//! to its event (DEC-694 item 2). [`connect_digest`] is the connect's step-up digest, paper only
//! (DEC-693 items 1, 2). Closing the request is the next slice of E10-13.
//!
//! A stream fact the control-stream fold cannot answer is a value of its own that refuses (DEC-693
//! item 7). No input, effect, or plan has a member a secret could sit in (DEC-699 I5, rule 7).

use mandate_canon::Digest;
use mandate_time::UtcNanos;

use crate::ConnectError;
use crate::record::{AccountRef, Broker, ConnectionId, Environment};

/// Journal spec §9.8's `step_up`: the evidence one connect spent (identity spec §7.2), written on
/// its request and again, unchanged, on the record that closes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepUpEvidence {
    pub assertion_id: String,
    pub authenticated_at: UtcNanos,
    pub method: String,
}

/// Whether the step-up evidence for this connect's digest counts (identity spec §7.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepUpCheck {
    Verified(StepUpEvidence),
    NotVerified,
}

/// A connect start as the owner asked for it, with the `connection_id` and `account_ref` the
/// manager assigned (journal spec §9.8). `user` is the workspace admin, opaque.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartRequest {
    pub connection_id: ConnectionId,
    pub account_ref: AccountRef,
    pub broker: Broker,
    pub environment: Environment,
    pub user: String,
    pub step_up: StepUpCheck,
}

/// What the control stream holds of the assigned `connection_id` (rule 131, opening).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdFold {
    /// No open request and no `ConnectionEstablished`; a refused or revoked attempt may retry.
    Free,
    RequestOpen,
    Established,
    /// The control-stream fold is unavailable, stale, or behind its watermark.
    CannotAnswer,
}

/// Whether an earlier `ConnectionRequested` or `ConnectionEstablished` names the assigned
/// `account_ref` (rule 131, opening; DEC-699 I3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountRefFold {
    Unused,
    Used,
    /// As [`IdFold::CannotAnswer`].
    CannotAnswer,
}

/// The stream facts a start is refused on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartFacts {
    pub id: IdFold,
    pub account_ref: AccountRefFold,
}

/// `ConnectionRequested`'s members, exactly rule 131's (journal spec §9.8). The record that closes
/// the request repeats them: an establishment all of them, a `ConnectionRefused` all but
/// `account_ref`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestMembers {
    pub connection_id: ConnectionId,
    pub account_ref: AccountRef,
    pub broker: Broker,
    pub environment: Environment,
    pub user: String,
    pub step_up: StepUpEvidence,
}

/// One record the server appends, in plan order, on the control stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManagerEffect {
    /// `ConnectionRequested`.
    Requested(RequestMembers),
}

/// Why a start is refused; nothing commits, so no step-up is spent (journal spec §9.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartRefusal {
    StepUpNotVerified,
    /// The start route connects paper only (connections spec §5.2, DEC-693 item 2).
    EnvironmentRefused,
    RequestOpen,
    AlreadyEstablished,
    AccountRefUsed,
    FoldCannotAnswer,
}

/// What the server does with a start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartPlan {
    Commit(Vec<ManagerEffect>),
    Refused(StartRefusal),
}

/// The connect's step-up digest (kind `connection`): SHA-256 of the canonical `{action:
/// "connect", workspace_id, broker, environment: "paper"}` (DEC-693 items 1, 2, 5). A
/// non-paper environment is `EnvironmentRefused` and never digested.
pub fn connect_digest(
    workspace_id: &str,
    broker: Broker,
    environment: Environment,
) -> Result<Digest, ConnectError> {
    let _ = (workspace_id, broker, environment);
    Err(ConnectError::Unimplemented { story: "E10-13" })
}

/// Plans a connect's start: exactly `[Requested]` with the request's members, or a refusal when
/// step-up is not verified, the environment is not paper, the id has an open request or an
/// establishment, the `account_ref` was used, or a fold cannot answer.
pub fn plan_start(start: &StartRequest, facts: &StartFacts) -> Result<StartPlan, ConnectError> {
    let _ = (start, facts);
    Err(ConnectError::Unimplemented { story: "E10-13" })
}
