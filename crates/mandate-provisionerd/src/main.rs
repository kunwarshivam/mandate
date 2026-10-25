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
//! `mandate-provisionerd`: one workspace's role provisioner (E21-33, DEC-2350).
//!
//! `mandate-provisionerd run --workspace <id> --namespace <ns> --probe <socket>
//! [--vault-address <address> --vault-dir <dir> | --fixture]` runs it over Postgres
//! (`MANDATE_JOURNAL_DSN`) and the vault at `--vault-address`, trusting the CA in `--vault-dir`
//! and logged in by AppRole with the provisioner's credentials mounted there, again every hour;
//! a session at its token's end that no login replaced stops the process, failed;
//! or, with `--fixture` in a build with debug assertions, over an in-memory journal and a vault
//! that keeps nothing; a release build refuses `--fixture` (`fixture_not_built`). The roles it
//! makes bind ServiceAccounts in `--namespace`, the workspace's. It passes once a second, drains on
//! SIGTERM, and exits with its lifecycle's status.
//! `mandate-provisionerd probe livez|readyz --probe <socket>` is its `exec` probe (DEC-994 item 5).
//! This is the one place the host clock is read (ES-05).

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mandate_canon::Digest;
use mandate_deploy_manager::{Alerts, ControlJournal, Identity};
#[cfg(debug_assertions)]
use mandate_deployd::MemoryControl;
use mandate_deployd::{PgControl, StderrAlerts};
use mandate_journal::StreamId;
use mandate_journal_pg::PgJournal;
use mandate_process::probe::{self, Probe, Status};
use mandate_process::{Input, Kind, Lifecycle, State};
use mandate_provisioner::driver::Provisioner;
use mandate_provisioner::record::ACTOR_ID;
#[cfg(debug_assertions)]
use mandate_provisionerd::fixture::FixtureVault;
use mandate_provisionerd::{
    Renewal, StartRefused, fixture_allowed, input, vault_config, vault_login,
};
use mandate_time::UtcNanos;
use mandate_vault::provisioner::RoleProvisioner;
use tokio::signal::unix::{SignalKind, signal};

const LIVENESS: Duration = Duration::from_secs(30);
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
            let (Some(workspace), Some(namespace)) = (flag("--workspace"), flag("--namespace"))
            else {
                eprintln!("workspace_or_namespace_missing");
                return ExitCode::from(2);
            };
            let fixture = match fixture_allowed(
                args.iter().any(|a| a == "--fixture"),
                cfg!(debug_assertions),
            ) {
                Ok(fixture) => fixture,
                Err(refused) => {
                    eprintln!("role_provisioner_start_failed: {}", refused.code());
                    return ExitCode::from(2);
                }
            };
            let vault = if fixture {
                None
            } else {
                match (flag("--vault-address"), flag("--vault-dir")) {
                    (Some(address), Some(dir)) => Some((address, PathBuf::from(dir))),
                    _ => {
                        eprintln!("vault_address_or_dir_missing");
                        return ExitCode::from(2);
                    }
                }
            };
            match run(&workspace, &namespace, &socket, vault) {
                Ok(status) => ExitCode::from(status),
                Err(code) => {
                    eprintln!("role_provisioner_start_failed: {code}");
                    ExitCode::from(2)
                }
            }
        }
        _ => {
            eprintln!("usage: mandate-provisionerd run|probe");
            ExitCode::from(2)
        }
    }
}

#[allow(
    clippy::disallowed_methods,
    reason = "the binary's edge reads the host clock, which the provisioner takes as an input (ES-05)"
)]
fn host_now() -> UtcNanos {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = i64::try_from(since.as_secs()).unwrap_or(i64::MAX);
    UtcNanos::from_parts(secs, since.subsec_nanos()).unwrap_or(UtcNanos::EPOCH)
}

fn run(
    workspace: &str,
    namespace: &str,
    socket: &Path,
    vault: Option<(String, PathBuf)>,
) -> Result<u8, &'static str> {
    let terminate = Arc::new(AtomicBool::new(false));
    watch_terminate(Arc::clone(&terminate)).map_err(|_| "signal_unavailable")?;
    let identity = Identity {
        workspace: workspace.to_owned(),
        actor_id: ACTOR_ID.to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        build: Digest::of(concat!("mandate-provisionerd/", env!("CARGO_PKG_VERSION")).as_bytes()),
    };
    let stream = StreamId::parse(&identity.stream()).ok_or("workspace_invalid")?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .map_err(|_| "runtime_unavailable")?;
    #[cfg(debug_assertions)]
    if vault.is_none() {
        let provisioner = Provisioner::new(
            identity,
            namespace,
            MemoryControl::new(stream),
            FixtureVault,
            StderrAlerts::default(),
        );
        return runtime.block_on(serve(
            provisioner,
            socket,
            &terminate,
            async |_| Session::Keep,
            || true,
        ));
    }
    let Some((address, dir)) = vault else {
        return Err(StartRefused::FixtureNotBuilt.code());
    };
    let config = vault_config(&address, &dir)?;
    let dsn = std::env::var("MANDATE_JOURNAL_DSN").map_err(|_| "journal_dsn_missing")?;
    runtime.block_on(async {
        let journal = PgJournal::from_dsn(&dsn).map_err(|e| e.code())?;
        let logged_in = host_now();
        let session = vault_login(&config, &dir, logged_in).await?;
        let provisioner = Provisioner::new(
            identity,
            namespace,
            PgControl::new(journal, stream),
            session,
            StderrAlerts::default(),
        );
        let renewal = Cell::new(Renewal::new(logged_in));
        let relogin = async |now: UtcNanos| {
            if renewal.get().due(now) {
                match vault_login(&config, &dir, now).await {
                    Ok(session) => {
                        renewal.set(Renewal::new(now));
                        return Session::Replace(session);
                    }
                    Err(code) => {
                        eprintln!("role_provisioner_relogin_failed: {code}");
                        let mut refused = renewal.get();
                        refused.refused(now);
                        renewal.set(refused);
                    }
                }
            }
            if renewal.get().usable(now) {
                Session::Keep
            } else {
                Session::Expired
            }
        };
        let usable = || renewal.get().usable(host_now());
        serve(provisioner, socket, &terminate, relogin, usable).await
    })
}

/// The vault session for the next pass.
enum Session<V> {
    /// The one in use.
    Keep,
    /// A fresh one, which replaces the one in use; the old one's token lapses at its own end.
    Replace(V),
    /// None: the one in use is at or near its token's end and no login replaced it.
    Expired,
}

/// Runs passes until the lifecycle exits. Before each pass `relogin` names the session to use,
/// and before each step of a pass `usable` says whether it may still be used. With none, the
/// process makes and removes no role and stops, failed, so it is started again and logs in
/// afresh rather than calling the vault with a lapsed token.
async fn serve<J: ControlJournal, V: RoleProvisioner, A: Alerts>(
    mut provisioner: Provisioner<J, V, A>,
    socket: &Path,
    terminate: &AtomicBool,
    mut relogin: impl AsyncFnMut(UtcNanos) -> Session<V>,
    usable: impl Fn() -> bool,
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
        match relogin(now).await {
            Session::Keep => {}
            Session::Replace(session) => *provisioner.vault() = session,
            Session::Expired => {
                eprintln!("role_provisioner_session_expired");
                lifecycle.observe(now, Input::Failed);
                continue;
            }
        }
        let pass = provisioner.tick_while(now, &usable).await;
        if let Err(halt) = &pass
            && pass != last
        {
            eprintln!("role_provisioner_pass_halted: {halt:?}");
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
