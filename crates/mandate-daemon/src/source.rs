//! The cell's research source (DEC-2214 items 2 to 4): each pass's context read from the recorded
//! bundle, the deployment's mandate document, and the workspace's control and agent streams. A
//! fact it cannot read gives the pass no context; it never fills one in (DEC-2210 item 4).

use mandate_canon::Value;
use mandate_research::MandateEnvelope;

/// Why a mandate document's research envelope cannot be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceError {
    /// DEC-2214's envelope read is not implemented yet (E21-54).
    Unimplemented,
    /// The member at this path is missing or out of its range.
    Unreadable { member: &'static str },
}

/// The research envelope of a mandate `document` (mandate spec §8.5 and §8.6): the universe's
/// fields, whether any signal model admits instruments, `behavior.research`, and the admission
/// autonomy decision.
///
/// # Errors
/// [`SourceError::Unreadable`] naming the first member that is missing or out of its range.
pub fn envelope(document: &Value) -> Result<MandateEnvelope, SourceError> {
    let _ = document;
    Err(SourceError::Unimplemented)
}
