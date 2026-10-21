//! The scheduler-stream records journal spec v0.47 §9.20 closes (DEC-1010): `StreamOpened`,
//! `ClockAdvanced`, `TradingDayStarted`, `ClockOffsetRecorded`, and `ClockToleranceExceeded` on
//! `clock:{workspace_id}`, each at schema version 1, with consistency rules 159 and 161 to 164
//! and subject rule 160. Offsets, delays, and tolerances are signed decimal seconds (§4.6), which
//! the rules compare exactly, digit by digit, never through a float. Every rule only refuses a
//! draft.

use std::cmp::Ordering;

use mandate_canon::Value;

use crate::schema::Ty;
use crate::{Invalid, InvalidReason, StreamId, StreamType};

const OPENED: &str = "StreamOpened";
const ADVANCED: &str = "ClockAdvanced";
const DAY: &str = "TradingDayStarted";
const MEASURED: &str = "ClockOffsetRecorded";
const EXCEEDED: &str = "ClockToleranceExceeded";

/// The envelope members rules 159 and 164 read.
pub(crate) struct Envelope<'a> {
    pub(crate) actor: Option<&'a Value>,
    pub(crate) clock_source: Option<&'a Value>,
    pub(crate) causation_id: Option<&'a Value>,
}

/// Whether §9.20 governs records on `stream`: every record of the scheduler stream.
pub(crate) fn governs(stream: &StreamId) -> bool {
    stream.stream_type() == StreamType::Scheduler
}

/// The payload normalized against its §9.20 schema, then rules 159 and 161 to 164 in number
/// order, reported only on a well-typed payload.
pub(crate) fn payload(
    event_type: &str,
    schema_version: u64,
    payload: &Value,
    envelope: &Envelope<'_>,
) -> Result<Value, Invalid> {
    let schema = schema(event_type, schema_version)
        .ok_or_else(|| Invalid::new(InvalidReason::UnknownSchema, "payload"))?;
    let payload = crate::schema::normalize(schema, payload, "payload")?;
    rules(event_type, &payload, envelope)?;
    Ok(payload)
}

fn schema(event_type: &str, schema_version: u64) -> Option<&'static Ty> {
    match (event_type, schema_version) {
        (OPENED, 1) => Some(&STREAM_OPENED),
        (ADVANCED, 1) => Some(&CLOCK_ADVANCED),
        (DAY, 1) => Some(&TRADING_DAY_STARTED),
        (MEASURED, 1) => Some(&CLOCK_OFFSET_RECORDED),
        (EXCEEDED, 1) => Some(&CLOCK_TOLERANCE_EXCEEDED),
        _ => None,
    }
}

fn rules(event_type: &str, payload: &Value, envelope: &Envelope<'_>) -> Result<(), Invalid> {
    let actor_kind = envelope
        .actor
        .and_then(|actor| actor.get("kind"))
        .and_then(Value::as_str);
    ensure(actor_kind == Some("system"), "actor.kind")?;
    ensure(
        envelope.clock_source.and_then(Value::as_str) == Some("scheduler"),
        "clock_source",
    )?;
    let decimal = |member: &str| payload.get(member).and_then(Value::as_str);
    match event_type {
        MEASURED => {
            let tolerance = decimal("tolerance").unwrap_or_default();
            ensure(positive(tolerance), "payload.tolerance")?;
            let samples = payload
                .get("samples")
                .and_then(Value::as_array)
                .unwrap_or_default();
            for (i, sample) in samples.iter().enumerate() {
                let stratum = sample.get("stratum").and_then(Value::as_int);
                ensure(
                    stratum.is_some_and(|s| (1..=15).contains(&s)),
                    &format!("payload.samples[{i}].stratum"),
                )?;
                let delay = sample
                    .get("delay")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                ensure(!negative(delay), &format!("payload.samples[{i}].delay"))?;
            }
            let servers: Vec<&str> = samples
                .iter()
                .filter_map(|sample| sample.get("server").and_then(Value::as_str))
                .collect();
            if !servers.windows(2).all(|w| matches!(w, [a, b] if a < b)) {
                return Err(Invalid::new(InvalidReason::NonCanonical, "payload.samples"));
            }
            let offset = decimal("offset");
            ensure(offset == selected(samples), "payload.offset")?;
            let within = payload.get("within_tolerance") == Some(&Value::Bool(true));
            ensure(
                within == offset.is_some_and(|o| magnitude(o, tolerance) != Ordering::Greater),
                "payload.within_tolerance",
            )
        }
        EXCEEDED => {
            let tolerance = decimal("tolerance").unwrap_or_default();
            ensure(positive(tolerance), "payload.tolerance")?;
            ensure(
                decimal("offset").is_none_or(|o| magnitude(o, tolerance) == Ordering::Greater),
                "payload.offset",
            )?;
            ensure(
                envelope.causation_id.and_then(Value::as_str).is_some(),
                "causation_id",
            )
        }
        _ => Ok(()),
    }
}

/// Subject rule 160 (`stream_mismatch`), reported after `artifact_refs` and `pii_refs`.
pub(crate) fn subject(event_type: &str, stream: &StreamId, payload: &Value) -> Result<(), Invalid> {
    if event_type != OPENED {
        return Ok(());
    }
    let workspace = payload
        .get("workspace_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if format!("clock:{workspace}") == stream.as_str() {
        Ok(())
    } else {
        Err(Invalid::new(InvalidReason::StreamMismatch, "stream_id"))
    }
}

/// Rule 162's offset: of the samples sorted by the magnitude of `offset`, then by `offset`, the
/// one at index n / 2 rounded down; `None` for no sample.
fn selected(samples: &[Value]) -> Option<&str> {
    let mut offsets: Vec<&str> = samples
        .iter()
        .filter_map(|sample| sample.get("offset").and_then(Value::as_str))
        .collect();
    offsets.sort_by(|a, b| magnitude(a, b).then_with(|| signed(a, b)));
    offsets.get(offsets.len() / 2).copied()
}

fn negative(decimal: &str) -> bool {
    decimal.starts_with('-')
}

pub(crate) fn positive(decimal: &str) -> bool {
    !negative(decimal) && decimal != "0"
}

/// The magnitudes of two normalized §4.6 decimals compared exactly: the longer integer part is
/// larger (normalized decimals carry no leading zero), then the integer digits, then the
/// fraction's digits with the shorter padded by zeros.
fn magnitude(a: &str, b: &str) -> Ordering {
    let (a_int, a_frac) = parts(a);
    let (b_int, b_frac) = parts(b);
    a_int
        .len()
        .cmp(&b_int.len())
        .then_with(|| a_int.cmp(b_int))
        .then_with(|| {
            let width = a_frac.len().max(b_frac.len());
            format!("{a_frac:0<width$}").cmp(&format!("{b_frac:0<width$}"))
        })
}

/// Two normalized decimals compared by value: every negative is below every non-negative, and
/// among negatives the larger magnitude is the smaller value.
pub(crate) fn signed(a: &str, b: &str) -> Ordering {
    match (negative(a), negative(b)) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (false, false) => magnitude(a, b),
        (true, true) => magnitude(b, a),
    }
}

fn parts(decimal: &str) -> (&str, &str) {
    let digits = decimal.trim_start_matches('-');
    digits.split_once('.').unwrap_or((digits, ""))
}

fn ensure(holds: bool, path: &str) -> Result<(), Invalid> {
    if holds {
        Ok(())
    } else {
        Err(Invalid::new(InvalidReason::Schema, path))
    }
}

/// `clock_sample`: one answering server, its `server` the configured name (`text`).
static SAMPLE: Ty = Ty::Record(&[
    ("server", Ty::Str),
    ("stratum", Ty::Int),
    ("offset", Ty::Decimal),
    ("delay", Ty::Decimal),
]);

static STREAM_OPENED: Ty = Ty::Record(&[
    ("stream_type", Ty::OneOf(&["scheduler"])),
    ("workspace_id", Ty::Ident),
]);

static CLOCK_ADVANCED: Ty = Ty::Record(&[("clock", Ty::RiskClock)]);

static TRADING_DAY_STARTED: Ty = Ty::Record(&[("date", Ty::Date)]);

static CLOCK_OFFSET_RECORDED: Ty = Ty::Record(&[
    (
        "trigger",
        Ty::OneOf(&["startup", "before_session", "periodic"]),
    ),
    ("samples", Ty::List(&SAMPLE)),
    ("offset", Ty::Nullable(&Ty::Decimal)),
    ("tolerance", Ty::Decimal),
    ("within_tolerance", Ty::Bool),
]);

static CLOCK_TOLERANCE_EXCEEDED: Ty = Ty::Record(&[
    ("offset", Ty::Nullable(&Ty::Decimal)),
    ("tolerance", Ty::Decimal),
]);
