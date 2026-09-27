//! The HTTPS transport to Alpaca's **paper** trading host, the only trading host this crate
//! knows.
//!
//! There is no live base URL in this crate at all and a `live` cargo feature is forbidden
//! (ADR-0001 ES-23: "Only the paper trading and data hosts are compiled in"). Credentials live in
//! [`SecretString`]s, travel only as request headers marked sensitive, and never appear in
//! `Debug` output, in an error, in a draft, or in a file this code writes (`AGENTS.md` rule 7).

use std::future::Future;
use std::time::Duration;

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

/// The seven endpoints this stream needs, as **whole paths**: a fixed path, or a fixed prefix
/// plus exactly one more segment where the broker's grammar takes an id.
///
/// Matching is on the whole path, not on a prefix, because `/v2/account/activities` is an
/// endpoint and `/v2/account/configurations` is not, and a prefix match cannot tell them apart.
/// There is no funding, transfer, or journal endpoint here and no way to add one at runtime
/// (`AGENTS.md` rule 8: no custody of funds).
pub const ENDPOINTS: [&str; 7] = [
    "/v2/orders",
    "/v2/orders:by_client_order_id",
    "/v2/orders/{id}",
    "/v2/positions",
    "/v2/positions/{symbol}",
    "/v2/account",
    "/v2/account/activities",
];

const KEY_ID_HEADER: HeaderName = HeaderName::from_static("apca-api-key-id");
const SECRET_HEADER: HeaderName = HeaderName::from_static("apca-api-secret-key");
const USER_AGENT: &str = concat!("mandate-alpaca/", env!("CARGO_PKG_VERSION"));
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// The HTTP methods this crate uses. `PUT` is absent because §5.1 forbids our own replaces.
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

/// One request against the paper trading host: the method, the path and query, and the canonical
/// request body.
///
/// A trading call is identified by **what it sends** as well as by where it sends it, which is
/// why the recorded fixtures record the method and the body and not only the path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: Method,
    pub path_and_query: String,
    pub body: Option<String>,
}

/// An HTTP response: the status code and the body bytes. No headers are carried out of the
/// transport, so an authorisation header cannot travel with one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Sends one request to the paper trading host.
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

/// Whether `path_and_query` names one of the seven paper trading endpoints, with a path and
/// query that cannot change the host, add a header, or reach another endpoint.
///
/// The path is compared whole against [`ENDPOINTS`]; a `{…}` placeholder matches exactly one
/// further segment of unreserved characters. A crafted symbol or order id therefore cannot walk
/// out of the endpoint it was given to.
pub fn is_paper_trading_path(path_and_query: &str) -> bool {
    let (path, query) = match path_and_query.split_once('?') {
        Some((path, query)) => (path, query),
        None => (path_and_query, ""),
    };
    if path.contains("..") || !safe(query, b"-._~%&=:,") {
        return false;
    }
    ENDPOINTS.iter().any(|endpoint| matches(endpoint, path))
}

/// One endpoint pattern against one whole path.
fn matches(endpoint: &str, path: &str) -> bool {
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
    /// The path is checked against the allowlist **before anything is sent**, so no caller can
    /// reach a host or an endpoint this crate does not name, and a refusal costs no round trip.
    async fn send(&self, request: &HttpRequest) -> Result<Response, TransportError> {
        if !is_paper_trading_path(&request.path_and_query) {
            return Err(TransportError::RefusedPath);
        }
        let url = Url::parse(&format!("{PAPER_HOST}{}", request.path_and_query))
            .map_err(|_| TransportError::RefusedPath)?;
        let base = Url::parse(PAPER_HOST).map_err(|_| TransportError::RefusedPath)?;
        if url.origin() != base.origin() || url.fragment().is_some() {
            return Err(TransportError::RefusedPath);
        }
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
