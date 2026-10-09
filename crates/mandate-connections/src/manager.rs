//! The connection manager's start and finish of a connect (connections spec §5.2 steps 1, 5, and
//! 6, §9.1; journal spec §9.8 rule 131; DEC-694, DEC-699). [`plan_start`] returns a typed plan: the
//! pending record `ConnectionRequested`, which commits with the step-up challenge marked used
//! (identity spec §7.2 step 5), or a refusal, never both (DEC-697). [`plan_finish`] closes the open
//! request with its establishment or its `ConnectionRefused`, each repeating the request's members
//! (DEC-699 item 4), or plans nothing after a revoke closed it (DEC-699 item 5). `mandate-api-server` maps each [`ManagerEffect`]
//! to its event (DEC-694 item 2). [`connect_digest`] is the connect's step-up digest, paper only
//! (DEC-693 items 1, 2). A fold that cannot answer is a value of its own that refuses (DEC-693
//! item 7). No type here has a member a secret could sit in (DEC-699 I5, `AGENTS.md` rule 7).

use std::collections::BTreeSet;

use mandate_canon::Digest;
use mandate_time::UtcNanos;

use crate::ConnectError;
use crate::checks::Reason;
use crate::record::{AccountRef, Broker, ConnectionId, Environment};

/// Journal spec §9.8's `step_up`: the evidence one connect spent (identity spec §7.2).
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
    /// No open request and no `ConnectionEstablished` of the id: a new id, or one whose attempts
    /// were all refused or revoked while `connecting`.
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

/// `ConnectionRequested`'s members, exactly rule 131's (journal spec §9.8), which the record that
/// closes the request repeats (`ConnectionRefused` all but `account_ref`).
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
    /// `ConnectionEstablished` version 2: the request's members, `account_ref` among them, and the
    /// scopes granted. Paper only, so `margin_attestation` is `null` (rule 64, DEC-884 item 2).
    Established {
        request: RequestMembers,
        scopes: BTreeSet<String>,
    },
    /// `ConnectionRefused`, occasion `connect`: the request's `connection_id`, `broker`,
    /// `environment`, `user`, and `step_up` (its `account_ref` is not a member), and the refusal.
    ConnectRefused {
        request: RequestMembers,
        refused: RefusedAs,
    },
}

/// `ConnectionRefused`'s `check`, `reason`, and `existing_connection_id`, as journal spec §9.8
/// writes them (rules 54 and 55).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefusedAs {
    pub check: Option<&'static str>,
    pub reason: &'static str,
    pub existing_connection_id: Option<ConnectionId>,
}

/// What the control-stream fold holds of the connection's request (rule 131).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestFold {
    Open(RequestMembers),
    /// The latest request was closed by a `ConnectionRevoked` (DEC-699 item 5).
    ClosedByRevoke,
    /// No request, or the latest was closed by an establishment or a refusal.
    NotOpen,
    /// As [`IdFold::CannotAnswer`].
    CannotAnswer,
}

/// Why a connect is refused (connections spec §5.2 steps 5 and 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefusalCause {
    /// The executor's check 1, 2, 3, or 7 failed; the reason names its check.
    Check(Reason),
    /// Check 3's fingerprint comparison, the manager's alone.
    AccountMismatch,
    /// Check 4, naming the connection that holds the account.
    AlreadyConnected(ConnectionId),
    Timeout,
    RestartPastDeadline,
    ExecutorStopped,
    StartFailed,
}

/// How a connect in `connecting` ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectOutcome {
    /// Checks 1 to 4 and 7 passed, granting `scopes`.
    Passed {
        scopes: BTreeSet<String>,
    },
    Refused(RefusalCause),
}

/// Why a finish is refused; nothing is appended (DEC-884).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinishRefusal {
    FoldCannotAnswer,
    /// No open request to close (rule 131, closing).
    NotOpen,
    /// A live request cannot be established here: it has no margin attestation (rule 64).
    LiveNotServed,
    /// `already_connected` named the request's own id (rule 55).
    ExistingIsSelf,
}

/// What the server does with a finish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinishPlan {
    Commit(Vec<ManagerEffect>),
    /// A revoke closed the request: the teardown runs and nothing is journaled.
    ClosedByRevoke,
    Refused(FinishRefusal),
}

/// Why a start is refused; nothing commits, so no step-up is spent (journal spec §9.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartRefusal {
    StepUpNotVerified,
    /// The start route connects paper only (connections spec §5.2, DEC-693 item 2).
    EnvironmentRefused,
    /// The route serves Alpaca OAuth only (connections spec §5.2, DEC-821 item 7, DEC-883 item 2).
    BrokerRefused,
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

/// Plans a connect's start: exactly `[Requested]` with the request's members, or the first
/// refusal that holds, in DEC-883's order: step-up not verified, environment not paper, broker
/// not Alpaca, a fold that cannot answer, an open request, an establishment, a used `account_ref`.
pub fn plan_start(start: &StartRequest, facts: &StartFacts) -> Result<StartPlan, ConnectError> {
    let _ = (start, facts);
    Err(ConnectError::Unimplemented { story: "E10-13" })
}

/// Plans a connect's finish, by DEC-884's order: a fold that cannot answer, no open request, and a
/// request a revoke closed come first, whatever the outcome. Then a passed outcome is exactly
/// `[Established]`, and a refused one exactly `[ConnectRefused]` with journal spec §9.8's check
/// and reason, every request member repeated. Never an effect and a refusal together (DEC-697).
pub fn plan_finish(
    fold: &RequestFold,
    outcome: &ConnectOutcome,
) -> Result<FinishPlan, ConnectError> {
    let _ = (fold, outcome);
    Err(ConnectError::Unimplemented { story: "E10-13" })
}
