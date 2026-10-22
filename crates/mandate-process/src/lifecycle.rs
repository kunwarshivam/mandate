//! The lifecycle fold, the drain bound, and the crash-loop bound (infrastructure design §3.4,
//! §7.3).

use std::time::Duration;

use mandate_journal::AppendOutcome;
use mandate_time::UtcNanos;

/// How long a drain may take before it is abandoned (design §7.3, Proposed: 90 seconds).
pub const DRAIN_BOUND: Duration = Duration::from_secs(90);

/// The crash-loop bound (design §3.4, Proposed): this many restarts within
/// [`CRASH_LOOP_WINDOW`].
pub const CRASH_LOOP_RESTARTS: usize = 5;
/// The window of the crash-loop bound (design §3.4, Proposed: ten minutes).
pub const CRASH_LOOP_WINDOW: Duration = Duration::from_secs(600);

const NANOS_PER_SEC: i128 = 1_000_000_000;

/// What readiness asks of a process (design §3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Its writer epoch, its stream replayed to the head, and its last reconciliation passed.
    Executor,
    /// Its writer epoch, its stream replayed to the head, and its mode known, reported as
    /// [`Input::Reconciled`].
    Runtime,
    /// Its writer epoch and its stream replayed to the head: the scheduler, or any other single
    /// writer of one stream.
    Writer,
    /// Nothing beyond its start-up checks: a process that holds no writer epoch.
    Service,
}

impl Kind {
    fn holds_epoch(self) -> bool {
        !matches!(self, Self::Service)
    }

    fn reconciles(self) -> bool {
        matches!(self, Self::Executor | Self::Runtime)
    }
}

/// One thing a process observed. Every input is also a heartbeat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    /// The loop ran with nothing else to report.
    Heartbeat,
    /// The start-up checks passed (design §3.4). A check that fails is [`Input::Failed`].
    StartupPassed,
    /// The process took its stream's writer epoch. Replay positions and reconciliations observed
    /// before it no longer count: readiness needs them under the epoch (design §7.3 step 3). A
    /// second one changes nothing.
    EpochTaken,
    /// How far replay has read: `position` is the last sequence folded, `head` the stream's head.
    Replayed { position: u64, head: u64 },
    /// The last reconciliation's result (executor), or whether the mode is known (runtime).
    Reconciled { passed: bool },
    /// An append answered `Fenced`, or the lease no longer holds: a newer writer has the stream.
    Fenced,
    /// The orchestrator asked the process to drain. A repeated request keeps the first deadline.
    DrainRequested,
    /// The drain settled what was in flight. Outside a drain it changes nothing.
    Settled,
    /// A fail-closed stop: a start-up check, the reconciliation's error, a cycle, or a settle.
    Failed,
    /// The process found nothing left for it to do and exits [`Exit::Completed`] from any state
    /// it has not exited (DEC-2347). Only the executor sends it, once its pass finds every agent
    /// it follows stopped and nothing owed on the account; no other process may.
    Completed,
}

impl Input {
    /// The input an append's outcome reports, if any: only `Fenced` changes the lifecycle.
    pub fn from_append(outcome: &AppendOutcome) -> Option<Self> {
        matches!(outcome, AppendOutcome::Fenced { .. }).then_some(Self::Fenced)
    }
}

/// Why a process exited. Each has a stable code and exit status, and none carries content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// Drained on request.
    Drained,
    /// A newer writer took the stream; nothing further may be done.
    Fenced,
    /// It never became ready: a start-up check, the epoch, replay, or reconciliation failed.
    Refused,
    /// A cycle stopped fail-closed after the process was up.
    CycleFailed,
    /// A drain whose settle failed, or that passed [`DRAIN_BOUND`] and was abandoned, not forced;
    /// the next process recovers as after a crash.
    DrainIncomplete,
    /// Nothing was left for it to do (DEC-2347): status 0, which the deployment manager reads as
    /// succeeded, with its own code so it is never mistaken for a drain.
    Completed,
}

impl Exit {
    pub fn code(self) -> &'static str {
        match self {
            Self::Drained => "drained",
            Self::Fenced => "fenced",
            Self::Refused => "refused",
            Self::CycleFailed => "cycle_failed",
            Self::DrainIncomplete => "drain_incomplete",
            Self::Completed => "completed",
        }
    }

    pub fn exit_status(self) -> u8 {
        match self {
            Self::Drained | Self::Completed => 0,
            Self::Refused => 2,
            Self::Fenced => 3,
            Self::CycleFailed => 4,
            Self::DrainIncomplete => 5,
        }
    }
}

/// Where a process is. States only move forward in the order listed, except `Ready` and
/// `NotReady`, which alternate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Starting,
    Replaying,
    Ready,
    NotReady,
    Draining,
    Exited(Exit),
}

/// What a process asks before it acts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// A new decision cycle or opening: only while ready.
    Open,
    /// A risk exit or protective re-placement: once started, holding the epoch, and replayed
    /// under it, whether or not ready, draining included (design §3.4, §7.3).
    RiskExit,
    /// Applying or forwarding the kill switch: whenever the process is up (design §3.4).
    KillSwitch,
}

/// One process's lifecycle, folded from its inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lifecycle {
    kind: Kind,
    liveness: i128,
    now: i128,
    started: bool,
    epoch: bool,
    replayed: bool,
    at_head: bool,
    reconciled: bool,
    drain_from: Option<i128>,
    exit: Option<Exit>,
}

impl Lifecycle {
    /// A process of `kind` starting at `at`, live while it handles an input at least every
    /// `liveness`.
    pub fn new(kind: Kind, liveness: Duration, at: UtcNanos) -> Self {
        Self {
            kind,
            liveness: span(liveness),
            now: nanos(at),
            started: false,
            epoch: false,
            replayed: false,
            at_head: false,
            reconciled: false,
            drain_from: None,
            exit: None,
        }
    }

    /// Folds `input`, observed at `at`, and returns the new state. The fold's time is the latest
    /// `at` it has seen, so an earlier one never moves it back. A drain past its bound is
    /// abandoned before the input is applied, so an input that arrives after the bound, a
    /// `Fenced` one included, finds the process already exited `DrainIncomplete`; an exited
    /// process ignores every input. The bounds are judged only here: a loop must keep observing,
    /// at least a [`Input::Heartbeat`] each tick, and one that stops is caught by liveness.
    pub fn observe(&mut self, at: UtcNanos, input: Input) -> State {
        self.now = self.now.max(nanos(at));
        if self.exit.is_some() {
            return self.state();
        }
        if self
            .drain_from
            .is_some_and(|from| self.now.saturating_sub(from) > span(DRAIN_BOUND))
        {
            self.exit = Some(Exit::DrainIncomplete);
            return self.state();
        }
        match input {
            Input::Heartbeat => {}
            Input::StartupPassed => self.started = true,
            Input::EpochTaken => {
                if !self.epoch {
                    self.epoch = true;
                    self.replayed = false;
                    self.at_head = false;
                    self.reconciled = false;
                }
            }
            Input::Replayed { position, head } => {
                self.at_head = position >= head;
                self.replayed |= self.at_head;
            }
            Input::Reconciled { passed } => self.reconciled = passed,
            Input::Fenced => self.exit = Some(Exit::Fenced),
            Input::DrainRequested => {
                self.drain_from.get_or_insert(self.now);
            }
            Input::Settled => {
                if self.drain_from.is_some() {
                    self.exit = Some(Exit::Drained);
                }
            }
            Input::Failed => {
                self.exit = Some(match self.state() {
                    State::Starting | State::Replaying => Exit::Refused,
                    State::Ready | State::NotReady => Exit::CycleFailed,
                    State::Draining => Exit::DrainIncomplete,
                    State::Exited(exit) => exit,
                });
            }
            Input::Completed => self.exit = Some(Exit::Completed),
        }
        self.state()
    }

    pub fn state(&self) -> State {
        if let Some(exit) = self.exit {
            return State::Exited(exit);
        }
        if self.drain_from.is_some() {
            return State::Draining;
        }
        if !self.started {
            return State::Starting;
        }
        if !self.replayed_under_epoch() {
            return State::Replaying;
        }
        let at_head = self.at_head || !self.kind.holds_epoch();
        let reconciled = self.reconciled || !self.kind.reconciles();
        if at_head && reconciled {
            State::Ready
        } else {
            State::NotReady
        }
    }

    /// Whether the process may take `action` now. Nothing is permitted once it has exited.
    pub fn permits(&self, action: Action) -> bool {
        if self.exit.is_some() {
            return false;
        }
        match action {
            Action::Open => self.state() == State::Ready,
            Action::RiskExit => self.started && self.replayed_under_epoch(),
            Action::KillSwitch => true,
        }
    }

    /// Liveness at `now`: the process has not exited, and its last input was within its bound.
    pub fn live(&self, now: UtcNanos) -> bool {
        self.exit.is_none() && nanos(now).saturating_sub(self.now) <= self.liveness
    }

    /// What the probes read, as of the fold's time.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            state: self.state(),
            live_until: Some(self.now.saturating_add(self.liveness)),
        }
    }

    fn replayed_under_epoch(&self) -> bool {
        !self.kind.holds_epoch() || (self.epoch && self.replayed)
    }
}

/// A process's state and the time until which it counts as live, published for the probes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Snapshot {
    pub(crate) state: State,
    pub(crate) live_until: Option<i128>,
}

impl Snapshot {
    /// A snapshot with no heartbeat bound: live until it exits. For a process whose loop does not
    /// yet report heartbeats (`mandate-agentd`'s first slice, DEC-968 item 7).
    pub fn unbounded(state: State) -> Self {
        Self {
            state,
            live_until: None,
        }
    }

    pub fn state(self) -> State {
        self.state
    }
}

/// The crash-loop bound over a process's journaled restarts (design §3.4): whether at least
/// [`CRASH_LOOP_RESTARTS`] of them fall within [`CRASH_LOOP_WINDOW`] before `now`, both ends
/// included. A restart later than `now` counts too, so a clock behind the journal never hides a
/// loop. The caller reads the restarts from the journal, never from the orchestrator.
pub fn crash_loop<I: IntoIterator<Item = UtcNanos>>(restarts: I, now: UtcNanos) -> bool {
    let since = nanos(now).saturating_sub(span(CRASH_LOOP_WINDOW));
    restarts
        .into_iter()
        .filter(|restart| nanos(*restart) >= since)
        .count()
        >= CRASH_LOOP_RESTARTS
}

pub(crate) fn nanos(at: UtcNanos) -> i128 {
    i128::from(at.secs())
        .saturating_mul(NANOS_PER_SEC)
        .saturating_add(i128::from(at.nanos()))
}

fn span(duration: Duration) -> i128 {
    i128::try_from(duration.as_nanos()).unwrap_or(i128::MAX)
}
