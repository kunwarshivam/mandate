//! The deployment manager's records journal spec v0.48 §9.21 closes at schema version 1
//! (DEC-1170): the starts, stops, and crash loops of the processes it runs, on the workspace
//! control stream, with consistency rules 165 to 168, each refusing at its member. No member is
//! free text: IDs, closed enumerations, and integers only. Workspace control services write them as
//! a `system` actor (rule 165). Every rule only refuses a draft, and none compares one record with
//! another: a slot's generations are the writer's to order at this version.

use mandate_canon::Value;

use crate::schema::Ty;
use crate::{Invalid, InvalidReason, StreamType};

const STARTED: &str = "ProcessStartIssued";
const STOPPED: &str = "ProcessStopIssued";
const LOOPED: &str = "ProcessCrashLooped";

/// The control-stream event types §9.21 adds to [`crate::control::governs`].
pub(crate) const CONTROL: [&str; 3] = [STARTED, STOPPED, LOOPED];

const PROCESSES: &[&str] = &["runtime", "executor"];

/// The §9.21 schema of `event_type` at `schema_version` on `stream`, or `None` when this module
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
        STARTED => Some(&PROCESS_START_ISSUED),
        STOPPED => Some(&PROCESS_STOP_ISSUED),
        LOOPED => Some(&PROCESS_CRASH_LOOPED),
        _ => None,
    }
}

/// Rules 165 to 168 on a payload its schema has normalized, in rule order: the first that fails
/// is reported. `actor` is the envelope's.
pub(crate) fn rules(
    event_type: &str,
    payload: &Value,
    actor: Option<&Value>,
) -> Result<(), Invalid> {
    if !CONTROL.contains(&event_type) {
        return Ok(());
    }
    let kind = actor
        .and_then(|a| a.get("kind"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    ensure(kind == "system", "actor.kind")?;
    let int = |member: &str| payload.get(member).and_then(Value::as_int).unwrap_or(0);
    let generation = int("generation");
    ensure(generation >= 1, "payload.generation")?;
    if event_type == STARTED {
        let deployed = payload.get("cause").and_then(Value::as_str) == Some("deployed");
        ensure(deployed == (generation == 1), "payload.cause")?;
    }
    if event_type == LOOPED {
        ensure(int("restarts") >= 1, "payload.restarts")?;
        ensure(int("window_seconds") >= 1, "payload.window_seconds")?;
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

static PROCESS_START_ISSUED: Ty = Ty::Record(&[
    ("process", Ty::OneOf(PROCESSES)),
    ("subject", Ty::Ident),
    ("generation", Ty::Int),
    ("cause", Ty::OneOf(&["deployed", "restart", "kill_switch"])),
]);

static PROCESS_STOP_ISSUED: Ty = Ty::Record(&[
    ("process", Ty::OneOf(PROCESSES)),
    ("subject", Ty::Ident),
    ("generation", Ty::Int),
    ("cause", Ty::OneOf(&["agent_stopped"])),
]);

static PROCESS_CRASH_LOOPED: Ty = Ty::Record(&[
    ("process", Ty::OneOf(PROCESSES)),
    ("subject", Ty::Ident),
    ("generation", Ty::Int),
    ("restarts", Ty::Int),
    ("window_seconds", Ty::Int),
]);
