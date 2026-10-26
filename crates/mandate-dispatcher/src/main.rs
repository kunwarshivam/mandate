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
//! `mandate-dispatcher`: one workspace's notification dispatcher (E8-10 slice D10, DEC-2500,
//! DEC-2516).
//!
//! `mandate-dispatcher run --config <path> --probe <socket>` reads its configuration file, takes
//! the notice stream's writer epoch over Postgres (`MANDATE_JOURNAL_DSN`, its own role), and steps
//! once a second, draining on SIGTERM and exiting with its lifecycle's status. Email goes only
//! through the recorded transport, which keeps each message and sends nothing (DEC-2511), and web
//! push stays off (DEC-2504). `mandate-dispatcher probe live|ready --probe <socket>` is its `exec`
//! probe (DEC-2500 item 5). This is the one place the host clock is read.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mandate_dispatcher::{
    Dispatcher, EmailProvider, OsRandom, PgNotices, RecordedTransport, read_config,
};
use mandate_journal::StreamId;
use mandate_notify::email::FounderAddress;
use mandate_process::State;
use mandate_process::probe::{self, Probe, Status};
use mandate_time::UtcNanos;
use tokio::signal::unix::{SignalKind, signal};

const TICK: Duration = Duration::from_secs(1);

/// The address the recorded transport is bound to. It sends nothing, so this is a fixture on the
/// reserved `.invalid` domain, never a person's address (DEC-2511, `AGENTS.md` rule 7).
const RECORDED_ADDRESS: &str = "founder@mail.invalid";

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
            let asked = match args.get(1).map(String::as_str) {
                Some("live") => Probe::parse("livez"),
                Some("ready") => Probe::parse("readyz"),
                _ => None,
            };
            let Some(asked) = asked else {
                return ExitCode::from(2);
            };
            let answer = probe::query(&socket, asked).map_or(1, |a| a.exit_status());
            ExitCode::from(answer)
        }
        Some("run") => {
            let Some(config) = flag("--config").map(PathBuf::from) else {
                eprintln!("config_missing");
                return ExitCode::from(2);
            };
            match run(&config, &socket) {
                Ok(status) => ExitCode::from(status),
                Err(code) => {
                    eprintln!("dispatcher_start_failed: {code}");
                    ExitCode::from(2)
                }
            }
        }
        _ => {
            eprintln!("usage: mandate-dispatcher run|probe");
            ExitCode::from(2)
        }
    }
}

#[allow(
    clippy::disallowed_methods,
    reason = "the binary's edge reads the host clock, which the dispatcher takes as an input"
)]
fn host_now() -> UtcNanos {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = i64::try_from(since.as_secs()).unwrap_or(i64::MAX);
    UtcNanos::from_parts(secs, since.subsec_nanos()).unwrap_or(UtcNanos::EPOCH)
}

fn run(config: &Path, socket: &Path) -> Result<u8, &'static str> {
    let bytes = std::fs::read(config).map_err(|_| "config_unreadable")?;
    let loaded = read_config(&bytes).map_err(|refused| refused.member)?;
    let terminate = Arc::new(AtomicBool::new(false));
    watch_terminate(Arc::clone(&terminate)).map_err(|_| "signal_unavailable")?;
    let address = FounderAddress::parse(RECORDED_ADDRESS).map_err(|_| "recorded_address")?;
    let transport = RecordedTransport::bound_to(address).map_err(|_| "recorded_transport")?;
    let provider =
        EmailProvider::new(loaded.founder.clone(), transport).map_err(|_| "email_provider")?;
    let dsn = std::env::var("MANDATE_JOURNAL_DSN").map_err(|_| "journal_dsn_missing")?;
    let mut journal = PgNotices::connect(&dsn).map_err(|_| "journal_unavailable")?;
    let notices =
        StreamId::parse(&format!("ntf:{}", loaded.settings.workspace)).ok_or("workspace")?;
    let writer = journal.take(notices).map_err(|_| "epoch_unavailable")?;
    let dispatcher = Dispatcher::start(
        journal,
        provider,
        OsRandom::new(),
        writer,
        loaded.settings,
        host_now(),
    );
    serve(dispatcher, socket, &terminate)
}

fn serve<J, P, R>(
    mut dispatcher: Dispatcher<J, P, R>,
    socket: &Path,
    terminate: &AtomicBool,
) -> Result<u8, &'static str>
where
    J: mandate_dispatcher::Journal,
    P: mandate_notify::Provider,
    R: mandate_notify::SecureRandom,
{
    let status = Status::new(dispatcher.lifecycle().snapshot());
    probe::serve(socket, status.clone(), host_now).map_err(|_| "probe_socket_unavailable")?;
    loop {
        if let State::Exited(stopped) = dispatcher.lifecycle().state() {
            status.publish(dispatcher.lifecycle().snapshot());
            eprintln!("{}", stopped.code());
            return Ok(stopped.exit_status());
        }
        if terminate.load(Ordering::SeqCst) {
            dispatcher.drain(host_now());
        }
        dispatcher.step(host_now());
        status.publish(dispatcher.lifecycle().snapshot());
        thread::sleep(TICK);
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
