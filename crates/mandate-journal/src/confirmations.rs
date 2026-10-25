//! The CLI's step-up confirmation journal spec v0.82 §9.29 closes at schema version 1 (DEC-2104,
//! DEC-2125; E9-4 S2): `StepUpConfirmed`, on the workspace control stream, the record that the
//! owner confirmed on the host one step-up challenge the workspace API issued for a paper grant or a
//! paper OAuth connect's start made in the web app. Rule 242 refuses `schema` at its member: the
//! kind is one of [`KINDS`], the envelope is `paper`, the writer is a `user` whose `id` is the
//! record's `principal`, and `confirmed_at` is the envelope's `event_time` in whole seconds. Stream
//! rule 243 (one confirmation per challenge) is the CLI's own and is not judged here; the API's
//! verifier counts the confirmations itself.

use mandate_canon::Value;
use mandate_time::UtcNanos;

use crate::schema::Ty;
use crate::{Invalid, InvalidReason, StreamType};

const CONFIRMED: &str = "StepUpConfirmed";

/// The control-stream event types §9.29 adds to [`crate::control::governs`].
pub(crate) const CONTROL: [&str; 1] = [CONFIRMED];

/// The action kinds a confirmation may name (rule 242): a grant's `approve`, and a paper OAuth
/// connect's `connection` (DEC-2125 item 2).
const KINDS: [&str; 2] = ["approve", "connection"];

/// How many characters a code has (DEC-2104 item 1).
const CODE_LEN: usize = 8;

/// §9.29's `confirm_code`: Crockford base 32, upper case, no hyphen.
fn is_confirm_code(text: &str) -> bool {
    text.len() == CODE_LEN
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b.is_ascii_uppercase() && !b"ILOU".contains(&b)))
}

/// The §9.29 schema of `event_type` at `schema_version` on `stream`, or `None` when this module
/// registers none there.
pub(crate) fn schema(
    event_type: &str,
    schema_version: u64,
    stream: StreamType,
) -> Option<&'static Ty> {
    (event_type == CONFIRMED && schema_version == 1 && stream == StreamType::Control)
        .then_some(&STEP_UP_CONFIRMED)
}

/// Rule 242 on a payload its schema has normalized, in the spec's order: the first clause that
/// fails is reported. `environment`, `actor` and `event_time` are the envelope's.
pub(crate) fn rules(
    event_type: &str,
    payload: &Value,
    environment: Option<&Value>,
    actor: Option<&Value>,
    event_time: Option<&Value>,
) -> Result<(), Invalid> {
    if event_type != CONFIRMED {
        return Ok(());
    }
    ensure(
        KINDS.contains(&text(Some(payload), "action_kind")),
        "payload.action_kind",
    )?;
    ensure(
        environment.and_then(Value::as_str) == Some("paper"),
        "environment",
    )?;
    ensure(text(actor, "kind") == "user", "actor.kind")?;
    ensure(
        text(actor, "id") == text(Some(payload), "principal"),
        "actor.id",
    )?;
    let second = event_time
        .and_then(Value::as_str)
        .and_then(|t| UtcNanos::parse(t).ok())
        .and_then(|t| u64::try_from(t.secs()).ok());
    let confirmed = payload.get("confirmed_at").and_then(Value::as_int);
    ensure(
        second.is_some() && second == confirmed,
        "payload.confirmed_at",
    )
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

static STEP_UP_CONFIRMED: Ty = Ty::Record(&[
    ("challenge", Ty::Ulid),
    ("action_kind", Ty::Str),
    ("action_digest", Ty::DigestRef),
    ("principal", Ty::Ulid),
    ("code", Ty::Text(is_confirm_code)),
    ("confirmed_at", Ty::Int),
]);
