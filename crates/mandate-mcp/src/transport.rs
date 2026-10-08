//! The streamable HTTP transport: one `POST` per message to the pinned endpoint.

use std::fmt::Debug;
use std::sync::Mutex;
use std::sync::atomic::AtomicU64;
use std::time::{Duration, Instant};

use reqwest::header::HeaderValue;
use serde_json::Value;

use crate::budget::{BucketConfig, BudgetConfig, CallClass, RateBudget};
use crate::endpoint::PinnedEndpoint;
use crate::error::{McpError, ServerText};

/// The MCP revision whose streamable HTTP transport this implements.
pub const PROTOCOL_VERSION: &str = "2025-06-18";

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

#[derive(Debug)]
#[expect(
    dead_code,
    reason = "read by E7-16's implementation, which replaces the stubs below"
)]
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
        let _ = (endpoint, config, clock);
        Err(McpError::Unimplemented { story: "E7-16" })
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
        let _ = (class, method, params);
        Err(McpError::Unimplemented { story: "E7-16" })
    }

    /// Sends notification `method`, which the server acknowledges with `202 Accepted`.
    pub async fn notify(
        &self,
        class: CallClass,
        method: &str,
        params: &Value,
    ) -> Result<(), McpError> {
        let _ = (class, method, params);
        Err(McpError::Unimplemented { story: "E7-16" })
    }
}
