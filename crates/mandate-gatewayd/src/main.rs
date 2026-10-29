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
//! and reads no key. `mandate-gatewayd run --registry <registry.json> --listen <addr> --probe
//! <socket> --vault-address <url> --vault-dir <dir>` starts live (DEC-2216 item 4): it logs in to
//! the vault as the model gateway with the credentials mounted in `<dir>`, reads the provider key
//! once, builds the HTTPS transport, and serves the founder's vetted entries, none offered until a
//! gate passes (`mandate_gatewayd::live`). `mandate-gatewayd probe livez|readyz --probe <socket>` is
//! its `exec` probe (DEC-994 item 5). Every refusal exits 2 with its code alone on stderr.

use std::collections::BTreeSet;
use std::net::TcpListener;
use std::path::Path;
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mandate_artifacts_fs::FsArtifactStore;
use mandate_gateway::wire::PutRefusal;
use mandate_gateway_http::{HttpsTransport, SystemClock, TlsRoots};
use mandate_gatewayd::config::Config;
use mandate_gatewayd::control::StoreControl;
use mandate_gatewayd::key::HeldKey;
use mandate_gatewayd::live::{self, ChatLive, live_service_with_chat};
use mandate_gatewayd::recorded::Replies;
use mandate_gatewayd::run::{
    self, Command, LiveArgs, Parts, RunArgs, WriteLog, answer_recorded, heartbeat, host_now,
    observe, recorded_service,
};
use mandate_gatewayd::serve::{Limits, Server};
use mandate_gatewayd::writer::ArtifactWriter;
use mandate_process::lifecycle::{Input, Kind, Lifecycle};
use mandate_process::probe::{self, Status};
use mandate_research_run::live::LiveEntry;
use mandate_store::PgStore;
use mandate_time::UtcNanos;
use mandate_vault::mounted::{read_mounted_ca, read_mounted_credentials};
use mandate_vault::openbao::{OpenBaoConfig, OpenBaoSource};
use mandate_vault::policy::{Identity, Mounts};

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
        Ok(Command::Run(args)) => {
            match serve(&args.probe, |started, lifecycle, status| {
                serve_recorded(&args, started, lifecycle, status)
            }) {
                Ok(()) => ExitCode::SUCCESS,
                Err(code) => refuse(code),
            }
        }
        Ok(Command::Live(args)) => {
            match serve(&args.probe, |started, lifecycle, status| {
                serve_live(&args, started, lifecycle, status)
            }) {
                Ok(()) => ExitCode::SUCCESS,
                Err(code) => refuse(code),
            }
        }
        Err(error) => refuse(error.code()),
    }
}

fn refuse(code: &str) -> ExitCode {
    eprintln!("{code}");
    ExitCode::from(2)
}

/// Opens the probe socket and the heartbeat, then starts the service `start` builds and serves it
/// until the listener stops.
fn serve(
    probe_socket: &Path,
    start: impl FnOnce(UtcNanos, &Mutex<Lifecycle>, &Status) -> Result<(), &'static str>,
) -> Result<(), &'static str> {
    let started = host_now();
    let lifecycle = Lifecycle::new(Kind::Service, LIVENESS, started);
    let status = Status::new(lifecycle.snapshot());
    let lifecycle = Arc::new(Mutex::new(lifecycle));
    probe::serve(probe_socket, status.clone(), host_now).map_err(|_| "probe_socket_unavailable")?;
    let serving = Arc::new(AtomicBool::new(true));
    heartbeat(
        Arc::clone(&lifecycle),
        status.clone(),
        Arc::clone(&serving),
        HEARTBEAT,
        host_now,
    );
    let served = start(started, &lifecycle, &status);
    serving.store(false, Ordering::SeqCst);
    observe(&lifecycle, &status, host_now(), Input::Failed);
    served
}

/// What every service is built over besides its gateway: its meters' stores, clock and ids, the
/// control stream, the artifact store, and the research artifacts' writer.
fn parts(
    config: &Config,
    started: UtcNanos,
) -> Result<(Parts<PgStore, SystemClock>, Option<ArtifactWriter>), &'static str> {
    let artifacts = FsArtifactStore::open_existing(&config.artifacts)
        .map_err(|_| "artifact_store_unavailable")?;
    let writer = config
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
        ids: run::ids(config, started),
        control: Box::new(control),
        artifacts: Box::new(artifacts),
    };
    Ok((parts, writer))
}

/// The recorded start's steps after the probe socket, then the serve loop.
fn serve_recorded(
    args: &RunArgs,
    started: UtcNanos,
    lifecycle: &Mutex<Lifecycle>,
    status: &Status,
) -> Result<(), &'static str> {
    let config = Config::from_env(&|name| std::env::var(name).ok()).map_err(|e| e.code())?;
    let model = run::read_recorded(&args.recorded).map_err(|e| e.code())?;
    let (parts, writer) = parts(&config, started)?;
    let mut service = recorded_service(&config, &model, parts).map_err(|e| e.code())?;
    let replies = Replies::new(&model);
    let listener = TcpListener::bind(args.listen).map_err(|_| "listen_unavailable")?;
    let mut writer = writer;
    observe(lifecycle, status, host_now(), Input::StartupPassed);
    server(listener).run_storing(
        |request| answer_recorded(&mut service, &replies, &request),
        |put| {
            writer
                .as_mut()
                .map_or(Err(PutRefusal::StoreUnavailable), |writer| writer.put(put))
        },
    );
    Err("listener_stopped")
}

/// The live start's steps after the probe socket (DEC-2216 item 4): the start-up values, the
/// registry file, the key read once from the vault, the HTTPS transport, then the serve loop. Each
/// entry the registry refuses is logged by its id and code alone.
fn serve_live(
    args: &LiveArgs,
    started: UtcNanos,
    lifecycle: &Mutex<Lifecycle>,
    status: &Status,
) -> Result<(), &'static str> {
    let config = Config::from_env(&|name| std::env::var(name).ok()).map_err(|e| e.code())?;
    let entries = live::read_registry(&args.registry).map_err(|e| e.code())?;
    let chats = live::read_chat(&args.registry).map_err(|e| e.code())?;
    let key = read_key(&args.vault_address, &args.vault_dir, &entries, &chats)?;
    let transport = HttpsTransport::new(TlsRoots::Platform).map_err(|_| "https_unavailable")?;
    let (parts, writer) = parts(&config, started)?;
    let (mut service, refused) =
        live_service_with_chat(&config, &entries, &chats, parts, transport, Box::new(key))
            .map_err(|e| e.code())?;
    for refusal in &refused {
        eprintln!("registry_refused {} {:?}", refusal.model_id, refusal.error);
    }
    let listener = TcpListener::bind(args.listen).map_err(|_| "listen_unavailable")?;
    let mut writer = writer;
    observe(lifecycle, status, host_now(), Input::StartupPassed);
    server(listener).run_storing(
        |request| service.answer(&request),
        |put| {
            writer
                .as_mut()
                .map_or(Err(PutRefusal::StoreUnavailable), |writer| writer.put(put))
        },
    );
    Err("listener_stopped")
}

/// The provider key, read once through a vault session as the model gateway (DEC-2020 items 3
/// and 5), for every upstream `entries` route through and no other.
fn read_key(
    address: &str,
    dir: &Path,
    entries: &[LiveEntry],
    chats: &[ChatLive],
) -> Result<HeldKey, &'static str> {
    let ca = read_mounted_ca(dir).map_err(|_| "vault_ca_unreadable")?;
    let (role_id, secret_id) = read_mounted_credentials(dir, &Identity::ModelGateway)
        .map_err(|_| "vault_credentials_unreadable")?;
    let config = OpenBaoConfig {
        address: address.to_owned(),
        ca_pem: ca.into_bytes(),
        mounts: Mounts::default(),
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "runtime_unavailable")?;
    let vault = runtime
        .block_on(OpenBaoSource::login(
            &config,
            Identity::ModelGateway,
            &role_id,
            &secret_id,
            host_now(),
        ))
        .map_err(|_| "vault_login_failed")?;
    let mut upstreams = live::upstreams(entries)
        .into_iter()
        .chain(live::chat_upstreams(chats))
        .collect::<BTreeSet<_>>()
        .into_iter();
    let first = upstreams.next().ok_or("registry_malformed")?;
    let key = runtime
        .block_on(HeldKey::read(&vault, &first))
        .map_err(|e| e.code())?;
    Ok(upstreams.fold(key, |key, upstream| key.with_upstream(&upstream)))
}

/// The server over `listener`, under the service's connection limits.
fn server(listener: TcpListener) -> Server {
    Server::new(
        listener,
        Limits {
            io_timeout: IO_TIMEOUT,
            max_connections: MAX_CONNECTIONS,
        },
        Arc::new(WriteLog::new(std::io::stderr())),
    )
}
