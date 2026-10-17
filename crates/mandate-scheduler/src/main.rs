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
//! `mandate-scheduler`: one workspace's scheduler process (E21-29, DEC-1010).
//!
//! `mandate-scheduler run --workspace <id> --probe <socket> --servers <a,b,…> [--fixture]` runs it
//! over Postgres (`MANDATE_JOURNAL_DSN`) or, with `--fixture`, an in-memory journal. It ticks once
//! per second of the monotonic clock, drains on SIGTERM, and exits with its lifecycle's status.
//! `mandate-scheduler probe livez|readyz --probe <socket>` is its `exec` probe (DEC-994 item 5).
//! This is the one place the host clock is read (ES-05).

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mandate_journal::Environment;
use mandate_process::probe::{self, Probe, Status};
use mandate_scheduler::scheduler::exit;
use mandate_scheduler::sntp::Servers;
use mandate_scheduler::store::{MemoryStore, PgStore};
use mandate_scheduler::{ClockStore, Config, Scheduler, measure};
use mandate_time::{ExchangeCalendar, UtcNanos};
use tokio::signal::unix::{SignalKind, signal};

const LIVENESS: Duration = Duration::from_secs(10);
const TICK: Duration = Duration::from_secs(1);

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i.checked_add(1)?))
            .cloned()
    };
    let Some(socket) = flag("--probe").map(PathBuf::from) else {
        eprintln!("probe_socket_missing");
        return ExitCode::from(2);
    };
    match args.first().map(String::as_str) {
        Some("probe") => {
            let asked = match args.get(1).and_then(|w| Probe::parse(w)) {
                Some(asked) => asked,
                None => return ExitCode::from(2),
            };
            let answer = probe::query(&socket, asked).map_or(1, |a| a.exit_status());
            ExitCode::from(answer)
        }
        Some("run") => {
            let Some(workspace) = flag("--workspace") else {
                eprintln!("workspace_missing");
                return ExitCode::from(2);
            };
            let servers: Vec<String> = flag("--servers")
                .map(|s| s.split(',').map(str::to_owned).collect())
                .unwrap_or_default();
            let fixture = args.iter().any(|a| a == "--fixture");
            match run(&workspace, servers, &socket, fixture) {
                Ok(status) => ExitCode::from(status),
                Err(code) => {
                    eprintln!("scheduler_start_failed: {code}");
                    ExitCode::from(2)
                }
            }
        }
        _ => {
            eprintln!("usage: mandate-scheduler run|probe");
            ExitCode::from(2)
        }
    }
}

#[allow(
    clippy::disallowed_methods,
    reason = "the binary's edge reads the host clock, which the scheduler takes as an input (ES-05)"
)]
fn host_now() -> UtcNanos {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = i64::try_from(since.as_secs()).unwrap_or(i64::MAX);
    UtcNanos::from_parts(secs, since.subsec_nanos()).unwrap_or(UtcNanos::EPOCH)
}

fn run(
    workspace: &str,
    servers: Vec<String>,
    socket: &std::path::Path,
    fixture: bool,
) -> Result<u8, &'static str> {
    let terminate = Arc::new(AtomicBool::new(false));
    watch_terminate(Arc::clone(&terminate)).map_err(|_| "signal_unavailable")?;
    let calendar = ExchangeCalendar::us_equities().map_err(|e| e.code())?;
    let config = Config {
        workspace: workspace.to_owned(),
        environment: Environment::Paper,
        tolerance_ns: measure::TOLERANCE_NS,
        liveness: LIVENESS,
    };
    let servers: Servers<fn() -> UtcNanos> = Servers {
        names: servers,
        now: host_now,
    };
    if fixture {
        let scheduler = Scheduler::start(MemoryStore::default(), &config, calendar, host_now());
        return serve(scheduler, servers, socket, &terminate);
    }
    let dsn = std::env::var("MANDATE_JOURNAL_DSN").map_err(|_| "journal_dsn_missing")?;
    let store = PgStore::connect(&dsn)?;
    let scheduler = Scheduler::start(store, &config, calendar, host_now());
    serve(scheduler, servers, socket, &terminate)
}

#[allow(
    clippy::disallowed_methods,
    reason = "the binary's edge reads a monotonic clock only to pace one tick per second (ES-05)"
)]
fn serve<S: ClockStore>(
    mut scheduler: Scheduler<S>,
    mut servers: Servers<fn() -> UtcNanos>,
    socket: &std::path::Path,
    terminate: &AtomicBool,
) -> Result<u8, &'static str> {
    let status = Status::new(scheduler.lifecycle().snapshot());
    probe::serve(socket, status.clone(), host_now).map_err(|_| "probe_socket_unavailable")?;
    let started = Instant::now();
    let mut seconds: u32 = 0;
    loop {
        if let Some(stopped) = exit(&scheduler) {
            status.publish(scheduler.lifecycle().snapshot());
            eprintln!("{}", stopped.code());
            return Ok(stopped.exit_status());
        }
        if terminate.load(Ordering::SeqCst) {
            scheduler.drain(host_now());
            continue;
        }
        scheduler.step(host_now(), &mut servers);
        status.publish(scheduler.lifecycle().snapshot());
        seconds = seconds.saturating_add(1);
        let next = TICK.saturating_mul(seconds);
        thread::sleep(next.saturating_sub(started.elapsed()));
    }
}

/// Registers SIGTERM before the loop starts, and sets `terminate` when it arrives.
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
