//! The §8.2 rows whose source is a journal record, read from the stored record's payload. The
//! journal is authoritative (design §8.2), so these are the alerts a replay raises again.

use mandate_canon::Value;
use mandate_journal::StoredEvent;

use crate::{AlertError, AlertKind};

/// The alert a stored record raises, if any:
///
/// - `ReconciliationRun` with `result` `mismatch`: reconciliation mismatch.
/// - `ProtectionChanged` with `action` `interval_limit`: the unprotected interval reached its
///   limit; with `action` `watchdog`: the triggered-stop watchdog fired.
/// - `GateDecided` with `reason_code` `protection_unattributed`: openings held.
/// - `KillSwitchActivated`: informational, or SEV-1 when its `scope` is `workspace` and its
///   `initiator` is not the owner.
/// - `VerificationRun` with `result` `fail` or `incomplete`.
/// - `AnchorComputed` whose `token` is null: an anchor or timestamping gap.
/// - `ClockToleranceExceeded`, whatever it holds.
/// - `ApprovalDelivered` with `status` `failed`.
/// - `OwnerAlertSent` with `kind` `data_feed_down` or `spend_cap`.
///
/// # Errors
/// [`AlertError::Unreadable`] for a body that does not parse or holds no payload object. The
/// caller treats that as the journal's own integrity question, not as no alert.
pub fn from_event(row: &StoredEvent) -> Result<Option<AlertKind>, AlertError> {
    let _ = (row, Value::Null, AlertKind::AnchorGap, mandate_canon::parse);
    Err(AlertError::Unimplemented { story: "E21-4" })
}
