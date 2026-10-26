//! The strict parse of a research output (§6.5): `{"theses": [...], "invalidations": [...]}`,
//! read with the canonical parser, so a float, a duplicate key, an unknown member, or trailing data
//! is a rejection rather than a guess.

use std::collections::BTreeSet;

use mandate_canon::{Object, Value, parse};
use mandate_num::{Conviction, Unit};

use crate::agentic::RunKind;

/// The output schema's bounds, inside the research entry's content hash (§6.5, §7.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputBounds {
    pub max_output_bytes: u32,
    pub max_theses: u32,
    pub max_invalidations: u32,
    pub min_horizon_s: u32,
    pub max_horizon_s: u32,
    pub max_text_bytes: u32,
    pub max_evidence: u32,
}

impl OutputBounds {
    /// §6.5's defaults: at most 4 theses, a horizon from 1 day to 30 days, and text and evidence
    /// bounded so one output cannot flood the journal (DEC-431 item 7).
    pub const fn v0_1() -> Self {
        Self {
            max_output_bytes: 65_536,
            max_theses: 4,
            max_invalidations: 50,
            min_horizon_s: 86_400,
            max_horizon_s: 2_592_000,
            max_text_bytes: 2_000,
            max_evidence: 20,
        }
    }
}

/// One candidate thesis, as the model wrote it. Every field is the model's; the platform fills the
/// rest later (§6.5's field split, HI-18). `instrument` stays text here so that a symbol the
/// snapshot does not know is journaled as `unknown_instrument` rather than lost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateThesis {
    pub instrument: String,
    pub direction: String,
    pub conviction: Conviction,
    pub confidence: Unit,
    pub horizon_s: u32,
    pub thesis: String,
    pub invalidation: String,
    pub evidence: Vec<String>,
    pub predecessor_thesis_id: Option<String>,
}

/// One candidate invalidation: a thesis id and the items that show its condition holds (§6.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateInvalidation {
    pub thesis_id: String,
    pub evidence: Vec<String>,
}

/// A parsed output: an opinion and nothing else. There is no member the harness executes (HI-4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateOutput {
    pub theses: Vec<CandidateThesis>,
    pub invalidations: Vec<CandidateInvalidation>,
}

/// Why an output was not read. Each one means the run has no output (HI-6): the harness never
/// repairs, re-asks, or reads part of an output.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OutputRejection {
    #[error("the output is longer than its bound")]
    TooLong,
    #[error("the output is not strict JSON")]
    NotJson,
    #[error("the output's shape is not the schema's at {at}")]
    Shape { at: &'static str },
    #[error("a value is outside its range at {at}")]
    Range { at: &'static str },
    #[error("more items than the bound allows at {at}")]
    TooMany { at: &'static str },
    #[error("two invalidations name the same thesis")]
    DuplicateInvalidation,
}

impl OutputRejection {
    pub fn code(&self) -> &'static str {
        match self {
            Self::TooLong => "too_long",
            Self::NotJson => "not_json",
            Self::Shape { .. } => "shape",
            Self::Range { .. } => "range",
            Self::TooMany { .. } => "too_many",
            Self::DuplicateInvalidation => "duplicate_invalidation",
        }
    }
}

/// Parses `bytes` against `bounds`, as the output schema of a `kind` run pins it (agent harness spec
/// §6.5, §6.9): an `ideate` run's output is exactly `theses` and `invalidations`, and a `monitor`
/// run's exactly `invalidations`, so a thesis from a monitor run is refused, as the gateway's
/// `schema_invalid` refuses it, and its invalidations are never lost to a member it cannot have.
///
/// # Errors
/// [`OutputRejection`] for anything but an exact schema-valid output within its bounds.
pub fn parse_output(
    bytes: &[u8],
    bounds: &OutputBounds,
    kind: RunKind,
) -> Result<CandidateOutput, OutputRejection> {
    if bytes.len() > usize::try_from(bounds.max_output_bytes).unwrap_or(usize::MAX) {
        return Err(OutputRejection::TooLong);
    }
    let value = parse(bytes).map_err(|_| OutputRejection::NotJson)?;
    let required: &[&str] = match kind {
        RunKind::Ideate => &["invalidations", "theses"],
        RunKind::Monitor => &["invalidations"],
    };
    let top = exact(&value, required, &[], "output")?;
    let theses = match kind {
        RunKind::Ideate => list(top, "theses", bounds.max_theses)?
            .iter()
            .map(|thesis| candidate_thesis(thesis, bounds))
            .collect::<Result<Vec<_>, _>>()?,
        RunKind::Monitor => Vec::new(),
    };
    let invalidations = list(top, "invalidations", bounds.max_invalidations)?
        .iter()
        .map(|invalidation| candidate_invalidation(invalidation, bounds))
        .collect::<Result<Vec<_>, _>>()?;
    let mut named = BTreeSet::new();
    if !invalidations
        .iter()
        .all(|invalidation| named.insert(invalidation.thesis_id.as_str()))
    {
        return Err(OutputRejection::DuplicateInvalidation);
    }
    Ok(CandidateOutput {
        theses,
        invalidations,
    })
}

/// The longest id the parser accepts, as the worker's own id grammar bounds it.
const MAX_ID_BYTES: usize = 128;
/// The longest symbol or direction, which the snapshot and §8.2 bound far below this.
const MAX_WORD_BYTES: usize = 32;

/// `value` as an object whose members are exactly `required` plus any of `optional`.
fn exact<'a>(
    value: &'a Value,
    required: &[&str],
    optional: &[&str],
    at: &'static str,
) -> Result<&'a Object, OutputRejection> {
    let object = value.as_object().ok_or(OutputRejection::Shape { at })?;
    let known = object
        .keys()
        .all(|key| required.contains(&key.as_str()) || optional.contains(&key.as_str()));
    let complete = required.iter().all(|name| object.contains_key(*name));
    if known && complete {
        Ok(object)
    } else {
        Err(OutputRejection::Shape { at })
    }
}

fn list<'a>(
    object: &'a Object,
    name: &'static str,
    max: u32,
) -> Result<&'a [Value], OutputRejection> {
    let items = object
        .get(name)
        .and_then(Value::as_array)
        .ok_or(OutputRejection::Shape { at: name })?;
    if items.len() > usize::try_from(max).unwrap_or(usize::MAX) {
        return Err(OutputRejection::TooMany { at: name });
    }
    Ok(items)
}

/// A string member of at most `max` bytes that is not blank and holds no control character, so
/// every text the platform later wraps in a typed value (an `Invalidation`, an id) is one it can
/// hold, and a candidate is never dropped between the parse and its verdict (HI-22).
fn bounded(object: &Object, name: &'static str, max: usize) -> Result<String, OutputRejection> {
    let text = object
        .get(name)
        .and_then(Value::as_str)
        .ok_or(OutputRejection::Shape { at: name })?;
    if text.trim().is_empty() || text.len() > max || text.chars().any(char::is_control) {
        return Err(OutputRejection::Range { at: name });
    }
    Ok(text.to_owned())
}

fn evidence(object: &Object, bounds: &OutputBounds) -> Result<Vec<String>, OutputRejection> {
    list(object, "evidence", bounds.max_evidence)?
        .iter()
        .map(|id| {
            let id = id
                .as_str()
                .ok_or(OutputRejection::Shape { at: "evidence" })?;
            if id.trim().is_empty() || id.len() > MAX_ID_BYTES || id.chars().any(char::is_control) {
                return Err(OutputRejection::Range { at: "evidence" });
            }
            Ok(id.to_owned())
        })
        .collect()
}

fn candidate_thesis(
    value: &Value,
    bounds: &OutputBounds,
) -> Result<CandidateThesis, OutputRejection> {
    let object = exact(
        value,
        &[
            "confidence",
            "conviction",
            "direction",
            "evidence",
            "horizon_s",
            "instrument",
            "invalidation",
            "thesis",
        ],
        &["predecessor_thesis_id"],
        "thesis",
    )?;
    let text_bytes = usize::try_from(bounds.max_text_bytes).unwrap_or(usize::MAX);
    let conviction = object
        .get("conviction")
        .and_then(Value::as_str)
        .ok_or(OutputRejection::Shape { at: "conviction" })?;
    let confidence = object
        .get("confidence")
        .and_then(Value::as_str)
        .ok_or(OutputRejection::Shape { at: "confidence" })?;
    let horizon_s = object
        .get("horizon_s")
        .and_then(Value::as_int)
        .ok_or(OutputRejection::Shape { at: "horizon_s" })?;
    let horizon_s = u32::try_from(horizon_s)
        .ok()
        .filter(|h| (bounds.min_horizon_s..=bounds.max_horizon_s).contains(h))
        .ok_or(OutputRejection::Range { at: "horizon_s" })?;
    let predecessor_thesis_id = match object.get("predecessor_thesis_id") {
        None => None,
        Some(_) => Some(bounded(object, "predecessor_thesis_id", MAX_ID_BYTES)?),
    };
    Ok(CandidateThesis {
        instrument: bounded(object, "instrument", MAX_WORD_BYTES)?,
        direction: bounded(object, "direction", MAX_WORD_BYTES)?,
        conviction: Conviction::parse(conviction)
            .map_err(|_| OutputRejection::Range { at: "conviction" })?,
        confidence: Unit::parse(confidence)
            .map_err(|_| OutputRejection::Range { at: "confidence" })?,
        horizon_s,
        thesis: bounded(object, "thesis", text_bytes)?,
        invalidation: bounded(object, "invalidation", text_bytes)?,
        evidence: evidence(object, bounds)?,
        predecessor_thesis_id,
    })
}

fn candidate_invalidation(
    value: &Value,
    bounds: &OutputBounds,
) -> Result<CandidateInvalidation, OutputRejection> {
    let object = exact(value, &["evidence", "thesis_id"], &[], "invalidation")?;
    Ok(CandidateInvalidation {
        thesis_id: bounded(object, "thesis_id", MAX_ID_BYTES)?,
        evidence: evidence(object, bounds)?,
    })
}
