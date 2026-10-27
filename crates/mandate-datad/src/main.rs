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
//! `mandate-datad`: one workspace's data process in the local cell.
//!
//! `mandate-datad run --store <root> --artifacts <root> --instruments
//! <symbol>:<asset-id>:<YYYY-MM-DD>=<close>[,...] --probe <socket>` stores, for each instrument, the
//! recorded series ([`mandate_datad::series`]) under `--store`: its daily bars, and the latest
//! trading day's minute bars. It then puts the closes object the runtime's observation names
//! ([`mandate_datad::observation`]) in the artifact store at `--artifacts`. It does this once at
//! start, which is its start-up check, and again every [`series::REFRESH`], so a session that
//! completes while it runs is added. Any failed write exits, fail closed, and the orchestrator
//! restarts it. It drains on SIGTERM. `mandate-datad probe livez|readyz --probe <socket>` is its
//! `exec` probe (DEC-994 item 5). This is the one place the host clock is read (ES-05).
//!
//! `mandate-datad run --source alpaca-iex --workspace <id> --store <root>/alpaca-iex --artifacts
//! <root> --vault-address <address> --vault-dir <dir> --probe <socket>` is the hosted source
//! (data-plane spec §3.8, DEC-2283): it reads the platform data key once as the bars reader, from
//! the vault at `--vault-address` with the AppRole credentials and CA mounted in `--vault-dir`,
//! and the workspace's control stream `ctl:<id>` read-only over Postgres (`MANDATE_JOURNAL_DSN`),
//! again every [`series::REFRESH`]; at every pass it writes the generations due under `--store`
//! and puts their closes objects in `--artifacts`. It logs counts and codes only. A refused key
//! stops it, failed, and the orchestrator's restart reads the key again.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mandate_artifacts_fs::FsArtifactStore;
use mandate_bars::generations;
use mandate_datad::iex::generation::Writer;
use mandate_datad::iex::instruments::{Registered, registered};
use mandate_datad::iex::key::{platform_key, vault_config};
use mandate_datad::iex::pass::Pass;
use mandate_datad::iex::schedule::Jitter;
use mandate_datad::iex::source::ClientSource;
use mandate_datad::{Instrument, observation, series};
use mandate_journal::StreamId;
use mandate_journal_pg::PgJournal;
use mandate_marketdata::client::{Client, TokioPause};
use mandate_marketdata::http::AlpacaDataHttp;
use mandate_process::probe::{self, Probe, Status};
use mandate_process::{Input, Kind, Lifecycle, State};
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
        Some("run") if flag("--source").as_deref() == Some("alpaca-iex") => {
            let (Some(workspace), Some(store), Some(artifacts), Some(address), Some(dir)) = (
                flag("--workspace"),
                flag("--store"),
                flag("--artifacts"),
                flag("--vault-address"),
                flag("--vault-dir"),
            ) else {
                eprintln!("iex_arguments_missing");
                return ExitCode::from(2);
            };
            let target = IexTarget {
                workspace,
                store: PathBuf::from(store),
                artifacts: PathBuf::from(artifacts),
                address,
                dir: PathBuf::from(dir),
            };
            match run_iex(&target, &socket) {
                Ok(status) => ExitCode::from(status),
                Err(code) => {
                    eprintln!("data_start_failed: {code}");
                    ExitCode::from(2)
                }
            }
        }
        Some("run") if flag("--source").is_some_and(|source| source != "recorded") => {
            eprintln!("source_unknown");
            ExitCode::from(2)
        }
        Some("run") => {
            let (Some(store), Some(artifacts), Some(instruments)) =
                (flag("--store"), flag("--artifacts"), flag("--instruments"))
            else {
                eprintln!("store_or_instruments_missing");
                return ExitCode::from(2);
            };
            let Ok(instruments) = Instrument::parse_list(&instruments) else {
                eprintln!("instruments_malformed");
                return ExitCode::from(2);
            };
            let target = Target {
                store: PathBuf::from(store),
                artifacts: PathBuf::from(artifacts),
                instruments,
            };
            match run(&target, &socket) {
                Ok(status) => ExitCode::from(status),
                Err(code) => {
                    eprintln!("data_start_failed: {code}");
                    ExitCode::from(2)
                }
            }
        }
        _ => {
            eprintln!("usage: mandate-datad run|probe");
            ExitCode::from(2)
        }
    }
}

#[allow(
    clippy::disallowed_methods,
    reason = "the binary's edge reads the host clock, which the series takes as an input (ES-05)"
)]
fn host_now() -> UtcNanos {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = i64::try_from(since.as_secs()).unwrap_or(i64::MAX);
    UtcNanos::from_parts(secs, since.subsec_nanos()).unwrap_or(UtcNanos::EPOCH)
}

/// Where the series and their closes objects go, and for which instruments.
struct Target {
    store: PathBuf,
    artifacts: PathBuf,
    instruments: Vec<Instrument>,
}

/// Stores each instrument's daily series, its minute bars, and then its closes object; the
/// objects' references in the instruments' order, or why not.
fn store_once(
    target: &Target,
    calendar: &ExchangeCalendar,
    now: UtcNanos,
) -> Result<Vec<String>, series::SeriesError> {
    let mut stored = Vec::with_capacity(target.instruments.len());
    for instrument in &target.instruments {
        let (symbol, anchor) = (&instrument.symbol, &instrument.anchor);
        let published = series::publish(&target.store, symbol, anchor, calendar, now)?;
        series::publish_minutes(&target.store, symbol, anchor, calendar, now)?;
        let closes = observation::put_closes(
            &target.artifacts,
            &published.dir,
            symbol,
            &instrument.asset_id,
            now,
        )?;
        stored.push(closes.digest().to_hex());
    }
    Ok(stored)
}

#[allow(
    clippy::disallowed_methods,
    reason = "the binary's edge reads a monotonic clock only to pace its loop (ES-05)"
)]
fn run(target: &Target, socket: &Path) -> Result<u8, &'static str> {
    let terminate = Arc::new(AtomicBool::new(false));
    watch_terminate(Arc::clone(&terminate)).map_err(|_| "signal_unavailable")?;
    let calendar = ExchangeCalendar::us_equities().map_err(|e| e.code())?;
    let mut lifecycle = Lifecycle::new(Kind::Service, LIVENESS, host_now());
    let status = Status::new(lifecycle.snapshot());
    probe::serve(socket, status.clone(), host_now).map_err(|_| "probe_socket_unavailable")?;
    let mut stored_at: Option<Instant> = None;
    let mut last_closes: Vec<String> = Vec::new();
    loop {
        if let State::Exited(exit) = lifecycle.state() {
            status.publish(lifecycle.snapshot());
            eprintln!("{}", exit.code());
            return Ok(exit.exit_status());
        }
        let now = host_now();
        if terminate.load(Ordering::SeqCst) {
            lifecycle.observe(now, Input::DrainRequested);
            lifecycle.observe(now, Input::Settled);
            continue;
        }
        let due = series::due(stored_at.map(|at| at.elapsed()));
        let input = if !due {
            Input::Heartbeat
        } else {
            match store_once(target, &calendar, now) {
                Ok(closes) => {
                    if closes != last_closes {
                        for digest in &closes {
                            eprintln!("closes_stored sha256:{digest}");
                        }
                        last_closes = closes;
                    }
                    let first = stored_at.is_none();
                    stored_at = Some(Instant::now());
                    if first {
                        Input::StartupPassed
                    } else {
                        Input::Heartbeat
                    }
                }
                Err(error) => {
                    eprintln!("series_not_stored: {}", error.code());
                    Input::Failed
                }
            }
        };
        lifecycle.observe(now, input);
        status.publish(lifecycle.snapshot());
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

/// Where the IEX source writes, for which workspace, and the vault it reads the key from.
struct IexTarget {
    workspace: String,
    store: PathBuf,
    artifacts: PathBuf,
    address: String,
    dir: PathBuf,
}

/// The workspace's registered instruments, read from its control stream and the artifact store.
fn registrations(
    runtime: &tokio::runtime::Runtime,
    journal: &PgJournal,
    stream: &StreamId,
    artifacts: &Path,
) -> Result<Registered, &'static str> {
    let rows = runtime
        .block_on(journal.rows(stream))
        .map_err(|e| e.code())?;
    let store = FsArtifactStore::open_existing(artifacts).map_err(|_| "artifacts_unavailable")?;
    Ok(registered(&rows, &store))
}

#[allow(
    clippy::disallowed_methods,
    reason = "the binary's edge reads a monotonic clock only to pace its loop (ES-05)"
)]
fn run_iex(target: &IexTarget, socket: &Path) -> Result<u8, &'static str> {
    if !generations::is_generational(&target.store) {
        return Err("store_not_generations");
    }
    let stream =
        StreamId::parse(&format!("ctl:{}", target.workspace)).ok_or("workspace_invalid")?;
    let terminate = Arc::new(AtomicBool::new(false));
    watch_terminate(Arc::clone(&terminate)).map_err(|_| "signal_unavailable")?;
    let calendar = ExchangeCalendar::us_equities().map_err(|e| e.code())?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "runtime_unavailable")?;
    let config = vault_config(&target.address, &target.dir)?;
    let key = runtime.block_on(platform_key(&config, &target.dir, host_now()))?;
    let transport = AlpacaDataHttp::bars_only(key).map_err(|e| e.code())?;
    let source = ClientSource::new(Client::new(transport, TokioPause))?;
    let dsn = std::env::var("MANDATE_JOURNAL_DSN").map_err(|_| "journal_dsn_missing")?;
    let journal = runtime
        .block_on(async { PgJournal::from_dsn(&dsn) })
        .map_err(|e| e.code())?;
    let writer =
        Writer::new(target.store.clone(), target.artifacts.clone()).map_err(|e| e.code())?;
    let mut pass = Pass::new(writer, Jitter::of(&target.workspace));
    let mut lifecycle = Lifecycle::new(Kind::Service, LIVENESS, host_now());
    let status = Status::new(lifecycle.snapshot());
    probe::serve(socket, status.clone(), host_now).map_err(|_| "probe_socket_unavailable")?;
    let mut read_at: Option<Instant> = None;
    let mut held: Option<Registered> = None;
    let mut started = false;
    loop {
        if let State::Exited(exit) = lifecycle.state() {
            status.publish(lifecycle.snapshot());
            eprintln!("{}", exit.code());
            return Ok(exit.exit_status());
        }
        let now = host_now();
        if terminate.load(Ordering::SeqCst) {
            lifecycle.observe(now, Input::DrainRequested);
            lifecycle.observe(now, Input::Settled);
            continue;
        }
        if series::due(read_at.map(|at| at.elapsed())) {
            match registrations(&runtime, &journal, &stream, &target.artifacts) {
                Ok(registered) => held = Some(registered),
                Err(code) => eprintln!("registrations_not_read: {code}"),
            }
            read_at = Some(Instant::now());
        }
        let input = match &held {
            None => Input::Heartbeat,
            Some(registered) => {
                let tick = pass.step(&source, &calendar, registered, now);
                for line in &tick.log {
                    eprintln!("{line}");
                }
                if tick.stop {
                    Input::Failed
                } else if started {
                    Input::Heartbeat
                } else {
                    started = true;
                    Input::StartupPassed
                }
            }
        };
        lifecycle.observe(now, input);
        status.publish(lifecycle.snapshot());
        thread::sleep(TICK);
    }
}
