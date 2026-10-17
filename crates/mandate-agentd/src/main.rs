#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]
//! `mandate-agentd`: one deployment's long-running process (E21-2, DEC-968).
//!
//! Slice 1 hosts only [`IdleHost`] over an in-memory journal, behind `--fixture`; without it the
//! process refuses, because no deployment's production cycle is wired in yet (DEC-968 item 6).
//! It prints `health <addr>` once it is listening and SIGTERM is handled, drains on SIGTERM, and
//! exits with its stop's status. SIGINT is not handled: it ends the process as a crash does, which
//! the next process recovers from the journal.

use std::cell::RefCell;
use std::net::SocketAddr;
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use mandate_agentd::{Config, IdleHost, MemoryStore, Status, Supervisor, health};
use mandate_journal::{Environment, MemoryJournal};
use mandate_time::UtcNanos;
use tokio::signal::unix::{SignalKind, signal};

const STREAMS: [&str; 2] = ["agent:fixture:agent_a", "acct:fixture:paper_a"];
const TICK: Duration = Duration::from_millis(10);

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.iter().any(|arg| arg == "--fixture") {
        eprintln!("deployment_host_not_wired");
        return ExitCode::from(2);
    }
    match run() {
        Ok(status) => ExitCode::from(status),
        Err(error) => {
            eprintln!("agentd_start_failed: {}", error.kind());
            ExitCode::from(2)
        }
    }
}

#[allow(
    clippy::disallowed_methods,
    reason = "the binary's edge reads a monotonic clock only to pace its schedule; the supervisor takes time as an input (ES-05)"
)]
fn run() -> std::io::Result<u8> {
    let terminate = Arc::new(AtomicBool::new(false));
    watch_terminate(Arc::clone(&terminate))?;
    let streams: Vec<String> = STREAMS.into_iter().map(str::to_owned).collect();
    let config = Config {
        environment: Environment::Paper,
        streams: streams.clone(),
        period_ns: 1_000_000_000,
    };
    let store = MemoryStore::new(Rc::new(RefCell::new(MemoryJournal::new())), UtcNanos::EPOCH);
    let status = Status::default();
    let addr = health::serve(SocketAddr::from(([127, 0, 0, 1], 0)), status.clone())?;
    println!("health {addr}");
    let started = Instant::now();
    let now = || u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
    let mut supervisor = Supervisor::start(store, IdleHost { streams }, &config, status, now());
    loop {
        if let mandate_agentd::Phase::Stopped(stop) = supervisor.phase() {
            eprintln!("{}", stop.code());
            return Ok(stop.exit_status());
        }
        if terminate.load(Ordering::SeqCst) {
            supervisor.drain();
            continue;
        }
        supervisor.tick(now());
        thread::sleep(TICK);
    }
}

/// Registers SIGTERM before the process announces itself, and sets `terminate` when it arrives.
fn watch_terminate(terminate: Arc<AtomicBool>) -> std::io::Result<()> {
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
