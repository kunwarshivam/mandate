//! A connection's stream rules (journal spec v0.20 §9.8 rules 66 to 68) and §11's two connection
//! checks, `connection_lifecycle_mismatch` and `connection_cause_mismatch` (E7-17, DEC-800 item 9).
//! `append` folds no stream, so these rules are not `append`'s: the connection manager and the
//! executor check them against their own fold before appending, and verification runs them here
//! over the stored rows.
//!
//! Both functions take the rows of the control stream and of the account streams together, in
//! commit order, from each stream's first event: rules 66 and 67 fold the control stream, rule 68
//! folds each account stream on its own, and the cause check follows a `causation_id` from one
//! stream to the other. Rows of other event types are skipped, and the first failing row is
//! reported by its index in `rows`.

use crate::StoredEvent;

/// A §9.8 stream rule: 66 (establishment), 67 (rotation and refusal), 68 (the account stream).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ConnectionStreamRule {
    /// A connection is established again only after its revocation, for the same broker,
    /// environment, and `account_ref`, and one `account_ref` belongs to one connection.
    Established,
    /// A rotation or a refused `reauthorize` names a live connection, a refused `reconnect` a
    /// revoked one, a refused `connect` a new id; and a rotation never widens the scopes.
    Rotated,
    /// One account stream, one connection: its binding, contract, rotations, and state changes.
    AccountStream,
}

impl ConnectionStreamRule {
    /// The rule's number in journal spec §9.8.
    pub fn number(self) -> u8 {
        match self {
            Self::Established => 66,
            Self::Rotated => 67,
            Self::AccountStream => 68,
        }
    }
}

/// §11's connection checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionCheck {
    /// A record breaks a stream rule.
    LifecycleMismatch(ConnectionStreamRule),
    /// A version-2 establishment's or a rotation's check is not the passing check of its occasion
    /// on the bound stream for the same connection, or a copy differs from its original.
    CauseMismatch,
}

impl ConnectionCheck {
    /// The check's code as §11 writes it.
    pub fn code(self) -> &'static str {
        match self {
            Self::LifecycleMismatch(_) => "connection_lifecycle_mismatch",
            Self::CauseMismatch => "connection_cause_mismatch",
        }
    }
}

/// The first row, by its index in the rows given, that fails a connection check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectionFailure {
    pub index: usize,
    pub check: ConnectionCheck,
}

/// Why a connection check did not pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionVerifyError {
    Mismatch(ConnectionFailure),
    /// The check is not built yet (DEC-77).
    Unimplemented {
        story: &'static str,
    },
}

/// §11's `connection_lifecycle_mismatch`: the first row that breaks rule 66, 67, or 68.
pub fn verify_connection_lifecycle(rows: &[StoredEvent]) -> Result<(), ConnectionVerifyError> {
    let _ = rows;
    Err(ConnectionVerifyError::Unimplemented { story: "E7-17" })
}

/// §11's `connection_cause_mismatch`: the first version-2 `ConnectionEstablished` or
/// `ConnectionCredentialRotated` on the control stream whose `causation_id` is not the passing
/// `ConnectionChecked` of its occasion (`connect`, `reconnect` for a second establishment, or
/// `reauthorize`) on the stream its `account_ref` names, for the same connection, listing
/// `contract` for an MCP broker; or the first copy on an account stream whose `causation_id`
/// names no control-stream original of its type, or whose payload without `risk_clock` differs
/// from that original's.
pub fn verify_connection_causes(rows: &[StoredEvent]) -> Result<(), ConnectionVerifyError> {
    let _ = rows;
    Err(ConnectionVerifyError::Unimplemented { story: "E7-17" })
}
