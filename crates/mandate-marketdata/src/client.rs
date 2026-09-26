//! Fetching one UTC day of one dataset, or a symbol's corporate actions: pagination, pacing to the
//! host's rate limit, retries (a 429 waits for the next rate window, server errors back off
//! exponentially), and ordering checks. The transport and the clock are injected so tests replay
//! recorded responses without a network or real waiting (ADR-0001 ES-19).

use std::collections::BTreeSet;
use std::future::Future;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime};

use mandate_time::{Date, UtcNanos};

use crate::alpaca::{self, WireError};
use crate::model::{CorporateActions, DatasetId, DayRange, Records, Symbol};
use crate::rate::Pacer;
pub use crate::rate::RatePolicy;
use crate::timestamp::TimestampError;

/// A response's rate-limit headers as the host sent them; the client parses them and ignores any
/// that do not parse.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RateHeaders {
    /// `X-Ratelimit-Limit`: requests allowed per window.
    pub limit: Option<String>,
    /// `X-Ratelimit-Remaining`: requests left.
    pub remaining: Option<String>,
    /// `X-Ratelimit-Reset`: when the allowance is whole again, in seconds since the Unix epoch.
    pub reset: Option<String>,
}

/// An HTTP response: the status code, the rate-limit headers, and the body bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub rate: RateHeaders,
    pub body: Vec<u8>,
}

/// Why a request produced no response. Carries no URL, header, or body, so no credential can
/// reach an error message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TransportError {
    #[error("the request timed out")]
    Timeout,
    #[error("the connection failed")]
    Connect,
    #[error("the request failed")]
    Request,
    #[error("the path is not a market-data endpoint; nothing was sent")]
    RefusedPath,
}

impl TransportError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Connect => "connect",
            Self::Request => "request",
            Self::RefusedPath => "refused_path",
        }
    }

    pub fn is_retryable(self) -> bool {
        !matches!(self, Self::RefusedPath)
    }
}

/// Sends a GET for a path and query on the market-data host.
pub trait Transport {
    fn get(&self, path_and_query: &str) -> impl Future<Output = Result<Response, TransportError>>;
}

/// Tells the time and waits between requests. The client paces against `now`, so a `pause` must
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
        reason = "the adapter's edge reads the wall clock to compare with the host's rate-limit reset (ES-05)"
    )]
    fn now(&self) -> UtcNanos {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
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

/// A server error or transport failure `n` (from 1) waits `min(first_delay × 2^(n-1), max_delay)`,
/// up to `max_attempts` such failures. A 429 instead waits for the next rate window
/// ([`RatePolicy`]), up to `rate_limit_windows` times.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub first_delay: Duration,
    pub max_delay: Duration,
    pub rate_limit_windows: u32,
}

impl Default for RetryPolicy {
    /// Six attempts over about half a minute for server errors, and three windows of waiting for
    /// rate limits, so a window that slips past its reset does not fail a download.
    fn default() -> Self {
        Self {
            max_attempts: 6,
            first_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(32),
            rate_limit_windows: 3,
        }
    }
}

/// What the last failed attempt returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryCause {
    Status(u16),
    Transport(TransportError),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FetchError {
    #[error("the data host answered HTTP {status}")]
    Status { status: u16 },
    #[error("gave up after {attempts} attempts; the last was {last:?}")]
    Exhausted { attempts: u32, last: RetryCause },
    #[error("the transport failed: {0}")]
    Transport(TransportError),
    #[error("page {page}: {source}")]
    Wire { page: usize, source: WireError },
    #[error("page {page} repeats an earlier page token")]
    RepeatedPageToken { page: usize },
    #[error("records are not in time order at {time}")]
    OutOfOrder { time: UtcNanos },
    #[error("building the request: {0}")]
    Request(TimestampError),
}

impl FetchError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Status { .. } => "status",
            Self::Exhausted { .. } => "exhausted",
            Self::Transport(_) => "transport",
            Self::Wire { .. } => "wire",
            Self::RepeatedPageToken { .. } => "repeated_page_token",
            Self::OutOfOrder { .. } => "out_of_order",
            Self::Request(_) => "request",
        }
    }
}

/// The market-data client over an injected transport and clock. Requests made through one client,
/// concurrently or not, share its rate-limit pacing.
#[derive(Debug)]
pub struct Client<T, P> {
    transport: T,
    pause: P,
    retry: RetryPolicy,
    pacer: Mutex<Pacer>,
    page_limit: u32,
}

impl<T: Transport, P: Pause> Client<T, P> {
    /// A client with the default retry and rate policies and Alpaca's largest page.
    pub fn new(transport: T, pause: P) -> Self {
        Self {
            transport,
            pause,
            retry: RetryPolicy::default(),
            pacer: Mutex::new(Pacer::new(RatePolicy::default())),
            page_limit: alpaca::MAX_PAGE_LIMIT,
        }
    }

    pub fn with_retry(self, retry: RetryPolicy) -> Self {
        Self { retry, ..self }
    }

    pub fn with_rate(self, rate: RatePolicy) -> Self {
        Self {
            pacer: Mutex::new(Pacer::new(rate)),
            ..self
        }
    }

    /// Records per page, clamped to 1..=10,000.
    pub fn with_page_limit(self, page_limit: u32) -> Self {
        Self {
            page_limit: page_limit.clamp(1, alpaca::MAX_PAGE_LIMIT),
            ..self
        }
    }

    /// Every record of `dataset` in the UTC day `day`, following page tokens to the end.
    pub async fn fetch_day(&self, dataset: &DatasetId, day: Date) -> Result<Records, FetchError> {
        let mut records = Records::empty(dataset.kind());
        let mut seen_tokens = BTreeSet::new();
        let mut token: Option<String> = None;
        let mut last = None;
        let mut page = 0_usize;
        loop {
            page = page.saturating_add(1);
            let path = alpaca::page_path(dataset, day, self.page_limit, token.as_deref())
                .map_err(FetchError::Request)?;
            let body = self.get_with_retry(&path).await?;
            let parsed = alpaca::parse_page(dataset, day, &body)
                .map_err(|source| FetchError::Wire { page, source })?;
            append(&mut records, &mut last, parsed.records)?;
            match parsed.next_page_token {
                None => return Ok(records),
                Some(next) => {
                    if !seen_tokens.insert(next.clone()) {
                        return Err(FetchError::RepeatedPageToken { page });
                    }
                    token = Some(next);
                }
            }
        }
    }

    /// Every corporate action of `symbol` dated in `ex_dates` ([`CorporateActions::dated_in`]),
    /// following page tokens to the end of the wider process-date window Alpaca filters on. An
    /// action ID seen on two pages is an error.
    pub async fn fetch_corporate_actions(
        &self,
        symbol: &Symbol,
        ex_dates: DayRange,
    ) -> Result<CorporateActions, FetchError> {
        let limit = self.page_limit.min(alpaca::MAX_CORPORATE_ACTIONS_LIMIT);
        let mut actions = CorporateActions::none(symbol.clone());
        let mut ids = BTreeSet::new();
        let mut seen_tokens = BTreeSet::new();
        let mut token: Option<String> = None;
        let mut page = 0_usize;
        loop {
            page = page.saturating_add(1);
            let path = alpaca::corporate_actions_path(symbol, ex_dates, limit, token.as_deref())
                .map_err(FetchError::Request)?;
            let body = self.get_with_retry(&path).await?;
            let parsed = alpaca::parse_corporate_actions(symbol, &body)
                .map_err(|source| FetchError::Wire { page, source })?;
            if let Some(repeated) = parsed.actions.ids().find(|id| !ids.insert(id.to_string())) {
                let source = WireError::DuplicateAction(repeated.to_owned());
                return Err(FetchError::Wire { page, source });
            }
            actions.splits.extend(parsed.actions.splits);
            actions.cash_dividends.extend(parsed.actions.cash_dividends);
            actions.other.extend(parsed.actions.other);
            match parsed.next_page_token {
                None => return Ok(actions.dated_in(ex_dates)),
                Some(next) => {
                    if !seen_tokens.insert(next.clone()) {
                        return Err(FetchError::RepeatedPageToken { page });
                    }
                    token = Some(next);
                }
            }
        }
    }

    async fn get_with_retry(&self, path: &str) -> Result<Vec<u8>, FetchError> {
        let attempts = self.retry.max_attempts.max(1);
        let mut failures = 0_u32;
        let mut rate_limited = 0_u32;
        loop {
            while let Some(wait) = self.admit() {
                self.pause.pause(wait).await;
            }
            let reply = self.transport.get(path).await;
            let headers = reply.as_ref().ok().map(|response| &response.rate);
            self.pacer().complete(headers, self.pause.now());
            let cause = match reply {
                Ok(Response {
                    status: 200, body, ..
                }) => return Ok(body),
                Ok(Response { status: 429, .. }) => {
                    self.pacer().rate_limited(self.pause.now());
                    rate_limited = rate_limited.saturating_add(1);
                    if rate_limited <= self.retry.rate_limit_windows {
                        continue;
                    }
                    return Err(FetchError::Exhausted {
                        attempts: failures.saturating_add(rate_limited),
                        last: RetryCause::Status(429),
                    });
                }
                Ok(Response { status, .. }) if (500..600).contains(&status) => {
                    RetryCause::Status(status)
                }
                Ok(Response { status, .. }) => return Err(FetchError::Status { status }),
                Err(e) if e.is_retryable() => RetryCause::Transport(e),
                Err(e) => return Err(FetchError::Transport(e)),
            };
            failures = failures.saturating_add(1);
            if failures >= attempts {
                return Err(FetchError::Exhausted {
                    attempts: failures.saturating_add(rate_limited),
                    last: cause,
                });
            }
            self.pause.pause(self.retry.delay(failures)).await;
        }
    }

    fn admit(&self) -> Option<Duration> {
        self.pacer().admit(self.pause.now())
    }

    fn pacer(&self) -> MutexGuard<'_, Pacer> {
        self.pacer.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl RetryPolicy {
    /// The wait after failed attempt `attempt` (from 1).
    pub fn delay(self, attempt: u32) -> Duration {
        let doublings = attempt.saturating_sub(1);
        2_u32
            .checked_pow(doublings)
            .and_then(|factor| self.first_delay.checked_mul(factor))
            .map_or(self.max_delay, |d| d.min(self.max_delay))
    }
}

fn append(
    records: &mut Records,
    last: &mut Option<UtcNanos>,
    page: Records,
) -> Result<(), FetchError> {
    for time in page.times() {
        if last.is_some_and(|previous| previous > time) {
            return Err(FetchError::OutOfOrder { time });
        }
        *last = Some(time);
    }
    match (records, page) {
        (Records::Bars(all), Records::Bars(more)) => all.extend(more),
        (Records::Trades(all), Records::Trades(more)) => all.extend(more),
        (Records::Quotes(all), Records::Quotes(more)) => all.extend(more),
        (Records::Bars(_) | Records::Trades(_) | Records::Quotes(_), _) => {}
    }
    Ok(())
}
