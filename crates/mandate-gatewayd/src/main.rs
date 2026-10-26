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
//! (`mandate_gatewayd::config`). It reaches no provider and reads no key. `mandate-gatewayd probe
//! livez|readyz --probe <socket>` is its `exec` probe (DEC-994 item 5). A live start is refused
//! (`mandate_gatewayd::run::command`). Every refusal exits 2 with its code alone on stderr.

use std::net::TcpListener;
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use mandate_artifacts_fs::FsArtifactStore;
use mandate_gateway_http::SystemClock;
use mandate_gatewayd::config::Config;
use mandate_gatewayd::control::StoreControl;
use mandate_gatewayd::recorded::Replies;
use mandate_gatewayd::run::{
    self, Command, Parts, RunArgs, WriteLog, answer_recorded, heartbeat, host_now, opened_ids,
    recorded_service, started_ids,
};
use mandate_gatewayd::serve::{Limits, Server};
use mandate_process::lifecycle::{Input, Kind, Lifecycle};
use mandate_process::probe::{self, Status};
use mandate_store::PgStore;

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
    let mut lifecycle = Lifecycle::new(Kind::Service, LIVENESS, started);
    let status = Status::new(lifecycle.snapshot());
    probe::serve(&args.probe, status.clone(), host_now).map_err(|_| "probe_socket_unavailable")?;
    let config = Config::from_env(&|name| std::env::var(name).ok()).map_err(|e| e.code())?;
    let model = run::read_recorded(&args.recorded).map_err(|e| e.code())?;
    let artifacts = FsArtifactStore::open_existing(&config.artifacts)
        .map_err(|_| "artifact_store_unavailable")?;
    let control = StoreControl::new(PgStore::connect(config.journal_dsn())?, &config.workspace)
        .ok_or("workspace_missing")?;
    let parts = Parts {
        workspace_store: PgStore::connect(config.journal_dsn())?,
        deployment_store: PgStore::connect(config.journal_dsn())?,
        clock: SystemClock,
        now: Rc::new(host_now),
        ids: [
            Box::new(started_ids(started, 0)),
            Box::new(opened_ids(&format!("meter:{}", config.workspace))),
            Box::new(opened_ids(&format!("dmeter:{}", config.deployment))),
            Box::new(started_ids(started, 1)),
        ],
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
    lifecycle.observe(host_now(), Input::StartupPassed);
    status.publish(lifecycle.snapshot());
    let serving = Arc::new(AtomicBool::new(true));
    heartbeat(
        lifecycle.clone(),
        status.clone(),
        Arc::clone(&serving),
        HEARTBEAT,
        host_now,
    );
    server.run(|request| answer_recorded(&mut service, &replies, &request));
    serving.store(false, Ordering::SeqCst);
    lifecycle.observe(host_now(), Input::Failed);
    status.publish(lifecycle.snapshot());
    Err("listener_stopped")
}
