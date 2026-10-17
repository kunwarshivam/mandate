#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]
//! The model gateway's HTTPS transport ([DEC-2200](../../../docs/project/decisions/DEC-2200.md);
//! inference spec §3.4, §8.2): one attempt of a gateway call, sent to the one host the gateway's
//! egress allowlist permitted, over TLS, inside the attempt's deadline, with the provider key as a
//! sensitive header.
//!
//! **Not wired to a live endpoint** (DEC-2200 item 2): no binary or default builds it. It is wired
//! only once the meter stream is journaled (DEC-998 item 8) and DEC-997 is Accepted; until lane V2's
//! `SecretSource` lands the only key source is `NoKeys`, so a hosted call ends `credential_invalid`
//! and sends nothing.
//!
//! What an attempt may do:
//! - it goes only to `https://` + the request's host + its path, and only when [`url_for`] accepts
//!   both, so the transport never reinterprets the host the allowlist checked (item 4);
//! - it uses no proxy and follows no redirect, so no second host sees it (item 4);
//! - a failure before anything reached the provider is [`Delivery::ConnectFailed`], which the
//!   gateway may retry; a failure once the request may have reached it is [`Delivery::TimedOut`],
//!   an attempt of unknown cost that is never retried (item 5);
//! - it gets only the time left before its deadline, reading the reply included (item 6), and
//!   stops reading a body at [`MAX_BODY_BYTES`] (item 7);
//! - the key travels only as `Authorization: Bearer` on a header marked sensitive, and never
//!   appears in a `Debug` output or an error (item 9, `AGENTS.md` rule 7).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mandate_gateway::provider::{Clock, Delivery, HttpRequest, ProviderKey, Transport};
use mandate_time::UtcNanos;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderValue, RETRY_AFTER};
use reqwest::{Url, redirect};
use tokio::runtime::{Builder, Runtime};

/// The most bytes of a response body the transport reads; a longer body makes the attempt
/// [`Delivery::TimedOut`] (DEC-2200 item 7). Far above any reply `max_output_tokens` allows.
pub const MAX_BODY_BYTES: u64 = 4 * 1024 * 1024;

/// The certificate authorities a transport trusts (DEC-2200 item 10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TlsRoots {
    /// The platform's roots, through reqwest's default verifier: production.
    Platform,
    /// Only these DER-encoded certificate authorities: tests, against a local server.
    Only(Vec<Vec<u8>>),
}

/// Why a transport could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SetupError {
    #[error("the transport's runtime could not be started")]
    Runtime,
    #[error("a trusted root is not a DER-encoded certificate, or none was given")]
    Root,
    #[error("building the HTTPS client failed")]
    Client,
}

impl SetupError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(self) -> &'static str {
        match self {
            Self::Runtime => "runtime",
            Self::Root => "root",
            Self::Client => "client",
        }
    }
}

/// One gateway attempt over HTTPS (DEC-2200).
#[derive(Debug)]
pub struct HttpsTransport {
    client: reqwest::Client,
    runtime: Runtime,
}

impl HttpsTransport {
    /// HTTPS only, no proxy, no redirect, TLS on the `ring` provider, trusting `roots`.
    pub fn new(roots: TlsRoots) -> Result<Self, SetupError> {
        let _already_installed = rustls::crypto::ring::default_provider().install_default();
        let builder = reqwest::Client::builder()
            .https_only(true)
            .no_proxy()
            .redirect(redirect::Policy::none());
        let builder = match roots {
            TlsRoots::Platform => builder,
            TlsRoots::Only(certificates) => builder.tls_backend_preconfigured(only(&certificates)?),
        };
        let client = builder.build().map_err(|_| SetupError::Client)?;
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| SetupError::Runtime)?;
        Ok(Self { client, runtime })
    }

    async fn attempt(
        &self,
        url: Url,
        body: Vec<u8>,
        key: Option<HeaderValue>,
        budget: Duration,
    ) -> Delivery {
        let mut request = self
            .client
            .post(url)
            .timeout(budget)
            .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
            .body(body);
        if let Some(key) = key {
            request = request.header(AUTHORIZATION, key);
        }
        let mut response = match request.send().await {
            Ok(response) => response,
            Err(error) if error.is_connect() || error.is_builder() => {
                return Delivery::ConnectFailed;
            }
            Err(_) => return Delivery::TimedOut,
        };
        let status = response.status().as_u16();
        let retry_after_ms = response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(retry_after_ms);
        if response
            .content_length()
            .is_some_and(|length| length > MAX_BODY_BYTES)
        {
            return Delivery::TimedOut;
        }
        let mut body = Vec::new();
        loop {
            match response.chunk().await {
                Ok(Some(chunk)) => {
                    if !append_capped(&mut body, &chunk) {
                        return Delivery::TimedOut;
                    }
                }
                Ok(None) => break,
                Err(_) => return Delivery::TimedOut,
            }
        }
        Delivery::Response {
            status,
            body,
            retry_after_ms,
        }
    }
}

/// A rustls client configuration that trusts only `certificates`.
fn only(certificates: &[Vec<u8>]) -> Result<rustls::ClientConfig, SetupError> {
    if certificates.is_empty() {
        return Err(SetupError::Root);
    }
    let mut store = rustls::RootCertStore::empty();
    for der in certificates {
        store
            .add(rustls::pki_types::CertificateDer::from(der.clone()))
            .map_err(|_| SetupError::Root)?;
    }
    Ok(rustls::ClientConfig::builder()
        .with_root_certificates(store)
        .with_no_client_auth())
}

impl Transport for HttpsTransport {
    fn send(
        &mut self,
        request: &HttpRequest,
        key: Option<&ProviderKey>,
        deadline: UtcNanos,
        clock: &mut dyn Clock,
    ) -> Delivery {
        let Some(url) = url_for(&request.host, &request.path) else {
            return Delivery::ConnectFailed;
        };
        let key = match key.map(bearer) {
            None => None,
            Some(Some(header)) => Some(header),
            Some(None) => return Delivery::ConnectFailed,
        };
        let Some(budget) = budget(clock.now(), deadline) else {
            return Delivery::TimedOut;
        };
        let body = request.body.clone();
        let this = &*self;
        std::thread::scope(|scope| {
            scope
                .spawn(|| this.runtime.block_on(this.attempt(url, body, key, budget)))
                .join()
                .unwrap_or(Delivery::TimedOut)
        })
    }
}

/// The URL of an attempt: `https://` + `host` + `path`, or `None` when either breaks DEC-2200 item
/// 4. The host is a lowercase DNS name or IPv4 address, each label non-empty, with an optional port
/// from 1 to 65535, and the URL must parse back to exactly that host, port, and path. So nothing is
/// reinterpreted: a path the parser would split or rewrite (a query, a fragment, a backslash, a
/// space, a non-ASCII byte, a `..` segment, or no leading `/`) is refused, not sent changed.
pub fn url_for(host: &str, path: &str) -> Option<Url> {
    let (name, port) = match host.split_once(':') {
        Some((name, digits)) => (name, Some(digits.parse::<u16>().ok().filter(|&p| p > 0)?)),
        None => (host, None),
    };
    let name_ok = !name.is_empty()
        && name.split('.').all(|label| {
            !label.is_empty()
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        });
    if !name_ok {
        return None;
    }
    let url = Url::parse(&format!("https://{host}{path}")).ok()?;
    (url.host_str() == Some(name) && url.port() == port && url.path() == path).then_some(url)
}

/// The `Retry-After` delay in milliseconds, from its delay-seconds form only (DEC-2200 item 8).
pub fn retry_after_ms(value: &str) -> Option<u64> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    value.parse::<u64>().ok()?.checked_mul(1_000)
}

/// The time left from `now` to `deadline`, or `None` when none is left (DEC-2200 item 6).
pub fn budget(now: UtcNanos, deadline: UtcNanos) -> Option<Duration> {
    let left = nanos(deadline).checked_sub(nanos(now))?;
    let left = u64::try_from(left).ok().filter(|&n| n > 0)?;
    Some(Duration::from_nanos(left))
}

fn nanos(at: UtcNanos) -> i128 {
    i128::from(at.secs())
        .saturating_mul(1_000_000_000)
        .saturating_add(i128::from(at.nanos()))
}

/// The provider key as an `Authorization: Bearer` value marked sensitive, or `None` when the key
/// cannot be a header value (DEC-2200 item 9).
fn bearer(key: &ProviderKey) -> Option<HeaderValue> {
    let mut header = HeaderValue::from_str(&format!("Bearer {}", key.expose_secret())).ok()?;
    header.set_sensitive(true);
    Some(header)
}

/// Appends `chunk` to `body`, or answers `false` when the body would pass [`MAX_BODY_BYTES`].
fn append_capped(body: &mut Vec<u8>, chunk: &[u8]) -> bool {
    let fits = body
        .len()
        .checked_add(chunk.len())
        .and_then(|total| u64::try_from(total).ok())
        .is_some_and(|total| total <= MAX_BODY_BYTES);
    if fits {
        body.extend_from_slice(chunk);
    }
    fits
}

/// The system's clock, for a cell: `now` is the system time, and `wait_until` sleeps.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    #[allow(
        clippy::disallowed_methods,
        reason = "the transport's edge reads the wall clock for a cell; the gateway's core never does (ES-05)"
    )]
    fn now(&mut self) -> UtcNanos {
        let since = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let secs = i64::try_from(since.as_secs()).unwrap_or(i64::MAX);
        UtcNanos::from_parts(secs, since.subsec_nanos()).unwrap_or(UtcNanos::EPOCH)
    }

    fn wait_until(&mut self, at: UtcNanos) {
        if let Some(left) = budget(self.now(), at) {
            std::thread::sleep(left);
        }
    }
}
