//! The supervisor: start, schedule, drain, and the phase health reads (infrastructure design
//! §3.4, §7.3).

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use mandate_journal::Environment;
use mandate_process::State;
use mandate_shell::Cause;
use mandate_shell::stages::JournalWriter;

use crate::lease::{Epochs, Lease};

/// One deployment's runtime and executor, as the supervisor drives them. Every call is handed the
/// lease as its journal, so whatever it appends is under the lease's epochs.
pub trait Host {
    /// The existing start-up reconciliation (trading spec §11, journal spec §5.2): replay, then
    /// look up every submission without a broker answer before anything new is sent.
    ///
    /// # Errors
    /// Anything unexplained; the supervisor then runs no cycle.
    fn reconcile(&mut self, journal: &mut dyn JournalWriter) -> Result<(), Cause>;

    /// One decision cycle, which may open.
    ///
    /// # Errors
    /// A fail-closed stop; the supervisor runs no further cycle.
    fn cycle(&mut self, journal: &mut dyn JournalWriter) -> Result<(), Cause>;

    /// Carries what is in flight to its end during a drain: a submission to its broker answer, an
    /// exit sequence to re-placed protection, within trading §5.4's bound. It never opens and
    /// never cancels resting protection (design §7.3).
    ///
    /// # Errors
    /// What could not be settled; the next process recovers it as after a crash.
    fn settle(&mut self, journal: &mut dyn JournalWriter) -> Result<(), Cause>;
}

/// A host with nothing to run, which reads its streams at every call. It lets the process's
/// lifecycle run until a deployment's production cycle is wired in (DEC-968 item 6).
#[derive(Debug, Clone)]
pub struct IdleHost {
    pub streams: Vec<String>,
}

impl IdleHost {
    fn read_all(&self, journal: &dyn JournalWriter) -> Result<(), Cause> {
        self.streams
            .iter()
            .try_for_each(|stream| journal.read(stream).map(drop))
    }
}

impl Host for IdleHost {
    fn reconcile(&mut self, journal: &mut dyn JournalWriter) -> Result<(), Cause> {
        self.read_all(journal)
    }

    fn cycle(&mut self, journal: &mut dyn JournalWriter) -> Result<(), Cause> {
        self.read_all(journal)
    }

    fn settle(&mut self, journal: &mut dyn JournalWriter) -> Result<(), Cause> {
        self.read_all(journal)
    }
}

/// Why a supervisor stopped: the lifecycle's exits, shared with every long-running process
/// (DEC-994 item 7), so each keeps its stable code and exit status.
pub use mandate_process::Exit as Stop;

/// Where a supervisor is. Only `Ready` runs cycles and answers ready.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Starting,
    Ready,
    Draining,
    Stopped(Stop),
}

impl Phase {
    const ALL: [Self; 9] = [
        Self::Starting,
        Self::Ready,
        Self::Draining,
        Self::Stopped(Stop::Drained),
        Self::Stopped(Stop::Fenced),
        Self::Stopped(Stop::Refused),
        Self::Stopped(Stop::CycleFailed),
        Self::Stopped(Stop::DrainIncomplete),
        Self::Stopped(Stop::Completed),
    ];

    /// The shared lifecycle's state for this phase, which the probes answer from (DEC-994 item 7).
    pub fn state(self) -> State {
        match self {
            Self::Starting => State::Starting,
            Self::Ready => State::Ready,
            Self::Draining => State::Draining,
            Self::Stopped(stop) => State::Exited(stop),
        }
    }

    fn index(self) -> u8 {
        match self {
            Self::Starting => 0,
            Self::Ready => 1,
            Self::Draining => 2,
            Self::Stopped(Stop::Drained) => 3,
            Self::Stopped(Stop::Fenced) => 4,
            Self::Stopped(Stop::Refused) => 5,
            Self::Stopped(Stop::CycleFailed) => 6,
            Self::Stopped(Stop::DrainIncomplete) => 7,
            Self::Stopped(Stop::Completed) => 8,
        }
    }
}

/// The phase, shared with the health listener's thread.
#[derive(Debug, Clone)]
pub struct Status(Arc<AtomicU8>);

impl Default for Status {
    fn default() -> Self {
        Self(Arc::new(AtomicU8::new(Phase::Starting.index())))
    }
}

impl Status {
    pub fn phase(&self) -> Phase {
        let index = self.0.load(Ordering::SeqCst);
        Phase::ALL
            .into_iter()
            .find(|phase| phase.index() == index)
            .unwrap_or(Phase::Starting)
    }

    fn set(&self, phase: Phase) {
        self.0.store(phase.index(), Ordering::SeqCst);
    }
}

/// What a supervisor runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub environment: Environment,
    /// The deployment's own streams, its agent stream and its account stream (journal spec §2).
    pub streams: Vec<String>,
    /// Nanoseconds between the starts of two cycles.
    pub period_ns: u64,
}

/// One deployment's process.
pub struct Supervisor<S, H> {
    lease: Option<Lease<S>>,
    host: H,
    status: Status,
    period_ns: u64,
    next_cycle_ns: u64,
}

impl<S: JournalWriter + Epochs, H: Host> Supervisor<S, H> {
    /// Starts at `now_ns`: refuses any environment but paper before taking anything, takes the
    /// lease, and runs the host's reconciliation. Only then is it ready, with its first cycle due
    /// at once.
    pub fn start(store: S, host: H, config: &Config, status: Status, now_ns: u64) -> Self {
        let mut this = Self {
            lease: None,
            host,
            status,
            period_ns: config.period_ns,
            next_cycle_ns: now_ns,
        };
        if config.environment != Environment::Paper {
            this.stop(Stop::Refused);
            return this;
        }
        let Ok(mut lease) = Lease::acquire(store, &config.streams) else {
            this.stop(Stop::Refused);
            return this;
        };
        if this.host.reconcile(&mut lease).is_err() {
            this.stop(Stop::Refused);
            return this;
        }
        this.lease = Some(lease);
        this.status.set(Phase::Ready);
        this
    }

    pub fn phase(&self) -> Phase {
        self.status.phase()
    }

    /// Runs one cycle if one is due at `now_ns` and the process is ready. The lease is asked
    /// first; once it no longer holds, the process stops `Fenced` and the host is not called.
    pub fn tick(&mut self, now_ns: u64) {
        if self.phase() != Phase::Ready || now_ns < self.next_cycle_ns {
            return;
        }
        let Some(lease) = self.lease.as_mut().filter(|lease| lease.held()) else {
            self.stop(Stop::Fenced);
            return;
        };
        self.next_cycle_ns = now_ns.saturating_add(self.period_ns);
        if self.host.cycle(lease).is_err() {
            let stop = if lease.held() {
                Stop::CycleFailed
            } else {
                Stop::Fenced
            };
            self.stop(stop);
        }
    }

    /// Drains: no new cycle starts; while the lease holds, the host settles what is in flight;
    /// then the lease is released. A settle that fails stops `DrainIncomplete`, never `Drained`
    /// (`AGENTS.md` rule 3). A supervisor that already stopped is left as it is.
    pub fn drain(&mut self) {
        if matches!(self.phase(), Phase::Stopped(_)) {
            return;
        }
        self.status.set(Phase::Draining);
        let Some(lease) = self.lease.as_mut().filter(|lease| lease.held()) else {
            self.stop(Stop::Fenced);
            return;
        };
        let stop = match self.host.settle(lease) {
            Ok(()) => Stop::Drained,
            Err(_) => Stop::DrainIncomplete,
        };
        self.stop(stop);
    }

    fn stop(&mut self, stop: Stop) {
        self.lease.take().map(Lease::release);
        self.status.set(Phase::Stopped(stop));
    }
}
