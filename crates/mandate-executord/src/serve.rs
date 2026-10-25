//! The executor's run behind journal rule 68's gate (DEC-2341), and the wait before it on a
//! connection that is still connecting (DEC-2346; connections spec §5.2 step 4).
//!
//! The binary's edge is handed in as closures ([`Edge`]): the host clock, the lease's tick,
//! SIGTERM, and stderr; the probe reads the [`Status`] both publish. Everything the run decides is
//! here, so tests drive it with a fixture wiring that would admit an opening.

use std::thread;
use std::time::Duration;

use mandate_journal::StreamId;
use mandate_process::probe::Status;
use mandate_process::{Input, Kind, Lifecycle, Snapshot, State};
use mandate_store::Store;
use mandate_time::UtcNanos;

use crate::establish::{Establishment, GateReport, OpeningGate, pass, read_establishment};
use crate::follow::agents_of;
use crate::run::{Run, Unimplemented};
use crate::{Broker, Config, ExecutorProcess, Wiring};

/// What the run is handed from the binary's edge.
pub struct Edge<'a> {
    /// The host clock, the one place it is read (ES-05).
    pub now: &'a dyn Fn() -> UtcNanos,
    /// The lease's tick at a time: the code that stops the run, or `None` while it goes on and
    /// for a run that holds no lease.
    pub tick: &'a mut dyn FnMut(UtcNanos) -> Option<&'static str>,
    /// Whether SIGTERM has arrived.
    pub terminated: &'a dyn Fn() -> bool,
    /// Where each code goes: one line of stderr.
    pub say: &'a mut dyn FnMut(&str),
    /// The pause between passes.
    pub interval: Duration,
}

/// How a wait ended without refusing the start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waited {
    /// The account stream is established, or due its copy: the process may start.
    Ready,
    /// SIGTERM arrived first: nothing was started, so nothing is owed.
    Stopped,
    /// The connection was refused or revoked before its copy, and no agent is deployed on the
    /// account: nothing was started and nothing is owed, so the executor ends cleanly (DEC-2346
    /// item 2, DEC-2347's agentless executor).
    Closed,
}

/// How long a start may take, from the process's first instant to the wait's first pass (the
/// vault login, the exchanged check and the lease), before the probe reports it stalled and the
/// kubelet restarts it.
pub const STARTUP: Duration = Duration::from_secs(120);

/// What the probe reads while the executor starts: starting, and live only for [`STARTUP`] from
/// `now`, so a start that hangs is restarted rather than held live forever.
pub fn starting(now: UtcNanos) -> Snapshot {
    Lifecycle::new(Kind::Executor, STARTUP, now).snapshot()
}

/// DEC-2346: before the process starts, waits until the account stream in `config`'s scope is
/// established or due its copy, reading the journal once a pass and ticking the lease, and
/// publishes on `status` a lifecycle that is live and not ready. The wait appends nothing and asks
/// no broker anything. While it waits no agent can be deployed on the connection, so the platform
/// owes no exit or protective order (`AGENTS.md` rule 13).
///
/// # Errors
/// `connection_closed` for a connection refused or revoked before its copy while an agent is
/// deployed on the account ([`Waited::Closed`] when none is), the establishment's
/// own code for a record that does not read, `stream_unavailable` for a stream that cannot be
/// read, and the lease's code when its tick stops the run: each exits before anything started.
pub fn wait<S: Store>(
    store: &S,
    config: &Config,
    status: &Status,
    edge: &mut Edge<'_>,
) -> Result<Waited, &'static str> {
    let mut lifecycle = Lifecycle::new(Kind::Executor, config.liveness, (edge.now)());
    loop {
        match read_establishment(store, &config.scope, config.environment)? {
            Establishment::Established | Establishment::ToCopy(_) => return Ok(Waited::Ready),
            Establishment::Closed => return closed(store, config),
            Establishment::Connecting => {}
        }
        let now = (edge.now)();
        lifecycle.observe(now, Input::Heartbeat);
        status.publish(lifecycle.snapshot());
        if let Some(code) = (edge.tick)(now) {
            return Err(code);
        }
        if (edge.terminated)() {
            return Ok(Waited::Stopped);
        }
        thread::sleep(edge.interval);
    }
}

/// How a wait ends on a closed connection: cleanly when no agent is deployed on the account, and
/// as `connection_closed` while one is.
///
/// # Errors
/// `connection_closed` while an agent is deployed, `stream_unavailable` for a control stream that
/// cannot be read, and `connection_record_unreadable` for a deployment that does not read.
fn closed<S: Store>(store: &S, config: &Config) -> Result<Waited, &'static str> {
    let control = StreamId::parse(&format!("ctl:{}", config.scope.workspace.0))
        .and_then(|stream| store.rows(&stream))
        .ok_or("stream_unavailable")?;
    let following = agents_of(&control, &config.scope, config.environment)
        .map_err(|_| "connection_record_unreadable")?;
    if following.agents.is_empty() {
        Ok(Waited::Closed)
    } else {
        Err("connection_closed")
    }
}

/// A broker a test scripts, which [`serve`] runs with any wiring (DEC-2359 item 9). The library
/// implements it for none of its own brokers, and the orphan rule keeps the binary from
/// implementing it for one of them, so the binary serves a library broker only as a [`Run`]
/// through [`serve_run`]:
///
/// ```compile_fail,E0277
/// fn scripted<B: mandate_executord::serve::ScriptedBroker>() {}
/// scripted::<mandate_executord::alpaca::AlpacaBroker<mandate_executord::paper::FixturePaper>>();
/// ```
///
/// This guards against an accidental swap, not intent: a type of the binary's own that wraps a
/// broker and implements [`Broker`] and this trait could still be served with any wiring.
/// `tests/paper_only.rs` refuses such an implementation in `src/main.rs`.
pub trait ScriptedBroker: Broker {}

/// Runs `run`'s broker with the wiring paired with it, as [`serve`] runs a scripted one: the
/// binary's only way to serve.
///
/// # Errors
/// As [`serve`]'s.
pub fn serve_run<S: Store, B: Broker>(
    store: S,
    run: Run<B>,
    config: &Config,
    status: &Status,
    edge: Edge<'_>,
) -> Result<u8, &'static str> {
    let _ = (store, run, config, status, edge);
    Err(Unimplemented)
}

/// Runs the process over `store` and `broker` with `wiring` behind rule 68's gate: the gate
/// guards `wiring`'s binding-gate source, and after each step a pass writes the establishment's
/// copy when it is due and opens the gate once the stream holds it. Each gate code is said once
/// per change, and the lifecycle is published on `status` each pass. A lease that stops the run,
/// or SIGTERM, drains it; it answers the exit status of the lifecycle it ended in.
///
/// # Errors
/// `drain_unfinished`, or the lease's code, when a drain does not end the lifecycle.
pub fn serve<S: Store, B: Broker>(
    store: S,
    broker: B,
    mut wiring: Wiring,
    config: &Config,
    status: &Status,
    edge: Edge<'_>,
) -> Result<u8, &'static str> {
    let Edge {
        now,
        tick,
        terminated,
        say,
        interval,
    } = edge;
    let gate = OpeningGate::default();
    wiring.gate = gate.guard(wiring.gate);
    let mut process = ExecutorProcess::start(store, broker, wiring, config, now());
    status.publish(process.lifecycle().snapshot());
    let mut report = GateReport::default();
    loop {
        let stop = tick(now());
        let stopping = stop.is_some() || terminated();
        if stopping {
            process.drain(now());
        }
        if let State::Exited(stopped) = process.lifecycle().state() {
            status.publish(process.lifecycle().snapshot());
            if let Some(code) = stop {
                say(code);
            }
            say(stopped.code());
            return Ok(stopped.exit_status());
        }
        if stopping {
            return Err(stop.unwrap_or("drain_unfinished"));
        }
        process.step(now());
        let passed = pass(
            &mut process,
            &gate,
            &config.scope,
            config.environment,
            now(),
        );
        if let Some(code) = report.changed(passed) {
            say(code);
        }
        status.publish(process.lifecycle().snapshot());
        thread::sleep(interval);
    }
}
