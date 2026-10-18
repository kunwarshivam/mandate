//! The evaluation's four input files ([DEC-2244](../../../docs/project/decisions/DEC-2244.md) item
//! 3), each one JSON object that `mandate-canon` parses: decimals are strings, counts are integers,
//! and instants are RFC 3339 UTC text. Every member is required and a member no reader names is
//! refused, so a misspelt input fails closed rather than falling back to anything, as the backtest
//! runner's inputs do (DEC-1144 item 3).

use mandate_research::AssetId;
use mandate_research::score::EvaluationDecision;

use crate::{Closes, CostModel, EvaluationError};

/// The four files' bytes, by name, as the directory holds them.
#[derive(Debug, Clone, Copy)]
pub struct InputFiles<'a> {
    pub decision: &'a [u8],
    pub closes: &'a [u8],
    pub cost: &'a [u8],
    pub baselines: &'a [u8],
}

/// The file names [`InputFiles`] are read from, in its order.
pub const INPUT_FILE_NAMES: [&str; 4] = [
    "decision.json",
    "closes.json",
    "cost.json",
    "baselines.json",
];

/// Every input of one evaluation but the thesis records and the mandate documents, decoded.
#[derive(Debug, Clone)]
pub struct EvaluationInputs {
    /// The recorded decision that registered the window, metric and threshold (DEC-122).
    pub decision_id: String,
    pub decision: EvaluationDecision,
    pub closes: Closes,
    pub cost: CostModel,
    pub basket: Vec<AssetId>,
    pub index: AssetId,
}

/// Decodes the four files.
///
/// # Errors
/// `unimplemented` until E17-8's run append builds it.
pub fn decode(_files: &InputFiles<'_>) -> Result<EvaluationInputs, EvaluationError> {
    Err(EvaluationError::Unimplemented { story: "E17-8" })
}
