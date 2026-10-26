//! The customer real-money gate's records, which journal spec v0.85 §9.35 closes at schema version 1
//! ([DEC-2134](../../../docs/project/decisions/DEC-2134.md)): `RealMoneyGateChanged`, a platform
//! operator's setting of whether the workspace may reach an environment other than `paper`, and
//! `RealMoneyRefused`, the workspace API's record of a request it refused while that setting was
//! off. Rule 269 refuses `schema` at its member: the setting's actor is a `platform_operator` whose
//! `id` is the record's `operator`, and an on setting names its decision. Rule 270: the refusal's
//! actor is a `user` whose `id` is its `principal`.

use mandate_canon::Value;

use crate::schema::Ty;
use crate::{Invalid, InvalidReason, StreamType};

const CHANGED: &str = "RealMoneyGateChanged";
const REFUSED: &str = "RealMoneyRefused";

/// The control-stream event types §9.35 adds to [`crate::control::governs`].
pub(crate) const CONTROL: [&str; 2] = [CHANGED, REFUSED];

/// How many digits a `decision_id`'s number may have (§9.35).
const DECISION_DIGITS: usize = 5;

/// §9.35's `decision_id`: `DEC-` then a decimal number with no leading zero, at most five digits.
fn is_decision_id(text: &str) -> bool {
    text.strip_prefix("DEC-").is_some_and(|number| {
        (1..=DECISION_DIGITS).contains(&number.len())
            && number.bytes().all(|b| b.is_ascii_digit())
            && !number.starts_with('0')
    })
}

/// The §9.35 schema of `event_type` at `schema_version` on `stream`, or `None` when this module
/// registers none there.
pub(crate) fn schema(
    event_type: &str,
    schema_version: u64,
    stream: StreamType,
) -> Option<&'static Ty> {
    if schema_version != 1 || stream != StreamType::Control {
        return None;
    }
    match event_type {
        CHANGED => Some(&REAL_MONEY_GATE_CHANGED),
        REFUSED => Some(&REAL_MONEY_REFUSED),
        _ => None,
    }
}

/// Rules 269 and 270 on a payload its schema has normalized, in the spec's order: the first clause
/// that fails is reported. `actor` is the envelope's.
pub(crate) fn rules(event_type: &str, payload: &Value, actor: Option<&Value>) -> Result<(), Invalid> {
    let (kind, member) = match event_type {
        CHANGED => ("platform_operator", "operator"),
        REFUSED => ("user", "principal"),
        _ => return Ok(()),
    };
    ensure(text(actor, "kind") == kind, "actor.kind")?;
    ensure(text(actor, "id") == text(Some(payload), member), "actor.id")?;
    if event_type == CHANGED {
        let enabled = payload.get("enabled") == Some(&Value::Bool(true));
        let decided = payload.get("decision").is_some_and(|d| !matches!(d, Value::Null));
        ensure(!enabled || decided, "payload.decision")?;
    }
    Ok(())
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

static REAL_MONEY_GATE_CHANGED: Ty = Ty::Record(&[
    ("enabled", Ty::Bool),
    ("operator", Ty::Ulid),
    ("decision", Ty::Nullable(&Ty::Text(is_decision_id))),
]);

static REAL_MONEY_REFUSED: Ty = Ty::Record(&[
    (
        "action",
        Ty::OneOf(&["create_version", "deploy", "connect", "go_live"]),
    ),
    ("requested", Ty::OneOf(&["live"])),
    ("principal", Ty::Ulid),
    ("reason", Ty::OneOf(&["gate_off"])),
]);
