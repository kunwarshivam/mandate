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
//! `mandate-gatewayd`: one workspace's model-gateway service (E21-62, DEC-2257, DEC-2265,
//! DEC-2326).
//!
//! `mandate-gatewayd run --recorded <research.json> --listen <addr> --probe <socket>` serves the
//! runtime's framed calls over the recorded transport, metering through the journaled meter
//! streams in Postgres (`MANDATE_JOURNAL_DSN`) under the budgets its environment names
//! (`mandate_gatewayd::config`), and stores the runtime's research artifacts in the research store
//! `MANDATE_RESEARCH_ARTIFACTS` names, create-only and capped (DEC-2279). It reaches no provider
//! and reads no key. `mandate-gatewayd probe
//! livez|readyz --probe <socket>` is its `exec` probe (DEC-994 item 5). A live start is refused
//! (`mandate_gatewayd::run::command`). Every refusal exits 2 with its code alone on stderr.

use std::net::TcpListener;
use std::path::Path;
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mandate_artifacts_fs::FsArtifactStore;
use mandate_gateway::wire::PutRefusal;
use mandate_gateway_http::SystemClock;
use mandate_gatewayd::config::Config;
use mandate_gatewayd::control::StoreControl;
use mandate_gatewayd::recorded::Replies;
use mandate_gatewayd::run::{
    self, Command, Parts, RunArgs, WriteLog, answer_recorded, heartbeat, host_now, observe,
    recorded_service,
};
use mandate_gatewayd::serve::{Limits, Server};
use mandate_gatewayd::writer::ArtifactWriter;
use mandate_process::lifecycle::{Input, Kind, Lifecycle};
use mandate_process::probe::{self, Status};
use mandate_store::PgStore;
use mandate_time::UtcNanos;

/// How long the process stays live without a heartbeat.
const LIVENESS: Duration = Duration::from_secs(10);
/// How often the heartbeat thread reports the process live while it serves.
const HEARTBEAT: Duration = Duration::from_secs(1);
/// Each socket read and write's bound: a call's deadline is at most a minute and a half
/// (inference spec §1.2), and a stalled runtime gives up its connection no later than this.
const IO_TIMEOUT: Duration = Duration::from_secs(120);
/// The most calls served at once: one runtime's turns, with room for its restarts.
const MAX_CONNECTIONS: usize = 16;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run::command(&args) {
        Ok(Command::Probe { asked, socket }) => {
            ExitCode::from(probe::query(&socket, asked).map_or(1, |a| a.exit_status()))
        }
        Ok(Command::Run(args)) => match serve(&args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(code) => refuse(code),
        },
        Err(error) => refuse(error.code()),
    }
}

fn refuse(code: &str) -> ExitCode {
    eprintln!("{code}");
    ExitCode::from(2)
}

/// Starts the recorded service and serves it until the listener stops.
fn serve(args: &RunArgs) -> Result<(), &'static str> {
    let started = host_now();
    let lifecycle = Lifecycle::new(Kind::Service, LIVENESS, started);
    let status = Status::new(lifecycle.snapshot());
    let lifecycle = Arc::new(Mutex::new(lifecycle));
    probe::serve(&args.probe, status.clone(), host_now).map_err(|_| "probe_socket_unavailable")?;
    let serving = Arc::new(AtomicBool::new(true));
    heartbeat(
        Arc::clone(&lifecycle),
        status.clone(),
        Arc::clone(&serving),
        HEARTBEAT,
        host_now,
    );
    let served = serve_started(args, started, &lifecycle, &status);
    serving.store(false, Ordering::SeqCst);
    observe(&lifecycle, &status, host_now(), Input::Failed);
    served
}

/// The start's steps after the probe socket, then the serve loop until the listener stops.
fn serve_started(
    args: &RunArgs,
    started: UtcNanos,
    lifecycle: &Mutex<Lifecycle>,
    status: &Status,
) -> Result<(), &'static str> {
    let config = Config::from_env(&|name| std::env::var(name).ok()).map_err(|e| e.code())?;
    let model = run::read_recorded(&args.recorded).map_err(|e| e.code())?;
    let artifacts = FsArtifactStore::open_existing(&config.artifacts)
        .map_err(|_| "artifact_store_unavailable")?;
    let mut writer = config
        .research_artifacts
        .as_deref()
        .map(|root| ArtifactWriter::open(Path::new(root), &config.workspace))
        .transpose()
        .map_err(|_| "research_artifacts_unavailable")?;
    let control = StoreControl::new(PgStore::connect(config.journal_dsn())?, &config.workspace)
        .ok_or("workspace_missing")?;
    let parts = Parts {
        workspace_store: PgStore::connect(config.journal_dsn())?,
        deployment_store: PgStore::connect(config.journal_dsn())?,
        clock: SystemClock,
        now: Rc::new(host_now),
        ids: run::ids(&config, started),
        control: Box::new(control),
        artifacts: Box::new(artifacts),
    };
    let mut service = recorded_service(&config, &model, parts).map_err(|e| e.code())?;
    let replies = Replies::new(&model);
    let listener = TcpListener::bind(args.listen).map_err(|_| "listen_unavailable")?;
    let server = Server::new(
        listener,
        Limits {
            io_timeout: IO_TIMEOUT,
            max_connections: MAX_CONNECTIONS,
        },
        Arc::new(WriteLog::new(std::io::stderr())),
    );
    observe(lifecycle, status, host_now(), Input::StartupPassed);
    server.run_storing(
        |request| answer_recorded(&mut service, &replies, &request),
        |put| {
            writer
                .as_mut()
                .map_or(Err(PutRefusal::StoreUnavailable), |writer| writer.put(put))
        },
    );
    Err("listener_stopped")
}
