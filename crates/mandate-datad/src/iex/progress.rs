//! What the IEX source's worker tells the process's probe loop (DEC-2283): the worker fetches and
//! reads the journal, which may take as long as the data host's pacing asks, while the probe loop
//! answers liveness every second whatever the worker is doing. The loop fails the process only when
//! the platform key was refused, the worker stopped, or it has finished no pass for [`STALL_S`].

use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

use mandate_time::UtcNanos;

/// How long the worker may go without finishing a pass, in seconds: past the longest paced fetch,
/// a few rate windows and the client's retries, so a throttled vendor is never a restart, while a
/// worker stuck for good is.
pub const STALL_S: i64 = 600;

/// The worker's progress, shared with the probe loop.
#[derive(Debug, Default)]
pub struct Progress {
    passed_at: AtomicI64,
    started: AtomicBool,
    stop: AtomicBool,
    counted: AtomicBool,
}

/// What the probe loop makes of the worker's progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Judged {
    /// The key was refused, the worker stopped, or it is stalled: the process fails.
    Failed,
    /// No pass has finished yet: not ready. Its bound is the orchestrator's start-up probe: a
    /// journal that never reads leaves the process here, not ready, until that probe restarts it.
    Starting,
    /// The first pass has finished: ready. A workspace that is not paper, or whose snapshots do
    /// not read, has passes that write nothing, and is ready all the same.
    Started,
    /// Passes are finishing.
    Passing,
}

impl Progress {
    /// A pass finished at `now`.
    pub fn passed(&self, now: UtcNanos) {
        self.passed_at.store(now.secs(), Ordering::SeqCst);
        self.started.store(true, Ordering::SeqCst);
    }

    /// The worker stopped, for a refused key or a failure.
    pub fn stopped(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    /// The probe loop's reading at `now`: [`Judged::Started`] once, at the first reading after the
    /// first pass, and [`Judged::Passing`] after.
    pub fn judged(&self, now: UtcNanos) -> Judged {
        if self.stop.load(Ordering::SeqCst) {
            return Judged::Failed;
        }
        if !self.started.load(Ordering::SeqCst) {
            return Judged::Starting;
        }
        let since = now
            .secs()
            .saturating_sub(self.passed_at.load(Ordering::SeqCst));
        if since > STALL_S {
            Judged::Failed
        } else if self.counted.swap(true, Ordering::SeqCst) {
            Judged::Passing
        } else {
            Judged::Started
        }
    }
}
