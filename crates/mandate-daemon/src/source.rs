//! The cell's research source (DEC-2214 items 2 to 4): each pass's context read from the recorded
//! bundle, the deployment's mandate document, and the workspace's control and agent streams. A
//! fact it cannot read gives the pass no context; it never fills one in (DEC-2210 item 4).

use mandate_canon::Value;
use mandate_journal::{ArtifactSource, Environment, StoredEvent, StreamId};
use mandate_research::MandateEnvelope;
use mandate_runtime::RuntimeState;
use mandate_store::Store;

use crate::recorded::Bundle;
use crate::runtime::{ResearchParts, ResearchSource};

/// Where the source reads a stream's records: the process's store, or a read connection of its own.
pub trait Rows {
    /// The stream's records, or `None` when they cannot be read.
    fn rows(&self, stream: &StreamId) -> Option<Vec<StoredEvent>>;
}

impl<S: Store> Rows for S {
    #[expect(
        clippy::todo,
        reason = "the read returns no error, so its stub is todo!(), the other form DEC-137 names"
    )]
    fn rows(&self, _stream: &StreamId) -> Option<Vec<StoredEvent>> {
        todo!()
    }
}

/// Why a mandate document's research envelope cannot be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceError {
    /// DEC-2214's envelope read is not implemented yet (E21-54).
    Unimplemented,
    /// The member at this path is missing or out of its range.
    Unreadable { member: &'static str },
}

/// Who the source reads for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFor {
    pub workspace: String,
    pub agent: String,
    pub environment: Environment,
}

/// The cell's research source.
pub struct CellSource {
    _unbuilt: (),
}

impl CellSource {
    /// The source over `bundle`'s recorded facts, reading the streams through `rows` and the
    /// mandate documents from `documents`.
    #[expect(
        clippy::todo,
        reason = "the constructor returns no error, so its stub is todo!(), the other form DEC-137 names"
    )]
    pub fn new(
        bundle: &Bundle,
        who: SourceFor,
        rows: Box<dyn Rows>,
        documents: Box<dyn ArtifactSource>,
    ) -> Self {
        let _ = (bundle, who, rows, documents);
        todo!()
    }
}

impl ResearchSource for CellSource {
    #[expect(
        clippy::todo,
        reason = "the trait's method returns no error, so its stub is todo!(), the other form DEC-137 names"
    )]
    fn parts(&self, _state: &RuntimeState) -> Option<ResearchParts> {
        todo!()
    }
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
