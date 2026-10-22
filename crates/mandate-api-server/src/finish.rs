//! The connect's finish ([DEC-2342](../../../docs/project/decisions/DEC-2342.md); connections
//! spec §5.2 steps 5 and 6; journal spec §9.8): from the control stream's open request and the
//! account stream's `ConnectionChecked`, what `mandate_connections::manager::plan_finish` is asked,
//! and what it answers. It decides only; the pass that appends and tears down is the server's.

use std::collections::BTreeSet;
use std::future::{Future, ready};
use std::pin::Pin;

use mandate_canon::{Value, parse};
use mandate_connections::ConnectError;
use mandate_connections::checks::Reason;
use mandate_connections::grant::check_scope;
use mandate_connections::manager::{
    ConnectOutcome, FinishPlan, RefusalCause, RequestFold, RequestMembers, StepUpEvidence,
    plan_finish,
};
use mandate_connections::record::{AccountRef, Broker, ConnectionId, Environment};
use mandate_journal::StoredEvent;
use mandate_time::UtcNanos;

/// How long a connect may stay open from its request's `recorded_at`: the code's ten minutes and
/// one more (connections spec §5.2 step 6, DEC-2342 item 2).
pub const CONNECT_DEADLINE_SECS: i64 = 660;

/// The scopes an establishment records: the scope check passes on exactly these (DEC-2342 item 1).
pub const ESTABLISHED_SCOPES: &str = "data trading";

/// Every reason a `ConnectionChecked` may give, by journal spec §9.8's table.
const REASONS: [Reason; 9] = [
    Reason::ScopeMismatch,
    Reason::FundMovement,
    Reason::PermissionsUnreadable,
    Reason::WrongEnvironment,
    Reason::ReachesBoth,
    Reason::AccountUnreadable,
    Reason::NotDedicated,
    Reason::ToolsMissing,
    Reason::ContractDrift,
];

/// What the control stream holds of one connection's request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub fold: RequestFold,
    /// When the open request was recorded; `None` unless it is open.
    pub opened_at: Option<UtcNanos>,
    /// Whether the open request is a version-2 `oauth` one, the only kind this finish closes
    /// (DEC-2621 item 3).
    pub oauth: bool,
}

/// The latest `connect` check of the connection on its account stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checked {
    /// Every result passed; the account's personal-data reference.
    Passed { event_id: String, pii_ref: String },
    /// The first failed result's reason.
    Failed { event_id: String, reason: Reason },
}

/// An account stream whose check of the connection cannot be read as the executor writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unreadable;

/// Checks 3 and 4, which the control services alone run (journal spec §9.8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Judged {
    Unique,
    AccountMismatch,
    AlreadyConnected(ConnectionId),
    /// Nothing can be said: nothing is established and nothing refused on it.
    CannotAnswer,
}

/// A fingerprint port's judgement of one request.
pub type Judging<'a> = Pin<Box<dyn Future<Output = Judged> + Send + 'a>>;

/// A release that did not reach the vault, or that the vault refused; the next pass retries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleaseFailed;

/// One release's answer.
pub type Released<'a> = Pin<Box<dyn Future<Output = Result<(), ReleaseFailed>> + Send + 'a>>;

/// Where the finish has checks 3 and 4 judged: the account behind `account_pii_ref` against the
/// request's connection and every established one.
pub trait Fingerprints: Send + Sync {
    fn judge<'a>(&'a self, request: &'a RequestMembers, account_pii_ref: &'a str) -> Judging<'a>;

    /// Frees the fingerprint the closed request of `connection` on `account_ref` held, once its
    /// deletes have succeeded (DEC-2348 item 2). A restarted pass calls it again, so it is
    /// idempotent; a release that fails is retried on the next pass.
    fn release<'a>(
        &'a self,
        connection: &'a ConnectionId,
        account_ref: &'a AccountRef,
    ) -> Released<'a>;
}

/// The port while no fingerprint backing exists (DEC-2342 item 7): it answers nothing, so no
/// connect is established and each reaches its deadline's teardown.
#[derive(Debug, Clone, Copy, Default)]
pub struct Unbacked;

impl Fingerprints for Unbacked {
    fn judge<'a>(&'a self, _request: &'a RequestMembers, _account_pii_ref: &'a str) -> Judging<'a> {
        Box::pin(ready(Judged::CannotAnswer))
    }

    fn release<'a>(
        &'a self,
        _connection: &'a ConnectionId,
        _account_ref: &'a AccountRef,
    ) -> Released<'a> {
        Box::pin(ready(Ok(())))
    }
}

/// What one pass does for a connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pass {
    /// Nothing yet: no check and not past the deadline, or not this finish's request.
    Wait,
    /// `plan_finish`'s answer, and the `causation_id` its effect carries.
    Finish {
        plan: FinishPlan,
        causation: Option<String>,
    },
}

/// The request for `connection` on `control`: open from its `ConnectionRequested` until the
/// establishment, refusal or revoke that closes it (rule 131). A row about the connection that
/// lacks a member the request needs leaves it [`RequestFold::CannotAnswer`].
pub fn request(control: &[StoredEvent], connection: &ConnectionId) -> Request {
    let mut fold = RequestFold::NotOpen;
    let mut opened_at = None;
    let mut oauth = false;
    for row in control {
        let kind = row.event_type.as_str();
        if !matches!(
            kind,
            "ConnectionRequested"
                | "ConnectionEstablished"
                | "ConnectionRefused"
                | "ConnectionRevoked"
        ) {
            continue;
        }
        let Some(payload) = payload(row) else {
            return cannot_answer();
        };
        let Some(named) = payload.get("connection_id").and_then(Value::as_str) else {
            return cannot_answer();
        };
        if named != connection.as_str() {
            continue;
        }
        let open = matches!(fold, RequestFold::Open(_));
        match kind {
            "ConnectionRequested" if !open => {
                let Some(members) = members(&payload, connection) else {
                    return cannot_answer();
                };
                let Some(at) = UtcNanos::parse_rfc3339(&row.recorded_at).ok() else {
                    return cannot_answer();
                };
                oauth = row.schema_version == 2
                    && payload.get("credential").and_then(Value::as_str) == Some("oauth");
                opened_at = Some(at);
                fold = RequestFold::Open(members);
            }
            "ConnectionRevoked" if open => {
                fold = RequestFold::ClosedByRevoke;
                opened_at = None;
                oauth = false;
            }
            "ConnectionEstablished" | "ConnectionRefused" => {
                fold = RequestFold::NotOpen;
                opened_at = None;
                oauth = false;
            }
            _ => {}
        }
    }
    Request {
        fold,
        opened_at,
        oauth,
    }
}

fn cannot_answer() -> Request {
    Request {
        fold: RequestFold::CannotAnswer,
        opened_at: None,
        oauth: false,
    }
}

fn payload(row: &StoredEvent) -> Option<Value> {
    parse(&row.body).ok()?.get("payload").cloned()
}

/// The request's members, rule 131's, from a `ConnectionRequested`'s payload.
fn members(payload: &Value, connection: &ConnectionId) -> Option<RequestMembers> {
    let text = |name: &str| payload.get(name).and_then(Value::as_str);
    let step_up = payload.get("step_up")?;
    let step_text = |name: &str| step_up.get(name).and_then(Value::as_str).map(str::to_owned);
    let broker = match text("broker")? {
        "alpaca" => Broker::Alpaca,
        "robinhood" => Broker::Robinhood,
        "kraken_derivatives_us" => Broker::KrakenDerivativesUs,
        _ => return None,
    };
    let environment = match text("environment")? {
        "paper" => Environment::Paper,
        "live" => Environment::Live,
        _ => return None,
    };
    Some(RequestMembers {
        connection_id: connection.clone(),
        account_ref: AccountRef::new(text("account_ref")?).ok()?,
        broker,
        environment,
        user: text("user")?.to_owned(),
        step_up: StepUpEvidence {
            assertion_id: step_text("assertion_id")?,
            authenticated_at: UtcNanos::parse_rfc3339(&step_text("authenticated_at")?).ok()?,
            method: step_text("method")?,
        },
    })
}

/// The checks journal rule 57 says every `ConnectionChecked` lists; a check that omits one is
/// read as [`Unreadable`], never as passed.
const RULE_57_CHECKS: [&str; 3] = ["account", "environment", "scope"];

/// The latest `ConnectionChecked` of `connection` with occasion `connect` on `account`, read as
/// the executor writes it (rules 57, 58, 62): `account`, `environment` and `scope` listed, and
/// every result `passed` with a non-null `account_pii_ref`, or the first failed result's reason.
///
/// # Errors
/// [`Unreadable`] for a check of the connection that cannot be read so: the finish cannot answer.
pub fn checked(
    account: &[StoredEvent],
    connection: &ConnectionId,
) -> Result<Option<Checked>, Unreadable> {
    let mut latest = None;
    for row in account {
        if row.event_type != "ConnectionChecked" {
            continue;
        }
        let payload = payload(row).ok_or(Unreadable)?;
        let text = |name: &str| payload.get(name).and_then(Value::as_str);
        if text("connection_id").ok_or(Unreadable)? != connection.as_str()
            || text("occasion") != Some("connect")
        {
            continue;
        }
        let results = payload
            .get("results")
            .and_then(Value::as_array)
            .ok_or(Unreadable)?;
        let listed = |name: &str| {
            results
                .iter()
                .any(|result| result.get("check").and_then(Value::as_str) == Some(name))
        };
        if !RULE_57_CHECKS.iter().all(|name| listed(name)) {
            return Err(Unreadable);
        }
        let failed = results
            .iter()
            .find(|result| result.get("result").and_then(Value::as_str) != Some("passed"));
        latest = Some(match failed {
            None => Checked::Passed {
                event_id: row.event_id.clone(),
                pii_ref: text("account_pii_ref").ok_or(Unreadable)?.to_owned(),
            },
            Some(result) => {
                let code = result
                    .get("reason")
                    .and_then(Value::as_str)
                    .ok_or(Unreadable)?;
                Checked::Failed {
                    event_id: row.event_id.clone(),
                    reason: *REASONS
                        .iter()
                        .find(|r| r.code() == code)
                        .ok_or(Unreadable)?,
                }
            }
        });
    }
    Ok(latest)
}

/// One pass's decision for a connection, by DEC-2342's order:
/// - a request not open, or a fold that cannot answer, goes to `plan_finish` as it is;
/// - an open request that is not version-2 `oauth` waits, as it is not this finish's;
/// - a failed check refuses with its reason, caused by that check;
/// - a passed check is judged by `fingerprints`: unique establishes with
///   [`ESTABLISHED_SCOPES`], caused by that check; a mismatch or a holder refuses, uncaused;
/// - otherwise, from [`CONNECT_DEADLINE_SECS`] after the request, the deadline refuses as
///   `timeout`, or as `restart_past_deadline` on a pass after a restart; before it, it waits.
///
/// # Errors
/// `plan_finish`'s.
pub async fn decide(
    request: &Request,
    checked: Result<Option<Checked>, Unreadable>,
    fingerprints: &impl Fingerprints,
    now: UtcNanos,
    restarted: bool,
) -> Result<Pass, ConnectError> {
    let RequestFold::Open(members) = &request.fold else {
        return finished(
            &request.fold,
            &ConnectOutcome::Refused(RefusalCause::Timeout),
            None,
        );
    };
    if !request.oauth {
        return Ok(Pass::Wait);
    }
    let Ok(checked) = checked else {
        return finished(
            &RequestFold::CannotAnswer,
            &ConnectOutcome::Refused(RefusalCause::Timeout),
            None,
        );
    };
    let past_deadline = request
        .opened_at
        .is_some_and(|at| now.secs() >= at.secs().saturating_add(CONNECT_DEADLINE_SECS));
    let deadline = || {
        if past_deadline {
            let cause = if restarted {
                RefusalCause::RestartPastDeadline
            } else {
                RefusalCause::Timeout
            };
            finished(&request.fold, &ConnectOutcome::Refused(cause), None)
        } else {
            Ok(Pass::Wait)
        }
    };
    match checked {
        None => deadline(),
        Some(Checked::Failed { event_id, reason }) => finished(
            &request.fold,
            &ConnectOutcome::Refused(RefusalCause::Check(reason)),
            Some(event_id),
        ),
        Some(Checked::Passed { event_id, pii_ref }) => {
            match fingerprints.judge(members, &pii_ref).await {
                Judged::Unique => {
                    let scopes: BTreeSet<String> = check_scope(ESTABLISHED_SCOPES)?
                        .iter()
                        .map(str::to_owned)
                        .collect();
                    finished(
                        &request.fold,
                        &ConnectOutcome::Passed { scopes },
                        Some(event_id),
                    )
                }
                Judged::AccountMismatch => finished(
                    &request.fold,
                    &ConnectOutcome::Refused(RefusalCause::AccountMismatch),
                    None,
                ),
                Judged::AlreadyConnected(holder) => finished(
                    &request.fold,
                    &ConnectOutcome::Refused(RefusalCause::AlreadyConnected(holder)),
                    None,
                ),
                Judged::CannotAnswer => deadline(),
            }
        }
    }
}

fn finished(
    fold: &RequestFold,
    outcome: &ConnectOutcome,
    causation: Option<String>,
) -> Result<Pass, ConnectError> {
    Ok(Pass::Finish {
        plan: plan_finish(fold, outcome)?,
        causation,
    })
}
