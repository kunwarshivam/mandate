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
//! `mandate-api-server --bind <ip>:<port> --probe-bind <ip>:<port> [--pod-address <ip>]
//! [--drain-bound-s <1 to 90>]`: the workspace API process (§3.11, E21-31, DEC-978). The drain
//! bound may only be shortened from [`DRAIN_BOUND`].
//!
//! `mandate-api-server finish`: the connect finish's own process (DEC-2348 item 3), which holds
//! the connection manager's vault identity alone and runs the finish pass every five seconds.
//! Without its settings it refuses with exit status 2 and one line naming them. SIGTERM lets the
//! pass in flight end, and the process exits 0 before the next.
//!
//! It makes every start check before it listens, and refuses with exit status 2 and one line on
//! stderr naming the check. Once both listeners are bound it prints
//! `listening api=<addr> probes=<addr>` and nothing else. SIGTERM starts the drain: readiness
//! turns `not_ready` while both listeners still accept, and once no API request is in flight the
//! listeners close and the process exits 0. One bound covers the whole drain, the listeners'
//! closing included, so a connection that never finishes its request cannot hold the process: at
//! the bound, or when a listener stops before any SIGTERM, it exits 1 with one line on stderr.
//!
//! Not yet: an idle process settles at once, so `/readyz` reports `not_ready` only while an API
//! request is in flight; holding the probes up for a grace period comes with `mandate-process`
//! (E21-28). During that window new API requests are served as before; answering ordinary calls
//! `busy` and keeping API-7 operations served (§3.11 item 4) arrive with those routes.

use std::env;
use std::ffi::OsString;
use std::future::Future;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mandate_api_server::cache::{CachedJournal, Limits};
use mandate_api_server::challenges::Challenges;
use mandate_api_server::clock::{Clock, SystemClock};
use mandate_api_server::config::SessionKey;
use mandate_api_server::config::{
    COOL_OFF_WAIVER, INTERNAL_PAPER_WORKSPACE, StartRefusal, artifact_store, check_bind,
    check_database_url, check_environment, check_internal_paper, check_key_set_tier,
    cool_off_waiver, internal_paper_workspace, read_session_key,
};
use mandate_api_server::confirm::CliConfirm;
use mandate_api_server::connect_binding::{ConnectBinding, bound, connect_binding};
use mandate_api_server::finish_binding::{finish_binding, off_line};
use mandate_api_server::finish_pass::{Finisher, LineAlerts, PASS_SECS, run_until_stopped};
use mandate_api_server::http::{self, App, Settings};
use mandate_api_server::journal::{Journal, JournalUnavailable, Postgres};
use mandate_api_server::keys::{FetchedKeys, REFETCH};
use mandate_api_server::lifecycle::{DRAIN_BOUND, Drain, Local};
use mandate_api_server::members::{self, Hosted};
use mandate_api_server::methods::{CoolOffWaiver, Methods, Passkey};
use mandate_api_server::objects::{ObjectStore, Objects};
use mandate_api_server::operator;
use mandate_api_server::reduction::{self, Reduction, rp_id};
use mandate_api_server::session::{OsRandom, Sessions};
use mandate_api_server::subject;
use mandate_api_server::token::Issuer;
use mandate_api_server::vault_fingerprints::VaultFingerprints;
use mandate_api_server::vault_teardown::VaultTeardown;
use mandate_artifacts_fs::FsArtifactStore;
use mandate_authn::{Algorithm, IssuerConfig};
use mandate_identity::{OrgId, WorkspaceId};
use mandate_journal::ArtifactSource;
use mandate_journal_pg::PgJournal;
use mandate_passkey::RelyingParty;
use mandate_release::startup::{TIER_VAR, Tier};
use tokio::net::TcpListener;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

/// What the arguments name.
struct Args {
    bind: SocketAddr,
    probe_bind: SocketAddr,
    pod_address: Option<IpAddr>,
    drain_bound: Duration,
}

fn args(mut raw: impl Iterator<Item = OsString>) -> Result<Args, StartRefusal> {
    let mut bind = None;
    let mut probe_bind = None;
    let mut pod_address = None;
    let mut drain_bound = DRAIN_BOUND;
    while let Some(flag) = raw.next() {
        let value = raw.next().and_then(|v| v.into_string().ok());
        let parsed = match flag.to_str() {
            Some("--bind") => value.and_then(|v| v.parse().ok()).map(|a| bind = Some(a)),
            Some("--probe-bind") => value
                .and_then(|v| v.parse().ok())
                .map(|a| probe_bind = Some(a)),
            Some("--pod-address") => value
                .and_then(|v| v.parse().ok())
                .map(|a| pod_address = Some(a)),
            Some("--drain-bound-s") => value
                .and_then(|v| v.parse().ok())
                .map(Duration::from_secs)
                .filter(|bound| !bound.is_zero() && *bound <= DRAIN_BOUND)
                .map(|bound| drain_bound = bound),
            _ => None,
        };
        if parsed.is_none() {
            return Err(StartRefusal::Setting { name: "arguments" });
        }
    }
    Ok(Args {
        bind: bind.ok_or(StartRefusal::Setting { name: "--bind" })?,
        probe_bind: probe_bind.ok_or(StartRefusal::Setting {
            name: "--probe-bind",
        })?,
        pod_address,
        drain_bound,
    })
}

/// The app origin exactly as a browser serializes it (§3.3 item 5), so CORS can match it byte for
/// byte: `https://`, a lowercase ASCII host of letters, digits, dots, and hyphens, and an optional
/// port that is not the default 443. No user information, path, query, or trailing slash.
fn app_origin() -> Result<String, StartRefusal> {
    let refused = StartRefusal::Setting {
        name: "MANDATE_API_APP_ORIGIN",
    };
    let origin = env::var("MANDATE_API_APP_ORIGIN").map_err(|_| refused.clone())?;
    let rest = origin.strip_prefix("https://").ok_or(refused.clone())?;
    let (host, port) = rest
        .split_once(':')
        .map_or((rest, None), |(h, p)| (h, Some(p)));
    let host_ok = !host.is_empty()
        && !host.starts_with(['.', '-'])
        && !host.ends_with(['.', '-'])
        && host
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-');
    let port_ok = port
        .is_none_or(|port| port != "443" && !port.starts_with('0') && port.parse::<u16>().is_ok());
    if host_ok && port_ok {
        Ok(origin)
    } else {
        Err(refused)
    }
}

/// Every start check, in order, then the two listeners' addresses and the settings.
/// What the start checks hand the process.
struct Start {
    settings: Settings,
    key: SessionKey,
    issuer: IssuerConfig,
    keys: Arc<FetchedKeys>,
    database_url: String,
    hosted: Hosted,
    artifacts: Option<Arc<Objects<FsArtifactStore>>>,
    ingress: Vec<IpAddr>,
    connect: ConnectBinding,
    /// The test cell's waiver of a first passkey's cool-off, when set (DEC-2146 item 2).
    cool_off_waiver: Option<CoolOffWaiver>,
}

/// A required setting's value, refused when absent, empty, or not Unicode.
fn setting(name: &'static str) -> Result<String, StartRefusal> {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or(StartRefusal::Setting { name })
}

fn checked() -> Result<(Args, Start), StartRefusal> {
    check_environment(env::vars_os())?;
    let tier = Tier::from_setting(env::var(TIER_VAR).ok().as_deref());
    let connect = connect_binding(tier, |name| env::var_os(name))?;
    let args = args(env::args_os().skip(1))?;
    check_bind(args.bind, args.pod_address)?;
    check_bind(args.probe_bind, args.pod_address)?;
    let database_url = setting("MANDATE_API_DATABASE_URL")?;
    check_database_url(&database_url)?;
    let hosted = Hosted {
        org: OrgId::from_ulid_text(&setting("MANDATE_API_ORG")?).map_err(|_| {
            StartRefusal::Setting {
                name: "MANDATE_API_ORG",
            }
        })?,
        workspace: WorkspaceId::from_ulid_text(&setting("MANDATE_API_WORKSPACE")?).map_err(
            |_| StartRefusal::Setting {
                name: "MANDATE_API_WORKSPACE",
            },
        )?,
        internal_paper: internal_paper_workspace(env::var_os(INTERNAL_PAPER_WORKSPACE).as_deref())?,
    };
    let credentials = env::var_os("CREDENTIALS_DIRECTORY")
        .map(PathBuf::from)
        .ok_or(StartRefusal::SessionKeyMissing)?;
    let key = read_session_key(&credentials)?;
    let settings = Settings {
        app_origin: app_origin()?,
    };
    let issuer = IssuerConfig::new(
        &setting("MANDATE_API_ISSUER")?,
        &[setting("MANDATE_API_AUDIENCE")?.as_str()],
        &[Algorithm::Es256],
    )
    .map_err(|_| StartRefusal::Setting {
        name: "MANDATE_API_ISSUER",
    })?;
    let key_set = setting("MANDATE_API_JWKS_URL")?;
    check_key_set_tier(&key_set, tier)?;
    let keys = FetchedKeys::new(&key_set).map_err(|_| StartRefusal::Setting {
        name: "MANDATE_API_JWKS_URL",
    })?;
    let artifacts = artifact_store(env::var_os("MANDATE_API_ARTIFACTS").as_deref())?;
    let ingress_setting = env::var_os(reduction::INGRESS_PEERS)
        .map(|value| value.into_string())
        .transpose()
        .map_err(|_| StartRefusal::Setting {
            name: reduction::INGRESS_PEERS,
        })?;
    let ingress = reduction::ingress_peers(ingress_setting.as_deref())?;
    let waiver = cool_off_waiver(tier, env::var_os(COOL_OFF_WAIVER).as_deref())?;
    let start = Start {
        settings,
        key,
        issuer,
        keys: Arc::new(keys),
        database_url,
        hosted,
        artifacts,
        ingress,
        connect,
        cool_off_waiver: waiver,
    };
    Ok((args, start))
}

/// How long the internal paper workspace's start check waits for the control stream, and the
/// operator's `real-money` command for its read and append.
const START_READ_BOUND: Duration = Duration::from_secs(5);

/// DEC-1143 item 2: with [`INTERNAL_PAPER_WORKSPACE`] on, the hosted control stream must be read
/// within [`START_READ_BOUND`] and be paper throughout, or the process does not start.
async fn internal_paper_checked(start: &Start) -> Result<(), StartRefusal> {
    let refused = StartRefusal::InternalWorkspaceNotPaper;
    let journal = Postgres(PgJournal::from_dsn(&start.database_url).map_err(|_| refused.clone())?);
    let workspace = start.hosted.workspace.to_ulid_text().map_err(|_| refused)?;
    let stream = format!("ctl:{workspace}");
    let rows = tokio::time::timeout(START_READ_BOUND, journal.rows(&stream))
        .await
        .unwrap_or(Err(JournalUnavailable));
    check_internal_paper(rows.as_deref().map_err(|&unavailable| unavailable))
}

/// Serves until `stop` resolves and in-flight requests finish.
async fn serve(
    listener: TcpListener,
    router: axum::Router,
    stop: impl Future<Output = ()> + Send + 'static,
) -> io::Result<()> {
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(stop)
    .await
}

/// What ends the wait for SIGTERM: the signal, or a listener that stopped on its own.
enum Wake {
    Terminate,
    ListenerEnded,
}

async fn run(args: Args, start: Start) -> io::Result<()> {
    let drain = Drain::new();
    let api = TcpListener::bind(args.bind).await?;
    let probes = TcpListener::bind(args.probe_bind).await?;
    let mut term = signal(SignalKind::terminate())?;
    println!(
        "listening api={} probes={}",
        api.local_addr()?,
        probes.local_addr()?
    );
    let (stopped, stop) = watch::channel(false);
    let (wake, mut woken) = mpsc::unbounded_channel();
    let fetcher = Arc::clone(&start.keys);
    tokio::spawn(async move {
        loop {
            let _ = fetcher.fetch().await;
            tokio::time::sleep(REFETCH).await;
        }
    });
    let challenges = Arc::new(Challenges::default());
    let rp = RelyingParty {
        rp_id: rp_id(&start.settings.app_origin)
            .ok_or_else(|| io::Error::other("the app origin names no relying party"))?,
        origin: start.settings.app_origin.clone(),
    };
    let step_up = Methods {
        passkey: Passkey {
            challenges: Arc::clone(&challenges),
            rp,
            keys: start.artifacts.clone().map(|store| {
                let store: Arc<dyn ArtifactSource + Send + Sync> = store;
                store
            }),
            cool_off_waiver: start.cool_off_waiver,
        },
        cli_confirm: CliConfirm {
            challenges: Arc::clone(&challenges),
            links: Arc::new(subject::Fixed),
        },
    };
    let app = App {
        settings: start.settings,
        sessions: Arc::new(Sessions::new(&start.key, Box::new(OsRandom::new()))),
        tokens: Arc::new(Issuer::new(start.issuer, start.keys)),
        links: Arc::new(subject::Fixed),
        clock: Arc::new(SystemClock),
        journal: Arc::new(CachedJournal::new(
            Arc::new(Postgres(PgJournal::from_dsn(&start.database_url).map_err(
                |_| io::Error::other("the journal's connection settings are refused"),
            )?)),
            Arc::new(SystemClock),
            Limits::SERVER,
        )),
        members: Arc::new(members::control_parser(start.hosted.workspace)),
        hosted: start.hosted,
        artifacts: start.artifacts.clone().map(|store| {
            let store: Arc<dyn ArtifactSource + Send + Sync> = store;
            store
        }),
        objects: start.artifacts.map(|store| {
            let store: Arc<dyn ObjectStore> = store;
            store
        }),
        step_up: Arc::new(step_up),
        challenges,
        connect: bound(
            start.connect,
            Box::new(OsRandom::new()),
            Arc::new(SystemClock),
        ),
        reduction: Arc::new(Reduction::new(start.ingress, Box::new(OsRandom::new()))),
        folds: Arc::default(),
    };
    let api = spawn_listener(
        serve(
            api,
            http::api(app, Arc::clone(&drain)),
            wait_for(stop.clone()),
        ),
        wake.clone(),
    );
    let probes = spawn_listener(
        serve(
            probes,
            http::probes(Arc::new(Local::new(Arc::clone(&drain)))),
            wait_for(stop),
        ),
        wake.clone(),
    );
    tokio::spawn(async move {
        term.recv().await;
        let _ = wake.send(Wake::Terminate);
    });
    match woken.recv().await {
        Some(Wake::Terminate) => {}
        Some(Wake::ListenerEnded) | None => {
            return Err(io::Error::other("a listener stopped before any drain"));
        }
    }
    drain.begin();
    let drained = tokio::time::timeout(args.drain_bound, async {
        drain.settled(args.drain_bound).await;
        let _ = stopped.send(true);
        let _ = api.await;
        let _ = probes.await;
    })
    .await;
    drained.map_err(|_| {
        io::Error::other("the drain bound was reached with requests or connections open")
    })
}

/// Runs one listener, and wakes the main task if it ends.
fn spawn_listener(
    listener: impl Future<Output = io::Result<()>> + Send + 'static,
    wake: mpsc::UnboundedSender<Wake>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let _ = listener.await;
        let _ = wake.send(Wake::ListenerEnded);
    })
}

async fn wait_for(mut stop: watch::Receiver<bool>) {
    let _ = stop.wait_for(|stopped| *stopped).await;
}

/// The finish's own process: the pass over the hosted workspace's connects, every
/// [`PASS_SECS`], as the connection manager, until the process is stopped.
fn finish() -> ExitCode {
    if env::args_os().nth(2).is_some() {
        eprintln!("refused: {}", StartRefusal::Setting { name: "arguments" });
        return ExitCode::from(2);
    }
    if let Err(refusal) = check_environment(env::vars_os()) {
        eprintln!("refused: {refusal}");
        return ExitCode::from(2);
    }
    let bound = match finish_binding(|name| env::var_os(name)) {
        Ok(bound) => bound,
        Err(names) => {
            eprintln!("{}", off_line(&names));
            return ExitCode::from(2);
        }
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("refused: the runtime could not start: {error}");
            return ExitCode::from(2);
        }
    };
    let in_the_runtime_the_pool_needs = runtime.enter();
    let Ok(journal) = PgJournal::from_dsn(&bound.database_url) else {
        eprintln!("refused: the journal's connection settings are refused");
        return ExitCode::from(2);
    };
    let login = Arc::new(bound.login);
    let finisher = Finisher::new(
        Arc::new(Postgres(journal)),
        VaultFingerprints {
            login: Arc::clone(&login),
            clock: Arc::new(SystemClock),
        },
        VaultTeardown {
            login,
            clock: Arc::new(SystemClock),
        },
        LineAlerts(Mutex::new(io::stderr())),
        bound.workspace,
    );
    drop(in_the_runtime_the_pool_needs);
    let drained = run_until_stopped(
        runtime,
        finisher,
        Arc::new(SystemClock),
        Duration::from_secs(PASS_SECS),
        || {
            let mut term = signal(SignalKind::terminate())?;
            println!("finishing");
            Ok(async move {
                term.recv().await;
            })
        },
    );
    match drained {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("refused: the signal handler could not start: {error}");
            ExitCode::from(2)
        }
    }
}

/// `mandate-api-server real-money`: the platform operator's setting of the hosted workspace's
/// real-money gate ([`operator`], DEC-2134 items 3 and 6), appended with the deployment's own
/// journal settings. It prints the appended record's id, or `refused: <code>`.
fn real_money() -> ExitCode {
    let refused = |code: &str| {
        eprintln!("refused: {code}");
        ExitCode::from(2)
    };
    let arguments = env::args().skip(2);
    let command = match operator::command(arguments) {
        Ok(command) => command,
        Err(refusal) => return refused(refusal.code()),
    };
    if let Err(refusal) = check_environment(env::vars_os()) {
        return refused(&refusal.to_string());
    }
    let (Ok(database_url), Ok(workspace)) = (
        setting("MANDATE_API_DATABASE_URL"),
        setting("MANDATE_API_WORKSPACE"),
    ) else {
        return refused(operator::Refusal::Arguments.code());
    };
    let founder = env::var(operator::FOUNDER_WORKSPACE).ok();
    let Some(now) = SystemClock.now() else {
        return refused(operator::Refusal::JournalUnavailable.code());
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => return refused(operator::Refusal::JournalUnavailable.code()),
    };
    let set = runtime.block_on(async {
        let journal = Postgres(
            PgJournal::from_dsn(&database_url)
                .map_err(|_| operator::Refusal::JournalUnavailable)?,
        );
        let setting = operator::Setting {
            workspace: &workspace,
            operator: &command.operator,
            key: &command.key,
            requested: command.requested.clone(),
        };
        tokio::time::timeout(
            START_READ_BOUND,
            operator::set(&journal, &setting, founder.as_deref(), now),
        )
        .await
        .unwrap_or(Err(operator::Refusal::JournalUnavailable))
    });
    match set {
        Ok(row) => {
            println!("{}", row.event_id);
            ExitCode::SUCCESS
        }
        Err(refusal) => refused(refusal.code()),
    }
}

fn main() -> ExitCode {
    if let Err(refused) = mandate_release::startup::check_running() {
        eprintln!("{}", refused.code());
        return ExitCode::from(2);
    }
    if env::args_os().nth(1).is_some_and(|mode| mode == "finish") {
        return finish();
    }
    if env::args_os()
        .nth(1)
        .is_some_and(|mode| mode == "real-money")
    {
        return real_money();
    }
    let (args, start) = match checked() {
        Ok(checked) => checked,
        Err(refusal) => {
            eprintln!("refused: {refusal}");
            return ExitCode::from(2);
        }
    };
    if let Some(line) = start.connect.off_line() {
        eprintln!("{line}");
    }
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("refused: the runtime could not start: {error}");
            return ExitCode::from(2);
        }
    };
    if start.hosted.internal_paper
        && let Err(refusal) = runtime.block_on(internal_paper_checked(&start))
    {
        eprintln!("refused: {refusal}");
        return ExitCode::from(2);
    }
    match runtime.block_on(run(args, start)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("stopped: {error}");
            ExitCode::FAILURE
        }
    }
}
