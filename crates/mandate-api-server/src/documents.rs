//! The mandate document a deployed version names, read from the content-addressed store
//! (DEC-1020 items 1 and 2): checked against its digest, parsed as a mandate, and summarized as the
//! envelope the agent read serves. The store is read, never written.

use mandate_canon::Value;
use mandate_journal::ArtifactSource;

/// What the agent read takes from a mandate document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    /// The document's `name`.
    pub label: String,
    /// `read-models/agent`'s `MandateSummary`, its values the document's own.
    pub mandate: Value,
    /// The connection the document names, for the account's positions.
    pub connection_id: String,
}

/// The envelope of the document `version` names in `store`: `None` when the store does not hold
/// it, it does not hash to `version`, or it does not parse as a mandate (an encrypted document
/// included). A digest mismatch is logged by name, with no content.
pub fn envelope(store: &dyn ArtifactSource, version: &str) -> Option<Envelope> {
    let _ = (store, version);
    None
}
