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
//! `mandate-news-reader`: one workspace's news reader process (E2-10, DEC-2278, DEC-2280).
//!
//! `mandate-news-reader run --workspace <id> --probe <socket> --allowlist <file> --artifacts <dir>
//! --user-agent <text> [--follow <agent>=<account>]... (--vault-address <address> --vault-dir <dir>
//! | --recorded <file> | --fixture)` runs it over Postgres (`MANDATE_JOURNAL_DSN`) and the HTTPS transport, logged in
//! to the vault by AppRole as the news reader, with the role and secret ids and the vault's CA
//! mounted in `--vault-dir`; with `--fixture`, over an in-memory journal and no vault, so it holds
//! no key and stays not ready; with `--recorded`, at the `dev` and `ci` tiers alone, over Postgres
//! on the recording `<file>` and the fixture key, reaching no vault and no host (DEC-2085). It asks for a page at most once a second,
//! drains on SIGTERM, and exits with its lifecycle's status. A reason it is not ready is written
//! once on its standard error, as a code alone (DEC-2280 item 6).
//! `mandate-news-reader probe livez|readyz --probe <socket>` is its `exec` probe (DEC-994 item 5).
//! This is the one place the host clock is read (ES-05).

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mandate_artifacts_fs::FsArtifactStore;
use mandate_journal::{ArtifactStore, Environment};
use mandate_news_reader::parse_follows;
use mandate_news_reader::reader::exit;
use mandate_news_reader::recorded::{RecordedNews, admitted, recorded_key};
use mandate_news_reader::{Clock, Config, KeyRefusal, NewsReader, vault_key};
use mandate_process::probe::{self, Probe, Status};
use mandate_release::startup::TIER_VAR;
use mandate_sources::allowlist::Allowlist;
use mandate_sources::fetch::{NewsAuth, Transport};
use mandate_sources_http::HttpsTransport;
use mandate_store::{MemoryStore, PgStore, Store};
use mandate_time::UtcNanos;
use tokio::runtime::Runtime;
use tokio::signal::unix::{SignalKind, signal};

const LIVENESS: Duration = Duration::from_secs(10);
const STEP: Duration = Duration::from_secs(1);

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
        Some("run") => match run(&args, &flag, &socket) {
            Ok(status) => ExitCode::from(status),
            Err(code) => {
                eprintln!("news_reader_start_failed: {code}");
                ExitCode::from(2)
            }
        },
        _ => {
            eprintln!("usage: mandate-news-reader run|probe");
            ExitCode::from(2)
        }
    }
}

#[allow(
    clippy::disallowed_methods,
    reason = "the binary's edge reads the host clock, which the reader takes as an input (ES-05)"
)]
fn host_now() -> UtcNanos {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = i64::try_from(since.as_secs()).unwrap_or(i64::MAX);
    UtcNanos::from_parts(secs, since.subsec_nanos()).unwrap_or(UtcNanos::EPOCH)
}

/// The trusted time of DEC-2289: the host clock read once, at the loop's start, plus a monotonic
/// clock's elapsed time since, which a later step of the host clock cannot move.
struct Trusted {
    wall: UtcNanos,
    since: Instant,
}

impl Trusted {
    #[allow(
        clippy::disallowed_methods,
        reason = "the binary's edge starts the monotonic clock the trusted time counts from (ES-05)"
    )]
    fn start() -> Self {
        Self {
            wall: host_now(),
            since: Instant::now(),
        }
    }

    /// A step's readings: the host clock now, and the trusted time.
    fn clock(&self) -> Clock {
        Clock {
            now: host_now(),
            trusted: Clock::trusted_after(self.wall, self.since.elapsed()),
        }
    }
}

fn run(
    args: &[String],
    flag: &dyn Fn(&str) -> Option<String>,
    socket: &Path,
) -> Result<u8, &'static str> {
    let terminate = Arc::new(AtomicBool::new(false));
    watch_terminate(Arc::clone(&terminate)).map_err(|_| "signal_unavailable")?;
    let workspace = flag("--workspace").ok_or("workspace_missing")?;
    let allowlist = flag("--allowlist").ok_or("allowlist_missing")?;
    let allowlist = std::fs::read_to_string(allowlist).map_err(|_| "allowlist_unreadable")?;
    let allowlist = Allowlist::parse(&allowlist).map_err(|e| e.code())?;
    let artifacts = flag("--artifacts").ok_or("artifacts_missing")?;
    let artifacts = FsArtifactStore::open_existing(Path::new(&artifacts))
        .map_err(|_| "artifact_store_unavailable")?;
    let user_agent = flag("--user-agent").ok_or("user_agent_missing")?;
    let config = Config {
        workspace,
        environment: Environment::Paper,
        allowlist,
        follows: parse_follows(args),
        liveness: LIVENESS,
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "runtime_unavailable")?;
    if let Some(recording) = flag("--recorded") {
        admitted(std::env::var(TIER_VAR).ok().as_deref())?;
        let recording = std::fs::read(recording).map_err(|_| "recorded_news_unreadable")?;
        let recording = RecordedNews::parse(&recording)?;
        let dsn = std::env::var("MANDATE_JOURNAL_DSN").map_err(|_| "journal_dsn_missing")?;
        let store = PgStore::connect(&dsn)?;
        let reader = NewsReader::start(
            store,
            artifacts,
            recording,
            &config,
            recorded_key(),
            host_now(),
        );
        return serve(reader, &runtime, socket, &terminate);
    }
    let transport = HttpsTransport::new(&user_agent).map_err(|e| e.code())?;
    if args.iter().any(|a| a == "--fixture") {
        let reader = NewsReader::start(
            MemoryStore::default(),
            artifacts,
            transport,
            &config,
            Err(KeyRefusal::NoKey),
            host_now(),
        );
        return serve(reader, &runtime, socket, &terminate);
    }
    let key = key(flag, &runtime);
    let dsn = std::env::var("MANDATE_JOURNAL_DSN").map_err(|_| "journal_dsn_missing")?;
    let store = PgStore::connect(&dsn)?;
    let reader = NewsReader::start(store, artifacts, transport, &config, key, host_now());
    serve(reader, &runtime, socket, &terminate)
}

/// The key, read once by [`vault_key`] from the vault at `--vault-address` with what is mounted in
/// `--vault-dir`; either flag missing is `vault_unavailable`.
fn key(flag: &dyn Fn(&str) -> Option<String>, runtime: &Runtime) -> Result<NewsAuth, KeyRefusal> {
    let address = flag("--vault-address").ok_or(KeyRefusal::VaultUnavailable)?;
    let dir = flag("--vault-dir")
        .map(PathBuf::from)
        .ok_or(KeyRefusal::VaultUnavailable)?;
    runtime.block_on(vault_key(&address, &dir, host_now()))
}

#[allow(
    clippy::disallowed_methods,
    reason = "the binary's edge paces one step a second (ES-05)"
)]
fn serve<S: Store, A: ArtifactStore, T: Transport>(
    mut reader: NewsReader<S, A, T>,
    runtime: &Runtime,
    socket: &Path,
    terminate: &AtomicBool,
) -> Result<u8, &'static str> {
    let status = Status::new(reader.lifecycle().snapshot());
    probe::serve(socket, status.clone(), host_now).map_err(|_| "probe_socket_unavailable")?;
    if let Some(code) = reader.not_ready() {
        eprintln!("{code}");
    }
    let trusted = Trusted::start();
    loop {
        if let Some(stopped) = exit(&reader) {
            status.publish(reader.lifecycle().snapshot());
            eprintln!("{}", stopped.code());
            return Ok(stopped.exit_status());
        }
        if terminate.load(Ordering::SeqCst) {
            reader.drain(host_now());
            continue;
        }
        runtime.block_on(reader.step(trusted.clock()));
        for alert in reader.take_alerts() {
            eprintln!("{alert}");
        }
        status.publish(reader.lifecycle().snapshot());
        thread::sleep(STEP);
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
