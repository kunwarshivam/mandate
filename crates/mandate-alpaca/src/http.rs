//! The HTTPS transport to Alpaca's **paper** trading host, the only trading host this crate
//! knows.
//!
//! There is no live base URL in this crate at all and a `live` cargo feature is forbidden
//! (ADR-0001 ES-23: "Only the paper trading and data hosts are compiled in"). Credentials live in
//! [`SecretString`]s, travel only as request headers marked sensitive, and never appear in
//! `Debug` output, in an error, in a draft, or in a file this code writes (`AGENTS.md` rule 7).
//!
//! # The allowlist is a type
//!
//! An [`HttpRequest`] has private fields. The only ways to obtain one are
//! [`HttpRequest::new`], which checks the **method and the path together** against
//! [`ENDPOINTS`] and refuses both account-wide endpoints, and [`HttpRequest::cancel_all`] and
//! [`HttpRequest::close_position`], which take the executor's [`AccountWideScope`] — a witness
//! only the account and workspace kill-switch paths can construct. So `DELETE /v2/orders`
//! (cancel every order) and `DELETE /v2/positions/{symbol}` (close a position) cannot be built,
//! let alone sent, from an agent-scoped path (`AGENTS.md` rule 13), and no request to a path
//! outside the table can exist for [`TradingTransport::send`] to be handed.

use std::future::Future;
use std::time::Duration;

use mandate_accounting::InstrumentId;
use mandate_executor::AccountWideScope;
use reqwest::Url;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use secrecy::{ExposeSecret, SecretString};

use crate::error::{CredentialsError, HttpSetupError, TransportError};

/// The paper trading host. The only one compiled in.
pub const PAPER_HOST: &str = "https://paper-api.alpaca.markets";
/// Environment variable holding the paper API key ID (ADR-0001 ES-19).
pub const KEY_ID_VAR: &str = "MANDATE_ALPACA_PAPER_KEY_ID";
/// Environment variable holding the paper API secret (ADR-0001 ES-19).
pub const SECRET_VAR: &str = "MANDATE_ALPACA_PAPER_SECRET";

const KEY_ID_HEADER: HeaderName = HeaderName::from_static("apca-api-key-id");
const SECRET_HEADER: HeaderName = HeaderName::from_static("apca-api-secret-key");
const USER_AGENT: &str = concat!("mandate-alpaca/", env!("CARGO_PKG_VERSION"));
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// The HTTP methods this crate uses. `PUT` and `PATCH` are absent because §5.1 forbids our own
/// replaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Delete,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Delete => "DELETE",
        }
    }
}

/// One call the client may make: a method and a whole-path pattern, matched together, and
/// whether the call is one of the two account-wide endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Endpoint {
    pub method: Method,
    /// A fixed path, or a fixed prefix plus exactly one more segment where the broker's grammar
    /// takes an id or a symbol (`{id}`, `{symbol}`).
    pub path: &'static str,
    /// `DELETE /v2/orders` and `DELETE /v2/positions/{symbol}`: reachable only through
    /// [`HttpRequest::cancel_all`] and [`HttpRequest::close_position`] (trading-domain spec §5.5).
    pub account_wide: bool,
}

const fn ordinary(method: Method, path: &'static str) -> Endpoint {
    Endpoint {
        method,
        path,
        account_wide: false,
    }
}

/// Every call this stream makes, as method and **whole path** together.
///
/// Matching is on the whole path, not on a prefix, because `/v2/account/activities` is an
/// endpoint and `/v2/account/configurations` is not, and a prefix match cannot tell them apart.
/// It is on the method as well, because `GET /v2/orders` lists the open orders and
/// `DELETE /v2/orders` cancels every one of them. There is no funding, transfer, or journal
/// endpoint here and no way to add one at runtime (`AGENTS.md` rule 8: no custody of funds), and
/// no `DELETE /v2/positions`, which would close every position at once.
pub const ENDPOINTS: [Endpoint; 11] = [
    ordinary(Method::Post, "/v2/orders"),
    ordinary(Method::Get, "/v2/orders"),
    ordinary(Method::Get, "/v2/orders:by_client_order_id"),
    ordinary(Method::Get, "/v2/orders/{id}"),
    ordinary(Method::Delete, "/v2/orders/{id}"),
    ordinary(Method::Get, "/v2/positions"),
    ordinary(Method::Get, "/v2/positions/{symbol}"),
    ordinary(Method::Get, "/v2/account"),
    ordinary(Method::Get, "/v2/account/activities"),
    Endpoint {
        method: Method::Delete,
        path: "/v2/orders",
        account_wide: true,
    },
    Endpoint {
        method: Method::Delete,
        path: "/v2/positions/{symbol}",
        account_wide: true,
    },
];

/// One request against the paper trading host: the method, the path and query, and the canonical
/// request body.
///
/// The fields are private. A request exists only if its method and path are one of
/// [`ENDPOINTS`] (see the module documentation), so the transport is never handed anything else.
/// A trading call is identified by **what it sends** as well as by where it sends it, which is
/// why the recorded fixtures record the method and the body and not only the path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    method: Method,
    path_and_query: String,
    body: Option<String>,
}

impl HttpRequest {
    /// A request to one of the **ordinary** endpoints. Anything else — a path outside the table,
    /// a method the table does not pair with the path, or either account-wide endpoint — is
    /// [`TransportError::RefusedPath`], and no request exists to be sent.
    pub fn new(
        method: Method,
        path_and_query: &str,
        body: Option<String>,
    ) -> Result<Self, TransportError> {
        match endpoint_for(method, path_and_query) {
            Some(endpoint) if !endpoint.account_wide => Ok(Self {
                method,
                path_and_query: path_and_query.to_owned(),
                body,
            }),
            Some(_) | None => Err(TransportError::RefusedPath),
        }
    }

    /// The broker's cancel-all, `DELETE /v2/orders`. Only an [`AccountWideScope`] opens it, and
    /// only the account and workspace kill switches hold one (trading-domain spec §5.5).
    pub fn cancel_all(scope: &AccountWideScope) -> Self {
        let _ = scope;
        Self {
            method: Method::Delete,
            path_and_query: "/v2/orders".to_owned(),
            body: None,
        }
    }

    /// The broker's close-position for one instrument, `DELETE /v2/positions/{symbol}`. Only an
    /// [`AccountWideScope`] opens it, and the symbol is an [`InstrumentId`], which is already one
    /// path segment.
    pub fn close_position(scope: &AccountWideScope, instrument: &InstrumentId) -> Self {
        let _ = scope;
        Self {
            method: Method::Delete,
            path_and_query: format!("/v2/positions/{}", instrument.as_str()),
            body: None,
        }
    }

    pub fn method(&self) -> Method {
        self.method
    }

    pub fn path_and_query(&self) -> &str {
        &self.path_and_query
    }

    pub fn body(&self) -> Option<&str> {
        self.body.as_deref()
    }

    /// The whole URL: the paper host and this path, and nothing a caller chose.
    pub fn url(&self) -> String {
        format!("{PAPER_HOST}{}", self.path_and_query)
    }
}

/// An HTTP response: the status code and the body bytes. No headers are carried out of the
/// transport, so an authorisation header cannot travel with one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Sends one request to the paper trading host. Every [`HttpRequest`] is already one of
/// [`ENDPOINTS`], so an implementation has nothing left to decide about what may be sent.
pub trait TradingTransport {
    fn send(&self, request: &HttpRequest)
    -> impl Future<Output = Result<Response, TransportError>>;
}

/// The paper account's trading credentials, read through an injected lookup so a test never
/// touches the process environment.
///
/// `Debug` prints neither value: the derive is deliberately absent and the manual implementation
/// names the fields without their contents (`AGENTS.md` rule 7, ES-09).
pub struct Credentials {
    key_id: SecretString,
    secret: SecretString,
}

impl core::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Credentials")
            .field("key_id", &"<redacted>")
            .field("secret", &"<redacted>")
            .finish()
    }
}

impl Credentials {
    /// Reads [`KEY_ID_VAR`] and [`SECRET_VAR`] through `lookup`; an empty value counts as unset.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, CredentialsError> {
        let read = |variable: &'static str| {
            lookup(variable)
                .filter(|value| !value.is_empty())
                .ok_or(CredentialsError::Missing { variable })
        };
        let key_id = read(KEY_ID_VAR)?;
        let secret = read(SECRET_VAR)?;
        Ok(Self {
            key_id: SecretString::from(key_id),
            secret: SecretString::from(secret),
        })
    }

    /// Reads the credentials from the process environment. Phase 1 connects with the account
    /// owner's own **paper** credentials; live credentials would come only from the vault, and
    /// that path is M13's, not this stream's (milestones M6, `AGENTS.md` rule 8).
    pub fn from_env() -> Result<Self, CredentialsError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }
}

/// Whether `path_and_query` is the path of any endpoint in [`ENDPOINTS`], with a path and query
/// that cannot change the host, add a header, or reach another endpoint. The method is checked
/// together with the path by [`endpoint_for`], which is what [`HttpRequest::new`] calls.
///
/// The path is compared whole; a `{…}` placeholder matches exactly one further segment of
/// unreserved characters. A crafted symbol or order id therefore cannot walk out of the endpoint
/// it was given to.
pub fn is_paper_trading_path(path_and_query: &str) -> bool {
    ENDPOINTS
        .iter()
        .any(|endpoint| path_matches(endpoint.path, path_and_query))
}

/// The one endpoint `method` and `path_and_query` name together, or `None`.
pub fn endpoint_for(method: Method, path_and_query: &str) -> Option<&'static Endpoint> {
    ENDPOINTS
        .iter()
        .find(|endpoint| endpoint.method == method && path_matches(endpoint.path, path_and_query))
}

/// One endpoint pattern against one whole path and its query.
fn path_matches(endpoint: &str, path_and_query: &str) -> bool {
    let (path, query) = match path_and_query.split_once('?') {
        Some((path, query)) => (path, query),
        None => (path_and_query, ""),
    };
    if path.contains("..") || !safe(query, b"-._~%&=:,") {
        return false;
    }
    match endpoint.split_once('{') {
        None => endpoint == path,
        Some((prefix, _)) => path
            .strip_prefix(prefix)
            .is_some_and(|segment| !segment.is_empty() && safe(segment, b"-._~%")),
    }
}

/// Whether every byte is unreserved or one of the extra characters the caller allows. A path
/// separator is never among them, which is what keeps a single-segment placeholder single.
fn safe(text: &str, extra: &[u8]) -> bool {
    text.bytes()
        .all(|b| b.is_ascii_alphanumeric() || extra.contains(&b))
}

/// HTTPS to [`PAPER_HOST`] with the paper credentials: HTTPS only, no redirects, bounded
/// timeouts, and TLS on the `ring` provider.
#[derive(Debug)]
pub struct AlpacaPaperHttp {
    client: reqwest::Client,
    credentials: Credentials,
}

impl AlpacaPaperHttp {
    pub fn new(credentials: Credentials) -> Result<Self, HttpSetupError> {
        let _already_installed = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .user_agent(USER_AGENT)
            .build()
            .map_err(|_| HttpSetupError::Client)?;
        Ok(Self {
            client,
            credentials,
        })
    }
}

impl TradingTransport for AlpacaPaperHttp {
    /// The request is one of [`ENDPOINTS`] by construction, so nothing here decides what may be
    /// sent: this only dials [`PAPER_HOST`] with the credentials as sensitive headers.
    async fn send(&self, request: &HttpRequest) -> Result<Response, TransportError> {
        let url = Url::parse(&request.url()).map_err(|_| TransportError::Request)?;
        let method = match request.method {
            Method::Get => reqwest::Method::GET,
            Method::Post => reqwest::Method::POST,
            Method::Delete => reqwest::Method::DELETE,
        };
        let mut headers = HeaderMap::new();
        headers.insert(KEY_ID_HEADER, sensitive(&self.credentials.key_id)?);
        headers.insert(SECRET_HEADER, sensitive(&self.credentials.secret)?);
        let mut builder = self.client.request(method, url).headers(headers);
        if let Some(body) = &request.body {
            builder = builder
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body.clone());
        }
        let response = builder.send().await.map_err(classify)?;
        let status = response.status().as_u16();
        let body = response.bytes().await.map_err(classify)?.to_vec();
        Ok(Response { status, body })
    }
}

fn sensitive(value: &SecretString) -> Result<HeaderValue, TransportError> {
    let mut header =
        HeaderValue::from_str(value.expose_secret()).map_err(|_| TransportError::Request)?;
    header.set_sensitive(true);
    Ok(header)
}

/// Maps a transport failure to a reason that carries no URL, header, or body.
fn classify(error: reqwest::Error) -> TransportError {
    if error.is_timeout() {
        TransportError::Timeout
    } else if error.is_connect() {
        TransportError::Connect
    } else {
        TransportError::Request
    }
}
