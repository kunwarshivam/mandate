//! The connect's finish ([DEC-2342](../../../docs/project/decisions/DEC-2342.md); connections
//! spec §5.2 steps 5 and 6; journal spec §9.8): from the control stream's open request and the
//! account stream's `ConnectionChecked`, what `mandate_connections::manager::plan_finish` is asked,
//! and what it answers. It decides only; the pass that appends and tears down is the server's.

use mandate_connections::ConnectError;
use mandate_connections::checks::Reason;
use mandate_connections::manager::{FinishPlan, RequestFold, RequestMembers};
use mandate_connections::record::ConnectionId;
use mandate_journal::StoredEvent;
use mandate_time::UtcNanos;

/// How long a connect may stay open from its request's `recorded_at`: the code's ten minutes and
/// one more (connections spec §5.2 step 6, DEC-2342 item 2).
pub const CONNECT_DEADLINE_SECS: i64 = 660;

/// The scopes an establishment records: the scope check passes on exactly these (DEC-2342 item 1).
pub const ESTABLISHED_SCOPES: &str = "data trading";

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

/// Where the finish has checks 3 and 4 judged: the account behind `account_pii_ref` against the
/// request's connection and every established one.
pub trait Fingerprints {
    fn judge(&self, request: &RequestMembers, account_pii_ref: &str) -> Judged;
}

/// The port while no fingerprint backing exists (DEC-2342 item 7): it answers nothing, so no
/// connect is established and each reaches its deadline's teardown.
#[derive(Debug, Clone, Copy, Default)]
pub struct Unbacked;

impl Fingerprints for Unbacked {
    fn judge(&self, _request: &RequestMembers, _account_pii_ref: &str) -> Judged {
        Judged::CannotAnswer
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
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn request(_control: &[StoredEvent], _connection: &ConnectionId) -> Request {
    todo!()
}

/// The latest `ConnectionChecked` of `connection` with occasion `connect` on `account`, read as
/// the executor writes it (rules 57, 58, 62): every result `passed` with a non-null
/// `account_pii_ref`, or the first failed result's reason.
///
/// # Errors
/// [`Unreadable`] for a check of the connection that cannot be read so: the finish cannot answer.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn checked(
    _account: &[StoredEvent],
    _connection: &ConnectionId,
) -> Result<Option<Checked>, Unreadable> {
    todo!()
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
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn decide(
    _request: &Request,
    _checked: Result<Option<Checked>, Unreadable>,
    _fingerprints: &impl Fingerprints,
    _now: UtcNanos,
    _restarted: bool,
) -> Result<Pass, ConnectError> {
    todo!()
}
