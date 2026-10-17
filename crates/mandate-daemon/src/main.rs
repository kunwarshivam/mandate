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
//! `mandate-daemon`: one agent deployment's runtime process (E21-30 slice 1, DEC-1012).
//!
//! `mandate-daemon run --workspace <id> --agent <id> --connection <id> --owner <user> --probe
//! <socket> --fixture` runs it on the fixture wiring over an in-memory journal: it passes once a
//! second, or sooner on a wake hint, drains on SIGTERM, and exits with its lifecycle's status.
//! Without `--fixture` it refuses: a deployment's production ports are the post-R0 hook (DEC-968
//! item 6). `mandate-daemon probe livez|readyz --probe <socket>` is its `exec` probe (DEC-994
//! item 5). This is the one place the host clock is read (ES-05).

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mandate_daemon::wake::Sleep;
use mandate_daemon::{Config, RuntimeProcess, Wake, fixture};
use mandate_journal::Environment;
use mandate_process::State;
use mandate_process::probe::{self, Probe, Status};
use mandate_runtime::{AgentId, ConnectionId, Deployment, WorkspaceId};
use mandate_store::{MemoryStore, Store};
use mandate_time::UtcNanos;
use tokio::signal::unix::{SignalKind, signal};

const LIVENESS: Duration = Duration::from_secs(10);
const PASS: Duration = Duration::from_secs(1);

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
            let Some(asked) = args.get(1).and_then(|w| Probe::parse(w)) else {
                return ExitCode::from(2);
            };
            ExitCode::from(probe::query(&socket, asked).map_or(1, |a| a.exit_status()))
        }
        Some("run") => {
            let named = ["--workspace", "--agent", "--connection", "--owner"].map(flag);
            let [Some(workspace), Some(agent), Some(connection), Some(owner)] = named else {
                eprintln!("deployment_missing");
                return ExitCode::from(2);
            };
            if !args.iter().any(|a| a == "--fixture") {
                eprintln!("deployment_ports_not_wired");
                return ExitCode::from(2);
            }
            let config = Config {
                deployment: Deployment {
                    agent: AgentId(agent),
                    connection: ConnectionId(connection),
                    workspace: WorkspaceId(workspace),
                },
                environment: Environment::Paper,
                liveness: LIVENESS,
            };
            match run(&config, &owner, &socket) {
                Ok(status) => ExitCode::from(status),
                Err(code) => {
                    eprintln!("runtime_start_failed: {code}");
                    ExitCode::from(2)
                }
            }
        }
        _ => {
            eprintln!("usage: mandate-daemon run|probe");
            ExitCode::from(2)
        }
    }
}

#[allow(
    clippy::disallowed_methods,
    reason = "the binary's edge reads the host clock, which the process takes as an input (ES-05)"
)]
fn host_now() -> UtcNanos {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = i64::try_from(since.as_secs()).unwrap_or(i64::MAX);
    UtcNanos::from_parts(secs, since.subsec_nanos()).unwrap_or(UtcNanos::EPOCH)
}

fn run(config: &Config, owner: &str, socket: &Path) -> Result<u8, &'static str> {
    let terminate = Arc::new(AtomicBool::new(false));
    watch_terminate(Arc::clone(&terminate)).map_err(|_| "signal_unavailable")?;
    let process = RuntimeProcess::start(
        MemoryStore::default(),
        fixture::wiring(owner),
        config,
        host_now(),
    );
    serve(process, &mut Sleep, socket, &terminate)
}

fn serve<S: Store>(
    mut process: RuntimeProcess<S>,
    wake: &mut dyn Wake,
    socket: &Path,
    terminate: &AtomicBool,
) -> Result<u8, &'static str> {
    let status = Status::new(process.lifecycle().snapshot());
    probe::serve(socket, status.clone(), host_now).map_err(|_| "probe_socket_unavailable")?;
    loop {
        if let State::Exited(stopped) = process.lifecycle().state() {
            status.publish(process.lifecycle().snapshot());
            eprintln!("{}", stopped.code());
            return Ok(stopped.exit_status());
        }
        if terminate.load(Ordering::SeqCst) {
            process.drain(host_now());
            continue;
        }
        process.step(host_now());
        status.publish(process.lifecycle().snapshot());
        wake.wait(PASS);
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
