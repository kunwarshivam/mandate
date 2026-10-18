//! The cell's research worker (DEC-2210 item 3): retrieval over the recorded plane, and the one
//! call through the gateway over its recorded fake, the only transport this type can name. It
//! stores every artifact an answer names before it keeps the answer, and stamps each answer with
//! the risk clock of the pass it is handed in.

use mandate_journal::ArtifactStore;
use mandate_runtime::{ResearchInput, ResearchRequest, RiskClock};
use mandate_time::UtcNanos;

use crate::recorded::{Bundle, BundleError};
use crate::runtime::{ArtifactSink, ResearchParts, ResearchWorker};

/// Why the worker could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerError {
    /// DEC-2210 item 3's worker is not implemented yet (E21-54).
    Unimplemented,
    /// The bundle's entry or endpoint did not register.
    Bundle(BundleError),
}

/// Who the worker reads and calls for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerFor {
    pub workspace: String,
    pub agent: String,
    /// The mandate's `behavior.description`, which the call shows the model.
    pub description: String,
}

/// The cell's worker.
pub struct RecordedWorker {
    _unbuilt: (),
}

impl RecordedWorker {
    /// The worker over `bundle`, storing what its answers name through `sink`, its gateway's clock
    /// moved to `now()` before each call.
    ///
    /// # Errors
    /// [`WorkerError::Bundle`] when the bundle's entry or endpoint does not register.
    pub fn new(
        bundle: &Bundle,
        who: WorkerFor,
        sink: Box<dyn ArtifactSink>,
        now: Box<dyn FnMut() -> UtcNanos>,
    ) -> Result<Self, WorkerError> {
        let _ = (bundle, who, sink, now);
        Err(WorkerError::Unimplemented)
    }
}

impl ResearchWorker for RecordedWorker {
    #[expect(
        clippy::todo,
        reason = "the trait's methods return no error, so their stubs are todo!(), the other form DEC-137 names"
    )]
    fn hand(&mut self, _request: &ResearchRequest, _parts: &ResearchParts) {
        todo!()
    }

    #[expect(
        clippy::todo,
        reason = "the trait's methods return no error, so their stubs are todo!(), the other form DEC-137 names"
    )]
    fn answers(&mut self, _at: RiskClock) -> Vec<ResearchInput> {
        todo!()
    }
}

/// The artifact store as a research sink: each put is content-addressed, and stored only when the
/// store gives back the reference of the bytes it was handed.
pub struct StoreSink<A>(pub A);

impl<A: ArtifactStore> ArtifactSink for StoreSink<A> {
    #[expect(
        clippy::todo,
        reason = "the sink's put returns no error, so its stub is todo!(), the other form DEC-137 names"
    )]
    fn put(&mut self, _bytes: &[u8]) -> bool {
        todo!()
    }
}
