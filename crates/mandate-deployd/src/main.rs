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
//! `mandate-deployd`: one workspace's deployment manager (E21-33 slice 2, DEC-1172).
//!
//! `mandate-deployd run --workspace <id> --probe <socket> --templates <dir> [--fixture]` runs it
//! over Postgres (`MANDATE_JOURNAL_DSN`) and the Kubernetes API, reached with the pod's service
//! account files, or, with `--fixture`, over an in-memory journal and the fake orchestrator. It
//! passes once a second, drains on SIGTERM, and exits with its lifecycle's status.
//! `mandate-deployd probe livez|readyz --probe <socket>` is its `exec` probe (DEC-994 item 5).
//! This is the one place the host clock is read (ES-05).

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mandate_canon::Digest;
use mandate_deploy_manager::fake::Fake;
use mandate_deploy_manager::kubernetes::{InCluster, Kubernetes};
use mandate_deploy_manager::{Alerts, ControlJournal, Deployer, Identity, Orchestrator, Resolver};
use mandate_deployd::{MemoryControl, NoStore, PgControl, StderrAlerts, input, templates};
use mandate_journal::StreamId;
use mandate_journal_pg::PgJournal;
use mandate_process::probe::{self, Probe, Status};
use mandate_process::{Input, Kind, Lifecycle, State};
use mandate_time::UtcNanos;
use tokio::signal::unix::{SignalKind, signal};

const LIVENESS: Duration = Duration::from_secs(30);
const PASS: Duration = Duration::from_secs(1);
const ACCOUNT: &str = "/var/run/secrets/kubernetes.io/serviceaccount";
const API: &str = "https://kubernetes.default.svc";

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
            let (Some(workspace), Some(dir)) = (flag("--workspace"), flag("--templates")) else {
                eprintln!("workspace_or_templates_missing");
                return ExitCode::from(2);
            };
            let fixture = args.iter().any(|a| a == "--fixture");
            match run(&workspace, Path::new(&dir), &socket, fixture) {
                Ok(status) => ExitCode::from(status),
                Err(code) => {
                    eprintln!("deploy_manager_start_failed: {code}");
                    ExitCode::from(2)
                }
            }
        }
        _ => {
            eprintln!("usage: mandate-deployd run|probe");
            ExitCode::from(2)
        }
    }
}

#[allow(
    clippy::disallowed_methods,
    reason = "the binary's edge reads the host clock, which the manager takes as an input (ES-05)"
)]
fn host_now() -> UtcNanos {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = i64::try_from(since.as_secs()).unwrap_or(i64::MAX);
    UtcNanos::from_parts(secs, since.subsec_nanos()).unwrap_or(UtcNanos::EPOCH)
}

fn run(workspace: &str, dir: &Path, socket: &Path, fixture: bool) -> Result<u8, &'static str> {
    let terminate = Arc::new(AtomicBool::new(false));
    watch_terminate(Arc::clone(&terminate)).map_err(|_| "signal_unavailable")?;
    let templates = templates(dir).map_err(|e| e.code())?;
    let identity = Identity {
        workspace: workspace.to_owned(),
        actor_id: "deploy_manager".to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        build: Digest::of(concat!("mandate-deployd/", env!("CARGO_PKG_VERSION")).as_bytes()),
    };
    let stream = StreamId::parse(&identity.stream()).ok_or("workspace_invalid")?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .map_err(|_| "runtime_unavailable")?;
    if fixture {
        let deployer = Deployer::new(
            identity,
            MemoryControl::new(stream),
            Fake::new(),
            StderrAlerts::default(),
            NoStore,
        );
        return runtime.block_on(serve(deployer, socket, &terminate));
    }
    let account = Path::new(ACCOUNT);
    let namespace =
        std::fs::read_to_string(account.join("namespace")).map_err(|_| "namespace_unreadable")?;
    let orchestrator = Kubernetes::in_cluster(InCluster {
        api: API.to_owned(),
        token_file: account.join("token"),
        ca_file: account.join("ca.crt"),
        namespace: namespace.trim().to_owned(),
        workspace: workspace.to_owned(),
        templates,
    })
    .map_err(|_| "orchestrator_unavailable")?;
    let dsn = std::env::var("MANDATE_JOURNAL_DSN").map_err(|_| "journal_dsn_missing")?;
    runtime.block_on(async {
        let journal = PgJournal::from_dsn(&dsn).map_err(|e| e.code())?;
        let deployer = Deployer::new(
            identity,
            PgControl::new(journal, stream),
            orchestrator,
            StderrAlerts::default(),
            NoStore,
        );
        serve(deployer, socket, &terminate).await
    })
}

async fn serve<J: ControlJournal, O: Orchestrator, A: Alerts, R: Resolver>(
    mut deployer: Deployer<J, O, A, R>,
    socket: &Path,
    terminate: &AtomicBool,
) -> Result<u8, &'static str> {
    let mut lifecycle = Lifecycle::new(Kind::Service, LIVENESS, host_now());
    lifecycle.observe(host_now(), Input::StartupPassed);
    let status = Status::new(lifecycle.snapshot());
    probe::serve(socket, status.clone(), host_now).map_err(|_| "probe_socket_unavailable")?;
    let mut last = Ok(());
    loop {
        if let State::Exited(stopped) = lifecycle.state() {
            status.publish(lifecycle.snapshot());
            eprintln!("{}", stopped.code());
            return Ok(stopped.exit_status());
        }
        if terminate.load(Ordering::SeqCst) {
            lifecycle.observe(host_now(), Input::DrainRequested);
            lifecycle.observe(host_now(), Input::Settled);
            continue;
        }
        let now = host_now();
        let pass = deployer.tick(now).await;
        if let Err(halt) = &pass
            && pass != last
        {
            eprintln!("deploy_manager_pass_halted: {halt:?}");
        }
        lifecycle.observe(now, input(&pass));
        status.publish(lifecycle.snapshot());
        last = pass;
        tokio::time::sleep(PASS).await;
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
