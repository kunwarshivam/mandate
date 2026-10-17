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
///   `initiator` is not the owner: a risk limit or the platform operator, or none named.
/// - `VerificationRun` with `result` `fail` or `incomplete`.
/// - `AnchorComputed` whose `token` is null: an anchor or timestamping gap.
/// - `ClockToleranceExceeded`, whatever it holds.
/// - `ApprovalDelivered` with `status` `failed`.
/// - `OwnerAlertSent` with `kind` `data_feed_down` or `spend_cap`.
///
/// # Errors
/// [`AlertError::Unreadable`] for a body that does not parse or holds no payload object, and for
/// a `ReconciliationRun`, `ProtectionChanged`, `VerificationRun`, `ApprovalDelivered`, or
/// `OwnerAlertSent` whose discriminating member is absent or not text, so a dropped member never
/// silences an alert. The caller treats that as the journal's own integrity question.
pub fn from_event(row: &StoredEvent) -> Result<Option<AlertKind>, AlertError> {
    let body = mandate_canon::parse(&row.body).map_err(|_| AlertError::Unreadable)?;
    let payload = body
        .get("payload")
        .filter(|p| p.as_object().is_some())
        .ok_or(AlertError::Unreadable)?;
    let text = |member: &str| payload.get(member).and_then(Value::as_str);
    let required = |member: &str| text(member).ok_or(AlertError::Unreadable);
    let kind = match row.event_type.as_str() {
        "ReconciliationRun" => {
            (required("result")? == "mismatch").then_some(AlertKind::ReconciliationMismatch)
        }
        "ProtectionChanged" => match required("action")? {
            "interval_limit" => Some(AlertKind::UnprotectedIntervalLimit),
            "watchdog" => Some(AlertKind::WatchdogFired),
            _ => None,
        },
        "GateDecided" => (text("reason_code") == Some("protection_unattributed"))
            .then_some(AlertKind::ProtectionUnattributed),
        "KillSwitchActivated" => {
            let automated = text("initiator") != Some("owner");
            if automated && text("scope") == Some("workspace") {
                Some(AlertKind::KillSwitchAutomatedWorkspace)
            } else {
                Some(AlertKind::KillSwitchActivated)
            }
        }
        "VerificationRun" => match required("result")? {
            "fail" => Some(AlertKind::VerificationFailed),
            "incomplete" => Some(AlertKind::VerificationIncomplete),
            _ => None,
        },
        "AnchorComputed" => {
            matches!(payload.get("token"), None | Some(Value::Null)).then_some(AlertKind::AnchorGap)
        }
        "ClockToleranceExceeded" => Some(AlertKind::ClockOutOfTolerance),
        "ApprovalDelivered" => {
            (required("status")? == "failed").then_some(AlertKind::ApprovalNotDelivered)
        }
        "OwnerAlertSent" => match required("kind")? {
            "data_feed_down" => Some(AlertKind::MarketDataStale),
            "spend_cap" => Some(AlertKind::ModelSpendCap),
            _ => None,
        },
        _ => None,
    };
    Ok(kind)
}
