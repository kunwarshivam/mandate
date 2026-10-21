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
//! - `run --probe <socket> [--workspace <id> --account <ref>] [--fixture]
//!   [--broker alpaca-paper --vault-address <address> --vault-dir <dir>]` runs the process over
//!   Postgres (`MANDATE_JOURNAL_DSN`) or, with `--fixture`, an in-memory journal. It passes once a
//!   second, drains on SIGTERM, and exits with its lifecycle's status. A pod the deployment manager
//!   starts names its workspace and account by variables instead ([`launch`]), and is refused
//!   `--fixture` (`fixture_under_manager`), here and for `connect-check`.
//! - `connect-check --workspace <id> --account <ref> --connection <id> --pii-ref <ref> [--fixture]
//!   [--broker ...]` journals the connect check of a paper Alpaca connection and prints
//!   `checked <event id>` or `check_failed <event id>` ([`connect`]).
//! - `probe livez|readyz --probe <socket>` is its `exec` probe (DEC-994 item 5).
//!
//! The broker is the fixture paper broker unless `--broker alpaca-paper` names Alpaca's paper
//! trading host (DEC-2306 item 1). Then the process trusts the vault's CA mounted in
//! `--vault-dir`, logs in by Kubernetes auth with the pod's projected ServiceAccount token, takes
//! the lease on the connection the account stream's latest connect check passed for, and only
//! then takes the account stream; each pass ticks the
//! lease, a lost or rotated credential drains the process, and the lease is given back when it
//! stops ([`credential`]). It runs the fixture wiring ([`fixture`]), which admits no opening. This
//! is the one place the host clock is read (ES-05).

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mandate_connections::record::ConnectionId;
use mandate_executor::{AccountRef, AccountScope, WorkspaceId};
use mandate_executord::alpaca::AlpacaBroker;
use mandate_executord::connect::connect_check;
use mandate_executord::credential::{
    RunLease, VaultMount, checked_connection, paper_broker, paper_settings, vault_config,
    vault_login,
};
use mandate_executord::paper::FixturePaper;
use mandate_executord::{Broker, Config, ExecutorProcess, fixture, launch};
use mandate_journal::Environment;
use mandate_process::State;
use mandate_process::probe::{self, Probe, Status};
use mandate_store::{MemoryStore, PgStore, Store};
use mandate_time::UtcNanos;
use mandate_vault::mounted::{SERVICE_ACCOUNT_TOKEN_DIR, read_service_account_token};
use mandate_vault::openbao::OpenBaoSource;
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
    if let Err(code) = launch::fixture_permitted(fixture, &var) {
        eprintln!("{code}");
        return ExitCode::from(2);
    }
    let paper = match paper_settings(&flag, fixture) {
        Ok(paper) => paper,
        Err(code) => {
            eprintln!("{code}");
            return ExitCode::from(2);
        }
    };
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
            match run(&named, &socket, fixture, paper.as_ref()) {
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
            match check(&named, &connection, &pii, fixture, paper.as_ref()) {
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

/// The lease the paper broker is held under.
type PaperLease = RunLease<OpenBaoSource>;

fn run(
    named: &launch::Named,
    socket: &Path,
    fixture: bool,
    paper: Option<&VaultMount>,
) -> Result<u8, &'static str> {
    let terminate = Arc::new(AtomicBool::new(false));
    watch_terminate(Arc::clone(&terminate)).map_err(|_| "signal_unavailable")?;
    if fixture {
        return serve(MemoryStore::default(), named, socket, &terminate, paper);
    }
    let dsn = std::env::var("MANDATE_JOURNAL_DSN").map_err(|_| "journal_dsn_missing")?;
    serve(PgStore::connect(&dsn)?, named, socket, &terminate, paper)
}

/// The broker the run names, before the process takes the account stream: the fixture, or
/// Alpaca's paper host over the lease on the checked connection, refused without one.
fn serve<S: Store>(
    store: S,
    named: &launch::Named,
    socket: &Path,
    terminate: &AtomicBool,
    paper: Option<&VaultMount>,
) -> Result<u8, &'static str> {
    let Some(mount) = paper else {
        let broker = AlpacaBroker::new(FixturePaper::new(host_now()))?;
        return serve_on(store, broker, None, named, socket, terminate);
    };
    let connection = checked_connection(&store, &scope(named))?;
    let (broker, mut lease) = leased_broker(mount, &connection)?;
    let served = serve_on(store, broker, Some(&mut lease), named, socket, terminate);
    if let Err(code) = lease.give_back() {
        eprintln!("{code}");
    }
    served
}

/// Logs in as `connection`'s executor and takes its lease and broker.
fn leased_broker(
    mount: &VaultMount,
    connection: &ConnectionId,
) -> Result<(impl Broker, PaperLease), &'static str> {
    let config = vault_config(mount)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "runtime_unavailable")?;
    let vault = vault_login(
        || read_service_account_token(Path::new(SERVICE_ACCOUNT_TOKEN_DIR)),
        |identity, jwt| {
            runtime.block_on(OpenBaoSource::login_kubernetes(
                &config,
                identity,
                &jwt,
                host_now(),
            ))
        },
        connection,
    )?;
    paper_broker(vault, connection, host_now())
}

fn serve_on<S: Store, B: Broker>(
    store: S,
    broker: B,
    mut lease: Option<&mut PaperLease>,
    named: &launch::Named,
    socket: &Path,
    terminate: &AtomicBool,
) -> Result<u8, &'static str> {
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
        let credential_stop = lease.as_deref_mut().and_then(|held| held.pass(host_now()));
        let terminated = credential_stop.is_some() || terminate.load(Ordering::SeqCst);
        if terminated {
            process.drain(host_now());
        }
        if let State::Exited(stopped) = process.lifecycle().state() {
            status.publish(process.lifecycle().snapshot());
            if let Some(code) = credential_stop {
                eprintln!("{code}");
            }
            eprintln!("{}", stopped.code());
            return Ok(stopped.exit_status());
        }
        if terminated {
            return Err(credential_stop.unwrap_or("drain_unfinished"));
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
    paper: Option<&VaultMount>,
) -> Result<u8, &'static str> {
    if fixture {
        return checked(MemoryStore::default(), named, connection, pii, paper);
    }
    let dsn = std::env::var("MANDATE_JOURNAL_DSN").map_err(|_| "journal_dsn_missing")?;
    checked(PgStore::connect(&dsn)?, named, connection, pii, paper)
}

/// The connect check over the broker the run names: on Alpaca's paper host, under the lease on
/// the connection being checked, which is given back when the check is journaled.
fn checked<S: Store>(
    store: S,
    named: &launch::Named,
    connection: &str,
    pii: &str,
    paper: Option<&VaultMount>,
) -> Result<u8, &'static str> {
    let Some(mount) = paper else {
        let broker = AlpacaBroker::new(FixturePaper::new(host_now()))?;
        return checked_on(store, broker, named, connection, pii);
    };
    let id = ConnectionId::new(connection).map_err(|_| "connection_invalid")?;
    let (broker, lease) = leased_broker(mount, &id)?;
    let result = checked_on(store, broker, named, connection, pii);
    if let Err(code) = lease.give_back() {
        eprintln!("{code}");
    }
    result
}

fn checked_on<S: Store, B: Broker>(
    mut store: S,
    mut broker: B,
    named: &launch::Named,
    connection: &str,
    pii: &str,
) -> Result<u8, &'static str> {
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
