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

use mandate_canon::{Digest, Key, Object, Value, to_canonical};
use mandate_time::UtcNanos;

use crate::ConnectError;
use crate::checks::{Check, Reason};
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
/// non-paper environment is `EnvironmentRefused` and never digested. Any broker is bound; only
/// [`plan_start`] refuses one the route does not serve (DEC-883 item 3).
///
/// Its member names are valid canonical keys, so the `InvalidRecord` it would return for one that
/// is not cannot occur.
pub fn connect_digest(
    workspace_id: &str,
    broker: Broker,
    environment: Environment,
) -> Result<Digest, ConnectError> {
    if environment != Environment::Paper {
        return Err(ConnectError::EnvironmentRefused);
    }
    let members = [
        ("action", Value::Str("connect".to_owned())),
        ("workspace_id", Value::Str(workspace_id.to_owned())),
        ("broker", Value::Str(broker.code().to_owned())),
        ("environment", Value::Str(environment.as_str().to_owned())),
    ];
    let object = members
        .into_iter()
        .map(|(name, value)| Key::new(name).map(|key| (key, value)))
        .collect::<Result<Object, _>>()
        .map_err(|_| ConnectError::InvalidRecord {
            member: "connect digest",
        })?;
    Ok(Digest::of(&to_canonical(&Value::Object(object))))
}

/// Plans a connect's start: exactly `[Requested]` with the request's members, or the first
/// refusal that holds, in DEC-883's order: step-up not verified, environment not paper, broker
/// not Alpaca, a fold that cannot answer, an open request, an establishment, a used `account_ref`.
/// Step-up comes first so an unverified caller learns nothing of the control stream (DEC-883).
pub fn plan_start(start: &StartRequest, facts: &StartFacts) -> Result<StartPlan, ConnectError> {
    let evidence = match &start.step_up {
        StepUpCheck::Verified(evidence) => evidence,
        StepUpCheck::NotVerified => return Ok(StartPlan::Refused(StartRefusal::StepUpNotVerified)),
    };
    Ok(match start_refusal(start, facts) {
        Some(refusal) => StartPlan::Refused(refusal),
        None => StartPlan::Commit(vec![ManagerEffect::Requested(RequestMembers {
            connection_id: start.connection_id.clone(),
            account_ref: start.account_ref.clone(),
            broker: start.broker,
            environment: start.environment,
            user: start.user.clone(),
            step_up: evidence.clone(),
        })]),
    })
}

/// The first refusal after step-up, in [`plan_start`]'s order; `None` lets the start commit.
fn start_refusal(start: &StartRequest, facts: &StartFacts) -> Option<StartRefusal> {
    if start.environment != Environment::Paper {
        Some(StartRefusal::EnvironmentRefused)
    } else if start.broker != Broker::Alpaca {
        Some(StartRefusal::BrokerRefused)
    } else if facts.id == IdFold::CannotAnswer || facts.account_ref == AccountRefFold::CannotAnswer
    {
        Some(StartRefusal::FoldCannotAnswer)
    } else if facts.id == IdFold::RequestOpen {
        Some(StartRefusal::RequestOpen)
    } else if facts.id == IdFold::Established {
        Some(StartRefusal::AlreadyEstablished)
    } else if facts.account_ref == AccountRefFold::Used {
        Some(StartRefusal::AccountRefUsed)
    } else {
        None
    }
}

/// Plans a connect's finish, by DEC-884's order: a fold that cannot answer, no open request, and a
/// request a revoke closed come first, whatever the outcome. Then a passed outcome is exactly
/// `[Established]`, and a refused one exactly `[ConnectRefused]` with journal spec §9.8's check
/// and reason, every request member repeated. Never an effect and a refusal together (DEC-697).
///
/// With the request open, a passed `live` request is `LiveNotServed` (rule 64, DEC-884 item 2),
/// and `already_connected` naming the request's own id is `ExistingIsSelf` (rule 55, item 3); each
/// leaves the request open for its teardown.
pub fn plan_finish(
    fold: &RequestFold,
    outcome: &ConnectOutcome,
) -> Result<FinishPlan, ConnectError> {
    let request = match fold {
        RequestFold::CannotAnswer => {
            return Ok(FinishPlan::Refused(FinishRefusal::FoldCannotAnswer));
        }
        RequestFold::NotOpen => return Ok(FinishPlan::Refused(FinishRefusal::NotOpen)),
        RequestFold::ClosedByRevoke => return Ok(FinishPlan::ClosedByRevoke),
        RequestFold::Open(request) => request.clone(),
    };
    let effect = match outcome {
        ConnectOutcome::Passed { .. } if request.environment != Environment::Paper => {
            return Ok(FinishPlan::Refused(FinishRefusal::LiveNotServed));
        }
        ConnectOutcome::Refused(RefusalCause::AlreadyConnected(existing))
            if *existing == request.connection_id =>
        {
            return Ok(FinishPlan::Refused(FinishRefusal::ExistingIsSelf));
        }
        ConnectOutcome::Passed { scopes } => ManagerEffect::Established {
            request,
            scopes: scopes.clone(),
        },
        ConnectOutcome::Refused(cause) => ManagerEffect::ConnectRefused {
            request,
            refused: refused_as(cause),
        },
    };
    Ok(FinishPlan::Commit(vec![effect]))
}

/// `ConnectionRefused`'s `check`, `reason`, and `existing_connection_id` for `cause`, as journal
/// spec §9.8's reasons table writes them: an executor reason under its own check,
/// `account_mismatch` under `account`, `already_connected` under `uniqueness` naming the holder,
/// and the four teardowns with `check` `null` (rules 54 and 55, DEC-884 item 4).
fn refused_as(cause: &RefusalCause) -> RefusedAs {
    let (check, reason, existing_connection_id) = match cause {
        RefusalCause::Check(reason) => (Some(check_of(*reason).code()), reason.code(), None),
        RefusalCause::AccountMismatch => (Some(Check::Account.code()), "account_mismatch", None),
        RefusalCause::AlreadyConnected(existing) => (
            Some("uniqueness"),
            "already_connected",
            Some(existing.clone()),
        ),
        RefusalCause::Timeout => (None, "timeout", None),
        RefusalCause::RestartPastDeadline => (None, "restart_past_deadline", None),
        RefusalCause::ExecutorStopped => (None, "executor_stopped", None),
        RefusalCause::StartFailed => (None, "start_failed", None),
    };
    RefusedAs {
        check,
        reason,
        existing_connection_id,
    }
}

/// The check whose row of journal spec §9.8's reasons table holds `reason`.
fn check_of(reason: Reason) -> Check {
    match reason {
        Reason::ScopeMismatch | Reason::FundMovement | Reason::PermissionsUnreadable => {
            Check::Scope
        }
        Reason::WrongEnvironment | Reason::ReachesBoth => Check::Environment,
        Reason::AccountUnreadable | Reason::NotDedicated => Check::Account,
        Reason::ToolsMissing | Reason::ContractDrift => Check::Contract,
    }
}
