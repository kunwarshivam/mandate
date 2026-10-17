//! The control stream's `MemberActivated`, journal spec v0.25 §9.12 (DEC-648), closed at schema
//! version 1 with the consistency rules that read it: 96, 98 to 100, 103, 105, and 106, each
//! refusing at its member, in rule order (DEC-991 item 1). §3's client actor (rule 83) is
//! [`crate::workspace`]'s and refuses a client before the payload is read. Every rule only refuses
//! a draft.
//!
//! The other six `Member*` types stay unregistered, so `append` refuses them as
//! `unknown_event_type` until E9-7 registers them; the founding bootstrap writes only this one.
//! The cross-record checks (the fold's states, the order rule) are `mandate-identity`'s, not
//! `append`'s (§9.12).

use mandate_canon::Value;
use mandate_time::UtcNanos;

use crate::schema::Ty;
use crate::{Invalid, InvalidReason, StreamType};

const ACTIVATED: &str = "MemberActivated";

/// The control-stream event types §9.12 adds to [`crate::control::governs`] so far.
pub(crate) const CONTROL: [&str; 1] = [ACTIVATED];

/// §9.12's `role` type: the workspace roles of identity spec §4.1.
const ROLES: &[&str] = &[
    "approver",
    "auditor",
    "operator",
    "viewer",
    "workspace_admin",
];

/// The roles whose grant may cool off (rule 103).
const COOLING: [&str; 2] = ["approver", "operator"];

const FOUNDING: &str = "founding";

/// A cool-off's length when one is owed (identity spec §8.3).
const DAY_SECS: i64 = 86_400;

/// The §9.12 schema of `event_type` at `schema_version` on `stream`, or `None` when this module
/// registers none there.
pub(crate) fn schema(
    event_type: &str,
    schema_version: u64,
    stream: StreamType,
) -> Option<&'static Ty> {
    match (event_type, schema_version, stream) {
        (ACTIVATED, 1, StreamType::Control) => Some(&MEMBER_ACTIVATED),
        _ => None,
    }
}

static MEMBER_ACTIVATED: Ty = Ty::Record(&[
    ("member", Ty::Ulid),
    ("invitation", Ty::Nullable(&Ty::Ulid)),
    ("reason", Ty::OneOf(&["invitation_accepted", FOUNDING])),
    ("roles", Ty::List(&Ty::OneOf(ROLES))),
    ("method", Ty::OneOf(&["passkey", "oidc", "email_link"])),
    ("activated_at", Ty::Timestamp),
    ("independent_approval_required", Ty::Bool),
    ("cool_off_ends_at", Ty::Timestamp),
    ("session_ref", Ty::Nullable(&Ty::Str)),
]);

/// Rules 96 to 106 on a `MemberActivated` its schema has normalized, the first that fails reported.
/// `actor` and `event_time` are the envelope's.
pub(crate) fn rules(
    event_type: &str,
    payload: &Value,
    actor: Option<&Value>,
    event_time: Option<&Value>,
) -> Result<(), Invalid> {
    if event_type != ACTIVATED {
        return Ok(());
    }
    let roles: Vec<&str> = payload
        .get("roles")
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let ascending = roles
        .windows(2)
        .all(|w| matches!(w, [a, b] if a.as_bytes() < b.as_bytes()));
    if !ascending {
        return Err(Invalid::new(InvalidReason::NonCanonical, "payload.roles"));
    }
    let founding = text(Some(payload), "reason") == FOUNDING;
    let writer = text(actor, "kind");
    ensure(
        writer == if founding { "system" } else { "user" },
        "actor.kind",
    )?;
    let invited = payload.get("invitation").is_some_and(|i| *i != Value::Null);
    ensure(invited != founding, "payload.invitation")?;
    ensure(
        founding || text(Some(payload), "member") == text(actor, "id"),
        "payload.member",
    )?;
    ensure(!roles.is_empty(), "payload.roles")?;
    ensure(
        !founding || roles.contains(&"workspace_admin"),
        "payload.roles",
    )?;
    let at = event_time
        .and_then(Value::as_str)
        .and_then(|t| UtcNanos::parse(t).ok());
    let independent = payload.get("independent_approval_required") == Some(&Value::Bool(true));
    let cools = !founding && independent && roles.iter().any(|r| COOLING.contains(r));
    let owed = if cools {
        at.and_then(|t| UtcNanos::from_parts(t.secs().checked_add(DAY_SECS)?, t.nanos()).ok())
    } else {
        at
    };
    ensure(
        owed.is_some() && owed == instant(payload, "cool_off_ends_at"),
        "payload.cool_off_ends_at",
    )?;
    let sessioned = payload
        .get("session_ref")
        .is_some_and(|s| *s != Value::Null);
    ensure(sessioned == (writer == "user"), "payload.session_ref")?;
    ensure(
        at.is_some() && at == instant(payload, "activated_at"),
        "payload.activated_at",
    )
}

fn instant(payload: &Value, member: &str) -> Option<UtcNanos> {
    UtcNanos::parse(text(Some(payload), member)).ok()
}

fn text<'a>(value: Option<&'a Value>, member: &str) -> &'a str {
    value
        .and_then(|v| v.get(member))
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn ensure(holds: bool, path: &str) -> Result<(), Invalid> {
    if holds {
        Ok(())
    } else {
        Err(Invalid::new(InvalidReason::Schema, path))
    }
}
