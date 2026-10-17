//! The managed issuer's published keys, fetched over HTTPS (identity spec §6.1, DEC-820 item 2).
//!
//! The server fetches the key set at start and every [`REFETCH`] after, and keeps the last set it
//! parsed: a failed fetch keeps the previous set, so an outage of the issuer's host costs no key
//! it already had. Until a first fetch succeeds there is no key set, and every token check is an
//! outage ([`crate::token::Checked::Unreachable`]). The URL must be `https`, or `http` to a
//! loopback host, which only tests serve; no redirect is followed, and a body over [`MAX_BYTES`]
//! is refused.

use std::net::IpAddr;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use mandate_authn::Jwks;

use crate::token::Keys;

/// How often the key set is fetched again.
pub const REFETCH: Duration = Duration::from_secs(300);

/// The longest a fetch may take.
pub const TIMEOUT: Duration = Duration::from_secs(10);

/// The largest key set accepted, in bytes.
pub const MAX_BYTES: usize = 64 * 1024;

/// Why a key-set URL or fetch was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchError {
    /// The URL is not `https`, nor `http` to a loopback host.
    Url,
    /// The HTTP client could not be built.
    Client,
    /// The request failed, timed out, or did not answer 200.
    Unavailable,
    /// The body is too large or is not a key set.
    Body,
}

/// The last key set fetched from one URL.
pub struct FetchedKeys {
    url: reqwest::Url,
    client: reqwest::Client,
    latest: RwLock<Option<Arc<Jwks>>>,
}

impl FetchedKeys {
    /// A fetcher for `url`, holding no key set yet.
    ///
    /// # Errors
    /// [`FetchError::Url`] or [`FetchError::Client`].
    pub fn new(url: &str) -> Result<Self, FetchError> {
        let url = reqwest::Url::parse(url).map_err(|_| FetchError::Url)?;
        let host = url.host_str().unwrap_or_default();
        let bare = host
            .strip_prefix('[')
            .and_then(|h| h.strip_suffix(']'))
            .unwrap_or(host);
        let loopback =
            bare == "localhost" || bare.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback());
        let allowed = match url.scheme() {
            "https" => !host.is_empty(),
            "http" => loopback,
            _ => false,
        };
        if !allowed {
            return Err(FetchError::Url);
        }
        let _already_installed = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .https_only(url.scheme() == "https")
            .redirect(reqwest::redirect::Policy::none())
            .timeout(TIMEOUT)
            .build()
            .map_err(|_| FetchError::Client)?;
        Ok(Self {
            url,
            client,
            latest: RwLock::new(None),
        })
    }

    /// Fetches once; on success the new set replaces the last.
    ///
    /// # Errors
    /// [`FetchError::Unavailable`] or [`FetchError::Body`]; the last set is kept.
    pub async fn fetch(&self) -> Result<(), FetchError> {
        let mut response = self
            .client
            .get(self.url.clone())
            .send()
            .await
            .map_err(|_| FetchError::Unavailable)?;
        if response.status() != reqwest::StatusCode::OK {
            return Err(FetchError::Unavailable);
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| FetchError::Unavailable)?
        {
            if body.len().saturating_add(chunk.len()) > MAX_BYTES {
                return Err(FetchError::Body);
            }
            body.extend_from_slice(&chunk);
        }
        let text = std::str::from_utf8(&body).map_err(|_| FetchError::Body)?;
        let jwks = Jwks::parse(text).map_err(|_| FetchError::Body)?;
        if let Ok(mut latest) = self.latest.write() {
            *latest = Some(Arc::new(jwks));
        }
        Ok(())
    }
}

impl Keys for FetchedKeys {
    fn current(&self) -> Option<Arc<Jwks>> {
        self.latest.read().ok().and_then(|latest| latest.clone())
    }
}
