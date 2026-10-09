//! The streamable HTTP transport: one `POST` per message to the pinned endpoint.

use std::fmt::{self, Debug};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use reqwest::StatusCode;
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde_json::Value;

use crate::budget::{BucketConfig, BudgetConfig, CallClass, RateBudget};
use crate::endpoint::{Build, PinnedEndpoint};
use crate::error::{McpError, ServerText};
use crate::frame;

const SESSION_HEADER: &str = "mcp-session-id";
const PROTOCOL_HEADER: &str = "mcp-protocol-version";
/// The MCP revision whose streamable HTTP transport this implements.
pub const PROTOCOL_VERSION: &str = "2025-06-18";
const USER_AGENT: &str = concat!("mandate-mcp/", env!("CARGO_PKG_VERSION"));

/// A monotonic clock: the time since any fixed origin. The budget paces against it.
pub trait Monotonic: Debug + Send + Sync {
    fn elapsed(&self) -> Duration;
}

/// The process's monotonic clock, from the moment it was started.
#[derive(Debug, Clone, Copy)]
pub struct SystemMonotonic {
    origin: Instant,
}

impl SystemMonotonic {
    #[allow(
        clippy::disallowed_methods,
        reason = "the connector's edge reads the monotonic clock to pace calls; the core never does (ES-05)"
    )]
    pub fn start() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Monotonic for SystemMonotonic {
    fn elapsed(&self) -> Duration {
        self.origin.elapsed()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransportConfig {
    pub connect_timeout: Duration,
    /// The whole exchange, from connect to the last byte of the answer.
    pub request_timeout: Duration,
    /// The most bytes an answer may have; the conservative default is 4 MiB.
    pub max_answer_bytes: usize,
    pub budget: BudgetConfig,
}

impl TransportConfig {
    /// Spec §6.5's conservative default until the broker publishes its limits (U-R9).
    pub const CONSERVATIVE: Self = Self {
        connect_timeout: Duration::from_secs(10),
        request_timeout: Duration::from_secs(30),
        max_answer_bytes: 4_194_304,
        budget: BudgetConfig {
            ordinary: BucketConfig {
                capacity: 10,
                refill_every: Duration::from_secs(1),
            },
            reserved: BucketConfig {
                capacity: 5,
                refill_every: Duration::from_secs(1),
            },
        },
    };
}

pub struct McpTransport {
    client: reqwest::Client,
    endpoint: PinnedEndpoint,
    max_answer_bytes: usize,
    budget: Mutex<RateBudget>,
    clock: Box<dyn Monotonic>,
    session: Mutex<Option<HeaderValue>>,
    next_id: AtomicU64,
}

impl McpTransport {
    pub fn new(
        endpoint: PinnedEndpoint,
        config: TransportConfig,
        clock: Box<dyn Monotonic>,
    ) -> Result<Self, McpError> {
        let client = http_client(&config)?;
        let budget = RateBudget::new(config.budget, clock.elapsed())?;
        Ok(Self {
            client,
            endpoint,
            max_answer_bytes: config.max_answer_bytes,
            budget: Mutex::new(budget),
            clock,
            session: Mutex::new(None),
            next_id: AtomicU64::new(1),
        })
    }

    /// Sends request `method` and returns its result, or the server's JSON-RPC error as
    /// [`McpError::Rpc`]. Whether a call that fails after the budget admitted it reached the
    /// server is unknown to this layer; the connector treats an order call that way.
    pub async fn request(
        &self,
        class: CallClass,
        method: &str,
        params: &Value,
    ) -> Result<ServerText, McpError> {
        self.admit(class)?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let response = self
            .post(frame::request_body(Some(id), method, params))
            .await?;
        if response.status() != StatusCode::OK {
            return Err(McpError::HttpStatus {
                status: response.status().as_u16(),
            });
        }
        let is_stream = match media_type(response.headers()).as_deref() {
            Some("application/json") => false,
            Some("text/event-stream") => true,
            _ => return Err(McpError::ContentType),
        };
        let body = read_capped(response, self.max_answer_bytes).await?;
        if is_stream {
            frame::parse_sse(&body, id)
        } else {
            frame::parse_json(&body, id)
        }
    }

    /// Sends notification `method`, which the server acknowledges with `202 Accepted`.
    pub async fn notify(
        &self,
        class: CallClass,
        method: &str,
        params: &Value,
    ) -> Result<(), McpError> {
        self.admit(class)?;
        let response = self.post(frame::request_body(None, method, params)).await?;
        match response.status() {
            StatusCode::ACCEPTED => Ok(()),
            other => Err(McpError::HttpStatus {
                status: other.as_u16(),
            }),
        }
    }

    fn admit(&self, class: CallClass) -> Result<(), McpError> {
        let now = self.clock.elapsed();
        self.budget
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .try_acquire(class, now)
    }

    /// One `POST`, carrying the session the server assigned, if any, and keeping the one its
    /// answer assigns. A redirect and a session the server no longer knows end here.
    async fn post(&self, body: String) -> Result<reqwest::Response, McpError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/json, text/event-stream"),
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(PROTOCOL_HEADER, HeaderValue::from_static(PROTOCOL_VERSION));
        let session = self.session().clone();
        if let Some(session) = &session {
            headers.insert(SESSION_HEADER, session.clone());
        }
        let response = self
            .client
            .post(self.endpoint.url().clone())
            .headers(headers)
            .body(body)
            .send()
            .await
            .map_err(classify)?;
        let status = response.status();
        if status.is_redirection() {
            return Err(McpError::Redirected);
        }
        if status == StatusCode::NOT_FOUND && session.is_some() {
            *self.session() = None;
            return Err(McpError::SessionExpired);
        }
        if let Some(assigned) = response.headers().get(SESSION_HEADER)
            && status.is_success()
        {
            if !is_visible_ascii(assigned.as_bytes()) {
                return Err(McpError::BadSessionId);
            }
            let mut kept = assigned.clone();
            kept.set_sensitive(true);
            *self.session() = Some(kept);
        }
        Ok(response)
    }

    fn session(&self) -> std::sync::MutexGuard<'_, Option<HeaderValue>> {
        self.session.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The HTTP client every exchange of this crate uses: no redirect followed, no proxy, `https`
/// only outside this crate's test build, and the configuration's timeouts, none of them zero.
pub(crate) fn http_client(config: &TransportConfig) -> Result<reqwest::Client, McpError> {
    if config.connect_timeout.is_zero()
        || config.request_timeout.is_zero()
        || config.max_answer_bytes == 0
    {
        return Err(McpError::BadConfig);
    }
    let _already_installed = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .https_only(Build::CURRENT == Build::Production)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .connect_timeout(config.connect_timeout)
        .timeout(config.request_timeout)
        .user_agent(USER_AGENT)
        .build()
        .map_err(|_| McpError::ClientSetup)
}

/// The answer's body, refused as [`McpError::TooLarge`] once it would exceed `max_bytes`.
pub(crate) async fn read_capped(
    mut response: reqwest::Response,
    max_bytes: usize,
) -> Result<Vec<u8>, McpError> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(classify)? {
        if body.len().saturating_add(chunk.len()) > max_bytes {
            return Err(McpError::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Non-empty, and every byte visible ASCII (0x21 to 0x7e): no space, control character, or DEL.
pub(crate) fn is_visible_ascii(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.iter().all(|b| (0x21..=0x7e).contains(b))
}

/// The media type without parameters, lower case.
fn media_type(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(CONTENT_TYPE)?.to_str().ok()?;
    let essence = value.split(';').next()?;
    Some(essence.trim().to_ascii_lowercase())
}

pub(crate) fn classify(error: reqwest::Error) -> McpError {
    if error.is_timeout() {
        McpError::Timeout
    } else {
        McpError::Network
    }
}

/// The session id the server assigned is server text and works as a bearer token, so the
/// printout leaves it out, along with the client and the budget.
impl fmt::Debug for McpTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("McpTransport")
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}
