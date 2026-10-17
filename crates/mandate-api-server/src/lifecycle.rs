//! Readiness and the drain (§3.11 items 3 and 4, DEC-978 items 3 and 4).
//!
//! [`Lifecycle`] is the one seam the server reads readiness through. `mandate-process` (E21-28)
//! will implement it; until then [`Local`] answers: ready once serving and not draining.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// How long a drain may take before the process exits anyway: Proposed, by analogy with
/// infrastructure §7.3's 90 seconds for agent processes.
pub const DRAIN_BOUND: Duration = Duration::from_secs(90);

/// What the probes report. Readiness never gates an API-7 operation.
pub trait Lifecycle: Send + Sync {
    /// Whether the process should receive new ordinary traffic.
    fn ready(&self) -> bool;
}

/// The drain flag, set once by SIGTERM and never cleared.
#[derive(Debug, Default)]
pub struct Drain {
    begun: AtomicBool,
}

impl Drain {
    /// A process not yet draining.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Starts the drain; later calls change nothing.
    pub fn begin(&self) {
        self.begun.store(true, Ordering::SeqCst);
    }

    /// Whether the drain has begun.
    pub fn draining(&self) -> bool {
        self.begun.load(Ordering::SeqCst)
    }
}

/// The local readiness predicate until `mandate-process` lands.
#[derive(Debug, Clone)]
pub struct Local {
    drain: Arc<Drain>,
}

impl Local {
    /// Readiness over `drain`.
    pub fn new(drain: Arc<Drain>) -> Self {
        Self { drain }
    }
}

impl Lifecycle for Local {
    fn ready(&self) -> bool {
        !self.drain.draining()
    }
}
