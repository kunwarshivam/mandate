//! The owner's acknowledgment and its request journal spec v0.61 §9.28 closes at schema version 1
//! (DEC-2509; E8-3's acknowledgment follow-up): `OwnerAcknowledgmentRequested` and
//! `OwnerAcknowledged`, on the workspace control stream and as the executor's account-stream copies,
//! each the original's payload unchanged, with consistency rule 239. `append` sees one record, so
//! stream rules 240 and 241 are the writers' and the executor's, not this module's.
//!
//! Version 2 (v0.102, DEC-2824; v0.104, DEC-2830) is version 1's members and `subject`, after
//! `event`: `limit`, `restriction` or `reconciliation`, what `event` names. Rule 239 reads it as it
//! reads version 1.

use mandate_canon::Value;

use crate::schema::Ty;
use crate::{Invalid, InvalidReason, StreamType};

const REQUESTED: &str = "OwnerAcknowledgmentRequested";
const ACKNOWLEDGED: &str = "OwnerAcknowledged";

/// The event types §9.28 adds to [`crate::control::governs`], on the control and account streams.
pub(crate) const RECORDS: [&str; 2] = [REQUESTED, ACKNOWLEDGED];

/// The §9.28 schema of `event_type` at `schema_version` on `stream`, or `None` when this module
/// registers none there.
pub(crate) fn schema(
    event_type: &str,
    schema_version: u64,
    stream: StreamType,
) -> Option<&'static Ty> {
    if !matches!(stream, StreamType::Control | StreamType::Account) {
        return None;
    }
    match (event_type, schema_version) {
        (REQUESTED, 1) => Some(&OWNER_ACKNOWLEDGMENT_REQUESTED),
        (ACKNOWLEDGED, 1) => Some(&OWNER_ACKNOWLEDGED),
        (REQUESTED, 2) => Some(&OWNER_ACKNOWLEDGMENT_REQUESTED_V2),
        (ACKNOWLEDGED, 2) => Some(&OWNER_ACKNOWLEDGED_V2),
        _ => None,
    }
}

/// Rule 239 on a payload its schema has normalized, reported once at the first offending member:
/// a request requires an independent user and carries step-up evidence; an acknowledgment that
/// requires independence names a request and a user other than its requester, and one that does
/// not names no request and its own user as requester.
pub(crate) fn rules(event_type: &str, payload: &Value) -> Result<(), Invalid> {
    let null = |member: &str| matches!(payload.get(member), None | Some(Value::Null));
    let independent = payload.get("independent_required") == Some(&Value::Bool(true));
    match event_type {
        REQUESTED => {
            ensure(independent, "payload.independent_required")?;
            ensure(!null("step_up"), "payload.step_up")
        }
        ACKNOWLEDGED => {
            ensure(null("request") != independent, "payload.request")?;
            let own = payload.get("requested_by") == payload.get("user");
            ensure(own != independent, "payload.requested_by")
        }
        _ => Ok(()),
    }
}

fn ensure(holds: bool, path: &str) -> Result<(), Invalid> {
    if holds {
        Ok(())
    } else {
        Err(Invalid::new(InvalidReason::Schema, path))
    }
}

static OWNER_ACKNOWLEDGMENT_REQUESTED: Ty = Ty::Record(&[
    ("agent", Ty::Ident),
    ("event", Ty::Ulid),
    ("requested_by", Ty::Ident),
    ("independent_required", Ty::Bool),
    ("submitted_at", Ty::Int),
    ("risk_clock", Ty::RiskClock),
    ("step_up", Ty::Nullable(&crate::control::ANSWER_STEP_UP)),
]);

static OWNER_ACKNOWLEDGED: Ty = Ty::Record(&[
    ("agent", Ty::Ident),
    ("event", Ty::Ulid),
    ("user", Ty::Ident),
    ("requested_by", Ty::Ident),
    ("independent_required", Ty::Bool),
    ("request", Ty::Nullable(&Ty::Ulid)),
    ("submitted_at", Ty::Int),
    ("risk_clock", Ty::RiskClock),
    ("step_up", Ty::Nullable(&crate::control::ANSWER_STEP_UP)),
]);

/// What a version-2 acknowledgment or request names by its `event` (v0.102, v0.104).
const SUBJECTS: &[&str] = &["limit", "restriction", "reconciliation"];

static OWNER_ACKNOWLEDGMENT_REQUESTED_V2: Ty = Ty::Record(&[
    ("agent", Ty::Ident),
    ("event", Ty::Ulid),
    ("subject", Ty::OneOf(SUBJECTS)),
    ("requested_by", Ty::Ident),
    ("independent_required", Ty::Bool),
    ("submitted_at", Ty::Int),
    ("risk_clock", Ty::RiskClock),
    ("step_up", Ty::Nullable(&crate::control::ANSWER_STEP_UP)),
]);

static OWNER_ACKNOWLEDGED_V2: Ty = Ty::Record(&[
    ("agent", Ty::Ident),
    ("event", Ty::Ulid),
    ("subject", Ty::OneOf(SUBJECTS)),
    ("user", Ty::Ident),
    ("requested_by", Ty::Ident),
    ("independent_required", Ty::Bool),
    ("request", Ty::Nullable(&Ty::Ulid)),
    ("submitted_at", Ty::Int),
    ("risk_clock", Ty::RiskClock),
    ("step_up", Ty::Nullable(&crate::control::ANSWER_STEP_UP)),
]);
