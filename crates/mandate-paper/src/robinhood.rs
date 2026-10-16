//! The Robinhood session the runner's production ports log in to (G1a, LT-1, DEC-922, DEC-924
//! item 3, DEC-932 item 2): the founder's OAuth login (O1b's [`AuthServer`] and
//! [`CallbackListener`]) at the one endpoint this build pins, and nothing past it until the MCP
//! session carries the token (O2).
//!
//! - **The default build holds no Robinhood host** (LT-1): [`endpoint`] refuses
//!   [`EndpointRefusal::NotBuilt`], so no default, CI or release build can log in anywhere.
//! - **The `live` build holds one**: Robinhood's published MCP host, set in one place,
//!   [`MCP_HOST`], and never another; a rehearsal endpoint set in it is refused.
//! - **The `rh-sim` build holds none**: it takes the simulator's endpoint from
//!   [`REHEARSAL_ENDPOINT_VAR`] and refuses any that is not plain `http` on a loopback address,
//!   so it can never reach a broker (DEC-922 items 1 and 2).

use std::net::Ipv4Addr;

use mandate_alpaca::Pause;
use mandate_mcp::{
    AccessToken, AuthServer, CallbackListener, LOGIN_LIFETIME, McpError, PinnedEndpoint,
    SystemMonotonic, TransportConfig,
};
use mandate_shell::adapters::Sources;
use mandate_shell::paper::{Artifacts, BrokerFacts};
use mandate_shell::{Cause, ProductionCycle, Setup};
use tokio::runtime::Runtime;

use crate::{PaperError, Session};

/// Robinhood's published MCP host (DEC-924 item 3, connections spec §13's CN-3): the one place
/// a Robinhood host is written, and only the runner's `live` feature compiles it (LT-1).
#[cfg(feature = "live")]
pub const MCP_HOST: &str = "agent.robinhood.com";

/// The path of Robinhood's published MCP endpoint on [`MCP_HOST`].
pub const MCP_PATH: &str = "/mcp/trading";

/// The host this build pins, if any: [`MCP_HOST`] in a `live` build, none otherwise.
#[cfg(feature = "live")]
const PINNED_HOST: Option<&str> = Some(MCP_HOST);

/// The host this build pins, if any: none, since the default build holds no Robinhood host.
#[cfg(not(feature = "live"))]
const PINNED_HOST: Option<&str> = None;

/// The variable the rehearsal build reads the simulator's endpoint from (DEC-922 item 2).
pub const REHEARSAL_ENDPOINT_VAR: &str = "MANDATE_RH_SIM_ENDPOINT";

/// What a run waits for once the login has a token: the MCP session that carries it (O2), which
/// no build holds yet, so a Robinhood run stops before any read (DEC-932 item 2).
pub const SESSION_UNBUILT: &str = "the Robinhood MCP session over the login's token (O2)";

/// Which build is running, as [`endpoint`] reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Build {
    /// No `live` and no `rh-sim`: every CI and release build.
    Default,
    /// `live` without `rh-sim`: the founder's build for the live order.
    Live,
    /// `rh-sim`: the rehearsal against the simulator, loopback only.
    Rehearsal,
}

impl Build {
    /// This binary's build: `rh-sim` wins over `live`, so a build holding both dials loopback
    /// only.
    pub const CURRENT: Self = if cfg!(feature = "rh-sim") {
        Self::Rehearsal
    } else if cfg!(feature = "live") {
        Self::Live
    } else {
        Self::Default
    };
}

/// The MCP endpoint a Robinhood login dials: its host, which is also the one authorization
/// host the login accepts, and its full URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RobinhoodEndpoint {
    pub host: String,
    pub url: String,
}

/// Why a build has no Robinhood endpoint to dial; each refuses before anything is sent.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EndpointRefusal {
    /// The default build holds no Robinhood host (LT-1).
    #[error("this build holds no Robinhood host")]
    NotBuilt,
    /// The rehearsal's variable is set in a `live` build, which dials only the pinned host.
    #[error("{REHEARSAL_ENDPOINT_VAR} is set in a build that is not the rehearsal")]
    RehearsalInLive,
    /// The rehearsal build was given no simulator endpoint.
    #[error("the rehearsal build needs {REHEARSAL_ENDPOINT_VAR}")]
    RehearsalUnset,
    /// The rehearsal endpoint is not `http://127.x.x.x:<port>/<path>`.
    #[error("the rehearsal endpoint is not plain http on a loopback address")]
    OffLoopback,
}

impl EndpointRefusal {
    /// The stable reason code of each refusal (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotBuilt => "robinhood_not_built",
            Self::RehearsalInLive => "rehearsal_endpoint_in_live",
            Self::RehearsalUnset => "rehearsal_endpoint_unset",
            Self::OffLoopback => "rehearsal_off_loopback",
        }
    }
}

/// The endpoint `build` dials, given the rehearsal variable's value `rehearsal` (DEC-932 item
/// 2): none in the default build; in the `live` build the pinned host's [`MCP_PATH`], and a
/// rehearsal variable set beside it is refused; in the rehearsal build the variable's endpoint,
/// which must be `http://` on an IPv4 loopback address with a port, and no credentials, query or
/// fragment.
///
/// # Errors
/// Any [`EndpointRefusal`] that applies.
pub fn endpoint(
    build: Build,
    rehearsal: Option<&str>,
) -> Result<RobinhoodEndpoint, EndpointRefusal> {
    match (build, rehearsal) {
        (Build::Default, _) => Err(EndpointRefusal::NotBuilt),
        (Build::Live, Some(_)) => Err(EndpointRefusal::RehearsalInLive),
        (Build::Live, None) => {
            let host = PINNED_HOST.ok_or(EndpointRefusal::NotBuilt)?;
            Ok(RobinhoodEndpoint {
                host: host.to_owned(),
                url: format!("https://{host}{MCP_PATH}"),
            })
        }
        (Build::Rehearsal, None) => Err(EndpointRefusal::RehearsalUnset),
        (Build::Rehearsal, Some(text)) => loopback(text).ok_or(EndpointRefusal::OffLoopback),
    }
}

/// `text` as the rehearsal's endpoint when it is exactly `http://<IPv4 loopback>:<port><path>`:
/// a port from 1 to 65535 in decimal, a path starting with `/`, and nothing that could carry
/// credentials, a query or a fragment.
fn loopback(text: &str) -> Option<RobinhoodEndpoint> {
    let rest = text.strip_prefix("http://")?;
    let at = rest.find('/')?;
    let (authority, path) = rest.split_at(at);
    if path.contains(['?', '#']) {
        return None;
    }
    let (host, port) = authority.split_once(':')?;
    let address: Ipv4Addr = host.parse().ok()?;
    let port: u16 = port.parse().ok()?;
    let digits = port.to_string();
    if !address.is_loopback() || port == 0 || !authority.ends_with(&format!(":{digits}")) {
        return None;
    }
    Some(RobinhoodEndpoint {
        host: host.to_owned(),
        url: text.to_owned(),
    })
}

/// The Robinhood session of a `live` or rehearsal build: the founder's login at the build's
/// [`endpoint`], on the runtime its MCP session will run on (DEC-928 item 2). The URL to open is
/// shown on stderr; nothing else is.
///
/// # Errors
/// [`PaperError::Endpoint`] before anything is sent, or [`PaperError::Credentials`] when the
/// login fails; neither names a token.
pub(crate) fn connect() -> Result<Linked, PaperError> {
    let rehearsal =
        std::env::var_os(REHEARSAL_ENDPOINT_VAR).map(|value| value.to_string_lossy().into_owned());
    let endpoint = endpoint(Build::CURRENT, rehearsal.as_deref()).map_err(PaperError::Endpoint)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| PaperError::Credentials)?;
    let mut show = |url: &str| eprintln!("open this address in a browser to log in: {url}");
    let token = runtime
        .block_on(login(&endpoint, &mut show))
        .map_err(|_| PaperError::Credentials)?;
    Ok(Linked { runtime, token })
}

/// O1b's login at `endpoint`, its host the one authorization host: discovery, registration of
/// the loopback redirect, the authorization URL handed to `show`, one callback within
/// [`LOGIN_LIFETIME`], and the code exchange.
async fn login(
    endpoint: &RobinhoodEndpoint,
    show: &mut dyn FnMut(&str),
) -> Result<AccessToken, McpError> {
    let pinned = PinnedEndpoint::new(&endpoint.host, &endpoint.url)?;
    let hosts = [endpoint.host.as_str()];
    let config = TransportConfig::CONSERVATIVE;
    let server = AuthServer::discover(&pinned, &hosts, &config).await?;
    let (listener, redirect) = CallbackListener::bind().await?;
    let client = server.register(redirect, &config).await?;
    let clock = SystemMonotonic::start();
    let (url, pending) = server.begin(&client)?;
    show(url.as_str());
    let accepted = tokio::time::timeout(LOGIN_LIFETIME, listener.accept(pending, &clock)).await;
    let code = accepted.map_err(|_| McpError::LoginExpired)??;
    server.exchange(code, &config).await
}

/// A logged-in Robinhood session: the token, in memory only (LT-9), and the runtime the MCP
/// session will be connected on (DEC-928 item 2). Until that session exists (O2) it reads and
/// sends nothing, so a Robinhood run stops at its preflight with [`SESSION_UNBUILT`].
pub struct Linked {
    #[allow(
        dead_code,
        reason = "the MCP session (O2) is connected on this runtime; DEC-932 item 2"
    )]
    runtime: Runtime,
    #[allow(
        dead_code,
        reason = "the MCP session (O2) carries this token; DEC-932 item 2"
    )]
    token: AccessToken,
}

impl Session for Linked {
    fn preflight<P: Pause + Clone>(
        &self,
        _artifacts: &Artifacts,
        _pause: P,
    ) -> Result<BrokerFacts, Cause> {
        Err(Cause::Absent {
            what: SESSION_UNBUILT,
        })
    }

    fn cycle(self, _sources: Sources<()>, _setup: Setup) -> Result<ProductionCycle, Cause> {
        Err(Cause::Absent {
            what: SESSION_UNBUILT,
        })
    }
}
