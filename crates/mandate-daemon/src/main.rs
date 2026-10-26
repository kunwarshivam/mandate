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
//! `mandate-daemon`: one agent deployment's runtime process (E21-30 slice 1, DEC-1012).
//!
//! `mandate-daemon run --workspace <id> --agent <id> --connection <id> --owner <user> --probe
//! <socket> [--fixture]` runs it over Postgres (`MANDATE_JOURNAL_DSN`) or, with `--fixture`, an
//! in-memory journal: it passes once a second, or sooner on a wake hint, drains on SIGTERM, and
//! exits with its lifecycle's status. Either way it runs on the deny-all fixture wiring, which can
//! propose no order: a deployment's production ports are the post-R0 hook (DEC-968 item 6,
//! DEC-1013). Without `--fixture` and without a journal it refuses. `mandate-daemon probe
//! livez|readyz --probe <socket>` is its `exec` probe (DEC-994 item 5). This is the one place the
//! host clock is read (ES-05).
//!
//! A pod the deployment manager starts runs `mandate-daemon run --probe <socket>` and names its
//! workspace and agent by variables instead ([`launch`], DEC-1015). Until the connection and the
//! owner are read from the agent's deployment, such a launch refuses `deployment_unresolved`.
//!
//! `--research <bundle-dir>` asks for research over that recorded bundle (DEC-2210 item 5,
//! DEC-2215), for a paper deployment read from the journal only; anything else refuses the start,
//! and without it the process runs deny-all ([`cell`]). The flag with no bundle after it, an empty
//! one, another flag in its place, or written `--research=`, which the process does not read,
//! refuses `research_bundle_missing`, so asking for research is never read as not asking.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mandate_artifacts_fs::FsArtifactStore;
use mandate_daemon::bars::{BARS_ENV, BarsReader};
use mandate_daemon::cell::{self, CellDeployment};
use mandate_daemon::wake::Sleep;
use mandate_daemon::{Config, RuntimeProcess, Wake, deployment, fixture, launch, production};
use mandate_journal::{Environment, StreamId};
use mandate_process::State;
use mandate_process::probe::{self, Probe, Status};
use mandate_runtime::{AgentId, ConnectionId, Deployment, WorkspaceId};
use mandate_store::meter::JournalMeter;
use mandate_store::{MemoryStore, PgStore, Store};
use mandate_time::UtcNanos;
use tokio::signal::unix::{SignalKind, signal};

const LIVENESS: Duration = Duration::from_secs(10);
const PASS: Duration = Duration::from_secs(1);

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
            let var = |name: &str| match std::env::var(name) {
                Ok(value) => Some(value),
                Err(std::env::VarError::NotUnicode(_)) => Some(String::new()),
                Err(std::env::VarError::NotPresent) => None,
            };
            let named = match launch::named(&flag, &var) {
                Ok(named) => named,
                Err(code) => {
                    eprintln!("{code}");
                    return ExitCode::from(2);
                }
            };
            let fixture = args.iter().any(|a| a == "--fixture");
            let given = match (named.connection, named.owner) {
                (Some(connection), Some(owner)) => Given::Named { connection, owner },
                (None, None) => match var("MANDATE_ARTIFACTS").filter(|r| !r.is_empty()) {
                    Some(root) if !fixture => Given::Resolve { root },
                    _ => {
                        eprintln!("deployment_unresolved");
                        return ExitCode::from(2);
                    }
                },
                _ => {
                    eprintln!("deployment_missing");
                    return ExitCode::from(2);
                }
            };
            if let Err(code) = launch::fixture_permitted(fixture, &var) {
                eprintln!("{code}");
                return ExitCode::from(2);
            }
            let deployment = (named.workspace, named.agent);
            let joined = args.iter().any(|a| a.starts_with("--research="));
            let asked = joined || args.iter().any(|a| a == "--research");
            let research =
                flag("--research").filter(|dir| !dir.is_empty() && !dir.starts_with("--"));
            if asked && (joined || research.is_none()) {
                eprintln!("research_bundle_missing");
                return ExitCode::from(2);
            }
            let research = research.map(PathBuf::from);
            match run(deployment, given, &socket, fixture, research.as_deref()) {
                Ok(status) => ExitCode::from(status),
                Err(code) => {
                    eprintln!("runtime_start_failed: {code}");
                    ExitCode::from(2)
                }
            }
        }
        _ => {
            eprintln!("usage: mandate-daemon run|probe");
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

/// The rest of a deployment: named by flags, or read from the journal and the mandate documents
/// in the artifact store at `root` (DEC-1016).
enum Given {
    Named { connection: String, owner: String },
    Resolve { root: String },
}

fn run(
    (workspace, agent): (String, String),
    given: Given,
    socket: &Path,
    fixture: bool,
    research: Option<&Path>,
) -> Result<u8, &'static str> {
    let terminate = Arc::new(AtomicBool::new(false));
    watch_terminate(Arc::clone(&terminate)).map_err(|_| "signal_unavailable")?;
    let config = |connection: String| Config {
        deployment: Deployment {
            agent: AgentId(agent.clone()),
            connection: ConnectionId(connection),
            workspace: WorkspaceId(workspace.clone()),
        },
        environment: Environment::Paper,
        liveness: LIVENESS,
    };
    let artifacts = match &given {
        Given::Resolve { root } => {
            Some(FsArtifactStore::open_existing(root).map_err(|_| "artifact_store_unavailable")?)
        }
        Given::Named { .. } => None,
    };
    let opted = research
        .map(|dir| {
            cell::opt_in(
                dir,
                &CellDeployment {
                    workspace: workspace.clone(),
                    agent: agent.clone(),
                    environment: Environment::Paper,
                    resolved: matches!(given, Given::Resolve { .. }),
                },
            )
        })
        .transpose()
        .map_err(|error| error.code())?;
    if fixture {
        let Given::Named { connection, owner } = given else {
            return Err("deployment_unresolved");
        };
        let process = RuntimeProcess::start(
            MemoryStore::default(),
            fixture::wiring(&owner),
            &config(connection),
            host_now(),
        );
        return serve(process, &mut Sleep, socket, &terminate);
    }
    let dsn = std::env::var("MANDATE_JOURNAL_DSN").map_err(|_| "journal_dsn_missing")?;
    let store = PgStore::connect(&dsn)?;
    let (connection, wiring) = match (given, artifacts) {
        (Given::Named { connection, owner }, _) => (connection, fixture::wiring(&owner)),
        (Given::Resolve { root }, Some(artifacts)) => {
            let control =
                StreamId::parse(&format!("ctl:{workspace}")).ok_or("deployment_missing")?;
            let rows = store.rows(&control).ok_or("journal_unavailable")?;
            let found = deployment::resolved(&rows, &agent, Environment::Paper, &artifacts)?;
            let (version, document) =
                deployment::document(&rows, &agent, Environment::Paper, &artifacts)?;
            let rules = deployment::rule_set(&rows, Environment::Paper, &artifacts).ok();
            let mut wiring = production::wired(
                fixture::wiring_for(&found.approvers, &found.author),
                &version,
                &document,
                (
                    rules,
                    BarsReader::new(std::env::var(BARS_ENV).ok().as_deref()),
                ),
                &format!("clock:{workspace}"),
                &agent,
            );
            if let Some(opted) = opted {
                let meter = JournalMeter::open(
                    PgStore::connect(&dsn)?,
                    &workspace,
                    Environment::Paper,
                    Rc::new(host_now),
                    Box::new(cell::meter_opened_ids(&workspace)),
                );
                let wired = cell::research_metered(
                    opted,
                    Box::new(PgStore::connect(&dsn)?),
                    Box::new(meter),
                    Path::new(&root),
                    Box::new(host_now),
                )
                .map_err(|error| error.code())?;
                wiring.research = Some(wired.wiring);
                wiring.refs = wired.refs;
            }
            (found.connection, wiring)
        }
        (Given::Resolve { .. }, None) => return Err("artifact_store_unavailable"),
    };
    let process = RuntimeProcess::start(store, wiring, &config(connection), host_now());
    serve(process, &mut Sleep, socket, &terminate)
}

fn serve<S: Store>(
    mut process: RuntimeProcess<S>,
    wake: &mut dyn Wake,
    socket: &Path,
    terminate: &AtomicBool,
) -> Result<u8, &'static str> {
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
        for alert in process.step(host_now()) {
            eprintln!("alert {}", alert.key);
        }
        status.publish(process.lifecycle().snapshot());
        wake.wait(PASS);
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
