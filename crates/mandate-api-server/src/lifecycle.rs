//! Readiness and the drain (§3.11 items 3 and 4, DEC-978 items 3 and 4).
//!
//! [`Lifecycle`] is the one seam the server reads readiness through. `mandate-process` (E21-28)
//! will implement it; until then [`Local`] answers: ready once serving and not draining.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

/// How long a drain may take before the process exits anyway: Proposed, by analogy with
/// infrastructure §7.3's 90 seconds for agent processes.
pub const DRAIN_BOUND: Duration = Duration::from_secs(90);

/// What the probes report. Readiness never gates an API-7 operation.
pub trait Lifecycle: Send + Sync {
    /// Whether the process should receive new ordinary traffic.
    fn ready(&self) -> bool;
}

/// How often a drain looks at the in-flight count.
pub const DRAIN_POLL: Duration = Duration::from_millis(10);

/// The drain flag, set once by SIGTERM and never cleared, and the count of API requests in flight.
#[derive(Debug, Default)]
pub struct Drain {
    begun: AtomicBool,
    in_flight: AtomicUsize,
}

/// One API request in flight; dropping it ends the request's count.
#[derive(Debug)]
pub struct InFlight(Arc<Drain>);

impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
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

    /// Counts one request in flight until the returned guard is dropped.
    pub fn enter(self: &Arc<Self>) -> InFlight {
        self.in_flight.fetch_add(1, Ordering::SeqCst);
        InFlight(Arc::clone(self))
    }

    /// How many requests are in flight.
    pub fn in_flight(&self) -> usize {
        self.in_flight.load(Ordering::SeqCst)
    }

    /// Waits until no request is in flight, looking every [`DRAIN_POLL`], for at most `bound`:
    /// `true` once nothing is in flight, `false` if the bound came first.
    pub async fn settled(&self, bound: Duration) -> bool {
        let polls = bound
            .as_millis()
            .checked_div(DRAIN_POLL.as_millis())
            .unwrap_or_default();
        for _ in 0..=polls {
            if self.in_flight() == 0 {
                return true;
            }
            tokio::time::sleep(DRAIN_POLL).await;
        }
        false
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
