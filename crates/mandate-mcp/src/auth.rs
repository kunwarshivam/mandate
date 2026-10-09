//! The founder's OAuth login (E7-24), its first steps (O1a, DEC-847): discovery of the
//! authorization server from the MCP server's published metadata, and registration of a public
//! client with a loopback redirect, as the MCP authorization flow describes them. No token, code,
//! verifier, or secret passes through them.
//!
//! - **Pinned hosts only.** The protected-resource metadata is read only from the pinned MCP host;
//!   the authorization server and every endpoint it names must be on a host the connector
//!   compiles in. Nothing off those hosts is ever dialed (LT-1, connections spec §6.2 rule 1).
//! - **Identifiers exact.** The metadata's `resource` is the endpoint and its `issuer` is the URL it
//!   was read for, character for character (RFC 9728 §3.3, RFC 8414 §3.3).
//! - **A public client only.** `S256` must be offered and a registration endpoint must exist; a
//!   registration answer that issues a secret or changes the redirect is refused.
//! - **One callback per login** (O1b, DEC-855): fresh PKCE and an exact single-use `state`;
//!   secrets are [`SecretString`]s only, never printed or in an error (LT-9).

use std::io::Write;
use std::net::Ipv4Addr;
use std::time::Duration;

use reqwest::header::{ACCEPT, CONTENT_TYPE, WWW_AUTHENTICATE};
use reqwest::{RequestBuilder, Response, StatusCode, Url};
use secrecy::{ExposeSecret, SecretString};
use serde::de::{DeserializeOwned, IgnoredAny};
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};

use crate::endpoint::{Build, PinnedEndpoint, scheme_allowed};
use crate::error::McpError;
use crate::frame;
use crate::transport::{
    Monotonic, PROTOCOL_VERSION, TransportConfig, classify, http_client, is_visible_ascii,
    read_capped,
};

/// The JSON-RPC id of the unauthenticated `initialize` that draws the challenge.
const PROBE_ID: u64 = 1;
const RESOURCE_WELL_KNOWN: &str = "/.well-known/oauth-protected-resource";
const SERVER_WELL_KNOWN: &str = "/.well-known/oauth-authorization-server";
const JSON: &str = "application/json";
const FORM: &str = "application/x-www-form-urlencoded";
const BASE64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
const BLANK_LINE: &[u8] = b"\r\n\r\n";
/// The most bytes a callback's head may have, its blank line included (DEC-861 item 2).
const MAX_HEAD: usize = 8192;
const RECEIVED: &str = "HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\nconnection: close\r\n\r\n\
    Login received. You can close this window.";
const REFUSED: &str = "HTTP/1.1 400 Bad Request\r\ncontent-type: text/plain\r\nconnection: close\r\n\r\n\
    Login refused. You can close this window.";

/// The members of RFC 9728 protected-resource metadata discovery reads.
#[derive(Deserialize)]
struct ResourceMetadata {
    resource: String,
    authorization_servers: Vec<String>,
}

/// The members of RFC 8414 authorization server metadata discovery reads.
#[derive(Deserialize)]
struct ServerMetadata {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    registration_endpoint: Option<String>,
    response_types_supported: Vec<String>,
    #[serde(default)]
    code_challenge_methods_supported: Vec<String>,
}

/// The members of an RFC 6749 §5.1 token answer the exchange reads; a refresh token is never read.
#[derive(Deserialize)]
struct Granted {
    access_token: Option<Value>,
    token_type: Option<Value>,
}

/// The members of an RFC 7591 registration answer the login reads. A client secret is noted only
/// as present, `null` included, and its value is never kept.
#[derive(Deserialize)]
struct Registered {
    client_id: Option<Value>,
    #[serde(default, deserialize_with = "noted")]
    client_secret: bool,
    #[serde(default, deserialize_with = "kept")]
    token_endpoint_auth_method: Option<Value>,
    #[serde(default, deserialize_with = "kept")]
    redirect_uris: Option<Value>,
    #[serde(default, deserialize_with = "kept")]
    grant_types: Option<Value>,
    #[serde(default, deserialize_with = "kept")]
    response_types: Option<Value>,
}

/// A member that is in the answer, whatever its value, which is skipped unread.
fn noted<'de, D: Deserializer<'de>>(member: D) -> Result<bool, D::Error> {
    IgnoredAny::deserialize(member).map(|_| true)
}

/// A member that is in the answer, `null` included.
fn kept<'de, D: Deserializer<'de>>(member: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(member).map(Some)
}

/// The authorization server discovery found, with the endpoints the login dials.
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "the authorization request and the token exchange (O1b) read them"
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthServer {
    pub(crate) resource: Url,
    pub(crate) issuer: Url,
    pub(crate) authorization_endpoint: Url,
    pub(crate) token_endpoint: Url,
    pub(crate) registration_endpoint: Url,
}

/// The redirect a login registers: `http://127.0.0.1:<port>/callback`, an IP literal on loopback
/// (RFC 8252 §7.3). Only the port is chosen, so no other redirect can be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopbackRedirect {
    port: u16,
}

impl LoopbackRedirect {
    /// Port 0 is [`McpError::BadConfig`]: the redirect names the port the listener bound.
    pub fn new(port: u16) -> Result<Self, McpError> {
        if port == 0 {
            return Err(McpError::BadConfig);
        }
        Ok(Self { port })
    }

    pub fn uri(&self) -> String {
        format!("http://127.0.0.1:{}/callback", self.port)
    }
}

/// The client the authorization server registered: a public client id, which is not a secret
/// (RFC 6749 §2.2), and the one redirect it was registered with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientRegistration {
    pub(crate) client_id: String,
    pub(crate) redirect: LoopbackRedirect,
}

/// A login begun by [`AuthServer::begin`]. [`PendingLogin::callback`] takes it by value and it is
/// not `Clone`, so a replayed callback has nothing left to match:
///
/// ```compile_fail
/// fn replay(login: mandate_mcp::PendingLogin, target: &str) {
///     let _ = login.callback(target);
///     let _ = login.callback(target);
/// }
/// ```
///
/// ```compile_fail
/// fn copy(login: &mandate_mcp::PendingLogin) -> mandate_mcp::PendingLogin {
///     login.clone()
/// }
/// ```
#[derive(Debug)]
pub struct PendingLogin {
    pub(crate) state: SecretString,
    pub(crate) verifier: SecretString,
    pub(crate) client: ClientRegistration,
    pub(crate) issuer: Url,
}

/// The code the callback carried, with the verifier and client that redeem it once at the token
/// endpoint (O1b part 2).
#[derive(Debug)]
pub struct AuthorizationCode {
    pub(crate) code: SecretString,
    pub(crate) verifier: SecretString,
    pub(crate) client: ClientRegistration,
}

impl PendingLogin {
    /// The request-target of the browser's `GET` to the loopback redirect; any target spends the
    /// login. The path must be exactly `/callback` ([`McpError::Malformed`]); `state` must appear
    /// once, equal to the login's own once percent-decoded ([`McpError::StateMismatch`]); then an
    /// `iss`, if any, must appear once and equal the issuer ([`McpError::IssuerMismatch`]); then
    /// `error` is [`McpError::AuthorizationDenied`], its text unread; `code` must appear once, as
    /// non-empty visible ASCII ([`McpError::Malformed`]). Other members are not read. No error
    /// carries the state, the code or the target's text (DEC-855 items 1 to 4).
    pub fn callback(self, target: &str) -> Result<AuthorizationCode, McpError> {
        let query = target
            .strip_prefix("/callback?")
            .ok_or(McpError::Malformed)?;
        let mut url = Url::parse("http://127.0.0.1/").map_err(|_| McpError::Malformed)?;
        url.set_query(Some(query));
        let all = |name: &str| -> Vec<String> {
            let named = url.query_pairs().filter(|(key, _)| key == name);
            named.map(|(_, value)| value.into_owned()).collect()
        };
        let own = self.state.expose_secret();
        if !matches!(all("state").as_slice(), [state] if state == own) {
            return Err(McpError::StateMismatch);
        }
        let issuer = self.issuer.as_str();
        match all("iss").as_slice() {
            [] => {}
            [iss] if iss == issuer => {}
            _ => return Err(McpError::IssuerMismatch),
        }
        if !all("error").is_empty() {
            return Err(McpError::AuthorizationDenied);
        }
        match all("code").as_slice() {
            [code] if is_visible_ascii(code.as_bytes()) => Ok(AuthorizationCode {
                code: SecretString::from(code.clone()),
                verifier: self.verifier,
                client: self.client,
            }),
            _ => Err(McpError::Malformed),
        }
    }
}

/// A login's lifetime from when it began; a callback at or after it is spent (DEC-691 item 6).
pub const LOGIN_LIFETIME: Duration = Duration::from_secs(600);

/// The token, a [`SecretString`] only: never printed, displayed or written (LT-9, DEC-861).
#[cfg_attr(not(test), allow(dead_code, reason = "the MCP session (O2) reads it"))]
#[derive(Debug)]
pub struct AccessToken {
    pub(crate) secret: SecretString,
}

/// The listener on the loopback redirect: `127.0.0.1` only, on a port the system picks.
#[derive(Debug)]
pub struct CallbackListener {
    pub(crate) socket: tokio::net::TcpListener,
}

impl CallbackListener {
    /// Binds `127.0.0.1:0`, never another address, and returns the redirect naming its port.
    pub async fn bind() -> Result<(Self, LoopbackRedirect), McpError> {
        let socket = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|_| McpError::Network)?;
        let port = socket.local_addr().map_err(|_| McpError::Network)?.port();
        Ok((Self { socket }, LoopbackRedirect::new(port)?))
    }

    /// One request, then closed; `clock` started when `login` began (DEC-861 items 1 to 3).
    pub async fn accept(
        self,
        login: PendingLogin,
        clock: &dyn Monotonic,
    ) -> Result<AuthorizationCode, McpError> {
        let (stream, _) = self.socket.accept().await.map_err(|_| McpError::Network)?;
        drop(self.socket);
        let head = read_head(&stream).await;
        let outcome = if clock.elapsed() >= LOGIN_LIFETIME {
            Err(McpError::LoginExpired)
        } else {
            request_target(&head).and_then(|target| login.callback(target))
        };
        answer(stream, if outcome.is_ok() { RECEIVED } else { REFUSED });
        outcome
    }
}

impl AuthServer {
    /// The code exchange (RFC 6749 §4.1.3, RFC 7636 §4.5; DEC-861 items 4 and 5).
    pub async fn exchange(
        &self,
        code: AuthorizationCode,
        config: &TransportConfig,
    ) -> Result<AccessToken, McpError> {
        let client = http_client(config)?;
        let mut form = Url::parse("http://form.invalid/").map_err(|_| McpError::Malformed)?;
        form.query_pairs_mut()
            .append_pair("grant_type", "authorization_code")
            .append_pair("code", code.code.expose_secret())
            .append_pair("redirect_uri", &code.client.redirect.uri())
            .append_pair("client_id", &code.client.client_id)
            .append_pair("code_verifier", code.verifier.expose_secret())
            .append_pair("resource", self.resource.as_str());
        let request = client
            .post(self.token_endpoint.clone())
            .header(ACCEPT, JSON)
            .header(CONTENT_TYPE, FORM)
            .body(form.query().unwrap_or_default().to_owned());
        let response = send(request).await?;
        let granted: Granted = read_json(response, StatusCode::OK, config.max_answer_bytes).await?;
        let bearer = granted.token_type.as_ref().and_then(Value::as_str);
        if !bearer.is_some_and(|kind| kind.eq_ignore_ascii_case("Bearer")) {
            return Err(McpError::Malformed);
        }
        let token = granted.access_token.as_ref().and_then(Value::as_str);
        let token = token.filter(|token| is_visible_ascii(token.as_bytes()));
        let token = token.ok_or(McpError::Malformed)?;
        Ok(AccessToken {
            secret: SecretString::from(token.to_owned()),
        })
    }

    /// An unauthenticated `POST` of `initialize` to the endpoint, which must answer `401`; the
    /// Bearer challenge's `resource_metadata` URL, or without one the endpoint's RFC 9728 well-known
    /// URL, read from the pinned MCP host only; then the first of its `authorization_servers`,
    /// which must be on `auth_hosts`, read at its RFC 8414 well-known URL. Every endpoint named must
    /// be `https` (loopback `http` in this crate's test build) on `auth_hosts`, with no credentials
    /// or fragment; `S256` and response type `code` must be offered, and a registration endpoint
    /// must exist. No redirect is followed, and no server text reaches an error.
    pub async fn discover(
        endpoint: &PinnedEndpoint,
        auth_hosts: &[&str],
        config: &TransportConfig,
    ) -> Result<Self, McpError> {
        let client = http_client(config)?;
        let max_bytes = config.max_answer_bytes;
        let metadata_url = challenge(&client, endpoint).await?;
        let resource: ResourceMetadata = get_json(&client, metadata_url, max_bytes).await?;
        if resource.resource != endpoint.url().as_str() {
            return Err(McpError::ResourceMismatch);
        }
        let named = resource
            .authorization_servers
            .first()
            .ok_or(McpError::Malformed)?;
        let issuer = on_auth_host(named, auth_hosts)?;
        if issuer.query().is_some() {
            return Err(McpError::EndpointShape);
        }
        let metadata_url = well_known(&issuer, SERVER_WELL_KNOWN);
        let server: ServerMetadata = get_json(&client, metadata_url, max_bytes).await?;
        if server.issuer != *named {
            return Err(McpError::IssuerMismatch);
        }
        let authorization_endpoint = on_auth_host(&server.authorization_endpoint, auth_hosts)?;
        let token_endpoint = on_auth_host(&server.token_endpoint, auth_hosts)?;
        let registration = server
            .registration_endpoint
            .ok_or(McpError::RegistrationUnavailable)?;
        let registration_endpoint = on_auth_host(&registration, auth_hosts)?;
        if !server.response_types_supported.iter().any(|t| t == "code") {
            return Err(McpError::Malformed);
        }
        if !server
            .code_challenge_methods_supported
            .iter()
            .any(|m| m == "S256")
        {
            return Err(McpError::PkceUnsupported);
        }
        Ok(Self {
            resource: endpoint.url().clone(),
            issuer,
            authorization_endpoint,
            token_endpoint,
            registration_endpoint,
        })
    }

    /// [`AuthServer::begin_with`] with 32 bytes each for the verifier and the `state` from the
    /// operating system's generator, or [`McpError::RandomUnavailable`] if it fails.
    pub fn begin(&self, client: &ClientRegistration) -> Result<(Url, PendingLogin), McpError> {
        let mut verifier = [0_u8; 32];
        let mut state = [0_u8; 32];
        getrandom::getrandom(&mut verifier).map_err(|_| McpError::RandomUnavailable)?;
        getrandom::getrandom(&mut state).map_err(|_| McpError::RandomUnavailable)?;
        self.begin_with(client, &verifier, &state)
    }

    /// The authorization request (RFC 6749 §4.1.1, RFC 7636 §4.3, RFC 8707). The verifier and
    /// `state` are their seeds as unpadded base64url (RFC 7636 appendix B). The authorization
    /// endpoint must carry no query ([`McpError::EndpointShape`]); the URL adds exactly
    /// `response_type` `code`, `client_id`, `redirect_uri`, `code_challenge`
    /// BASE64URL(SHA-256(verifier)), `code_challenge_method` `S256`, `state`, and `resource` the
    /// MCP endpoint (DEC-855 items 5 and 6).
    pub(crate) fn begin_with(
        &self,
        client: &ClientRegistration,
        verifier_seed: &[u8; 32],
        state_seed: &[u8; 32],
    ) -> Result<(Url, PendingLogin), McpError> {
        if self.authorization_endpoint.query().is_some() {
            return Err(McpError::EndpointShape);
        }
        let verifier = base64url(verifier_seed);
        let state = base64url(state_seed);
        let challenge = base64url(mandate_canon::Digest::of(verifier.as_bytes()).as_bytes());
        let mut url = self.authorization_endpoint.clone();
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &client.client_id)
            .append_pair("redirect_uri", &client.redirect.uri())
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &state)
            .append_pair("resource", self.resource.as_str());
        let login = PendingLogin {
            state: SecretString::from(state),
            verifier: SecretString::from(verifier),
            client: client.clone(),
            issuer: self.issuer.clone(),
        };
        Ok((url, login))
    }

    /// RFC 7591 registration at the registration endpoint, as a public client: `client_name`
    /// `Mandate`, `authorization_code` only, response type `code`, `token_endpoint_auth_method`
    /// `none`, and `redirect` as the one redirect, with no other member and no `authorization`
    /// header. Only `201 Created` is a registration. An answer carrying a `client_secret` member
    /// (even empty or `null`) or another auth method is [`McpError::ClientSecretIssued`], and the
    /// secret is never read; one whose `redirect_uris` is anything but the one-element list sent
    /// is [`McpError::RedirectChanged`]; one without `token_endpoint_auth_method`, with
    /// `grant_types` or `response_types` other than those sent, or with a `client_id` that is
    /// missing or not non-empty visible ASCII is [`McpError::Malformed`]. No redirect is
    /// followed, and no server text reaches an error (DEC-847 items 5, 6 and 8).
    pub async fn register(
        &self,
        redirect: LoopbackRedirect,
        config: &TransportConfig,
    ) -> Result<ClientRegistration, McpError> {
        let client = http_client(config)?;
        let redirect_uris = json!([redirect.uri()]);
        let asked = json!({
            "client_name": "Mandate",
            "redirect_uris": redirect_uris,
            "grant_types": ["authorization_code"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none",
        });
        let request = client
            .post(self.registration_endpoint.clone())
            .header(ACCEPT, JSON)
            .header(CONTENT_TYPE, JSON)
            .body(asked.to_string());
        let response = send(request).await?;
        let registered: Registered =
            read_json(response, StatusCode::CREATED, config.max_answer_bytes).await?;
        let method = registered.token_endpoint_auth_method;
        let other_method = method.as_ref().is_some_and(|method| method != "none");
        if registered.client_secret || other_method {
            return Err(McpError::ClientSecretIssued);
        }
        if registered
            .redirect_uris
            .is_some_and(|listed| listed != redirect_uris)
        {
            return Err(McpError::RedirectChanged);
        }
        let other_grants = registered
            .grant_types
            .is_some_and(|grants| grants != json!(["authorization_code"]));
        let other_types = registered
            .response_types
            .is_some_and(|types| types != json!(["code"]));
        if method.is_none() || other_grants || other_types {
            return Err(McpError::Malformed);
        }
        let client_id = registered
            .client_id
            .as_ref()
            .and_then(Value::as_str)
            .filter(|id| is_visible_ascii(id.as_bytes()))
            .ok_or(McpError::Malformed)?;
        Ok(ClientRegistration {
            client_id: client_id.to_owned(),
            redirect,
        })
    }
}

/// The unauthenticated `initialize`, which must draw a `401`; the URL of the protected-resource
/// metadata its challenge names, which must be on the pinned MCP host, or else the endpoint's
/// RFC 9728 well-known URL.
async fn challenge(client: &reqwest::Client, endpoint: &PinnedEndpoint) -> Result<Url, McpError> {
    let params = json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": {},
        "clientInfo": {"name": "mandate-mcp", "version": env!("CARGO_PKG_VERSION")},
    });
    let request = client
        .post(endpoint.url().clone())
        .header(ACCEPT, "application/json, text/event-stream")
        .header(CONTENT_TYPE, JSON)
        .header("mcp-protocol-version", PROTOCOL_VERSION)
        .body(frame::request_body(Some(PROBE_ID), "initialize", &params));
    let response = send(request).await?;
    if response.status() != StatusCode::UNAUTHORIZED {
        return Err(McpError::HttpStatus {
            status: response.status().as_u16(),
        });
    }
    let named = response
        .headers()
        .get_all(WWW_AUTHENTICATE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find_map(resource_metadata);
    match named {
        Some(url) => {
            let host = endpoint.url().host_str().ok_or(McpError::HostNotPinned)?;
            Ok(PinnedEndpoint::new(host, url)?.url().clone())
        }
        None => Ok(well_known(endpoint.url(), RESOURCE_WELL_KNOWN)),
    }
}

/// The `resource_metadata` parameter of a challenge, a quoted string since a URL is no token
/// (RFC 9110 §11.2), matched only where a parameter starts: at the start or after a space or a
/// comma, and outside any quoted value. Quoted pairs are not read (DEC-847 item 8).
fn resource_metadata(challenge: &str) -> Option<&str> {
    const PARAMETER: &str = "resource_metadata=\"";
    let mut quoted = false;
    let mut boundary = true;
    for (at, byte) in challenge.bytes().enumerate() {
        if !quoted && boundary {
            let rest = challenge.get(at..)?;
            let named = rest
                .get(..PARAMETER.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(PARAMETER));
            if named {
                return rest.get(PARAMETER.len()..)?.split('"').next();
            }
        }
        if byte == b'"' {
            quoted = !quoted;
        }
        boundary = byte == b' ' || byte == b',';
    }
    None
}

/// The well-known URL inserted before the path, any terminating slash removed (RFC 9728 §3.1,
/// RFC 8414 §3.1).
fn well_known(url: &Url, prefix: &str) -> Url {
    let mut at = url.clone();
    at.set_path(&format!("{prefix}{}", url.path().trim_end_matches('/')));
    at
}

/// A URL the authorization server names: no credentials or fragment, `https` (loopback `http` in
/// this crate's test build), on one of `auth_hosts`. Nothing of the text reaches the error.
fn on_auth_host(text: &str, auth_hosts: &[&str]) -> Result<Url, McpError> {
    let url = Url::parse(text).map_err(|_| McpError::EndpointShape)?;
    if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return Err(McpError::EndpointShape);
    }
    if !scheme_allowed(&url, Build::CURRENT) {
        return Err(McpError::NotHttps);
    }
    if !url
        .host_str()
        .is_some_and(|host| auth_hosts.contains(&host))
    {
        return Err(McpError::AuthHostNotPinned);
    }
    Ok(url)
}

/// Unpadded base64url (RFC 7636 appendix A): each group of up to three bytes gives one more
/// character than it has bytes.
fn base64url(bytes: &[u8]) -> String {
    let mut text = String::new();
    for group in bytes.chunks(3) {
        let mut word = [0_u8; 4];
        for (to, from) in word.iter_mut().skip(1).zip(group) {
            *to = *from;
        }
        let word = u32::from_be_bytes(word);
        for shift in [18_u32, 12, 6, 0]
            .into_iter()
            .take(group.len().saturating_add(1))
        {
            let at = usize::try_from(word.wrapping_shr(shift) & 0x3f).ok();
            if let Some(&letter) = at.and_then(|at| BASE64URL.get(at)) {
                text.push(char::from(letter));
            }
        }
    }
    text
}

/// The bytes of one request up to its blank line, read until it ends, the connection closes, or
/// one byte more than [`MAX_HEAD`] has arrived.
async fn read_head(stream: &tokio::net::TcpStream) -> Vec<u8> {
    let mut head = Vec::new();
    let mut chunk = [0_u8; 1024];
    while !head.windows(4).any(|four| four == BLANK_LINE) {
        let room = MAX_HEAD.saturating_add(1).saturating_sub(head.len());
        let Some(into) = chunk.get_mut(..room.min(1024)) else {
            break;
        };
        let waiting = stream.peek(into).await.unwrap_or(0);
        let into = into.get_mut(..waiting).unwrap_or_default();
        let read = stream.try_read(into).unwrap_or(0);
        if read == 0 {
            break;
        }
        head.extend_from_slice(into.get(..read).unwrap_or_default());
    }
    head
}

/// The target of `GET <target> HTTP/1.1`, from a head that ends within [`MAX_HEAD`] bytes.
fn request_target(head: &[u8]) -> Result<&str, McpError> {
    let end = head.windows(4).position(|four| four == BLANK_LINE);
    let within = end
        .and_then(|end| end.checked_add(4))
        .filter(|&len| len <= MAX_HEAD);
    let head = within
        .and_then(|len| head.get(..len))
        .ok_or(McpError::Malformed)?;
    let text = std::str::from_utf8(head).map_err(|_| McpError::Malformed)?;
    let line = text.split("\r\n").next().unwrap_or_default();
    match line.split(' ').collect::<Vec<_>>().as_slice() {
        ["GET", target, "HTTP/1.1"] => Ok(target),
        _ => Err(McpError::Malformed),
    }
}

/// The fixed answer, written in blocking mode: a hundred bytes into a fresh loopback socket's
/// empty send buffer, so nothing waits. Dropping the stream closes it.
fn answer(stream: tokio::net::TcpStream, text: &str) {
    let Ok(mut stream) = stream.into_std() else {
        return;
    };
    let _blocking = stream.set_nonblocking(false);
    let _unread_by_a_closed_browser = stream.write_all(text.as_bytes());
}

/// A `3xx` ends the exchange: no redirect is followed.
async fn send(request: RequestBuilder) -> Result<Response, McpError> {
    let response = request.send().await.map_err(classify)?;
    if response.status().is_redirection() {
        return Err(McpError::Redirected);
    }
    Ok(response)
}

async fn get_json<T: DeserializeOwned>(
    client: &reqwest::Client,
    url: Url,
    max_bytes: usize,
) -> Result<T, McpError> {
    let response = send(client.get(url).header(ACCEPT, JSON)).await?;
    read_json(response, StatusCode::OK, max_bytes).await
}

/// The answer as `T` when its status is `expected`; any other status carries only its number.
async fn read_json<T: DeserializeOwned>(
    response: Response,
    expected: StatusCode,
    max_bytes: usize,
) -> Result<T, McpError> {
    if response.status() != expected {
        return Err(McpError::HttpStatus {
            status: response.status().as_u16(),
        });
    }
    let body = read_capped(response, max_bytes).await?;
    serde_json::from_slice(&body).map_err(|_| McpError::Malformed)
}
