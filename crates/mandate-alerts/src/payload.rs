//! The alert and the bytes that leave the cell for it (OPS-10): an alert id minted from a secure
//! random source, the reason code, the severity, the runbook, and the kind's fixed text. Nothing
//! else has a field.

use mandate_canon::{Key, Object, Value, to_canonical};
use mandate_notify::{NoticeId, SecureRandom};

use crate::{AlertError, AlertKind};

/// An alert as it is raised: its id and its kind, and no field for its cause. Where it came from
/// (the record, the stream) stays in the cell, keyed by the id, for the runbook to look up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Alert {
    id: NoticeId,
    kind: AlertKind,
}

impl Alert {
    /// Raises `kind` under a fresh id. The id comes only from `random`, never from an event id,
    /// so no record's id or creation time reaches a pager (NT-4, DEC-702).
    ///
    /// # Errors
    /// [`AlertError::Entropy`] when the source cannot fill an id; no alert is raised under a
    /// predictable one.
    pub fn raise(kind: AlertKind, random: &mut dyn SecureRandom) -> Result<Self, AlertError> {
        let _ = (kind, random, NoticeId::mint);
        Err(AlertError::Unimplemented { story: "E21-4" })
    }

    pub fn kind(&self) -> AlertKind {
        self.kind
    }

    /// The id as 32 lowercase hex digits.
    ///
    /// # Errors
    /// Never in practice; carried from the notice id's own encoder.
    pub fn id(&self) -> Result<String, AlertError> {
        self.id.hex().map_err(|_| AlertError::Unrepresentable)
    }
}

/// The payload: `{"alert", "reason", "runbook", "severity", "text"}` and nothing else.
///
/// # Errors
/// [`AlertError::Unrepresentable`] never in practice: every key is fixed and valid.
pub fn payload(alert: &Alert) -> Result<Value, AlertError> {
    let _ = (alert, Key::new, Object::new);
    Err(AlertError::Unimplemented { story: "E21-4" })
}

/// The payload's canonical bytes: everything a sink is handed.
///
/// # Errors
/// As [`payload`].
pub fn rendered(alert: &Alert) -> Result<Vec<u8>, AlertError> {
    let _ = (alert, to_canonical);
    Err(AlertError::Unimplemented { story: "E21-4" })
}

/// Where alerts go. No paging vendor is chosen (DEC-434 item 20, H6), so the only sink here is
/// [`CaptureSink`]; the cell's alert path (E21-39) adds its own.
pub trait Sink {
    /// Hands one alert's bytes on.
    ///
    /// # Errors
    /// [`AlertError::Undelivered`] when the sink cannot take it.
    fn page(&mut self, bytes: &[u8]) -> Result<(), AlertError>;
}

/// A sink that keeps every payload it is handed, for tests and for a cell with no paging tool.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CaptureSink {
    pub captured: Vec<Vec<u8>>,
}

impl Sink for CaptureSink {
    fn page(&mut self, bytes: &[u8]) -> Result<(), AlertError> {
        self.captured.push(bytes.to_vec());
        Ok(())
    }
}

/// Raises `kind` and hands its payload to `sink`, returning the alert so the caller can key its
/// cause by the id.
///
/// # Errors
/// [`AlertError::Entropy`] from [`Alert::raise`], or the sink's own.
pub fn send(
    kind: AlertKind,
    random: &mut dyn SecureRandom,
    sink: &mut dyn Sink,
) -> Result<Alert, AlertError> {
    let _ = (kind, random, sink);
    Err(AlertError::Unimplemented { story: "E21-4" })
}
