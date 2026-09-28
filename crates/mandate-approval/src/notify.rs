//! The whole of what leaves the workspace deployment for an approval (`AGENTS.md` rule 6, EI-9).
//! There is no field for anything else, which holds the rule at rung 1.

use std::fmt;

use mandate_canon::{Key, Object, Value};

use crate::ApprovalError;
use crate::admit::Request;

/// An `ApprovalRequested` event id: an opaque ULID. It is built only from text in the journal's
/// ULID shape (26 characters of uppercase Crockford base32, the first at most `7`, journal spec
/// §3) and displays only that text, so no symbol, price, account, or other free text can reach a
/// payload through it (`AGENTS.md` rule 6 at rung 1; DEC-165 item 13).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ApprovalRef(String);

impl ApprovalRef {
    /// # Errors
    /// [`ApprovalError::NotAnEventId`] for any text that is not ULID-shaped.
    pub fn of_requested_event(event_id: &str) -> Result<Self, ApprovalError> {
        if is_ulid(event_id) {
            Ok(Self(event_id.to_owned()))
        } else {
            Err(ApprovalError::NotAnEventId)
        }
    }
}

/// The journal's event-id shape. `mandate-journal` holds the same check at layer 2, which this
/// layer-1 crate cannot reach.
fn is_ulid(text: &str) -> bool {
    const CROCKFORD: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    text.len() == 26
        && text.bytes().all(|b| CROCKFORD.contains(&b))
        && text.bytes().next().is_some_and(|b| b <= b'7')
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
/// Never: the notification is built from the request's id alone.
pub fn notification_for(request: &Request) -> Result<Notification, ApprovalError> {
    Ok(Notification {
        subject: request.id.clone(),
        text: GenericText::ApprovalNeeded,
    })
}

/// The payload a channel sends: `{"subject": <id>, "text": <key>}` and nothing else.
///
/// # Errors
/// Never in practice: both keys are fixed and valid.
pub fn notification_payload(notification: &Notification) -> Result<Value, ApprovalError> {
    let key = |k| Key::new(k).map_err(|_| ApprovalError::Unrepresentable { what: "key" });
    let text = match notification.text {
        GenericText::ApprovalNeeded => "approval_needed",
    };
    let mut members = Object::new();
    members.insert(key("subject")?, Value::Str(notification.subject.0.clone()));
    members.insert(key("text")?, Value::Str(text.to_owned()));
    Ok(Value::Object(members))
}
