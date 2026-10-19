//! The connections read's fold (§4.5, [DEC-2620](../../../docs/project/decisions/DEC-2620.md)),
//! from the control stream's rows alone (API-14). It serves references only (API-11): no member
//! is a credential, the code, the `state`, a vault path, an account number, or the fingerprint,
//! and the journaled `account_ref`, `user`, and `step_up` are left out too (DEC-2620 item 4).
//!
//! Pure: rows in, values out.

use mandate_journal::StoredEvent;

/// Where a connection stands on the control stream (DEC-2620 item 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Connecting,
    Established,
    Refused,
    Revoked,
}

impl ConnectionState {
    /// The wire name `read-models/connection` gives it.
    pub fn code(self) -> &'static str {
        match self {
            Self::Connecting => "connecting",
            Self::Established => "established",
            Self::Refused => "refused",
            Self::Revoked => "revoked",
        }
    }
}

/// A connect's `ConnectionRefused`: the first §8.1 check that failed (`None` for a teardown no
/// check caused) and its reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub check: Option<String>,
    pub reason: String,
}

/// One connection, as `read-models/connection` states it (DEC-2620 item 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionView {
    pub connection_id: String,
    pub broker: String,
    pub environment: String,
    pub state: ConnectionState,
    /// The latest establishment's granted scopes; `None` before one.
    pub scopes: Option<Vec<String>>,
    /// The `recorded_at` of the record that set `state`.
    pub since: String,
    /// Set exactly when `state` is [`ConnectionState::Refused`].
    pub refusal: Option<Refusal>,
    /// Version 2's `reason` when `state` is [`ConnectionState::Revoked`]; `None` otherwise, and
    /// for a version-1 revocation, which names none.
    pub revoked_reason: Option<String>,
    /// The agents whose latest `AgentDeployed` is version 2, names this connection, and is not
    /// stopped, in first-deployment order.
    pub agents: Vec<String>,
}

/// Every connection the control stream names, in the order its first record names it.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn connections(_control: &[StoredEvent]) -> Vec<ConnectionView> {
    todo!()
}
