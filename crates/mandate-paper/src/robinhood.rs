//! The Robinhood session the runner's production ports log in to (G1a, LT-1, DEC-922, DEC-924
//! item 3, DEC-937 item 2): the founder's OAuth login (O1b's [`AuthServer`] and
//! [`CallbackListener`]) at the one endpoint this build pins, then the MCP session that carries
//! the token as its bearer (O2) and the one Robinhood connector over it ([`Linked`], DEC-947).
//!
//! - **The default build holds no Robinhood host** (LT-1): [`endpoint`] refuses
//!   [`EndpointRefusal::NotBuilt`], so no default, CI or release build can log in anywhere.
//! - **The `live` build holds one**: Robinhood's published MCP host, set in one place,
//!   [`MCP_HOST`], and never another; a rehearsal endpoint set in it is refused.
//! - **The `rh-sim` build holds none**: it takes the simulator's endpoint from
//!   [`REHEARSAL_ENDPOINT_VAR`] and refuses any that is not plain `http` on a loopback address,
//!   so it can never reach a broker (DEC-922 items 1 and 2), and it pins the simulator's
//!   [`REHEARSAL_HOST`], never [`MCP_HOST`], which it does not compile (DEC-947 item 3).

use std::net::Ipv4Addr;

use std::rc::Rc;

use mandate_accounting::InstrumentId;
use mandate_alpaca::{
    AlpacaPaperHttp, Credentials, DataTransport, LatestQuote, Pause, TradingTransport,
};
use mandate_mcp::{
    AccessToken, AuthServer, CallbackListener, LOGIN_LIFETIME, McpClient, McpError, McpTransport,
    PinnedEndpoint, SystemMonotonic, Tools, TransportConfig,
};
use mandate_robinhood::{AccountSnapshot, RobinhoodConnector, Tickers};
use mandate_shell::adapters::Sources;
use mandate_shell::paper::{Artifacts, BrokerFacts, fresh_quote, market_facts, robinhood_facts};
use mandate_shell::robinhood::RobinhoodBridge;
use mandate_shell::{Cause, ProductionCycle, Setup, robinhood_production_cycle};
use tokio::runtime::Runtime;

use crate::connect_check::{ACCOUNT_NUMBER_VAR, Observed, SeenContract, observe};
use crate::{BrokerAsk, PaperError, Session};

/// Robinhood's published MCP host (DEC-924 item 3, connections spec §13's CN-3): the one place
/// a Robinhood host is written, and only the runner's `live` feature without `rh-sim` compiles
/// it (LT-1, DEC-947 item 3), so the rehearsal build holds no production host at all.
#[cfg(all(feature = "live", not(feature = "rh-sim")))]
pub const MCP_HOST: &str = "agent.robinhood.com";

/// The path of Robinhood's published MCP endpoint on [`MCP_HOST`].
pub const MCP_PATH: &str = "/mcp/trading";

/// The Robinhood host a `live` build pins: [`MCP_HOST`] in a `live` build without `rh-sim`.
#[cfg(all(feature = "live", not(feature = "rh-sim")))]
const PINNED_HOST: Option<&str> = Some(MCP_HOST);

/// The Robinhood host a `live` build pins: none, since this build holds no Robinhood host.
#[cfg(not(all(feature = "live", not(feature = "rh-sim"))))]
const PINNED_HOST: Option<&str> = None;

/// The variable the rehearsal build reads the simulator's endpoint from (DEC-922 item 2).
pub const REHEARSAL_ENDPOINT_VAR: &str = "MANDATE_RH_SIM_ENDPOINT";

/// The simulator's host, the one the rehearsal build pins (DEC-922 items 1 and 2, DEC-947 item
/// 3): the loopback address `mandate-rh-sim` listens on. It reaches no broker, so every build may
/// hold it; only [`Build::Rehearsal`] pins it.
pub const REHEARSAL_HOST: &str = "127.0.0.1";

/// The host `build` pins, the one DEC-924 item 1's admission compares the dialed host with
/// (DEC-934 item 3, DEC-947 item 3): none in the default build; Robinhood's [`MCP_HOST`] in the
/// `live` build, and only there; [`REHEARSAL_HOST`] in the rehearsal build, whose
/// [`endpoint`] dials loopback alone.
pub fn pinned_host(build: Build) -> Option<&'static str> {
    match build {
        Build::Default => None,
        Build::Live => PINNED_HOST,
        Build::Rehearsal => Some(REHEARSAL_HOST),
    }
}

/// What a Robinhood run dials (DEC-947 item 5): the build's [`endpoint`] and the agentic
/// account's number every request names (CN-8). The number is personal data, so the printout
/// leaves it out (journal spec §6.4).
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

/// The dial `build` makes, read from the process environment `vars` the run was handed
/// (DEC-947 item 5): [`endpoint`] over [`REHEARSAL_ENDPOINT_VAR`], then the founder's account
/// number from [`ACCOUNT_NUMBER_VAR`]. Nothing is sent.
///
/// # Errors
/// Any [`EndpointRefusal`] of [`endpoint`], then [`EndpointRefusal::AccountUnset`] for a number
/// that is missing or empty.
pub fn dial(build: Build, vars: &[(String, String)]) -> Result<Dial, EndpointRefusal> {
    let read = |name: &str| {
        vars.iter()
            .find(|(given, _)| given == name)
            .map(|(_, value)| value.as_str())
    };
    let endpoint = endpoint(build, read(REHEARSAL_ENDPOINT_VAR))?;
    let account_number = read(ACCOUNT_NUMBER_VAR)
        .filter(|number| !number.is_empty())
        .ok_or(EndpointRefusal::AccountUnset)?;
    Ok(Dial {
        endpoint,
        account_number: account_number.to_owned(),
    })
}

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

/// The founder's login at the endpoint this build pins, read from the process environment as
/// [`endpoint`] reads it: the connect check's login (DEC-946), which observes the session itself
/// ([`LoggedIn::observe`]).
///
/// # Errors
/// [`PaperError::Endpoint`] before anything is sent, or [`PaperError::Credentials`] when the
/// login fails; neither names a token.
pub(crate) fn login_from_env() -> Result<LoggedIn, PaperError> {
    let rehearsal =
        std::env::var_os(REHEARSAL_ENDPOINT_VAR).map(|value| value.to_string_lossy().into_owned());
    let endpoint = endpoint(Build::CURRENT, rehearsal.as_deref()).map_err(PaperError::Endpoint)?;
    logged_in(endpoint)
}

/// The founder's login at `endpoint` (DEC-937 item 2), on the current-thread runtime the MCP
/// session will run on (DEC-928 item 2). The URL to open is shown on stderr; nothing else is.
///
/// # Errors
/// [`PaperError::Credentials`] when the login fails; no error names a token.
fn logged_in(endpoint: RobinhoodEndpoint) -> Result<LoggedIn, PaperError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| PaperError::Credentials)?;
    let mut show = |url: &str| eprintln!("open this address in a browser to log in: {url}");
    let token = runtime
        .block_on(login(&endpoint, &mut show))
        .map_err(|_| PaperError::Credentials)?;
    Ok(LoggedIn {
        runtime,
        token,
        endpoint,
    })
}

/// The Robinhood session of a `live` or rehearsal build (DEC-947 item 4): the founder's login at
/// `dial`'s endpoint, then [`LoggedIn::session`] scoped to `dial`'s account number. Nothing is
/// sent but the login, the handshake and the tool listing.
///
/// # Errors
/// [`PaperError::Credentials`] when the login or the session's handshake fails; no error names
/// a token.
pub(crate) fn connect(dial: &Dial) -> Result<Linked, PaperError> {
    logged_in(dial.endpoint.clone())?.session(&dial.account_number)
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

/// A logged-in Robinhood session (DEC-947 item 4): the bridge over the one Robinhood connector,
/// its MCP client connected with the login's token on the bridge's runtime (DEC-928 item 2),
/// and the Alpaca client for the market reads, if the environment held its paper keys. The
/// token lives inside the client's transport, in memory only (LT-9).
pub struct Linked<T = McpClient, M = AlpacaPaperHttp> {
    bridge: Rc<RobinhoodBridge<T>>,
    market: Option<M>,
}

/// The founder's login: the token, in memory only (LT-9), the runtime the MCP session will be
/// connected on (DEC-928 item 2), and the endpoint it was made at. Nothing is read or sent until
/// it becomes a session ([`LoggedIn::session`]) or is observed by the connect check.
pub struct LoggedIn {
    runtime: Runtime,
    token: AccessToken,
    endpoint: RobinhoodEndpoint,
}

impl LoggedIn {
    /// The connect check's reads (DEC-946): the first connect over an MCP session carrying the
    /// login's token, which reports the listed contract rather than refusing it
    /// ([`McpClient::connect_to_check`]), and, when it listed, the recorded account through the
    /// connector's own account check. A session that cannot be made or listed reads nothing, so
    /// every check fails closed. The token leaves memory with the session (LT-9).
    pub(crate) fn observe(self, account_number: &str) -> Observed {
        let Self {
            runtime,
            token,
            endpoint,
        } = self;
        runtime.block_on(async {
            let unread = Observed {
                contract: None,
                agentic_account_read: false,
            };
            let Ok(pinned) = PinnedEndpoint::new(&endpoint.host, &endpoint.url) else {
                return unread;
            };
            let clock = Box::new(SystemMonotonic::start());
            let Ok(transport) = McpTransport::new(pinned, TransportConfig::CONSERVATIVE, clock)
            else {
                return unread;
            };
            let transport = transport.with_bearer(token);
            let Ok((client, listed)) = McpClient::connect_to_check(transport).await else {
                return unread;
            };
            let Ok(hash) = client
                .contract()
                .map(|hash| mandate_canon::Digest::from_bytes(*hash.as_bytes()).to_hex())
            else {
                return unread;
            };
            let connector =
                RobinhoodConnector::new(client, account_number.to_owned(), Tickers::default());
            observe(Some(SeenContract { listed, hash }), &connector).await
        })
    }

    /// The MCP session over the login's token (O2, DEC-947 item 4): the transport pinned to the
    /// login's endpoint carrying the token as its bearer, the client whose tool contract is
    /// checked as a first connection's (a fund-movement tool or a missing allowlisted one gives no
    /// client, connections spec §8.1), and the one Robinhood connector scoped to
    /// `account_number` (CN-8), in the shell's bridge on the login's runtime. The Alpaca paper
    /// client for the market reads is built from the environment's paper keys when they are
    /// there, with no request; without them the session's preflight refuses (DEC-947 item 5).
    ///
    /// # Errors
    /// [`PaperError::Credentials`] when the transport or the handshake fails; no error names a
    /// token.
    pub(crate) fn session(self, account_number: &str) -> Result<Linked, PaperError> {
        let Self {
            runtime,
            token,
            endpoint,
        } = self;
        let pinned = PinnedEndpoint::new(&endpoint.host, &endpoint.url)
            .map_err(|_| PaperError::Credentials)?;
        let clock = Box::new(SystemMonotonic::start());
        let transport = McpTransport::new(pinned, TransportConfig::CONSERVATIVE, clock)
            .map_err(|_| PaperError::Credentials)?
            .with_bearer(token);
        let client = runtime
            .block_on(McpClient::connect(transport, None))
            .map_err(|_| PaperError::Credentials)?;
        let connector =
            RobinhoodConnector::new(client, account_number.to_owned(), Tickers::default());
        let market = Credentials::from_env()
            .ok()
            .and_then(|keys| AlpacaPaperHttp::new(keys).ok());
        Ok(Linked::new(
            RobinhoodBridge::new(runtime, connector),
            market,
        ))
    }
}

impl<T: Tools, M> Linked<T, M> {
    /// The session over `bridge`, with `market` for the Alpaca market reads.
    pub fn new(bridge: Rc<RobinhoodBridge<T>>, market: Option<M>) -> Self {
        Self { bridge, market }
    }
}

/// What a Robinhood preflight waits for when the environment held no Alpaca paper keys: the
/// market reads DEC-933 item 2 takes from Alpaca's data API (the rehearsal's R5).
pub const MARKET_UNREAD: &str = "the Alpaca market reads a Robinhood preflight takes (DEC-933)";

impl<T: Tools + 'static, M: TradingTransport + DataTransport + Clone> Session for Linked<T, M> {
    /// DEC-933 item 2: the pinned instrument's `(asset id, symbol)` registered with the
    /// connector (DEC-956 item 1), the agentic account and the instrument's Robinhood quote
    /// through the bridge, then Alpaca's asset record, IEX quote and minute bars, joined by
    /// `robinhood_facts`. No Alpaca client, no read at all.
    fn preflight<P: Pause + Clone>(
        &self,
        artifacts: &Artifacts,
        pause: P,
    ) -> Result<BrokerFacts, Cause> {
        let market = self.market.as_ref().ok_or(Cause::Absent {
            what: MARKET_UNREAD,
        })?;
        let identity = artifacts.production_identity();
        let asset = InstrumentId::new(identity.asset_id.as_str()).map_err(|_| Cause::Absent {
            what: "the pinned asset id",
        })?;
        let tickers =
            Tickers::registered([(asset, identity.symbol.as_str().to_owned())]).map_err(|_| {
                Cause::Absent {
                    what: "the pinned instrument's Robinhood symbol",
                }
            })?;
        self.bridge.register(tickers)?;
        let robinhood = self.bridge.preflight_facts(identity.symbol)?;
        let market = market_facts(artifacts, market.clone(), market.clone(), pause)?;
        robinhood_facts(market, &robinhood)
    }

    /// The shell's Robinhood cycle over this session's bridge (DEC-947 item 4).
    fn cycle(&self, sources: Sources<()>, setup: Setup) -> Result<ProductionCycle, Cause> {
        Ok(robinhood_production_cycle(
            with_bridge(sources, Rc::clone(&self.bridge)),
            setup,
        ))
    }

    /// The independent IEX quote is Alpaca's data client's (DEC-943 item 2), so a session the
    /// environment gave no Alpaca paper keys refuses as its preflight does.
    fn fresh_quote<P: Pause>(&self, artifacts: &Artifacts, pause: P) -> Result<LatestQuote, Cause> {
        let market = self.market.as_ref().ok_or(Cause::Absent {
            what: MARKET_UNREAD,
        })?;
        fresh_quote(artifacts, market.clone(), pause)
    }

    /// Robinhood's own ask, read through this session's bridge at the check (DEC-945 items 1
    /// and 4), never the preflight's quote.
    fn broker_ask(&self, _facts: &BrokerFacts) -> BrokerAsk {
        BrokerAsk::robinhood(Rc::clone(&self.bridge))
    }

    /// The bridge's read of the agentic account (DEC-947 item 1).
    fn account_snapshot(&self) -> Result<Option<AccountSnapshot>, Cause> {
        self.bridge.account_snapshot().map(Some)
    }
}

/// `sources` carrying `bridge`.
fn with_bridge<T>(
    sources: Sources<()>,
    bridge: Rc<RobinhoodBridge<T>>,
) -> Sources<Rc<RobinhoodBridge<T>>> {
    let Sources {
        mandate,
        journal,
        recorded_at,
        agent,
        workspace,
        account_ref,
        executor,
        run,
        artifacts,
        symbols,
        transport: (),
    } = sources;
    Sources {
        mandate,
        journal,
        recorded_at,
        agent,
        workspace,
        account_ref,
        executor,
        run,
        artifacts,
        symbols,
        transport: bridge,
    }
}
