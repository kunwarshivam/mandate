//! The fetchers (data-plane spec §4.2 step 1): Alpaca news and SEC EDGAR, each request built from
//! typed values, permitted by the allowlist before it reaches the [`Transport`] (DP-9), paced by
//! its source's minimum interval, and refused if its body is over the source's cap.
//!
//! In CI the transport is a recorded fixture; the HTTPS transport is `mandate-sources-http`, which
//! runs only in a cell whose egress policy allows the source's hosts. The Alpaca news credential
//! comes in as a [`NewsAuth`] the caller read from the workspace's `SecretSource` (lane V2), never
//! from an environment variable.

use std::collections::BTreeMap;
use std::future::Future;

use mandate_research::SourceId;
use mandate_time::UtcNanos;
use secrecy::SecretString;

use crate::SourceError;
use crate::allowlist::{Allowlist, PermittedUrl};
use crate::ingest::{EdgarIndexEntry, PageToken};
use crate::symbols::{Cik, Symbol};

/// The most tickers one news request names.
pub const MAX_NEWS_SYMBOLS: usize = 50;
/// Items per news page.
pub const NEWS_PAGE_LIMIT: u32 = 50;

/// The workspace's own Alpaca news credential (data-plane spec §1.3: each workspace's own access).
pub struct NewsAuth {
    key_id: SecretString,
    secret: SecretString,
}

impl NewsAuth {
    pub fn new(key_id: SecretString, secret: SecretString) -> Self {
        Self { key_id, secret }
    }

    pub fn key_id(&self) -> &SecretString {
        &self.key_id
    }

    pub fn secret(&self) -> &SecretString {
        &self.secret
    }
}

impl core::fmt::Debug for NewsAuth {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("NewsAuth(redacted)")
    }
}

/// A response: its status and its body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Sends one GET to a permitted URL. An implementation sends nothing else, follows no redirect,
/// and stops reading past the URL's response cap.
pub trait Transport {
    fn get(
        &self,
        url: &PermittedUrl,
        auth: Option<&NewsAuth>,
    ) -> impl Future<Output = Result<RawResponse, SourceError>>;
}

/// The fetchers over one allowlist version and one transport.
#[derive(Debug)]
pub struct Fetcher<T> {
    allowlist: Allowlist,
    transport: T,
    last_request: BTreeMap<SourceId, UtcNanos>,
}

impl<T: Transport> Fetcher<T> {
    pub fn new(allowlist: Allowlist, transport: T) -> Self {
        Self {
            allowlist,
            transport,
            last_request: BTreeMap::new(),
        }
    }

    pub fn allowlist(&self) -> &Allowlist {
        &self.allowlist
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// One page of news for `symbols` published from `start`, newest first.
    pub async fn alpaca_news(
        &mut self,
        source: &SourceId,
        auth: &NewsAuth,
        symbols: &[Symbol],
        start: UtcNanos,
        page: Option<&PageToken>,
        now: UtcNanos,
    ) -> Result<Vec<u8>, SourceError> {
        let _ = (source, auth, symbols, start, page, now);
        Err(SourceError::Unimplemented)
    }

    /// One filer's submissions index.
    pub async fn edgar_submissions(
        &mut self,
        source: &SourceId,
        cik: Cik,
        now: UtcNanos,
    ) -> Result<Vec<u8>, SourceError> {
        let _ = (source, cik, now);
        Err(SourceError::Unimplemented)
    }

    /// One filing's primary document.
    pub async fn edgar_document(
        &mut self,
        source: &SourceId,
        entry: &EdgarIndexEntry,
        now: UtcNanos,
    ) -> Result<Vec<u8>, SourceError> {
        let _ = (source, entry, now);
        Err(SourceError::Unimplemented)
    }
}
