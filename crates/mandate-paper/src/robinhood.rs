//! The Robinhood session the runner's production ports log in to (G1a, LT-1, DEC-922, DEC-924
//! item 3, DEC-937 item 2): the founder's OAuth login (O1b's [`AuthServer`] and
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

use std::rc::Rc;

use mandate_alpaca::{AlpacaPaperHttp, DataTransport, Pause, TradingTransport};
use mandate_mcp::{
    AccessToken, AuthServer, CallbackListener, LOGIN_LIFETIME, McpClient, McpError,
    PinnedEndpoint, SystemMonotonic, Tools, TransportConfig,
};
use mandate_num::Price;
use mandate_robinhood::AccountSnapshot;
use mandate_shell::adapters::Sources;
use mandate_shell::paper::{Artifacts, BrokerFacts};
use mandate_shell::robinhood::RobinhoodBridge;
use mandate_shell::{Cause, ProductionCycle, Setup};

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

/// The variable a Robinhood run reads the agentic account's number from (CN-8, DEC-947 item 5).
pub const ACCOUNT_NUMBER_VAR: &str = "MANDATE_RH_ACCOUNT_NUMBER";

/// The simulator's host, the one the rehearsal pins (DEC-947 item 3). Stub.
pub const REHEARSAL_HOST: &str = "";

/// The host `build` pins (DEC-947 item 3). Stub.
pub fn pinned_host(build: Build) -> Option<&'static str> {
    let _ = build;
    None
}

/// What a Robinhood run dials (DEC-947 item 5). Stub.
#[derive(Clone, PartialEq, Eq)]
pub struct Dial {
    pub endpoint: RobinhoodEndpoint,
    pub(crate) account_number: String,
}

impl Dial {
    /// The agentic account's number every request names (CN-8).
    pub fn account_number(&self) -> &str {
        &self.account_number
    }
}

impl std::fmt::Debug for Dial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dial").finish_non_exhaustive()
    }
}

/// The dial `build` makes over the process environment `vars` (DEC-947 item 5). Stub.
///
/// # Errors
/// Any [`EndpointRefusal`] that applies.
pub fn dial(build: Build, vars: &[(String, String)]) -> Result<Dial, EndpointRefusal> {
    let _ = (build, vars);
    Err(EndpointRefusal::NotBuilt)
}

/// What a run waits for once the login has a token: the MCP session that carries it (O2), which
/// no build holds yet, so a Robinhood run stops before any read (DEC-937 item 2).
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
    /// The run was given no agentic account number to scope every request to (CN-8).
    #[error("{ACCOUNT_NUMBER_VAR} is not set")]
    AccountUnset,
}

impl EndpointRefusal {
    /// The stable reason code of each refusal (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotBuilt => "robinhood_not_built",
            Self::RehearsalInLive => "rehearsal_endpoint_in_live",
            Self::RehearsalUnset => "rehearsal_endpoint_unset",
            Self::OffLoopback => "rehearsal_off_loopback",
            Self::AccountUnset => "robinhood_account_unset",
        }
    }
}

/// The endpoint `build` dials, given the rehearsal variable's value `rehearsal` (DEC-937 item
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
pub(crate) fn connect(dial: &Dial) -> Result<Linked, PaperError> {
    let endpoint = &dial.endpoint;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| PaperError::Credentials)?;
    let mut show = |url: &str| eprintln!("open this address in a browser to log in: {url}");
    let token = runtime
        .block_on(login(endpoint, &mut show))
        .map_err(|_| PaperError::Credentials)?;
    let _ = token;
    Err(PaperError::Unimplemented { story: "E7-26" })
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

/// A logged-in Robinhood session (DEC-947 item 4). Stub.
pub struct Linked<T = McpClient, M = AlpacaPaperHttp> {
    bridge: Rc<RobinhoodBridge<T>>,
    market: Option<M>,
}

impl<T: Tools, M> Linked<T, M> {
    /// The session over `bridge` and the Alpaca market reads `market`. Stub.
    pub fn new(bridge: Rc<RobinhoodBridge<T>>, market: Option<M>) -> Self {
        Self { bridge, market }
    }
}

impl<T: Tools + 'static, M: TradingTransport + DataTransport + Clone> Session for Linked<T, M> {
    fn preflight<P: Pause + Clone>(
        &self,
        _artifacts: &Artifacts,
        _pause: P,
    ) -> Result<BrokerFacts, Cause> {
        let _ = (&self.bridge, &self.market);
        Err(Cause::Unimplemented { story: "E7-26" })
    }

    fn cycle(self, _sources: Sources<()>, _setup: Setup) -> Result<ProductionCycle, Cause> {
        Err(Cause::Unimplemented { story: "E7-26" })
    }

    /// No Robinhood quote is read here, so an asked opening's cross-check has no broker ask and
    /// refuses (DEC-940 item 4).
    fn broker_ask(&self, _facts: &BrokerFacts) -> Option<Price> {
        None
    }

    fn account_snapshot(&self) -> Result<Option<AccountSnapshot>, Cause> {
        Err(Cause::Unimplemented { story: "E7-26" })
    }
}
