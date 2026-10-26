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
//! `mandate-datad run --store <root> --artifacts <root> --symbol <symbol> --asset-id <id> --anchor
//! <YYYY-MM-DD>=<close> --probe <socket>` stores the recorded series ([`mandate_datad::series`])
//! under `--store`, and the closes object the runtime's observation names
//! ([`mandate_datad::observation`]) in the artifact store at `--artifacts`, once at start,
//! which is its start-up check, and again every [`series::REFRESH`], so a session that completes while it
//! runs is added. Any failed write exits, fail closed, and the orchestrator restarts it. It drains
//! on SIGTERM. `mandate-datad probe livez|readyz --probe <socket>` is its `exec` probe (DEC-994
//! item 5). This is the one place the host clock is read (ES-05).

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mandate_datad::{Anchor, observation, series};
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
        Some("run") => {
            let (Some(store), Some(artifacts), Some(symbol), Some(asset), Some(anchor)) = (
                flag("--store"),
                flag("--artifacts"),
                flag("--symbol"),
                flag("--asset-id"),
                flag("--anchor"),
            ) else {
                eprintln!("store_symbol_or_anchor_missing");
                return ExitCode::from(2);
            };
            let Ok(anchor) = Anchor::parse(&anchor) else {
                eprintln!("anchor_malformed");
                return ExitCode::from(2);
            };
            let target = Target {
                store: PathBuf::from(store),
                artifacts: PathBuf::from(artifacts),
                symbol,
                asset,
            };
            match run(&target, &anchor, &socket) {
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

/// Where the series and its closes object go, and for which instrument.
struct Target {
    store: PathBuf,
    artifacts: PathBuf,
    symbol: String,
    asset: String,
}

/// Stores the series and then its closes object; the object's reference, or why not.
fn store_once(
    target: &Target,
    anchor: &Anchor,
    calendar: &ExchangeCalendar,
    now: UtcNanos,
) -> Result<String, series::SeriesError> {
    let published = series::publish(&target.store, &target.symbol, anchor, calendar, now)?;
    let closes = observation::put_closes(
        &target.artifacts,
        &published.dir,
        &target.symbol,
        &target.asset,
        now,
    )?;
    Ok(closes.digest().to_hex())
}

#[allow(
    clippy::disallowed_methods,
    reason = "the binary's edge reads a monotonic clock only to pace its loop (ES-05)"
)]
fn run(target: &Target, anchor: &Anchor, socket: &Path) -> Result<u8, &'static str> {
    let terminate = Arc::new(AtomicBool::new(false));
    watch_terminate(Arc::clone(&terminate)).map_err(|_| "signal_unavailable")?;
    let calendar = ExchangeCalendar::us_equities().map_err(|e| e.code())?;
    let mut lifecycle = Lifecycle::new(Kind::Service, LIVENESS, host_now());
    let status = Status::new(lifecycle.snapshot());
    probe::serve(socket, status.clone(), host_now).map_err(|_| "probe_socket_unavailable")?;
    let mut stored_at: Option<Instant> = None;
    let mut last_closes = String::new();
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
            match store_once(target, anchor, &calendar, now) {
                Ok(closes) => {
                    if closes != last_closes {
                        eprintln!("closes_stored sha256:{closes}");
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
