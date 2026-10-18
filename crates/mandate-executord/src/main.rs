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
//! `mandate-executord`: one broker account's executor process (E21-30, DEC-2003, DEC-2007).
//!
//! - `run --probe <socket> [--workspace <id> --account <ref>] [--fixture]` runs the process over
//!   Postgres (`MANDATE_JOURNAL_DSN`) or, with `--fixture`, an in-memory journal. It passes once a
//!   second, drains on SIGTERM, and exits with its lifecycle's status. A pod the deployment manager
//!   starts names its workspace and account by variables instead ([`launch`]).
//! - `connect-check --workspace <id> --account <ref> --connection <id> --pii-ref <ref> [--fixture]`
//!   journals the connect check of a paper Alpaca connection and prints `checked <event id>` or
//!   `check_failed <event id>` ([`connect`]).
//! - `probe livez|readyz --probe <socket>` is its `exec` probe (DEC-994 item 5).
//!
//! The broker is the fixture paper broker in every mode: this binary compiles no host transport
//! and reads no credential, so it reaches no broker host (E21-30; `AGENTS.md` rules 7 and 8). It
//! runs the fixture wiring ([`fixture`]), which admits no opening. This is the one place the host
//! clock is read (ES-05).

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mandate_executor::{AccountRef, AccountScope, WorkspaceId};
use mandate_executord::alpaca::AlpacaBroker;
use mandate_executord::connect::connect_check;
use mandate_executord::paper::FixturePaper;
use mandate_executord::{Config, ExecutorProcess, fixture, launch};
use mandate_journal::Environment;
use mandate_process::State;
use mandate_process::probe::{self, Probe, Status};
use mandate_store::{MemoryStore, PgStore, Store};
use mandate_time::UtcNanos;
use tokio::signal::unix::{SignalKind, signal};

const LIVENESS: Duration = Duration::from_secs(10);
const PASS: Duration = Duration::from_secs(1);
/// How many broker answers deep one input may lead (DEC-2003 item 7).
const MAX_CHAIN: usize = 64;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i.checked_add(1)?))
            .cloned()
    };
    let var = |name: &str| match std::env::var(name) {
        Ok(value) => Some(value),
        Err(std::env::VarError::NotUnicode(_)) => Some(String::new()),
        Err(std::env::VarError::NotPresent) => None,
    };
    let fixture = args.iter().any(|a| a == "--fixture");
    match args.first().map(String::as_str) {
        Some("probe") => {
            let (Some(socket), Some(asked)) = (
                flag("--probe").map(PathBuf::from),
                args.get(1).and_then(|w| Probe::parse(w)),
            ) else {
                return ExitCode::from(2);
            };
            ExitCode::from(probe::query(&socket, asked).map_or(1, |a| a.exit_status()))
        }
        Some("run") => {
            let Some(socket) = flag("--probe").map(PathBuf::from) else {
                eprintln!("probe_socket_missing");
                return ExitCode::from(2);
            };
            let named = match launch::named(&flag, &var) {
                Ok(named) => named,
                Err(code) => {
                    eprintln!("{code}");
                    return ExitCode::from(2);
                }
            };
            match run(&named, &socket, fixture) {
                Ok(status) => ExitCode::from(status),
                Err(code) => {
                    eprintln!("executor_start_failed: {code}");
                    ExitCode::from(2)
                }
            }
        }
        Some("connect-check") => {
            let (Some(workspace), Some(account), Some(connection), Some(pii)) = (
                flag("--workspace"),
                flag("--account"),
                flag("--connection"),
                flag("--pii-ref"),
            ) else {
                eprintln!("connect_check_arguments_missing");
                return ExitCode::from(2);
            };
            let named = launch::Named { workspace, account };
            match check(&named, &connection, &pii, fixture) {
                Ok(code) => ExitCode::from(code),
                Err(code) => {
                    eprintln!("connect_check_failed: {code}");
                    ExitCode::from(2)
                }
            }
        }
        _ => {
            eprintln!("usage: mandate-executord run|connect-check|probe");
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

fn scope(named: &launch::Named) -> AccountScope {
    AccountScope {
        account: AccountRef(named.account.clone()),
        workspace: WorkspaceId(named.workspace.clone()),
    }
}

fn run(named: &launch::Named, socket: &Path, fixture: bool) -> Result<u8, &'static str> {
    let terminate = Arc::new(AtomicBool::new(false));
    watch_terminate(Arc::clone(&terminate)).map_err(|_| "signal_unavailable")?;
    if fixture {
        return serve(MemoryStore::default(), named, socket, &terminate);
    }
    let dsn = std::env::var("MANDATE_JOURNAL_DSN").map_err(|_| "journal_dsn_missing")?;
    serve(PgStore::connect(&dsn)?, named, socket, &terminate)
}

fn serve<S: Store>(
    store: S,
    named: &launch::Named,
    socket: &Path,
    terminate: &AtomicBool,
) -> Result<u8, &'static str> {
    let broker = AlpacaBroker::new(FixturePaper::new(host_now()))?;
    let config = Config {
        scope: scope(named),
        environment: Environment::Paper,
        liveness: LIVENESS,
        max_chain: MAX_CHAIN,
    };
    let mut process =
        ExecutorProcess::start(store, broker, fixture::wiring()?, &config, host_now());
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
        thread::sleep(PASS);
    }
}

fn check(
    named: &launch::Named,
    connection: &str,
    pii: &str,
    fixture: bool,
) -> Result<u8, &'static str> {
    if fixture {
        return checked(MemoryStore::default(), named, connection, pii);
    }
    let dsn = std::env::var("MANDATE_JOURNAL_DSN").map_err(|_| "journal_dsn_missing")?;
    checked(PgStore::connect(&dsn)?, named, connection, pii)
}

fn checked<S: Store>(
    mut store: S,
    named: &launch::Named,
    connection: &str,
    pii: &str,
) -> Result<u8, &'static str> {
    let mut broker = AlpacaBroker::new(FixturePaper::new(host_now()))?;
    let checked = connect_check(
        &mut store,
        &mut broker,
        &fixture::Ids,
        &scope(named),
        connection,
        pii,
        host_now(),
    )?;
    if checked.passed {
        println!("checked {}", checked.event_id);
        Ok(0)
    } else {
        println!("check_failed {}", checked.event_id);
        Ok(1)
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
