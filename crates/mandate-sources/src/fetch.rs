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

/// Why a stored news data key is refused before any request (DEC-2277 item 4). Neither carries any
/// part of the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewsKeyRefusal {
    /// Not one line `<key id>:<secret>`: not UTF-8, no `:` or more than one, an empty part, or a
    /// part holding whitespace or a control character.
    Malformed,
    /// The key id does not begin `PK`, Alpaca's paper key prefix: a live key, or not Alpaca's
    /// (AGENTS.md rule 8). A guard on Alpaca's convention, not a proof.
    NotPaper,
}

impl NewsKeyRefusal {
    pub fn code(self) -> &'static str {
        match self {
            Self::Malformed => "news_key_malformed",
            Self::NotPaper => "not_paper",
        }
    }
}

/// The prefix of an Alpaca paper key's id (DEC-2277 item 4).
const PAPER_KEY_PREFIX: &str = "PK";

impl NewsAuth {
    /// The news reader's key as `mandate vault put news-data-key` stores it at
    /// `platform/news-data-key`: one line `<key id>:<secret>`, a final line break allowed. A key
    /// that is not that line is refused [`NewsKeyRefusal::Malformed`], and one whose id is not a
    /// paper key's [`NewsKeyRefusal::NotPaper`], keeping nothing of it (DEC-2277 items 2 and 4). A
    /// key the vault does not hold is the caller's `NoKey`, decided before this is called. A line
    /// is judged whole before its id, so a malformed line is `Malformed` whatever its prefix.
    pub fn from_stored(stored: &[u8]) -> Result<Self, NewsKeyRefusal> {
        let text = core::str::from_utf8(stored).map_err(|_| NewsKeyRefusal::Malformed)?;
        let line = text.strip_suffix('\n').unwrap_or(text);
        let (key_id, secret) = line.split_once(':').ok_or(NewsKeyRefusal::Malformed)?;
        let clean = |part: &str| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c != ':' && !c.is_whitespace() && !c.is_control())
        };
        if !clean(key_id) || !clean(secret) {
            return Err(NewsKeyRefusal::Malformed);
        }
        if !key_id.starts_with(PAPER_KEY_PREFIX) {
            return Err(NewsKeyRefusal::NotPaper);
        }
        Ok(Self::new(
            SecretString::from(key_id.to_owned()),
            SecretString::from(secret.to_owned()),
        ))
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
