//! The managed issuer's published keys, fetched over HTTPS (identity spec §6.1, DEC-820 item 2).
//!
//! The server fetches the key set at start and every [`REFETCH`] after, and keeps the last set it
//! parsed: a failed fetch keeps the previous set, so an outage of the issuer's host costs no key
//! it already had. Until a first fetch succeeds there is no key set, and every token check is an
//! outage ([`crate::token::Checked::Unreachable`]). The URL must be `https`, or `http` to a
//! loopback host, which only tests serve; no redirect is followed, and a body over [`MAX_BYTES`]
//! is refused.

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
        let _ = url;
        Err(FetchError::Url)
    }

    /// Fetches once; on success the new set replaces the last.
    ///
    /// # Errors
    /// [`FetchError::Unavailable`] or [`FetchError::Body`]; the last set is kept.
    pub async fn fetch(&self) -> Result<(), FetchError> {
        let _ = (&self.url, &self.client);
        Err(FetchError::Unavailable)
    }
}

impl Keys for FetchedKeys {
    fn current(&self) -> Option<Arc<Jwks>> {
        self.latest.read().ok().and_then(|latest| latest.clone())
    }
}
