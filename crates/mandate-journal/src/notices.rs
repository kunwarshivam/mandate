//! The alert and notice records journal spec v0.40 §9.15 closes (DEC-720, DEC-795, and, for
//! `NoticeAttempted`'s version 2 and rule 134, DEC-706): the subject stream owner's
//! `OwnerAlertSent` on the agent, account, and control streams, and the dispatcher's
//! `StreamOpened`, `NoticeIssued`, and `NoticeAttempted` on the notice stream `ntf:{workspace_id}`,
//! with the `notice_id`, `opaque`, and `kind` types, consistency rules 119 to 129, the batch rule
//! 130, and rule 134 at version 2. `append` admits `NoticeAttempted` only at version 2 (a stored
//! version-1 record still verifies and still replays, §8), so no writer leaves the verdict out by
//! writing the older shape. None of the four holds free text (`AGENTS.md` rule 6). Every rule only
//! refuses a draft.

use std::collections::BTreeSet;

use mandate_canon::Value;

use crate::StreamType::{Account, Agent, Control};
use crate::control::Envelope;
use crate::schema::Ty;
use crate::{Draft, Invalid, InvalidReason, StreamId, StreamType};

const ALERT: &str = "OwnerAlertSent";
const OPENED: &str = "StreamOpened";
const ISSUED: &str = "NoticeIssued";
const ATTEMPTED: &str = "NoticeAttempted";
/// §9.15's control-stream record of a member's web-push address, added or removed (v0.51,
/// DEC-2505 item 1).
const CHANGED: &str = "NotificationAddressChanged";

/// Notifications spec §3.2's `action` kinds.
const ACTION: [&str; 2] = ["approval_requested", "approval_reminder"];

/// Notifications spec §3.2's `safety` kinds, in its order (DEC-795 item 6 adds
/// `notification_address_changed`, DEC-2351 `kill_switch_unapplied`, DEC-2352 `retirement_held`,
/// DEC-2520 `connection_unavailable`, DEC-2855 `protection_unattributed` and `attribution_deficit`).
const SAFETY: [&str; 30] = [
    "risk_limit",
    "kill_switch",
    "kill_switch_unapplied",
    "agent_held",
    "retirement_held",
    "account_restriction",
    "connection_unavailable",
    "protection",
    "protection_unattributed",
    "attribution_deficit",
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

/// Each class with its kinds: the `kind` type's 38 values, and rule 122's table.
const CLASSES: [(&str, &[&str]); 3] = [("action", &ACTION), ("safety", &SAFETY), ("info", &INFO)];

/// The kinds no alert causes (notifications spec §3.4).
const NOT_ALERTED: [&str; 3] = ["approval_requested", "approval_reminder", "channel_lost"];

/// The two reasons that stop an `abandoned` attempt (rule 127).
const STOPS: [&str; 3] = ["not_pending", "retry_window_ended", "address_retired"];

/// The receipts, which name the delivered message's provider id (rule 128).
const RECEIPTS: [&str; 3] = ["bounced", "complained", "unsubscribed"];

/// The freshness window rule 212 shares with rule 102 (identity spec §7.2): an assertion
/// authenticated this many seconds before the envelope's `event_time`, no more.
const STEP_UP_WINDOW_SECS: i64 = 300;

/// The statuses of an attempt that sent nothing (rule 129).
const QUIET: [&str; 2] = ["suppressed_quiet_hours", "deferred_quiet_hours"];

/// Rule 134's fixed verdicts: each `failed` reason notifications spec §5.2 to §5.4 give one
/// verdict (DEC-706 item 4). `provider_error` and `too_large` take either, as accepted decisions
/// split them, and an `abandoned` attempt's two stops carry none.
const FIXED: [(&str, &str); 9] = [
    ("timeout", "retryable"),
    ("rate_limited", "retryable"),
    ("address_rejected", "permanent"),
    ("auth_failed", "permanent"),
    ("recipient_not_permitted", "permanent"),
    ("address_missing", "permanent"),
    ("bounced", "permanent"),
    ("complained", "permanent"),
    ("unsubscribed", "permanent"),
];

/// Whether §9.15 governs `event_type` on `stream`: every record of the notice stream, and the alert
/// on the three streams the catalogue admits it on.
pub(crate) fn governs(stream: &StreamId, event_type: &str) -> bool {
    stream.stream_type() == StreamType::Notice || event_type == ALERT || event_type == CHANGED
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
        (ATTEMPTED, 3, _) => Some(&NOTICE_ATTEMPTED_V3),
        (CHANGED, 1, StreamType::Control) => Some(&NOTIFICATION_ADDRESS_CHANGED),
        _ => None,
    }
}

/// Rules 119, 120, 122 to 124, 126 to 129, 134 at version 2, and §9.15's 210 to 216 on a payload
/// its schema has normalized, in rule order: the first that fails is reported. `envelope` carries
/// the `causation_id`, `actor`, `event_time`, `pii_refs` and `event_id` those rules read.
pub(crate) fn rules(
    event_type: &str,
    schema_version: u64,
    payload: &Value,
    envelope: Envelope<'_>,
) -> Result<(), Invalid> {
    let Envelope {
        actor,
        causation_id,
        pii_refs,
        event_time,
        event_id,
        environment,
        ..
    } = envelope;
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
            )?;
            let verdict = text("verdict");
            ensure(verdict.is_some() == (status == "failed"), "payload.verdict")?;
            let fixed = FIXED
                .iter()
                .find(|(fixed_reason, _)| Some(*fixed_reason) == reason)
                .map(|(_, fixed_verdict)| *fixed_verdict);
            ensure(
                verdict.is_none_or(|v| fixed.is_none_or(|f| f == v)),
                "payload.verdict",
            )?;
            if schema_version >= 3 {
                let address = text("address_ref");
                let web_push = text("channel") == Some("web_push");
                ensure(address.is_some() == web_push, "payload.address_ref")?;
                ensure(
                    match address {
                        Some(address) => only_pii(pii_refs, address),
                        None => pii_refs
                            .is_some_and(|refs| refs.as_array().is_some_and(<[Value]>::is_empty)),
                    },
                    "pii_refs",
                )?;
                ensure(
                    reason != Some("address_retired") || web_push,
                    "payload.reason",
                )
            } else {
                Ok(())
            }
        }
        CHANGED => {
            let action = text("action").unwrap_or_default();
            let address = text("address_ref").unwrap_or_default();
            let own = event_id.and_then(Value::as_str);
            ensure(
                (action == "added" && own == Some(address))
                    || (action == "removed" && own != Some(address)),
                "payload.address_ref",
            )?;
            ensure(only_pii(pii_refs, address), "pii_refs")?;
            let stepped = payload.get("step_up").is_some_and(|s| *s != Value::Null);
            ensure(action != "added" || stepped, "payload.step_up")?;
            ensure(actor_is_user(actor), "actor.kind")?;
            if stepped {
                let method = payload
                    .get("step_up")
                    .and_then(|s| s.get("method"))
                    .and_then(Value::as_str);
                let live = environment.and_then(Value::as_str) == Some("live");
                ensure(
                    method == Some("passkey") || (method == Some("cli_confirm") && !live),
                    "payload.step_up.method",
                )?;
                fresh_step_up(payload, event_time)?;
            }
            if action == "added" || stepped {
                ensure(
                    actor.and_then(|a| a.get("kind")).and_then(Value::as_str) == Some("user"),
                    "actor.kind",
                )?;
                ensure(
                    actor.and_then(|a| a.get("id")).and_then(Value::as_str) == text("member"),
                    "payload.member",
                )?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// The step-up window rule 212 shares with rule 102: an assertion authenticated 0 to 300 seconds
/// before the envelope's `event_time`. The method is judged first, as rule 102 reports it.
fn fresh_step_up(payload: &Value, event_time: Option<&Value>) -> Result<(), Invalid> {
    let step_up = payload.get("step_up");
    let method = step_up
        .and_then(|s| s.get("method"))
        .and_then(Value::as_str);
    ensure(
        method == Some("passkey") || method == Some("cli_confirm"),
        "payload.step_up.method",
    )?;
    let at = step_up
        .and_then(|s| s.get("authenticated_at"))
        .and_then(Value::as_str)
        .and_then(|t| mandate_time::UtcNanos::parse(t).ok());
    let event = event_time
        .and_then(Value::as_str)
        .and_then(|t| mandate_time::UtcNanos::parse(t).ok());
    let fresh = match (at, event) {
        (Some(at), Some(event)) => {
            let earliest = mandate_time::UtcNanos::from_parts(
                event.secs().saturating_sub(STEP_UP_WINDOW_SECS),
                event.nanos(),
            );
            at <= event && earliest.is_ok_and(|earliest| earliest <= at)
        }
        _ => false,
    };
    ensure(fresh, "payload.step_up.authenticated_at")
}

/// Whether a record's actor is a `user` or a `system`: the two a member's own address change admits.
/// A deactivation's `removed` rides a `system` actor's record, and a `client` never writes one
/// (rule 83 refuses it before the payload is read).
fn actor_is_user(actor: Option<&Value>) -> bool {
    matches!(
        actor.and_then(|a| a.get("kind")).and_then(Value::as_str),
        Some("user" | "system")
    )
}

/// Whether `pii_refs` is exactly the one entry `address`, which rules 211 and 215 require.
fn only_pii(pii_refs: Option<&Value>, address: &str) -> bool {
    pii_refs.is_some_and(|refs| {
        refs.as_array().is_some_and(|list| {
            list.len() == 1 && list.first().and_then(Value::as_str) == Some(address)
        })
    })
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
    address_change_batch(drafts)
}

/// Rule 214 over one batch (v0.51, DEC-2505 item 1), after rule 130:
///
/// - (a) at most one `added` and one `removed`, of one member, and every step-up the batch carries
///   is the **same** assertion — a replacement's two records are authorized by one step-up;
/// - (b) an `added` is always the subject of a `notification_address_changed` alert in the batch; a
///   `removed` is unless it rides a replacement's `added`;
/// - (c) every such alert names one of those records, and an alert of that kind with no address
///   change in its batch at all is refused at its subject;
/// - (d) no `removed` names an address twice or the batch's own `added`;
/// - (e) a `removed` with no step-up rides a deactivation, and no batch here holds one — so it is
///   refused unless it names the batch's own `added`.
///
/// Every refusal is `schema` at `payload.address_ref`, but (c)'s at `payload.subject`, which is
/// where the spec's own oracle places each.
fn address_change_batch(drafts: &[Draft]) -> Result<(), (usize, Invalid)> {
    let changes: Vec<(usize, &Draft)> = drafts
        .iter()
        .enumerate()
        .filter(|(_, d)| d.event_type() == CHANGED)
        .collect();
    let address_alerts: Vec<(usize, &Draft)> = drafts
        .iter()
        .enumerate()
        .filter(|(_, d)| {
            d.event_type() == ALERT && payload_text(d, "kind") == "notification_address_changed"
        })
        .collect();
    if changes.is_empty() {
        return match address_alerts.first() {
            Some((i, _)) => Err((*i, Invalid::new(InvalidReason::Schema, "payload.subject"))),
            None => Ok(()),
        };
    }
    let address_of = |draft: &Draft| {
        draft
            .fields()
            .get("payload")
            .and_then(|p| p.get("address_ref"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let Some((index, first)) = changes.first().copied() else {
        return Ok(());
    };
    let member = payload_text(first, "member").to_owned();
    let added: Vec<&(usize, &Draft)> = changes
        .iter()
        .filter(|(_, d)| payload_text(d, "action") == "added")
        .collect();
    let removed: Vec<&(usize, &Draft)> = changes
        .iter()
        .filter(|(_, d)| payload_text(d, "action") == "removed")
        .collect();
    if added.len() > 1 || removed.len() > 1 {
        return Err((
            index,
            Invalid::new(InvalidReason::Schema, "payload.address_ref"),
        ));
    }
    if changes
        .iter()
        .any(|(_, d)| payload_text(d, "member") != member)
    {
        return Err((
            index,
            Invalid::new(InvalidReason::Schema, "payload.address_ref"),
        ));
    }
    let replacement = !added.is_empty() && !removed.is_empty();
    let assertions: Vec<&str> = changes
        .iter()
        .filter_map(|(_, d)| {
            d.fields()
                .get("payload")
                .and_then(|p| p.get("step_up"))
                .filter(|v| **v != Value::Null)
                .and_then(|s| s.get("assertion_id"))
                .and_then(Value::as_str)
        })
        .collect();
    if assertions.windows(2).any(|w| matches!(w, [a, b] if a != b)) {
        return Err((
            index,
            Invalid::new(InvalidReason::Schema, "payload.address_ref"),
        ));
    }
    let subject_of = |draft: &Draft| alerted(draft).map(str::to_owned);
    let alerts: Vec<(usize, String)> = drafts
        .iter()
        .enumerate()
        .filter(|(_, d)| {
            d.event_type() == ALERT && payload_text(d, "kind") == "notification_address_changed"
        })
        .filter_map(|(i, d)| subject_of(d).map(|s| (i, s)))
        .collect();
    for (i, draft) in &changes {
        let named = alerts.iter().any(|(_, s)| s == draft.event_id());
        let wanted = payload_text(draft, "action") == "added" || !replacement;
        if named != wanted {
            return Err((
                *i,
                Invalid::new(InvalidReason::Schema, "payload.address_ref"),
            ));
        }
    }
    for (i, subject) in &alerts {
        if !changes.iter().any(|(_, d)| d.event_id() == subject) {
            return Err((*i, Invalid::new(InvalidReason::Schema, "payload.subject")));
        }
    }
    let added_address = added.first().map(|(_, d)| address_of(d));
    for (i, draft) in &changes {
        let unstepped = draft
            .fields()
            .get("payload")
            .and_then(|p| p.get("step_up"))
            .is_none_or(|s| *s == Value::Null);
        let rides = Some(&address_of(draft)) == added_address.as_ref();
        if payload_text(draft, "action") == "removed" && unstepped && !rides {
            return Err((
                *i,
                Invalid::new(InvalidReason::Schema, "payload.address_ref"),
            ));
        }
    }
    let mut named: BTreeSet<String> = BTreeSet::new();
    for (i, draft) in &removed {
        let address = address_of(draft);
        if Some(&address) == added_address.as_ref() || !named.insert(address) {
            return Err((
                *i,
                Invalid::new(InvalidReason::Schema, "payload.address_ref"),
            ));
        }
    }
    Ok(())
}

/// One text member of a draft's payload.
fn payload_text<'a>(draft: &'a Draft, member: &str) -> &'a str {
    draft
        .fields()
        .get("payload")
        .and_then(|p| p.get(member))
        .and_then(Value::as_str)
        .unwrap_or_default()
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

/// The `kind` type: notifications spec §3.2's 38 kinds.
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

/// §9.15's `NotificationAddressChanged` at version 1 (v0.51, DEC-2505 item 1): a member's web-push
/// address, added or removed. `address_ref` is the address's own event id on an `added` and the
/// address it removes otherwise; `pii_refs` is exactly `[address_ref]` (rules 210 and 211).
static NOTIFICATION_ADDRESS_CHANGED: Ty = Ty::Record(&[
    ("member", Ty::Ulid),
    ("channel", Ty::OneOf(&["web_push"])),
    ("action", Ty::OneOf(&["added", "removed"])),
    ("address_ref", Ty::Ulid),
    ("step_up", Ty::Nullable(&crate::credentials::STEP_UP_RECORD)),
]);

/// §9.15's `NoticeAttempted` at version 2. Not reachable from `schema` today: v0.51 refuses a
/// version-2 record at `append` and reads it as `address_ref: null`, so the type is kept for the
/// verifier's fold to read a stored record against rather than re-derived there.
#[allow(
    dead_code,
    reason = "v0.51 refuses version 2 at append but a stored version-2 record still replays (DEC-2505 item 1)"
)]
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
    (
        "verdict",
        Ty::Nullable(&Ty::OneOf(&["retryable", "permanent"])),
    ),
]);

/// §9.15's `NoticeAttempted` at version 3 (v0.51, DEC-2505 item 1): version 2's members in their
/// order, then `address_ref`, non-null exactly on web push (rule 215), plus §9.15's stop
/// `address_retired` (rule 216).
static NOTICE_ATTEMPTED_V3: Ty = Ty::Record(&[
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
            "address_retired",
            "bounced",
            "complained",
            "unsubscribed",
            "not_pending",
            "retry_window_ended",
        ])),
    ),
    ("provider_message_id", Ty::Nullable(&Ty::Text(is_opaque))),
    ("coalesced_into", Ty::Nullable(&Ty::Text(is_opaque))),
    (
        "verdict",
        Ty::Nullable(&Ty::OneOf(&["retryable", "permanent"])),
    ),
    ("address_ref", Ty::Nullable(&Ty::Ulid)),
]);
