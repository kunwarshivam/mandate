//! The `Idempotency-Key` and the control-stream event id derived from it (workspace API spec §3.4,
//! DEC-436 item 4): DEC-290's rule for the CLI, carried to the API.

use crate::Unimplemented;
use crate::wire::{EventId, Id};

/// A client's key for one user gesture: 16 to 64 characters from `[A-Za-z0-9_-]`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    /// The key, or a refusal for any other length or character. Never trimmed or case-folded.
    ///
    /// # Errors
    /// [`KeyError::Malformed`].
    pub fn parse(text: &str) -> Result<Self, KeyError> {
        let _ = text;
        Err(KeyError::Unimplemented(Unimplemented))
    }
}

/// What one event's id is derived from: the caller's workspace and principal, the operation's
/// name (the route table's, `[a-z][a-z_]*`, so it needs no escaping in the hashed object), the key, and, for a call that commits several events (§5.3, §5.6),
/// the event's position in its batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Derivation<'a> {
    pub workspace: &'a Id,
    pub principal: &'a Id,
    pub operation: &'a str,
    pub key: &'a IdempotencyKey,
    /// `None` for a call that commits one event; `Some(n)` for the event at position `n`, from 0,
    /// of a batch.
    pub position: Option<u32>,
}

/// The event id: a ULID whose 128 bits are the first 128 bits of the SHA-256 of the canonical
/// JSON object (journal spec §4) `{"key", "operation", "position", "principal", "workspace"}`,
/// `position` present only for a batch (DEC-681 item 5). The ULID's time component carries no
/// meaning (journal spec §3).
///
/// # Errors
/// [`KeyError::Operation`] for an operation name outside `[a-z][a-z_]*`.
pub fn event_id(derivation: &Derivation<'_>) -> Result<EventId, KeyError> {
    let _ = derivation;
    Err(KeyError::Unimplemented(Unimplemented))
}

/// Why a key was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum KeyError {
    #[error(transparent)]
    Unimplemented(Unimplemented),
    #[error("an Idempotency-Key is 16 to 64 characters from [A-Za-z0-9_-]")]
    Malformed,
    #[error("an operation name is [a-z][a-z_]*")]
    Operation,
}
