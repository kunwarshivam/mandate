//! What the provisioner has left to do, from the control stream alone: the requests whose roles it
//! has not answered, and the made roles whose request has closed or whose connection was revoked
//! since, with no removal yet. It reads §9.33's rules 262 and 263 the way
//! [`mandate_journal::verify_connection_roles`] does, so every step it plans journals a record that
//! fold accepts.

use mandate_journal::StoredEvent;

/// Why a removal is owed: the closer of the request the roles were made for, or a revocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Occasion {
    RequestClosed,
    Revoked,
}

impl Occasion {
    /// The occasion as `ConnectionRolesRemoved` journals it.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::RequestClosed => "request_closed",
            Self::Revoked => "revoked",
        }
    }
}

/// One step the provisioner owes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Make `connection`'s roles for the OAuth request `request`, by its `event_id`.
    Make { connection: String, request: String },
    /// Remove `connection`'s token exchange's role, for the closer or revocation `cause`.
    Remove {
        connection: String,
        cause: String,
        occasion: Occasion,
    },
}

/// The stream's environment, from its `StreamOpened`, which every record repeats, and the steps
/// owed, in the order the stream gives them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    pub environment: Option<String>,
    pub steps: Vec<Step>,
}

/// Why no plan was made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    /// The report of a plan E21-33 has not implemented yet (DEC-77); the implementation removes
    /// it.
    Unimplemented { story: &'static str },
}

/// The steps owed over the control stream's `rows`, in commit order from its first event. A body
/// that does not parse is skipped, as the verifier's fold skips it.
///
/// # Errors
/// None once implemented.
pub fn plan(rows: &[StoredEvent]) -> Result<Plan, PlanError> {
    let _ = rows;
    Err(PlanError::Unimplemented { story: "E21-33" })
}
