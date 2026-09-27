//! The paper trading client: one method per [`BrokerRequest`] variant, each answering with a
//! [`BrokerOutcome`] the executor folds.
//!
//! The transport and the clock are injected, so no test touches a network and no test waits
//! (ADR-0001 ES-19, the `mandate-marketdata` shape). The client holds no state a restart would
//! lose: every fact it produces is journaled by the executor before it is acted on.

use std::future::Future;
use std::time::Duration;

use mandate_executor::{BrokerConnector, BrokerOutcome, BrokerRequest, BrokerUnknown};
use mandate_time::UtcNanos;

use crate::error::ClientError;
use crate::http::TradingTransport;

/// Tells the time and waits between retries. The client paces against `now`, so a `pause` must
/// move `now` forward by at least its duration.
pub trait Pause {
    fn now(&self) -> UtcNanos;
    fn pause(&self, duration: Duration) -> impl Future<Output = ()>;
}

/// The system clock and the tokio timer.
#[derive(Debug, Clone, Copy, Default)]
pub struct TokioPause;

impl Pause for TokioPause {
    #[allow(
        clippy::disallowed_methods,
        reason = "the adapter's edge reads the wall clock to pace retries; the core never does (ES-05, ES-21)"
    )]
    fn now(&self) -> UtcNanos {
        std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .ok()
            .and_then(|since| {
                let secs = i64::try_from(since.as_secs()).ok()?;
                UtcNanos::from_parts(secs, since.subsec_nanos()).ok()
            })
            .unwrap_or(UtcNanos::EPOCH)
    }

    async fn pause(&self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}

/// How many times a request whose outcome is **settled** may be retried, and how long the waits
/// are. A request whose outcome is **unknown** is never retried here: it answers
/// [`BrokerUnknown`], and the executor resolves it by querying on the client order id, never by
/// resubmitting (trading-domain spec §5.7, task brief interpretation 10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub first_delay: Duration,
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            first_delay: Duration::from_millis(250),
            max_delay: Duration::from_secs(4),
        }
    }
}

/// The client. One method per request variant; nothing else reaches the host.
#[derive(Debug)]
pub struct TradingClient<T, P> {
    transport: T,
    pause: P,
    retry: RetryPolicy,
}

impl<T: TradingTransport, P: Pause> TradingClient<T, P> {
    pub fn new(transport: T, pause: P, retry: RetryPolicy) -> Self {
        Self {
            transport,
            pause,
            retry,
        }
    }

    /// The transport this client sends through, for the tests that assert what was sent.
    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// Submits one order, with **our** `client_order_id` on the wire, and answers what the broker
    /// said. A duplicate `client_order_id` is [`BrokerOutcome::DuplicateClientOrderId`], not a
    /// failure: the broker refusing our own id means the order is already there (E7-2 step 6).
    pub async fn submit(&self, request: &BrokerRequest) -> Result<BrokerOutcome, ClientError> {
        let _ = (&self.transport, &self.pause, self.retry, request);
        Err(ClientError::Unimplemented { story: "E7-2" })
    }

    /// Reads one order back by client order id, which is how an unacknowledged submission is
    /// resolved (journal spec §5.2).
    pub async fn order_by_client_id(
        &self,
        client_order_id: &str,
    ) -> Result<BrokerOutcome, ClientError> {
        let _ = (&self.transport, client_order_id);
        Err(ClientError::Unimplemented { story: "E7-2" })
    }

    /// One request of any variant, the shape [`BrokerConnector`] wraps.
    pub async fn call_one(&self, request: &BrokerRequest) -> Result<BrokerOutcome, ClientError> {
        let _ = (&self.transport, &self.pause, self.retry, request);
        Err(ClientError::Unimplemented { story: "E7-3" })
    }
}

/// The connector the executor declares.
///
/// The mapping is the interesting part: only a failure whose outcome is genuinely **unknown**
/// becomes a [`BrokerUnknown`], because that is what makes the executor query rather than
/// resubmit (task brief interpretation 10). A parse failure is an answer this crate could not
/// read, so it fails loudly as `Ambiguous` rather than being reported as a settled rejection —
/// treating it as a rejection is how a duplicate is born.
impl<T: TradingTransport, P: Pause> BrokerConnector for TradingClient<T, P> {
    async fn call(&mut self, request: &BrokerRequest) -> Result<BrokerOutcome, BrokerUnknown> {
        self.call_one(request)
            .await
            .map_err(|error| error.as_unknown().unwrap_or(BrokerUnknown::Ambiguous))
    }
}
