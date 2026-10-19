//! The cell's research, wired only when asked for (DEC-2210 item 5, DEC-2215): the recorded bundle
//! at the path the process is given, the recorded worker over it, and the cell's source, for a
//! paper deployment read from the journal. Without the opt-in the process runs deny-all
//! (DEC-2208 item 1).

use std::fmt;
use std::path::Path;

use mandate_journal::Environment;
use mandate_time::UtcNanos;

use crate::recorded::{Bundle, BundleError};
use crate::runtime::{Refs, ResearchWiring};
use crate::source::Rows;
use crate::worker::WorkerError;

/// The deployment research is asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellDeployment {
    pub workspace: String,
    pub agent: String,
    pub environment: Environment,
    /// Whether the process read the deployment from the journal (DEC-1016), rather than being
    /// handed it by flags.
    pub resolved: bool,
}

/// Why research could not be wired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CellError {
    /// DEC-2215's opt-in is not implemented yet (E21-54).
    Unimplemented,
    /// Research runs only for a paper deployment.
    NotPaper,
    /// Research runs only for a deployment read from the journal.
    Unresolved,
    /// The artifact store at the given root cannot be opened.
    ArtifactsUnavailable,
    /// The recorded bundle refused to load.
    Bundle(BundleError),
    /// The deployment or its mandate document could not be read, with `deployment`'s code.
    Mandate(&'static str),
    /// The recorded worker could not be built.
    Worker(WorkerError),
}

impl CellError {
    /// The code the binary stops the start with.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unimplemented => "unimplemented",
            Self::NotPaper => "research_needs_paper",
            Self::Unresolved => "research_needs_resolved_deployment",
            Self::ArtifactsUnavailable => "artifact_store_unavailable",
            Self::Bundle(_) => "research_bundle_unreadable",
            Self::Mandate(code) => code,
            Self::Worker(_) => "research_worker_unbuilt",
        }
    }
}

impl fmt::Display for CellError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for CellError {}

/// The research a process is wired with, and the refs its records carry.
pub struct CellResearch {
    pub wiring: ResearchWiring,
    pub refs: Box<dyn Refs>,
}

/// Research asked for over a recorded bundle, for a resolved paper deployment: made only by
/// [`opt_in`], so no research is built for any other deployment.
pub struct OptIn {
    bundle: Bundle,
    deployment: CellDeployment,
}

/// The opt-in for research over the recorded bundle at `dir`, for `deployment`. It reads no journal
/// and no artifact store, so a refused opt-in stops the start before the process connects to either.
///
/// # Errors
/// [`CellError::NotPaper`] or [`CellError::Unresolved`] for any but a resolved paper deployment, and
/// [`CellError::Bundle`] when the bundle does not load.
pub fn opt_in(dir: &Path, deployment: &CellDeployment) -> Result<OptIn, CellError> {
    let _ = (dir, deployment);
    Err(CellError::Unimplemented)
}

/// The research `opted` asks for, reading the streams through `rows` and the mandate documents and
/// artifacts from the store at `artifacts`.
///
/// # Errors
/// [`CellError`] when the artifact store, the deployment, its mandate document, or the worker cannot
/// be had.
pub fn research(
    opted: OptIn,
    rows: Box<dyn Rows>,
    artifacts: &Path,
    now: Box<dyn FnMut() -> UtcNanos>,
) -> Result<CellResearch, CellError> {
    let OptIn { bundle, deployment } = opted;
    let _ = (bundle, deployment, rows, artifacts, now);
    Err(CellError::Unimplemented)
}
