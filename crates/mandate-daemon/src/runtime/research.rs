//! Research in the runtime process (DEC-2208): the research driver (DEC-2204) behind a deny-all
//! default, its batches journaled on the agent stream with the step's run as `correlation_id` and
//! the core's own artifacts stored first, and its worker the only place research does I/O.

use std::collections::BTreeSet;

use mandate_research::{
    AdmissionFacts, LineageState, PolicyOverlay, ValidatedMandate, WorkingUniverse,
};
use mandate_research_run::Symbol;
use mandate_runtime::{
    ActiveThesis, Preconditions, ResearchConfig, ResearchInput, ResearchRequest, RiskClock,
    RuntimeState,
};
use mandate_store::Store;
use mandate_time::UtcNanos;

use super::{Alert, RuntimeProcess};

/// What a research wiring reads each pass besides the core's state: every member a fold of the
/// journal or a pinned configuration object, as the core's `ResearchContext` names them.
pub struct ResearchParts {
    pub preconditions: Preconditions,
    pub mandate: ValidatedMandate,
    pub overlay: PolicyOverlay,
    pub universe: WorkingUniverse,
    pub facts: AdmissionFacts,
    pub lineages: LineageState,
    pub active: Vec<ActiveThesis>,
    pub last_reservation: Option<RiskClock>,
    pub data_symbols: BTreeSet<Symbol>,
}

/// The research context of one pass; `None` runs no research on that pass.
pub trait ResearchSource {
    fn parts(&self, state: &RuntimeState) -> Option<ResearchParts>;
}

/// The research worker: retrieval and the gateway's calls. It stores the artifacts its answers name
/// (retrieved records, prompts, responses) before it hands an answer back.
pub trait ResearchWorker {
    /// Takes one of the core's requests, after the batch that authorises it committed.
    fn hand(&mut self, request: &ResearchRequest);

    /// The answers ready since the last pass, in the order they arrived.
    fn answers(&mut self) -> Vec<ResearchInput>;
}

/// Where the artifacts the core makes are stored (DEC-2206 item 4): content-addressed, put before
/// the batch that names them. Whether the bytes are stored.
pub trait ArtifactSink {
    fn put(&mut self, bytes: &[u8]) -> bool;
}

/// One deployment's research, as its wiring gives it (DEC-2208 item 2).
pub struct ResearchWiring {
    pub config: ResearchConfig,
    pub source: Box<dyn ResearchSource>,
    pub worker: Box<dyn ResearchWorker>,
    pub artifacts: Box<dyn ArtifactSink>,
}

/// The process's research: its wiring, until the driver is run (DEC-2208, tests first).
pub(super) struct Research {
    wiring: ResearchWiring,
}

impl Research {
    pub(super) fn new(wiring: ResearchWiring) -> Self {
        Self { wiring }
    }
}

impl<S: Store> RuntimeProcess<S> {
    /// The research input in doubt, handed again before anything else (DEC-2208 item 4). Not yet
    /// run: nothing research writes is ever in doubt.
    pub(super) fn research_retry(&mut self, _now: UtcNanos, _alerts: &mut Vec<Alert>) -> bool {
        true
    }

    /// The research pass (DEC-2208 item 3). Not yet run: it hands the driver nothing.
    pub(super) fn research_pass(&mut self, _now: UtcNanos, _alerts: &mut Vec<Alert>) {
        let _ = self.research.as_ref().map(|research| &research.wiring);
    }
}
