//! The strict parse of a research output (§6.5): `{"theses": [...], "invalidations": [...]}`,
//! read with the canonical parser, so a float, a duplicate key, an unknown member, or trailing data
//! is a rejection rather than a guess.

use mandate_num::{Conviction, Unit};

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
    #[error("the parser has not been implemented yet")]
    Unimplemented,
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
            Self::Unimplemented => "unimplemented",
        }
    }
}

/// Parses `bytes` against `bounds`.
///
/// # Errors
/// [`OutputRejection`] for anything but an exact schema-valid output within its bounds.
pub fn parse_output(
    _bytes: &[u8],
    _bounds: &OutputBounds,
) -> Result<CandidateOutput, OutputRejection> {
    Err(OutputRejection::Unimplemented)
}
