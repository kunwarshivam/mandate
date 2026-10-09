//! The alert and notice records journal spec v0.28 §9.15 closes at schema version 1 (DEC-720,
//! DEC-795): the subject stream owner's `OwnerAlertSent` on the agent, account, and control streams,
//! and the dispatcher's `StreamOpened`, `NoticeIssued`, and `NoticeAttempted` on the notice stream
//! `ntf:{workspace_id}`, with the `notice_id`, `opaque`, and `kind` types, consistency rules 119
//! to 129, and the batch rule 130. None of the four holds free text (`AGENTS.md` rule 6). Every rule
//! only refuses a draft.

use mandate_canon::Value;

use crate::StreamType::{Account, Agent, Control};
use crate::schema::Ty;
use crate::{Draft, Invalid, InvalidReason, StreamId, StreamType};

const ALERT: &str = "OwnerAlertSent";
const OPENED: &str = "StreamOpened";
const ISSUED: &str = "NoticeIssued";
const ATTEMPTED: &str = "NoticeAttempted";

/// Notifications spec §3.2's `action` kinds.
const ACTION: [&str; 2] = ["approval_requested", "approval_reminder"];

/// Notifications spec §3.2's `safety` kinds, in its order (DEC-795 item 6 adds
/// `notification_address_changed`).
const SAFETY: [&str; 25] = [
    "risk_limit",
    "kill_switch",
    "agent_held",
    "account_restriction",
    "protection",
    "exit_stalled",
    "reconciliation",
    "external_activity",
    "account_state",
    "data_feed_down",
    "integrity_incident",
    "credential_added",
    "new_device",
    "notification_address_changed",
    "recovery_used",
    "role_granted",
    "member_deactivated",
    "deprovisioned",
    "break_glass",
    "version_risk_increasing",
    "delegation_added",
    "connection_added",
    "went_live",
    "client_connected",
    "channel_lost",
];

/// Notifications spec §3.2's `info` kinds.
const INFO: [&str; 6] = [
    "daily_brief",
    "delegation_ended",
    "model_status",
    "research_status",
    "spend_cap",
    "approval_closed",
];

/// Each class with its kinds: the `kind` type's 33 values, and rule 122's table.
const CLASSES: [(&str, &[&str]); 3] = [("action", &ACTION), ("safety", &SAFETY), ("info", &INFO)];

/// The kinds no alert causes (notifications spec §3.4).
const NOT_ALERTED: [&str; 3] = ["approval_requested", "approval_reminder", "channel_lost"];

/// The two reasons that stop an `abandoned` attempt (rule 127).
const STOPS: [&str; 2] = ["not_pending", "retry_window_ended"];

/// The receipts, which name the delivered message's provider id (rule 128).
const RECEIPTS: [&str; 3] = ["bounced", "complained", "unsubscribed"];

/// The statuses of an attempt that sent nothing (rule 129).
const QUIET: [&str; 2] = ["suppressed_quiet_hours", "deferred_quiet_hours"];

/// Whether §9.15 governs `event_type` on `stream`: every record of the notice stream, and the alert
/// on the three streams the catalogue admits it on.
pub(crate) fn governs(stream: &StreamId, event_type: &str) -> bool {
    stream.stream_type() == StreamType::Notice || event_type == ALERT
}

/// The §9.15 schema of `event_type` at `schema_version` on `stream`, or `None` when this module
/// registers none there. The notice stream's `StreamOpened` is chosen by its stream type.
pub(crate) fn schema(
    event_type: &str,
    schema_version: u64,
    stream: StreamType,
) -> Option<&'static Ty> {
    match (event_type, schema_version, stream) {
        (ALERT, 1, _) => Some(&OWNER_ALERT_SENT),
        (OPENED, 1, StreamType::Notice) => Some(&NOTICE_STREAM_OPENED),
        (ISSUED, 1, _) => Some(&NOTICE_ISSUED),
        (ATTEMPTED, 1, _) => Some(&NOTICE_ATTEMPTED),
        _ => None,
    }
}

/// Rules 119, 120, 122 to 124, and 126 to 129 on a payload its schema has normalized, in rule
/// order: the first that fails is reported. `causation_id` is the envelope's.
pub(crate) fn rules(
    event_type: &str,
    payload: &Value,
    causation_id: Option<&Value>,
) -> Result<(), Invalid> {
    let text = |member: &str| payload.get(member).and_then(Value::as_str);
    let caused = causation_id.and_then(Value::as_str);
    match event_type {
        ALERT => {
            ensure(caused == text("subject"), "causation_id")?;
            ensure(
                text("owner_command").is_none() || text("kind") == Some("kill_switch"),
                "payload.owner_command",
            )
        }
        ISSUED => {
            ensure(class(text("kind")) == text("class"), "payload.class")?;
            let recipients: Vec<&str> = payload
                .get("recipients")
                .and_then(Value::as_array)
                .unwrap_or_default()
                .iter()
                .filter_map(Value::as_str)
                .collect();
            ensure(
                recipients.windows(2).all(|w| matches!(w, [a, b] if a < b)),
                "payload.recipients",
            )?;
            ensure(caused == text("cause"), "causation_id")
        }
        ATTEMPTED => {
            let attempt = payload.get("attempt").and_then(Value::as_int);
            ensure(attempt.is_some_and(|a| a >= 1), "payload.attempt")?;
            let status = text("status").unwrap_or_default();
            let reason = text("reason");
            let stopped = reason.is_some_and(|r| STOPS.contains(&r));
            let fits = match status {
                "failed" => reason.is_some() && !stopped,
                "abandoned" => stopped,
                _ => reason.is_none(),
            };
            ensure(fits, "payload.reason")?;
            let receipt = reason.is_some_and(|r| RECEIPTS.contains(&r));
            ensure(
                text("provider_message_id").is_some() == (status == "delivered" || receipt),
                "payload.provider_message_id",
            )?;
            ensure(
                text("coalesced_into").is_none() || !QUIET.contains(&status),
                "payload.coalesced_into",
            )
        }
        _ => Ok(()),
    }
}

/// Subject rules 121 and 125 (`stream_mismatch`) on a payload that passed [`rules`], reported after
/// `artifact_refs` and `pii_refs`.
pub(crate) fn subject(event_type: &str, stream: &StreamId, payload: &Value) -> Result<(), Invalid> {
    let text = |member: &str| {
        payload
            .get(member)
            .and_then(Value::as_str)
            .unwrap_or_default()
    };
    let (holds, path) = match event_type {
        OPENED => (
            format!("ntf:{}", text("workspace_id")) == stream.as_str(),
            "stream_id",
        ),
        ISSUED => {
            let cause = text("cause_stream");
            let holds = match text("kind") {
                "approval_requested" | "approval_reminder" => in_workspace(cause, stream, &[Agent]),
                "channel_lost" => cause == stream.as_str(),
                _ => in_workspace(cause, stream, &[Agent, Account, Control]),
            };
            (holds, "payload.cause_stream")
        }
        _ => return Ok(()),
    };
    if holds {
        Ok(())
    } else {
        Err(Invalid::new(InvalidReason::StreamMismatch, path))
    }
}

/// Batch rule 130 over one `append` batch whose drafts each passed [`Draft::parse`]: an alert's
/// `subject` is an earlier draft of the batch that is not an alert, and no earlier alert names it.
pub(crate) fn check_batch(drafts: &[Draft]) -> Result<(), (usize, Invalid)> {
    for (i, draft) in drafts.iter().enumerate() {
        if draft.event_type() != ALERT {
            continue;
        }
        let named = alerted(draft);
        let mut earlier = drafts.iter().take(i);
        let bound = earlier
            .clone()
            .any(|d| d.event_type() != ALERT && Some(d.event_id()) == named);
        let repeated = earlier.any(|d| d.event_type() == ALERT && alerted(d) == named);
        if !bound || repeated {
            return Err((i, Invalid::new(InvalidReason::Schema, "payload.subject")));
        }
    }
    Ok(())
}

/// The `subject` an alert names.
fn alerted(draft: &Draft) -> Option<&str> {
    draft.fields().get("payload")?.get("subject")?.as_str()
}

/// Whether `named` is a stream of one of `types` in `stream`'s workspace.
fn in_workspace(named: &str, stream: &StreamId, types: &[StreamType]) -> bool {
    let workspace = |id: &str| id.split(':').nth(1).map(str::to_owned);
    StreamId::parse(named).is_some_and(|n| {
        types.contains(&n.stream_type()) && workspace(n.as_str()) == workspace(stream.as_str())
    })
}

/// The class of `kind`, from [`CLASSES`].
fn class(kind: Option<&str>) -> Option<&'static str> {
    let kind = kind?;
    CLASSES
        .iter()
        .find(|(_, kinds)| kinds.contains(&kind))
        .map(|(class, _)| *class)
}

/// The `kind` type: notifications spec §3.2's 33 kinds.
fn is_kind(s: &str) -> bool {
    class(Some(s)).is_some()
}

/// An alert's `kind`: any kind an alert causes.
fn is_alert_kind(s: &str) -> bool {
    is_kind(s) && !NOT_ALERTED.contains(&s)
}

/// The `notice_id` type: exactly 32 lowercase hexadecimal digits, never an event id.
fn is_notice_id(s: &str) -> bool {
    s.len() == 32
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The `opaque` type: 1 to 256 characters, each U+0021 to U+007E, so it holds no space.
fn is_opaque(s: &str) -> bool {
    (1..=256).contains(&s.len()) && s.bytes().all(|b| (0x21..=0x7e).contains(&b))
}

fn ensure(holds: bool, path: &str) -> Result<(), Invalid> {
    if holds {
        Ok(())
    } else {
        Err(Invalid::new(InvalidReason::Schema, path))
    }
}

static OWNER_ALERT_SENT: Ty = Ty::Record(&[
    ("subject", Ty::Ulid),
    ("kind", Ty::Text(is_alert_kind)),
    ("owner_command", Ty::Nullable(&Ty::Ulid)),
]);

static NOTICE_STREAM_OPENED: Ty = Ty::Record(&[
    ("stream_type", Ty::OneOf(&["notice"])),
    ("workspace_id", Ty::Ident),
]);

static NOTICE_ISSUED: Ty = Ty::Record(&[
    ("notice", Ty::Text(is_notice_id)),
    ("kind", Ty::Text(is_kind)),
    ("class", Ty::OneOf(&["action", "safety", "info"])),
    ("cause", Ty::Ulid),
    ("cause_stream", Ty::StreamName),
    ("recipients", Ty::List(&Ty::Ident)),
]);

static NOTICE_ATTEMPTED: Ty = Ty::Record(&[
    ("notice", Ty::Text(is_notice_id)),
    ("recipient", Ty::Ident),
    (
        "channel",
        Ty::OneOf(&["email", "phone", "slack", "sms", "telegram", "web_push"]),
    ),
    ("attempt", Ty::Int),
    (
        "status",
        Ty::OneOf(&[
            "delivered",
            "failed",
            "suppressed_quiet_hours",
            "deferred_quiet_hours",
            "abandoned",
        ]),
    ),
    (
        "reason",
        Ty::Nullable(&Ty::OneOf(&[
            "timeout",
            "rate_limited",
            "provider_error",
            "address_rejected",
            "auth_failed",
            "too_large",
            "recipient_not_permitted",
            "address_missing",
            "bounced",
            "complained",
            "unsubscribed",
            "not_pending",
            "retry_window_ended",
        ])),
    ),
    ("provider_message_id", Ty::Nullable(&Ty::Text(is_opaque))),
    ("coalesced_into", Ty::Nullable(&Ty::Text(is_opaque))),
]);
