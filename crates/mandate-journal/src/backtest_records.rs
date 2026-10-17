//! The backtest and evaluation records journal spec v0.48 §9.18 closes at schema version 1
//! (DEC-1001, DEC-1002; E4-5, E17-8): `BacktestRunRecorded`, one backtest of a mandate version
//! through its own decision path, and `EvaluationRunRecorded`, one forward-paper evaluation, both
//! on the workspace control stream, with consistency rules 143 to 145, each refusing `schema` at
//! its member. Each names its report by artifact and the digests that reproduce it; no figure is
//! inline, and a backtest's `marker` is always `backtest`. An evaluation's `theses_head` holds bare
//! record hashes, `digest` and not `ref` (DEC-1142): they name no stored artifact, so they are not
//! listed in `artifact_refs` and §11 check 6 never reads them.
//!
//! **The writer** (DEC-1141). §9.18 names no actor. Workspace services, the control stream's
//! single writer, append both, and the envelope's `actor` is the `user` who asked or the `system`
//! runner or evaluator that ran it, the precedent §9.13 sets for a record a principal asks for and
//! a process runs. Any other kind (`agent`, `broker`, `platform_operator`) is refused `schema` at
//! `actor.kind`, after rules 143 to 145; a `client` is refused first by rule 83. The reading only
//! tightens: no fold reads either record, and it keeps the research agent from journaling a
//! backtest or an evaluation in its own name.
//!
//! The required `config_refs` (`rule_set` and `mandate_version` for a backtest, `rule_set` for an
//! evaluation) are the catalogue's, checked before the payload, so rule 144 always has a version
//! to compare. Every rule only refuses a draft, and none compares one record with another.

use mandate_canon::Value;

use crate::schema::Ty;
use crate::{Invalid, InvalidReason, StreamType};

const BACKTEST: &str = "BacktestRunRecorded";
const EVALUATION: &str = "EvaluationRunRecorded";

/// The control-stream event types §9.18 adds to [`crate::control::governs`].
pub(crate) const CONTROL: [&str; 2] = [BACKTEST, EVALUATION];

/// The actor kinds that may write either record (DEC-1141).
const WRITERS: [&str; 2] = ["user", "system"];

/// The §9.18 schema of `event_type` at `schema_version` on `stream`, or `None` when this module
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
        BACKTEST => Some(&BACKTEST_RUN_RECORDED),
        EVALUATION => Some(&EVALUATION_RUN_RECORDED),
        _ => None,
    }
}

/// Rules 143 to 145, then DEC-1141's writer, on a payload its schema has normalized: the first
/// that fails is reported. `config_refs` and `actor` are the envelope's.
pub(crate) fn rules(
    event_type: &str,
    payload: &Value,
    config_refs: Option<&Value>,
    actor: Option<&Value>,
) -> Result<(), Invalid> {
    if event_type == BACKTEST {
        let refused = text(Some(payload), "result") == "refused";
        ensure(is_null(payload, "report") == refused, "payload.report")?;
        ensure(is_null(payload, "refusal") != refused, "payload.refusal")?;
        let bound = config_refs
            .and_then(|refs| refs.get("mandate_version"))
            .and_then(Value::as_str);
        ensure(
            bound == Some(text(Some(payload), "mandate_version")),
            "payload.mandate_version",
        )?;
    } else if event_type == EVALUATION {
        let from = count(payload, "theses_from");
        ensure(from > 0, "payload.theses_from")?;
        ensure(
            count(payload, "theses_to") == from && count(payload, "theses_head") == from,
            "payload.theses_from",
        )?;
    } else {
        return Ok(());
    }
    ensure(WRITERS.contains(&text(actor, "kind")), "actor.kind")
}

fn text<'a>(value: Option<&'a Value>, member: &str) -> &'a str {
    value
        .and_then(|v| v.get(member))
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn is_null(value: &Value, member: &str) -> bool {
    value.get(member).is_none_or(|v| *v == Value::Null)
}

fn count(value: &Value, member: &str) -> usize {
    value
        .get(member)
        .and_then(Value::as_array)
        .unwrap_or_default()
        .len()
}

fn ensure(holds: bool, path: &str) -> Result<(), Invalid> {
    if holds {
        Ok(())
    } else {
        Err(Invalid::new(InvalidReason::Schema, path))
    }
}

static BACKTEST_RUN_RECORDED: Ty = Ty::Record(&[
    ("backtest_id", Ty::Ident),
    ("mandate_version", Ty::DigestRef),
    ("data_snapshot", Ty::DigestRef),
    ("model_outputs", Ty::DigestRef),
    ("theses", Ty::DigestRef),
    ("configuration", Ty::DigestRef),
    ("code_build", Ty::DigestRef),
    ("report", Ty::Nullable(&Ty::DigestRef)),
    ("report_version", Ty::Int),
    ("marker", Ty::OneOf(&["backtest"])),
    ("result", Ty::OneOf(&["completed", "refused"])),
    ("refusal", Ty::Nullable(&Ty::Str)),
]);

static EVALUATION_RUN_RECORDED: Ty = Ty::Record(&[
    ("evaluation_id", Ty::Ident),
    ("decision", Ty::Str),
    ("cut_off", Ty::Timestamp),
    ("theses", Ty::DigestRef),
    ("theses_from", Ty::List(&Ty::Str)),
    ("theses_to", Ty::List(&Ty::Str)),
    ("theses_head", Ty::List(&Ty::Digest)),
    ("closes", Ty::DigestRef),
    ("cost_model", Ty::DigestRef),
    ("code_build", Ty::DigestRef),
    ("report", Ty::DigestRef),
    ("report_version", Ty::Int),
    ("passed", Ty::Bool),
]);
