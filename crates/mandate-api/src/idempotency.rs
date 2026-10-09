//! The `Idempotency-Key` and the control-stream event id derived from it (workspace API spec §3.4,
//! DEC-436 item 4): DEC-290's rule for the CLI, carried to the API.

use mandate_canon::{Digest, Int, Key, Object, Value, to_canonical};

use crate::wire::{EventId, Id, is_segment};

/// A client's key for one user gesture: 16 to 64 characters from `[A-Za-z0-9_-]`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    /// The key, or a refusal for any other length or character. Never trimmed or case-folded.
    ///
    /// # Errors
    /// [`KeyError::Malformed`].
    pub fn parse(text: &str) -> Result<Self, KeyError> {
        if is_segment(text) && (16..=64).contains(&text.len()) {
            Ok(Self(text.to_owned()))
        } else {
            Err(KeyError::Malformed)
        }
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
    let mut letters = derivation.operation.chars();
    let first = letters.next().is_some_and(|c| c.is_ascii_lowercase());
    if !(first && letters.all(|c| c.is_ascii_lowercase() || c == '_')) {
        return Err(KeyError::Operation);
    }
    let digest = hashed(derivation).ok_or(KeyError::Operation)?;
    let (head, _) = digest
        .as_bytes()
        .split_first_chunk::<16>()
        .ok_or(KeyError::Operation)?;
    Ok(EventId::of(u128::from_be_bytes(*head)))
}

/// The SHA-256 of the derivation's canonical object; `None` only if a member name broke the key
/// grammar, which none of these does.
fn hashed(derivation: &Derivation<'_>) -> Option<Digest> {
    let text = |s: &str| Some(Value::Str(s.to_owned()));
    let position = derivation.position.map(u64::from).and_then(Int::new);
    let members = [
        ("key", text(&derivation.key.0)),
        ("operation", text(derivation.operation)),
        ("position", position.map(Value::Int)),
        ("principal", text(derivation.principal.as_str())),
        ("workspace", text(derivation.workspace.as_str())),
    ];
    let object = members
        .into_iter()
        .filter_map(|(name, value)| value.map(|value| Key::new(name).ok().map(|key| (key, value))))
        .collect::<Option<Object>>()?;
    Some(Digest::of(&to_canonical(&Value::Object(object))))
}

/// Why a key was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum KeyError {
    #[error("an Idempotency-Key is 16 to 64 characters from [A-Za-z0-9_-]")]
    Malformed,
    #[error("an operation name is [a-z][a-z_]*")]
    Operation,
}
