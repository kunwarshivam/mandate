//! `mandate-stub-issuer --bind <ip>:<port> [--audience <aud>] [--alpaca-client-id <id>
//! --alpaca-client-secret-file <path> --alpaca-redirect-uri <uri>]` (DEC-2100, DEC-2625), or the
//! fixture provider alone, `mandate-stub-issuer --alpaca-only --bind <ip>:<port> --public-host
//! <host>:<port>` with the provider's three flags (DEC-1176 item 8). Refusals are written to
//! standard error as their code alone, and the process exits 2.

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
    use mandate_stub_issuer::serve::{serve_fixture, serve_with};
    use mandate_stub_issuer::start::{Fixture, Refused, check, check_fixture};
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

    /// The flag that runs the fixture provider without the issuer (DEC-1176 item 8).
    const ALPACA_ONLY: &str = "--alpaca-only";

    pub(super) fn main() -> ExitCode {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let tier = std::env::var("MANDATE_DEPLOYMENT_TIER").ok();
        let bind = value(&args, "--bind").unwrap_or_default();
        let public_host = value(&args, "--public-host");
        if args.iter().any(|arg| arg == ALPACA_ONLY) {
            let fixture =
                match check_fixture(true, tier.as_deref(), bind, public_host.unwrap_or_default()) {
                    Ok(fixture) => fixture,
                    Err(refused) => return refuse(refused),
                };
            let alpaca = match alpaca(&args) {
                Ok(Some(alpaca)) => alpaca,
                Ok(None) => return refuse(Refused::Alpaca),
                Err(refused) => return refuse(refused),
            };
            return block_on(fixture_run(fixture, alpaca));
        }
        let bind = match check(true, tier.as_deref(), bind) {
            Ok(bind) => bind,
            Err(refused) => return refuse(refused),
        };
        if public_host.is_some() {
            return refuse(Refused::PublicHost);
        }
        let audience = value(&args, "--audience").unwrap_or(DEFAULT_AUDIENCE);
        let alpaca = match alpaca(&args) {
            Ok(alpaca) => alpaca,
            Err(refused) => return refuse(refused),
        };
        block_on(run(bind, audience, alpaca))
    }

    /// The provider's settings when any of its three flags is given, or none.
    fn alpaca(args: &[String]) -> Result<Option<Settings>, Refused> {
        let [client_id, secret_file, redirect_uri] = ALPACA_FLAGS.map(|flag| value(args, flag));
        if [client_id, secret_file, redirect_uri]
            .iter()
            .any(Option::is_some)
        {
            settings(client_id, secret_file, redirect_uri)
        } else {
            Ok(None)
        }
    }

    /// Runs `served` on a current-thread runtime.
    fn block_on(served: impl Future<Output = ExitCode>) -> ExitCode {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return ExitCode::from(FAILED);
        };
        runtime.block_on(served)
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

    /// The fixture provider alone: the port, then the announce line naming its public host, then
    /// serving until a signal.
    async fn fixture_run(fixture: Fixture, settings: Settings) -> ExitCode {
        let Ok(listener) = TcpListener::bind(fixture.bind).await else {
            return refuse(Refused::Bind);
        };
        let (Ok(term), Ok(int)) = (
            signal(SignalKind::terminate()),
            signal(SignalKind::interrupt()),
        ) else {
            return ExitCode::from(FAILED);
        };
        let announced = writeln!(
            std::io::stdout().lock(),
            "alpaca_fixture http://{}",
            fixture.host
        );
        if announced.is_err() {
            return ExitCode::from(FAILED);
        }
        let alpaca = Arc::new(Alpaca::new(settings, Box::new(SystemRandom::new())));
        match serve_fixture(listener, fixture.host, alpaca, stopped(term, int)).await {
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
