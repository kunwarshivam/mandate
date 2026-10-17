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

/// Where each request goes; [`Allowlist::permit`] still decides whether it may.
const ALPACA_NEWS: &str = "https://data.alpaca.markets/v1beta1/news";
const EDGAR_SUBMISSIONS: &str = "https://data.sec.gov/submissions/";
const EDGAR_ARCHIVES: &str = "https://www.sec.gov/Archives/edgar/data/";

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
        if symbols.is_empty() || symbols.len() > MAX_NEWS_SYMBOLS {
            return Err(SourceError::BadField);
        }
        let tickers: Vec<&str> = symbols.iter().map(Symbol::as_str).collect();
        let mut url = format!(
            "{ALPACA_NEWS}?symbols={}&start={}&limit={NEWS_PAGE_LIMIT}&sort=desc",
            tickers.join(","),
            query_time(start)
        );
        if let Some(token) = page {
            url.push_str("&page_token=");
            url.push_str(token.as_str());
        }
        self.send(source, &url, Some(auth), now).await
    }

    /// One filer's submissions index.
    pub async fn edgar_submissions(
        &mut self,
        source: &SourceId,
        cik: Cik,
        now: UtcNanos,
    ) -> Result<Vec<u8>, SourceError> {
        let url = format!("{EDGAR_SUBMISSIONS}CIK{}.json", cik.padded());
        self.send(source, &url, None, now).await
    }

    /// One filing's primary document.
    pub async fn edgar_document(
        &mut self,
        source: &SourceId,
        entry: &EdgarIndexEntry,
        now: UtcNanos,
    ) -> Result<Vec<u8>, SourceError> {
        let url = format!(
            "{EDGAR_ARCHIVES}{}/{}/{}",
            entry.cik.get(),
            entry.accession.as_str().replace('-', ""),
            entry.primary_document.as_str()
        );
        self.send(source, &url, None, now).await
    }

    /// Permits `url` (DP-9), paces it, sends it, and checks the answer. The request counts against
    /// the interval from the moment it is sent, whatever comes back.
    async fn send(
        &mut self,
        source: &SourceId,
        url: &str,
        auth: Option<&NewsAuth>,
        now: UtcNanos,
    ) -> Result<Vec<u8>, SourceError> {
        let permitted = self.allowlist.permit(source, url)?;
        let interval_ms = self
            .allowlist
            .active(source)
            .ok_or(SourceError::SourceNotAllowlisted)?
            .min_interval_ms();
        if let Some(last) = self.last_request.get(source) {
            let next = nanos_since_epoch(*last)?
                .checked_add(
                    i128::from(interval_ms)
                        .checked_mul(1_000_000)
                        .ok_or(SourceError::Time)?,
                )
                .ok_or(SourceError::Time)?;
            if nanos_since_epoch(now)? < next {
                return Err(SourceError::TooSoon);
            }
        }
        self.last_request.insert(source.clone(), now);
        let response = self.transport.get(&permitted, auth).await?;
        if response.status != 200 {
            return Err(SourceError::Status);
        }
        let length =
            u64::try_from(response.body.len()).map_err(|_| SourceError::ResponseTooLarge)?;
        if length > permitted.max_response_bytes() {
            return Err(SourceError::ResponseTooLarge);
        }
        Ok(response.body)
    }
}

fn nanos_since_epoch(t: UtcNanos) -> Result<i128, SourceError> {
    i128::from(t.secs())
        .checked_mul(1_000_000_000)
        .and_then(|n| n.checked_add(i128::from(t.nanos())))
        .ok_or(SourceError::Time)
}

/// RFC 3339 for a query string: whole seconds without a fraction, otherwise all nine digits.
fn query_time(t: UtcNanos) -> String {
    let text = t.to_string();
    match text.strip_suffix(".000000000Z") {
        Some(whole) => format!("{whole}Z"),
        None => text,
    }
}
