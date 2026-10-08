//! The alert and notice records DEC-720 closes (E8-9): `OwnerAlertSent`
//! on the agent, account, and control streams, and the notice stream's `StreamOpened`,
//! `NoticeIssued`, and `NoticeAttempted`, with consistency rules N1, N2, N4 to N6, and N8 to N11,
//! subject rules N3 and N7, and the batch rule N12. Every rule only refuses a draft. No member is
//! free text, so nothing about an order, a position, or a mandate reaches the notice stream
//! (`AGENTS.md` rule 6).

use mandate_canon::Value;

use crate::schema::Ty;
use crate::{Draft, Invalid, InvalidReason, StreamId, StreamType};

const ALERT: &str = "OwnerAlertSent";

/// Notifications spec §3.2's kinds. The first three are never caused by an alert (§3.4), so
/// `OwnerAlertSent` takes the rest.
const KINDS: [&str; 32] = [
    "approval_requested",
    "approval_reminder",
    "channel_lost",
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
    "daily_brief",
    "delegation_ended",
    "model_status",
    "research_status",
    "spend_cap",
    "approval_closed",
];

/// The class notifications spec §3.2 gives `kind` (rule N4).
fn class(kind: &str) -> &'static str {
    match kind {
        "approval_requested" | "approval_reminder" => "action",
        "daily_brief" | "delegation_ended" | "model_status" | "research_status" | "spend_cap"
        | "approval_closed" => "info",
        _ => "safety",
    }
}

/// The reasons an attempt stops without a provider failure (notifications spec §5.3): an `action`
/// notice's approval is no longer pending, or a `safety` or `info` notice's retry window ended.
const STOPS: [&str; 2] = ["not_pending", "retry_window_ended"];

/// The receipts that name the provider message they report on (§5.2, §5.6, §5.7).
const RECEIPTS: [&str; 3] = ["bounced", "complained", "unsubscribed"];

static OWNER_ALERT_SENT: Ty = Ty::Record(&[
    ("subject", Ty::Ulid),
    ("kind", Ty::OneOf(KINDS.split_at(3).1)),
    ("owner_command", Ty::Nullable(&Ty::Ulid)),
]);

static STREAM_OPENED: Ty = Ty::Record(&[
    ("stream_type", Ty::OneOf(&["notice"])),
    ("workspace_id", Ty::Ident),
]);

static NOTICE_ISSUED: Ty = Ty::Record(&[
    ("notice", Ty::NoticeId),
    ("kind", Ty::OneOf(&KINDS)),
    ("class", Ty::OneOf(&["action", "safety", "info"])),
    ("cause", Ty::Ulid),
    ("cause_stream", Ty::StreamId),
    ("recipients", Ty::List(&Ty::Ident)),
]);

static NOTICE_ATTEMPTED: Ty = Ty::Record(&[
    ("notice", Ty::NoticeId),
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
            "bounced",
            "complained",
            "unsubscribed",
            "not_pending",
            "retry_window_ended",
        ])),
    ),
    ("provider_message_id", Ty::Nullable(&Ty::Opaque)),
    ("coalesced_into", Ty::Nullable(&Ty::Opaque)),
]);

/// Whether DEC-720 governs `event_type` on `stream`.
pub(crate) fn governs(stream: &StreamId, event_type: &str) -> bool {
    match stream.stream_type() {
        StreamType::Account | StreamType::Agent | StreamType::Control => event_type == ALERT,
        StreamType::Notice => true,
        StreamType::Scheduler => false,
    }
}

/// The payload normalized against its DEC-720 schema, then rules N1, N2, N4 to N6, and N8 to N11 in
/// number order (only on a well-typed payload).
pub(crate) fn payload(
    event_type: &str,
    schema_version: u64,
    payload: &Value,
    causation_id: Option<&Value>,
) -> Result<Value, Invalid> {
    let schema = match (event_type, schema_version) {
        (ALERT, 1) => &OWNER_ALERT_SENT,
        ("StreamOpened", 1) => &STREAM_OPENED,
        ("NoticeIssued", 1) => &NOTICE_ISSUED,
        ("NoticeAttempted", 1) => &NOTICE_ATTEMPTED,
        _ => return Err(Invalid::new(InvalidReason::UnknownSchema, "payload")),
    };
    let payload = crate::schema::normalize(schema, payload, "payload")?;
    let text = |member: &str| payload.get(member).and_then(Value::as_str);
    let names_cause = |member: &str| causation_id.and_then(Value::as_str) == text(member);
    match event_type {
        ALERT => {
            ensure(names_cause("subject"), "causation_id")?;
            ensure(
                text("owner_command").is_none() || text("kind") == Some("kill_switch"),
                "payload.owner_command",
            )?;
        }
        "NoticeIssued" => {
            ensure(text("kind").map(class) == text("class"), "payload.class")?;
            let recipients: Vec<&str> = payload
                .get("recipients")
                .and_then(Value::as_array)
                .map(|items| items.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            ensure(
                recipients.windows(2).all(|w| matches!(w, [a, b] if a < b)),
                "payload.recipients",
            )?;
            ensure(names_cause("cause"), "causation_id")?;
        }
        "NoticeAttempted" => {
            let attempt = payload.get("attempt").and_then(Value::as_int);
            ensure(attempt.is_some_and(|a| a >= 1), "payload.attempt")?;
            let status = text("status").unwrap_or_default();
            let reason = text("reason");
            let reason_fits = match (status, reason) {
                ("failed", r) => r.is_some_and(|r| !STOPS.contains(&r)),
                ("abandoned", r) => r.is_some_and(|r| STOPS.contains(&r)),
                (_, r) => r.is_none(),
            };
            ensure(reason_fits, "payload.reason")?;
            let named = status == "delivered" || reason.is_some_and(|r| RECEIPTS.contains(&r));
            ensure(
                text("provider_message_id").is_some() == named,
                "payload.provider_message_id",
            )?;
            ensure(
                text("coalesced_into").is_none()
                    || !matches!(status, "suppressed_quiet_hours" | "deferred_quiet_hours"),
                "payload.coalesced_into",
            )?;
        }
        _ => {}
    }
    Ok(payload)
}

/// Subject rules N3 and N7 (`stream_mismatch`) on a payload that passed [`payload`], reported
/// after `artifact_refs` and `pii_refs`.
pub(crate) fn subject(event_type: &str, stream: &StreamId, payload: &Value) -> Result<(), Invalid> {
    let text = |member: &str| {
        payload
            .get(member)
            .and_then(Value::as_str)
            .unwrap_or_default()
    };
    let workspace = |id: &str| id.split(':').nth(1).map(str::to_owned);
    match event_type {
        "StreamOpened" => mismatch(
            format!("ntf:{}", text("workspace_id")) == stream.as_str(),
            "stream_id",
        ),
        "NoticeIssued" => {
            let cause = StreamId::parse(text("cause_stream"));
            let allowed = match text("kind") {
                "approval_requested" | "approval_reminder" => cause
                    .as_ref()
                    .is_some_and(|c| c.stream_type() == StreamType::Agent),
                "channel_lost" => cause.as_ref() == Some(stream),
                _ => cause.as_ref().is_some_and(|c| {
                    matches!(
                        c.stream_type(),
                        StreamType::Agent | StreamType::Account | StreamType::Control
                    )
                }),
            };
            let same_workspace = workspace(text("cause_stream")) == workspace(stream.as_str());
            mismatch(allowed && same_workspace, "payload.cause_stream")
        }
        _ => Ok(()),
    }
}

/// Rule N12 over one `append` batch whose drafts each passed [`Draft::parse`]: an `OwnerAlertSent`
/// names an earlier draft of the batch that is not an alert, and no two alerts name one subject.
/// Reported at the first alert that breaks it.
pub(crate) fn check_batch(drafts: &[Draft]) -> Result<(), (usize, Invalid)> {
    let subject = |draft: &Draft| {
        draft
            .fields()
            .get("payload")
            .and_then(|p| p.get("subject"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    for (i, draft) in drafts.iter().enumerate() {
        if draft.event_type() != ALERT {
            continue;
        }
        let named = subject(draft);
        let earlier = drafts.get(..i).unwrap_or_default();
        let found = earlier
            .iter()
            .any(|d| d.event_type() != ALERT && Some(d.event_id()) == named.as_deref());
        let repeated = earlier
            .iter()
            .any(|d| d.event_type() == ALERT && subject(d) == named);
        if !found || repeated {
            return Err((i, Invalid::new(InvalidReason::Schema, "payload.subject")));
        }
    }
    Ok(())
}

fn ensure(holds: bool, path: &str) -> Result<(), Invalid> {
    if holds {
        Ok(())
    } else {
        Err(Invalid::new(InvalidReason::Schema, path))
    }
}

fn mismatch(holds: bool, path: &str) -> Result<(), Invalid> {
    if holds {
        Ok(())
    } else {
        Err(Invalid::new(InvalidReason::StreamMismatch, path))
    }
}
