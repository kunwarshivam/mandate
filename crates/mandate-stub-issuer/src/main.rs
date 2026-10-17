//! `mandate-stub-issuer --bind <ip>:<port> [--audience <aud>]` (DEC-2100). Refusals are written to
//! standard error as their code alone, and the process exits 2. A started stub writes one line to
//! standard output, its issuer, key-set URL and audience, and nothing after it.

use std::process::ExitCode;
#[cfg(debug_assertions)]
use std::sync::Arc;

use clap::Parser;
use mandate_release::startup::TIER_VAR;
#[cfg(debug_assertions)]
use mandate_stub_issuer::issuer::Issuer;
#[cfg(debug_assertions)]
use mandate_stub_issuer::serve::serve;
use mandate_stub_issuer::start::{Refused, check};
#[cfg(debug_assertions)]
use tokio::signal::unix::{SignalKind, signal};

#[derive(Debug, Parser)]
struct Args {
    /// The loopback address to listen on: `127.0.0.1:<port>` or `[::1]:<port>`.
    #[arg(long)]
    bind: String,
    /// The `aud` of every token, as `MANDATE_API_AUDIENCE` names it.
    #[arg(long, default_value = "authenticated")]
    audience: String,
}

fn refuse(refused: Refused) -> ExitCode {
    eprintln!("{}", refused.code());
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let parsed = Args::try_parse();
    let tier = std::env::var(TIER_VAR).ok();
    let bind = parsed.as_ref().map_or("", |args| args.bind.as_str());
    let addr = match check(cfg!(debug_assertions), tier.as_deref(), bind) {
        Ok(addr) => addr,
        Err(refused) => {
            if let (Refused::Bind, Err(usage)) = (refused, &parsed) {
                usage.exit()
            }
            return refuse(refused);
        }
    };
    match parsed {
        #[cfg(debug_assertions)]
        Ok(args) => run(addr, &args.audience),
        #[cfg(not(debug_assertions))]
        Ok(_) => {
            let _ = addr;
            refuse(Refused::NotBuilt)
        }
        Err(usage) => usage.exit(),
    }
}

/// Binds before the key is made, so a port in use refuses with no key ever made (DEC-2100 item
/// 10), then serves until SIGTERM or SIGINT.
#[cfg(debug_assertions)]
fn run(addr: std::net::SocketAddr, audience: &str) -> ExitCode {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        eprintln!("stub_issuer_runtime");
        return ExitCode::from(1);
    };
    runtime.block_on(async {
        let Ok(listener) = tokio::net::TcpListener::bind(addr).await else {
            return refuse(Refused::Bind);
        };
        let (Ok(mut term), Ok(mut int)) = (
            signal(SignalKind::terminate()),
            signal(SignalKind::interrupt()),
        ) else {
            eprintln!("stub_issuer_runtime");
            return ExitCode::from(1);
        };
        let Ok(issuer) = Issuer::new(addr, audience) else {
            eprintln!("stub_issuer_key");
            return ExitCode::from(1);
        };
        println!(
            "issuer={} jwks={}/jwks.json audience={}",
            issuer.issuer, issuer.issuer, issuer.audience
        );
        let stop = async move {
            tokio::select! {
                _ = term.recv() => {}
                _ = int.recv() => {}
            }
        };
        match serve(listener, Arc::new(issuer), stop).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => {
                eprintln!("stub_issuer_io");
                ExitCode::from(1)
            }
        }
    })
}
