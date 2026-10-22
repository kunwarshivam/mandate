//! The role provisioner's records, journal spec v0.77 §9.33 (DEC-2350): `ConnectionRolesMade`,
//! `ConnectionRolesRefused` and `ConnectionRolesRemoved` on the control stream, and the stream
//! rules 262 and 263 that tie each to its request.
//!
//! Rules 262 and 263 are the provisioner's: it checks them against its own fold before it appends,
//! and the verifier's fold, [`verify_connection_roles`], fails closed on them. `append` folds no
//! stream, so it never judges them.

use crate::StoredEvent;

/// §9.33's stream rules, by the number the spec gives each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionRolesRule {
    /// Rule 262: a `ConnectionRolesMade` or `ConnectionRolesRefused` follows the version-2 OAuth
    /// `ConnectionRequested` its `causation_id` names, of its connection, once per request.
    Answered,
    /// Rule 263: a `ConnectionRolesRemoved` follows its connection's latest roles made, once, and
    /// names that request's own closer or a revocation after them.
    Removed,
}

impl ConnectionRolesRule {
    /// The rule's number in §9.33.
    #[must_use]
    pub fn number(self) -> u16 {
        match self {
            Self::Answered => 262,
            Self::Removed => 263,
        }
    }
}

/// Why [`verify_connection_roles`] refused the rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionRolesError {
    /// The first row, by its index in the rows given, that breaks `rule`.
    Broken {
        index: usize,
        rule: ConnectionRolesRule,
    },
    /// The report of a fold E21-33 has not implemented yet (DEC-77); the implementation removes it.
    Unimplemented { story: &'static str },
}

/// Rules 262 and 263 over a control stream's rows, in commit order from its first event: the
/// first §9.33 record that breaks either, by its index in `rows`. Rows of other event types are
/// folded for the requests, closers and revocations they hold, and a body that does not parse is
/// skipped, since §11's per-event check reports it.
///
/// # Errors
/// [`ConnectionRolesError::Broken`] at the first row that breaks rule 262 or 263.
pub fn verify_connection_roles(rows: &[StoredEvent]) -> Result<(), ConnectionRolesError> {
    let _ = rows;
    Err(ConnectionRolesError::Unimplemented { story: "E21-33" })
}
