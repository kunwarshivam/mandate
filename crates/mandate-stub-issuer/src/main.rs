//! `mandate-stub-issuer --bind <ip>:<port> [--audience <aud>] [--alpaca-client-id <id>
//! --alpaca-client-secret-file <path> --alpaca-redirect-uri <uri>]` (DEC-2100, DEC-2625). Refusals
//! are written to standard error as their code alone, and the process exits 2.

use std::io::Write as _;
use std::process::ExitCode;

use mandate_stub_issuer::start::Refused;

/// The exit code of every refusal.
const REFUSED: u8 = 2;

/// Writes `refused`'s code alone and exits 2.
fn refuse(refused: Refused) -> ExitCode {
    let _ = writeln!(std::io::stderr().lock(), "{}", refused.code());
    ExitCode::from(REFUSED)
}

/// A build without debug assertions holds no issuer: it refuses before reading anything (I1).
fn main() -> ExitCode {
    #[cfg(not(debug_assertions))]
    let exit = refuse(Refused::NotBuilt);
    #[cfg(debug_assertions)]
    let exit = started::main();
    exit
}

/// The stub as a debug build runs it: the start checks, then the port, then the key.
#[cfg(debug_assertions)]
mod started {
    use std::future::Future;
    use std::io::Write as _;
    use std::net::SocketAddr;
    use std::process::ExitCode;
    use std::sync::Arc;
    use std::task::Poll;

    use mandate_stub_issuer::alpaca::{Alpaca, Settings, settings};
    use mandate_stub_issuer::issuer::Issuer;
    use mandate_stub_issuer::serve::serve_with;
    use mandate_stub_issuer::start::{Refused, check};
    use ring::rand::SystemRandom;
    use tokio::net::TcpListener;
    use tokio::signal::unix::{Signal, SignalKind, signal};

    use super::refuse;

    /// The Alpaca provider's flags: the client id, the client secret's file, the redirect URI.
    const ALPACA_FLAGS: [&str; 3] = [
        "--alpaca-client-id",
        "--alpaca-client-secret-file",
        "--alpaca-redirect-uri",
    ];

    /// The audience of the managed issuer's access tokens, used when none is given.
    const DEFAULT_AUDIENCE: &str = "authenticated";

    /// Why a started stub stopped other than by a signal: no runtime, no signal handler, no key,
    /// a closed standard output, or a failed listener.
    const FAILED: u8 = 1;

    pub(super) fn main() -> ExitCode {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let tier = std::env::var("MANDATE_DEPLOYMENT_TIER").ok();
        let bind = match check(
            true,
            tier.as_deref(),
            value(&args, "--bind").unwrap_or_default(),
        ) {
            Ok(bind) => bind,
            Err(refused) => return refuse(refused),
        };
        let audience = value(&args, "--audience").unwrap_or(DEFAULT_AUDIENCE);
        let [client_id, secret_file, redirect_uri] = ALPACA_FLAGS.map(|flag| value(&args, flag));
        let alpaca = if [client_id, secret_file, redirect_uri]
            .iter()
            .any(Option::is_some)
        {
            match settings(client_id, secret_file, redirect_uri) {
                Ok(alpaca) => alpaca,
                Err(refused) => return refuse(refused),
            }
        } else {
            None
        };
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return ExitCode::from(FAILED);
        };
        runtime.block_on(run(bind, audience, alpaca))
    }

    /// The value after `name` in `args`.
    fn value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
        args.iter()
            .zip(args.iter().skip(1))
            .find_map(|(flag, value)| (flag == name).then_some(value.as_str()))
    }

    async fn run(bind: SocketAddr, audience: &str, alpaca: Option<Settings>) -> ExitCode {
        let Ok(listener) = TcpListener::bind(bind).await else {
            return refuse(Refused::Bind);
        };
        let (Ok(term), Ok(int)) = (
            signal(SignalKind::terminate()),
            signal(SignalKind::interrupt()),
        ) else {
            return ExitCode::from(FAILED);
        };
        let Ok(issuer) = Issuer::new(bind, audience) else {
            return ExitCode::from(FAILED);
        };
        let announced = writeln!(
            std::io::stdout().lock(),
            "issuer {} key_set {}/jwks.json audience {}",
            issuer.issuer,
            issuer.issuer,
            issuer.audience
        );
        if announced.is_err() {
            return ExitCode::from(FAILED);
        }
        let alpaca =
            alpaca.map(|settings| Arc::new(Alpaca::new(settings, Box::new(SystemRandom::new()))));
        match serve_with(listener, Arc::new(issuer), alpaca, stopped(term, int)).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::from(FAILED),
        }
    }

    /// Completes at the first SIGTERM or SIGINT.
    fn stopped(mut term: Signal, mut int: Signal) -> impl Future<Output = ()> + Send + 'static {
        std::future::poll_fn(move |cx| {
            if term.poll_recv(cx).is_ready() || int.poll_recv(cx).is_ready() {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
    }
}
