//! The dispatcher as a long-running process (E8-10 slice D10, DEC-2500 items 3, 4 and 6;
//! DEC-2516): one [`step`] per tick over the journal, under `mandate-process`'s writer lifecycle.
//!
//! - **Start.** The configuration was read and the notice stream's writer epoch taken before the
//!   process is built, so it starts `StartupPassed` and `EpochTaken`; it is ready once it has read
//!   its notice stream to the head, which a read failure only postpones.
//! - **Each tick.** It reads the control stream for the workspace's subject streams
//!   ([`subjects`]), then runs one [`step`]. A read that fails, or an append the journal answers
//!   `Unavailable`, `Ambiguous` or `HeadMismatch`, ends the tick with nothing more sent, and is no
//!   heartbeat, so a dispatcher that cannot read past its liveness bound is restarted. An append
//!   answered `Fenced` is `Fenced`: a newer dispatcher has the stream, and this one sends nothing
//!   more. Anything else the step refuses, an `Invalid` append included, is `Failed`: the process
//!   exits rather than loop over a record it cannot write (DEC-2500 item 4).
//! - **The drain.** A step runs to its end before the next input is read, so no send is in flight
//!   between ticks: a drain request settles at once.
//! - **The loop.** [`serve`] answers the probes on the process's socket and steps once a `tick`
//!   until the lifecycle exits, turning a SIGTERM [`watch_terminate`] saw into a drain, and returns
//!   the exit status the lifecycle gives.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use mandate_journal::{Environment, StreamId};
use mandate_notify::{Origin, Provider, PushChannel, SecureRandom};
use mandate_process::probe::{self, Status};
use mandate_process::{Input, Kind, Lifecycle, State};
use mandate_time::UtcNanos;
use tokio::signal::unix::{SignalKind, signal};

use crate::DispatchError;
use crate::step::{Config, Journal, NoticeWriter, step};
use crate::subjects::subjects;

/// How long the process may go without a step whose reads succeeded before its liveness probe
/// fails (DEC-2500 item 4): the notice lag alert's bound.
pub const LIVENESS: std::time::Duration = std::time::Duration::from_secs(60);

/// What the process sends under, read from its configuration file at start (DEC-2500 item 7).
#[derive(Debug, Clone)]
pub struct Settings {
    pub workspace: String,
    /// The configured subject streams: a floor, never a filter.
    pub floor: Vec<StreamId>,
    /// Each recipient and the push channel every notice goes to it on (DEC-704 item 4).
    pub audience: Vec<(String, PushChannel)>,
    pub origin: Origin,
    pub environment: Environment,
    /// This binary's digest, `sha256:<64 hex>`.
    pub build: String,
}

/// One dispatcher over its journal, provider and random source.
pub struct Dispatcher<J: Journal, P: Provider, R: SecureRandom> {
    lifecycle: Lifecycle,
    journal: J,
    provider: P,
    random: R,
    writer: NoticeWriter,
    settings: Settings,
    replayed: bool,
}

impl<J: Journal, P: Provider, R: SecureRandom> Dispatcher<J, P, R> {
    /// The process at `now`, its configuration read and `writer`'s epoch taken.
    pub fn start(
        journal: J,
        provider: P,
        random: R,
        writer: NoticeWriter,
        settings: Settings,
        now: UtcNanos,
    ) -> Self {
        let mut lifecycle = Lifecycle::new(Kind::Writer, LIVENESS, now);
        lifecycle.observe(now, Input::StartupPassed);
        lifecycle.observe(now, Input::EpochTaken);
        Self {
            lifecycle,
            journal,
            provider,
            random,
            writer,
            settings,
            replayed: false,
        }
    }

    /// One tick at `now`: the replay until it has reached the head, then the subject streams and
    /// one step. Nothing runs once the process has exited.
    pub fn step(&mut self, now: UtcNanos) {
        if matches!(self.lifecycle.state(), State::Exited(_)) {
            return;
        }
        if self.lifecycle.state() == State::Draining {
            self.lifecycle.observe(now, Input::Settled);
            return;
        }
        if !self.replayed {
            let Ok(stream) = self.writer.stream() else {
                self.lifecycle.observe(now, Input::Failed);
                return;
            };
            let Ok(rows) = self.journal.committed(stream) else {
                return;
            };
            let head = rows.last().map_or(0, |row| row.seq);
            self.lifecycle.observe(
                now,
                Input::Replayed {
                    position: head,
                    head,
                },
            );
            self.replayed = true;
        }
        let Some(control) = StreamId::parse(&format!("ctl:{}", self.settings.workspace)) else {
            self.lifecycle.observe(now, Input::Failed);
            return;
        };
        let Ok(rows) = self.journal.committed(&control) else {
            return;
        };
        let streams = subjects(
            &self.settings.workspace,
            &rows,
            self.settings.environment,
            &self.settings.floor,
        );
        let audience: Vec<(&str, PushChannel)> = self
            .settings
            .audience
            .iter()
            .map(|(recipient, channel)| (recipient.as_str(), *channel))
            .collect();
        let config = Config {
            writer: &self.writer,
            subjects: &streams,
            audience: &audience,
            origin: &self.settings.origin,
            environment: self.settings.environment,
            build: &self.settings.build,
        };
        let ran = step(
            &config,
            &mut self.journal,
            &mut self.random,
            &mut self.provider,
            now,
        );
        if let Some(input) = outcome(&ran) {
            self.lifecycle.observe(now, input);
        }
    }

    /// SIGTERM: no new step starts, and the next tick settles the drain.
    pub fn drain(&mut self, now: UtcNanos) {
        self.lifecycle.observe(now, Input::DrainRequested);
    }

    pub fn lifecycle(&self) -> &Lifecycle {
        &self.lifecycle
    }

    pub fn journal(&self) -> &J {
        &self.journal
    }

    pub fn provider(&self) -> &P {
        &self.provider
    }
}

/// The lifecycle input one step's result reports, or none for a step that sent nothing more and
/// tries again at the next tick (DEC-2500 item 4).
pub fn outcome(ran: &Result<(), DispatchError>) -> Option<Input> {
    match ran {
        Ok(()) => Some(Input::Heartbeat),
        Err(DispatchError::JournalUnavailable) => None,
        Err(DispatchError::Fenced { .. }) => Some(Input::Fenced),
        Err(DispatchError::NotCommitted { outcome })
            if matches!(*outcome, "Unavailable" | "Ambiguous" | "HeadMismatch") =>
        {
            None
        }
        Err(_) => Some(Input::Failed),
    }
}

/// Serves `dispatcher`'s probes on `socket` and steps it once a `tick`, reading `clock` for each
/// input, until its lifecycle exits; a set `terminate` is a drain request. Returns the lifecycle's
/// exit status.
///
/// # Errors
/// `probe_socket_unavailable` when the probe socket cannot be served.
pub fn serve<J, P, R, C>(
    mut dispatcher: Dispatcher<J, P, R>,
    socket: &Path,
    terminate: &AtomicBool,
    clock: C,
    tick: Duration,
) -> Result<u8, &'static str>
where
    J: Journal,
    P: Provider,
    R: SecureRandom,
    C: Fn() -> UtcNanos + Clone + Send + Sync + 'static,
{
    let status = Status::new(dispatcher.lifecycle().snapshot());
    probe::serve(socket, status.clone(), clock.clone()).map_err(|_| "probe_socket_unavailable")?;
    loop {
        if let State::Exited(stopped) = dispatcher.lifecycle().state() {
            status.publish(dispatcher.lifecycle().snapshot());
            return Ok(stopped.exit_status());
        }
        if terminate.load(Ordering::SeqCst) {
            dispatcher.drain(clock());
        }
        dispatcher.step(clock());
        status.publish(dispatcher.lifecycle().snapshot());
        thread::sleep(tick);
    }
}

/// Registers SIGTERM before the loop starts, and sets `terminate` when it arrives, so the process
/// drains rather than being killed by the signal.
///
/// # Errors
/// When the signal handler or its runtime cannot be set up.
pub fn watch_terminate(terminate: Arc<AtomicBool>) -> std::io::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .build()?;
    let mut sigterm = {
        let _entered = runtime.enter();
        signal(SignalKind::terminate())?
    };
    thread::spawn(move || {
        runtime.block_on(sigterm.recv());
        terminate.store(true, Ordering::SeqCst);
    });
    Ok(())
}
