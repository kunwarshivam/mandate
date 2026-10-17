//! The reason codes this crate's own callers share. Each is fixed text, checked by
//! [`crate::lint::reasons`].

use crate::ReasonCode;

/// The call ran to its end.
pub const COMPLETED: ReasonCode = ReasonCode::fixed("completed");
/// The call returned an error.
pub const FAILED: ReasonCode = ReasonCode::fixed("failed");
/// A record's labels were not its metric's declared keys.
pub const LABELS_NOT_DECLARED: ReasonCode = ReasonCode::fixed("labels_not_declared");
/// A record was offered to a metric of another kind.
pub const KIND_MISMATCH: ReasonCode = ReasonCode::fixed("kind_mismatch");

/// Every reason code above.
pub const ALL: [ReasonCode; 4] = [COMPLETED, FAILED, LABELS_NOT_DECLARED, KIND_MISMATCH];
