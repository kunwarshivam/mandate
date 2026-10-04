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

use crate::data::{DATA_HOST, DataTransport, QuoteRequest};
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
/// no `DELETE /v2/positions`, which would close every position at once. `GET /v2/assets/{symbol}`
/// is the instrument read of E7-8 (trading-domain spec §3.1, DEC-168).
pub const ENDPOINTS: [Endpoint; 12] = [
    ordinary(Method::Post, "/v2/orders"),
    ordinary(Method::Get, "/v2/orders"),
    ordinary(Method::Get, "/v2/orders:by_client_order_id"),
    ordinary(Method::Get, "/v2/orders/{id}"),
    ordinary(Method::Delete, "/v2/orders/{id}"),
    ordinary(Method::Get, "/v2/positions"),
    ordinary(Method::Get, "/v2/positions/{symbol}"),
    ordinary(Method::Get, "/v2/account"),
    ordinary(Method::Get, "/v2/account/activities"),
    ordinary(Method::Get, "/v2/assets/{symbol}"),
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
    pub fn cancel_all(scope: &AccountWideScope) -> Result<Self, TransportError> {
        let _ = scope;
        Self::account_wide(Method::Delete, "/v2/orders")
    }

    /// The broker's close-position for one instrument, `DELETE /v2/positions/{symbol}`. Only an
    /// [`AccountWideScope`] opens it.
    ///
    /// The path is [`position_path`]'s. A symbol it cannot write as one segment is
    /// [`TransportError::RefusedPath`] and nothing is sent, so the connector answers `NotSent`
    /// (DEC-133 item 32), never a broker's rejection: a kill switch must tell a URL we could not
    /// build from a broker that refused.
    pub fn close_position(
        scope: &AccountWideScope,
        instrument: &InstrumentId,
    ) -> Result<Self, TransportError> {
        let _ = scope;
        Self::close_position_path(instrument)
    }

    /// One instrument's position, `GET /v2/positions/{symbol}`, on [`position_path`]'s path.
    pub fn position(instrument: &InstrumentId) -> Result<Self, TransportError> {
        Self::new(Method::Get, &position_path(instrument)?, None)
    }

    /// One instrument's asset record, `GET /v2/assets/{symbol}`, on [`asset_path`]'s path (E7-8).
    pub fn asset(instrument: &InstrumentId) -> Result<Self, TransportError> {
        Self::new(Method::Get, &asset_path(instrument)?, None)
    }

    /// [`Self::close_position`] after its scope is shown, split out so this crate's tests, which
    /// hold no [`AccountWideScope`], build the very request the kill switch does.
    fn close_position_path(instrument: &InstrumentId) -> Result<Self, TransportError> {
        Self::account_wide(Method::Delete, &position_path(instrument)?)
    }

    /// The one constructor of an account-wide request. `method` and `path_and_query` must name an
    /// account-wide endpoint of [`ENDPOINTS`] together, which is [`is_paper_trading_path`]'s check
    /// with the method added, or no request exists.
    fn account_wide(method: Method, path_and_query: &str) -> Result<Self, TransportError> {
        match endpoint_for(method, path_and_query) {
            Some(endpoint) if endpoint.account_wide => Ok(Self {
                method,
                path_and_query: path_and_query.to_owned(),
                body: None,
            }),
            Some(_) | None => Err(TransportError::RefusedPath),
        }
    }

    /// [`Self::close_position`] for this crate's own tests, which hold no [`AccountWideScope`].
    #[cfg(test)]
    pub(crate) fn close_position_for_tests(
        instrument: &InstrumentId,
    ) -> Result<Self, TransportError> {
        Self::close_position_path(instrument)
    }

    /// [`Self::account_wide`] for this crate's own tests, which hold no [`AccountWideScope`]:
    /// only the executor's kill-switch paths can make one, and no public constructor is added.
    #[cfg(test)]
    pub(crate) fn account_wide_for_tests(
        method: Method,
        path_and_query: &str,
    ) -> Result<Self, TransportError> {
        Self::account_wide(method, path_and_query)
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
    if path.split('/').any(is_dot_segment) || !safe(query, b"-._~%&=:,") {
        return false;
    }
    match endpoint.split_once('{') {
        None => endpoint == path,
        Some((prefix, _)) => path
            .strip_prefix(prefix)
            .is_some_and(|segment| !segment.is_empty() && safe(segment, b"-._~%")),
    }
}

/// `/v2/positions/{symbol}` for one instrument, the one path both the positions read and the
/// account-wide close are built on, with the symbol written as [`symbol_segment`] writes it.
pub fn position_path(instrument: &InstrumentId) -> Result<String, TransportError> {
    Ok(format!("/v2/positions/{}", symbol_segment(instrument)?))
}

/// `/v2/assets/{symbol}` for one instrument, the instrument read of E7-8, with the symbol written
/// as [`symbol_segment`] writes it.
pub fn asset_path(instrument: &InstrumentId) -> Result<String, TransportError> {
    Ok(format!("/v2/assets/{}", symbol_segment(instrument)?))
}

/// One instrument's symbol as one path segment.
///
/// A crypto pair is written without its slash, `BTC/USD` as `BTCUSD`: Alpaca answers
/// `/v2/positions/BTC/USD` with a 404 and `/v2/positions/BTCUSD` with the position (alpaca-py
/// issue 537, on the paper host), and keeps the slash-free form as its legacy pair symbol. Only a
/// pair of two letter-and-digit halves loses its slash; every other symbol must already be one
/// segment of `wire`'s symbol alphabet, letters, digits and `.`, starting with a letter or a
/// digit. Anything else — a third segment, an empty half, a dot segment, a percent sign, a query
/// — is [`TransportError::RefusedPath`], so a hostile instrument id is never folded into another
/// symbol's path, and the allowlist still judges the path it is put in.
pub fn symbol_segment(instrument: &InstrumentId) -> Result<String, TransportError> {
    let symbol = instrument.as_str();
    let segment = match symbol.split_once('/') {
        None => symbol.to_owned(),
        Some((base, quote)) if is_pair_half(base) && is_pair_half(quote) => {
            format!("{base}{quote}")
        }
        Some(_) => return Err(TransportError::RefusedPath),
    };
    if is_symbol_like(&segment) {
        Ok(segment)
    } else {
        Err(TransportError::RefusedPath)
    }
}

/// One segment of `wire`'s symbol alphabet: letters, digits and `.`, starting with a letter or a
/// digit.
pub(crate) fn is_symbol_like(segment: &str) -> bool {
    segment
        .bytes()
        .next()
        .is_some_and(|b| b.is_ascii_alphanumeric())
        && segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.')
}

/// One half of a crypto pair: letters and digits only, and at least one.
pub(crate) fn is_pair_half(half: &str) -> bool {
    !half.is_empty() && half.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Whether `segment` is a dot segment, `.` or `..`, in any spelling a URL parser decodes: each dot
/// may be written `%2e` or `%2E` (the WHATWG URL standard, which `url` implements). A parser
/// removes such a segment, and `..` the one before it too, so `DELETE /v2/orders/%2e` would be sent
/// as `DELETE /v2/orders/`, the account-wide cancel-all. `A..B` is not one and is sent unchanged.
fn is_dot_segment(segment: &str) -> bool {
    let decoded = segment.to_ascii_lowercase().replace("%2e", ".");
    decoded == "." || decoded == ".."
}

/// The URL `path_and_query` is sent to, when a parser leaves the path and the query exactly as
/// they were built, and [`TransportError::RefusedPath`] otherwise. The allowlist judges the text
/// it was handed; this is what makes the text judged the text sent, so no normalisation, of a dot
/// segment or of anything a later parser version decodes, can move a request to another endpoint.
fn sent_as_built(path_and_query: &str) -> Result<Url, TransportError> {
    sent_as_built_on(PAPER_HOST, path_and_query)
}

/// [`sent_as_built`] on either compiled-in host: the paper trading host or, for a
/// [`QuoteRequest`], the data host.
fn sent_as_built_on(host: &str, path_and_query: &str) -> Result<Url, TransportError> {
    let url =
        Url::parse(&format!("{host}{path_and_query}")).map_err(|_| TransportError::Request)?;
    let sent = match url.query() {
        Some(query) => format!("{}?{query}", url.path()),
        None => url.path().to_owned(),
    };
    if sent == path_and_query {
        Ok(url)
    } else {
        Err(TransportError::RefusedPath)
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
        let url = sent_as_built(request.path_and_query())?;
        let method = match request.method {
            Method::Get => reqwest::Method::GET,
            Method::Post => reqwest::Method::POST,
            Method::Delete => reqwest::Method::DELETE,
        };
        self.dispatch(method, url, request.body.as_deref()).await
    }
}

impl DataTransport for AlpacaPaperHttp {
    /// The request is a latest-quote read by construction, so this only dials [`DATA_HOST`] with
    /// the same paper credentials, the same way (DEC-168 item 2).
    async fn send(&self, request: &QuoteRequest) -> Result<Response, TransportError> {
        self.dispatch(reqwest::Method::GET, data_url(request)?, None)
            .await
    }
}

/// The URL a latest-quote read is dialled at: always on [`DATA_HOST`], and only if a parser sends
/// it exactly as it was built.
fn data_url(request: &QuoteRequest) -> Result<Url, TransportError> {
    sent_as_built_on(DATA_HOST, request.path_and_query())
}

impl AlpacaPaperHttp {
    /// The one place either host is dialled: the credentials as sensitive headers, a JSON body
    /// when there is one, and a response that carries no header out.
    async fn dispatch(
        &self,
        method: reqwest::Method,
        url: Url,
        body: Option<&str>,
    ) -> Result<Response, TransportError> {
        let mut headers = HeaderMap::new();
        headers.insert(KEY_ID_HEADER, sensitive(&self.credentials.key_id)?);
        headers.insert(SECRET_HEADER, sensitive(&self.credentials.secret)?);
        let mut builder = self.client.request(method, url).headers(headers);
        if let Some(body) = body {
            builder = builder
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body.to_owned());
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

#[cfg(test)]
mod tests {
    use mandate_accounting::InstrumentId;

    use super::{
        AlpacaPaperHttp, Credentials, DATA_HOST, DataTransport, ENDPOINTS, HttpRequest, Method,
        QuoteRequest, TradingTransport, TransportError, data_url, is_dot_segment, position_path,
        sent_as_built, sent_as_built_on,
    };

    /// The account-wide constructor is the exact dual of [`HttpRequest::new`] over the whole
    /// endpoint table: it builds precisely the two endpoints the table marks account-wide and
    /// refuses every other, so no ordinary path can be reached through it and no account-wide
    /// path can be reached around it (#195 round 2, the coordinator's merge note). The loop runs
    /// through the `#[cfg(test)]` constructor, which builds a request and never a scope.
    #[test]
    fn the_account_wide_constructor_is_the_exact_dual_of_new_over_every_endpoint() {
        for endpoint in ENDPOINTS {
            let path = endpoint
                .path
                .replace("{id}", "abc")
                .replace("{symbol}", "AAPL");
            assert_eq!(
                HttpRequest::account_wide_for_tests(endpoint.method, &path).is_ok(),
                endpoint.account_wide,
                "{} {path}: the account-wide constructor is the exact dual of `new`",
                endpoint.method.as_str()
            );
        }
    }

    /// A request as [`HttpRequest::close_position`] builds one: straight from an instrument id,
    /// which the allowlist never sees, so only the transport's own check stands between a hostile
    /// symbol and the URL a parser would make of it.
    fn built_directly(method: Method, path_and_query: &str) -> HttpRequest {
        HttpRequest {
            method,
            path_and_query: path_and_query.to_owned(),
            body: None,
        }
    }

    fn transport() -> Result<AlpacaPaperHttp, String> {
        let credentials = Credentials::from_lookup(|name| Some(format!("unit-test-{name}")))
            .map_err(|e| e.to_string())?;
        AlpacaPaperHttp::new(credentials).map_err(|e| e.to_string())
    }

    #[test]
    fn a_dot_segment_is_one_in_every_spelling_a_parser_decodes() {
        for segment in [".", "..", "%2e", "%2E", ".%2e", "%2e.", "%2E%2e", "%2e%2E"] {
            assert!(is_dot_segment(segment), "{segment} is a dot segment");
        }
        for segment in [
            "",
            "A..B",
            "...",
            "%2e%2e%2e",
            "a.",
            ".a",
            "%2f",
            "AAPL",
            "BRK.B",
        ] {
            assert!(!is_dot_segment(segment), "{segment} is an ordinary segment");
        }
    }

    #[test]
    fn a_path_a_parser_leaves_alone_is_sent_as_built() {
        for path in [
            "/v2/orders/A..B",
            "/v2/orders?status=open&limit=50",
            "/v2/positions/BRK.B",
            "/v2/orders:by_client_order_id?client_order_id=md-abc",
        ] {
            let sent = sent_as_built(path).map(|url| url.to_string());
            assert_eq!(
                sent,
                Ok(format!("https://paper-api.alpaca.markets{path}")),
                "{path} is sent byte for byte as it was built"
            );
        }
    }

    #[tokio::test]
    async fn a_request_a_parser_would_move_is_refused_and_never_sent() -> Result<(), String> {
        let transport = transport()?;
        for (method, path) in [
            (Method::Delete, "/v2/positions/."),
            (Method::Delete, "/v2/positions/%2e"),
            (Method::Delete, "/v2/positions/%2E%2e"),
            (Method::Delete, "/v2/positions/AAPL/.."),
            (Method::Delete, "/v2/orders/.%2e"),
            (Method::Get, "/v2/orders/%2e"),
        ] {
            let request = built_directly(method, path);
            assert_eq!(
                TradingTransport::send(&transport, &request).await.err(),
                Some(TransportError::RefusedPath),
                "{} {path} would be sent to another endpoint once parsed, so it is refused \
                 before anything leaves (DEC-133 item 18a)",
                method.as_str()
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn a_quote_read_a_parser_would_move_is_refused_and_never_sent() -> Result<(), String> {
        let transport = transport()?;
        for path in [
            "/v2/stocks/%2e%2e/quotes/latest?feed=iex",
            "/v2/stocks/AAPL/../../v2/account",
        ] {
            let request = QuoteRequest::for_tests(path);
            assert_eq!(
                DataTransport::send(&transport, &request).await.err(),
                Some(TransportError::RefusedPath),
                "{path} would be sent somewhere else once parsed, so it is refused before \
                 anything leaves"
            );
        }
        Ok(())
    }

    /// #288 review, minor 2: the data transport dials the data host, never the trading host.
    #[test]
    fn a_quote_read_is_dialled_at_the_data_host() -> Result<(), String> {
        let aapl = InstrumentId::new("AAPL").map_err(|e| format!("{e:?}"))?;
        let request = QuoteRequest::latest(&aapl).map_err(|e| e.to_string())?;
        let url = data_url(&request).map_err(|e| e.to_string())?;
        assert_eq!(url.host_str(), Some("data.alpaca.markets"));
        assert_eq!(
            url.as_str(),
            "https://data.alpaca.markets/v2/stocks/AAPL/quotes/latest?feed=iex"
        );
        Ok(())
    }

    #[test]
    fn a_quote_read_is_sent_to_the_data_host_as_built() {
        for path in [
            "/v2/stocks/AAPL/quotes/latest?feed=iex",
            "/v1beta3/crypto/us/latest/quotes?symbols=BTC%2FUSD",
        ] {
            assert_eq!(
                sent_as_built_on(DATA_HOST, path).map(|url| url.to_string()),
                Ok(format!("https://data.alpaca.markets{path}")),
                "{path} is sent to the data host byte for byte"
            );
        }
        assert_eq!(
            sent_as_built_on(DATA_HOST, "/v2/stocks/%2e/quotes").err(),
            Some(TransportError::RefusedPath)
        );
    }

    fn instrument(symbol: &str) -> Result<InstrumentId, String> {
        InstrumentId::new(symbol).map_err(|e| format!("{e:?}"))
    }

    /// The path of a request, or its refusal.
    fn path_of(request: Result<HttpRequest, TransportError>) -> Result<String, TransportError> {
        request.map(|request| request.path_and_query().to_owned())
    }

    #[test]
    fn a_crypto_pair_is_one_segment_without_its_slash_in_a_position_path() -> Result<(), String> {
        for (symbol, path) in [
            ("BTC/USD", "/v2/positions/BTCUSD"),
            ("ETH/USDT", "/v2/positions/ETHUSDT"),
            ("BRK.B", "/v2/positions/BRK.B"),
            ("AAPL", "/v2/positions/AAPL"),
        ] {
            let instrument = instrument(symbol)?;
            let expected = Ok(path.to_owned());
            assert_eq!(
                position_path(&instrument),
                expected,
                "{symbol}: Alpaca answers 404 on `/v2/positions/BTC/USD` and 200 on \
                 `/v2/positions/BTCUSD`, its legacy pair symbol (#195 finding 1)"
            );
            assert_eq!(
                path_of(HttpRequest::position(&instrument)),
                expected,
                "the positions read of {symbol} is built on that path"
            );
            assert_eq!(
                path_of(HttpRequest::close_position_for_tests(&instrument)),
                expected,
                "and so is its account-wide close"
            );
            assert_eq!(
                sent_as_built(path).map(|url| url.to_string()),
                Ok(format!("https://paper-api.alpaca.markets{path}")),
                "{path} is sent byte for byte as it was built"
            );
        }
        let close = HttpRequest::close_position_for_tests(&instrument("BTC/USD")?);
        assert_eq!(close.as_ref().map(HttpRequest::method), Ok(Method::Delete));
        assert_eq!(close.as_ref().map(HttpRequest::body), Ok(None));
        let read = HttpRequest::position(&instrument("BTC/USD")?);
        assert_eq!(read.as_ref().map(HttpRequest::method), Ok(Method::Get));
        assert_eq!(read.as_ref().map(HttpRequest::body), Ok(None));
        Ok(())
    }

    #[test]
    fn a_hostile_symbol_never_becomes_a_position_path() -> Result<(), String> {
        for symbol in [
            "../AAPL",
            "AAPL/..",
            "A/B/C",
            "BTC/",
            "/USD",
            "/",
            "BTC/US.D",
            "BTC//USD",
            ".",
            "..",
            "%2e%2e",
            ".A",
            "A%2FB",
            "AAPL?percentage=1",
            "AAPL#x",
            "B TC",
            "BTC/USD?x=1",
        ] {
            let instrument = instrument(symbol)?;
            assert_eq!(
                position_path(&instrument),
                Err(TransportError::RefusedPath),
                "{symbol:?} is not a symbol, so no position path is written for it"
            );
            assert_eq!(
                path_of(HttpRequest::position(&instrument)),
                Err(TransportError::RefusedPath),
                "no read of {symbol:?} is built"
            );
            assert_eq!(
                path_of(HttpRequest::close_position_for_tests(&instrument)),
                Err(TransportError::RefusedPath),
                "and no close of {symbol:?}, so the connector answers `NotSent` \
                 (DEC-133 item 32)"
            );
        }
        Ok(())
    }
}
