//! The owner's acknowledgments, copied from the control stream into the account stream and judged
//! (journal spec §9.28, DEC-2828; E7-4 slice 5). Each copy is the original's payload unchanged,
//! caused by it, and an acknowledgment copy that fails a check is followed, as the very next record
//! of its batch, by `OwnerCommandRefused` with `command` `acknowledge`. A request copy is never
//! judged by itself.

use crate::batch::Batch;
use crate::error::ExecutorError;
use crate::types::AcknowledgmentHandoff;

/// Copies one acknowledgment or request and judges it (§9.28 checks 1 to 4).
pub(crate) fn copy(
    _batch: &mut Batch<'_, '_>,
    _handoff: &AcknowledgmentHandoff,
) -> Result<(), ExecutorError> {
    Err(ExecutorError::Unimplemented { story: "E7-4" })
}
