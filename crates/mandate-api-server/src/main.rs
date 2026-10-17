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
//! `mandate-api-server --bind <ip>:<port> --probe-bind <ip>:<port> [--pod-address <ip>]`: the
//! workspace API process (§3.11, E21-31, DEC-978).
//!
//! It makes every start check before it listens, and refuses with exit status 2 and one line on
//! stderr naming the check. Once both listeners are bound it prints
//! `listening api=<addr> probes=<addr>` and nothing else. SIGTERM starts the drain: readiness
//! turns `not_ready` while both listeners still accept, and once no API request is in flight the
//! listeners close and the process exits 0. At [`DRAIN_BOUND`] with requests still in flight, or
//! when a listener stops before any SIGTERM, it exits 1 with one line on stderr.

use std::env;
use std::ffi::OsString;
use std::future::Future;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use mandate_api_server::config::{
    StartRefusal, check_bind, check_database_url, check_environment, read_session_key,
};
use mandate_api_server::http::{self, Settings};
use mandate_api_server::lifecycle::{DRAIN_BOUND, Drain, Local};
use tokio::net::TcpListener;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

/// What the arguments name.
struct Args {
    bind: SocketAddr,
    probe_bind: SocketAddr,
    pod_address: Option<IpAddr>,
}

fn args(mut raw: impl Iterator<Item = OsString>) -> Result<Args, StartRefusal> {
    let mut bind = None;
    let mut probe_bind = None;
    let mut pod_address = None;
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
    let port_ok = port.is_none_or(|port| {
        port != "443"
            && !port.starts_with('0')
            && port.parse::<u16>().is_ok_and(|number| number > 0)
    });
    if host_ok && port_ok {
        Ok(origin)
    } else {
        Err(refused)
    }
}

/// Every start check, in order, then the two listeners' addresses and the settings.
fn checked() -> Result<(Args, Settings), StartRefusal> {
    check_environment(env::vars_os())?;
    let args = args(env::args_os().skip(1))?;
    check_bind(args.bind, args.pod_address)?;
    check_bind(args.probe_bind, args.pod_address)?;
    if let Some(url) = env::var_os("MANDATE_API_DATABASE_URL") {
        let url = url.into_string().map_err(|_| StartRefusal::Setting {
            name: "MANDATE_API_DATABASE_URL",
        })?;
        check_database_url(&url)?;
    }
    let credentials = env::var_os("CREDENTIALS_DIRECTORY")
        .map(PathBuf::from)
        .ok_or(StartRefusal::SessionKeyMissing)?;
    read_session_key(&credentials)?;
    let settings = Settings {
        app_origin: app_origin()?,
    };
    Ok((args, settings))
}

/// Serves until `stop` resolves and in-flight requests finish.
async fn serve(
    listener: TcpListener,
    router: axum::Router,
    stop: impl Future<Output = ()> + Send + 'static,
) -> io::Result<()> {
    axum::serve(listener, router)
        .with_graceful_shutdown(stop)
        .await
}

/// What ends the wait for SIGTERM: the signal, or a listener that stopped on its own.
enum Wake {
    Terminate,
    ListenerEnded,
}

async fn run(args: Args, settings: Settings) -> io::Result<()> {
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
    let api = spawn_listener(
        serve(
            api,
            http::api(settings, Arc::clone(&drain)),
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
    let settled = drain.settled(DRAIN_BOUND).await;
    let _ = stopped.send(true);
    if !settled {
        return Err(io::Error::other(
            "the drain bound was reached with requests in flight",
        ));
    }
    let _ = api.await;
    let _ = probes.await;
    Ok(())
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

fn main() -> ExitCode {
    let (args, settings) = match checked() {
        Ok(checked) => checked,
        Err(refusal) => {
            eprintln!("refused: {refusal}");
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
    match runtime.block_on(run(args, settings)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("stopped: {error}");
            ExitCode::FAILURE
        }
    }
}
