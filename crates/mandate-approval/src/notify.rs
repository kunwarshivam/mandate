//! The whole of what leaves the workspace deployment for an approval (`AGENTS.md` rule 6, EI-9).
//! There is no field for anything else, which holds the rule at rung 1.

use std::fmt;

use mandate_canon::Value;

use crate::ApprovalError;
use crate::admit::Request;

/// An `ApprovalRequested` event id: an opaque ULID. It is built only from that id and displays
/// only that id.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ApprovalRef(String);

impl ApprovalRef {
    pub fn of_requested_event(event_id: &str) -> Self {
        Self(event_id.to_owned())
    }
}

impl fmt::Display for ApprovalRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A closed set of generic texts. v0 has one; risk and mode alerts keep the runtime's own path
/// until E8-4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenericText {
    /// "An agent in your workspace needs your approval".
    ApprovalNeeded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub subject: ApprovalRef,
    pub text: GenericText,
}

/// # Errors
/// [`ApprovalError::Unimplemented`] until E8-1.
pub fn notification_for(request: &Request) -> Result<Notification, ApprovalError> {
    let _ = request;
    Err(ApprovalError::Unimplemented { story: "E8-1" })
}

/// The payload a channel sends: `{"subject": <id>, "text": <key>}` and nothing else.
///
/// # Errors
/// [`ApprovalError::Unimplemented`] until E8-1.
pub fn notification_payload(notification: &Notification) -> Result<Value, ApprovalError> {
    let _ = notification;
    Err(ApprovalError::Unimplemented { story: "E8-1" })
}
