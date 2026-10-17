//! Vendor payloads to [`Draft`]s (data-plane spec §4.2 steps 3 and 5): structured fields only,
//! every one checked against its allowed form, and text kept as bounded [`UntrustedText`] that no
//! typed field is derived from (DP-8).
//!
//! The caller stores the raw response by digest first ([`crate::store::ItemStore::put_raw`],
//! §4.2 step 2). A response larger than its source's cap is refused whole; one bad item in a news
//! page is refused and counted, and the rest are kept (§3.1's rule: refused and counted, never
//! completed).

use mandate_research::SourceId;
use mandate_time::UtcNanos;

use crate::SourceError;
use crate::allowlist::Allowlist;
use crate::record::{Capture, Draft, FormType, VendorId};
use crate::symbols::{Cik, CikTable, SymbolTable};

/// The most instruments one item may name; more is refused as a flood of tags.
pub const MAX_TAGS: usize = 32;
/// How far a source time may lie after its receipt before the item is refused as future-dated
/// (data-plane spec §3.4's `future_skew_ms` default).
pub const FUTURE_SKEW_MS: u32 = 1_000;
/// What the store keeps of each text field; the reader cuts further (agent harness spec §6.3).
pub const STORED_HEADLINE_BYTES: usize = 1_000;
pub const STORED_SUMMARY_BYTES: usize = 4_000;
pub const STORED_LINK_BYTES: usize = 2_000;
pub const STORED_EXCERPT_BYTES: usize = 8_000;

/// Who received what, when, and under which allowlist and tagging tables.
#[derive(Debug, Clone, Copy)]
pub struct IngestContext<'a> {
    pub allowlist: &'a Allowlist,
    pub source: &'a SourceId,
    pub received_at: UtcNanos,
    pub capture: Capture,
    pub symbols: &'a SymbolTable,
    pub ciks: &'a CikTable,
}

/// A vendor's continuation token: 1 to 256 characters of `A-Z`, `a-z`, `0-9`, `+`, `/`, `=`,
/// `_`, and `-`, so it can ride in a query string unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageToken(String);

impl PageToken {
    pub fn parse(text: &str) -> Result<Self, SourceError> {
        let _ = text;
        Err(SourceError::Unimplemented)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One page of Alpaca news.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewsPage {
    pub drafts: Vec<Draft>,
    /// Each refused item's position in the page and why.
    pub refused: Vec<(usize, SourceError)>,
    pub next_page_token: Option<PageToken>,
}

/// Parses one page of Alpaca's news endpoint (`{"news": [...], "next_page_token": ...}`).
pub fn alpaca_news_page(ctx: &IngestContext<'_>, body: &[u8]) -> Result<NewsPage, SourceError> {
    let _ = (ctx, body);
    Err(SourceError::Unimplemented)
}

/// A filing's primary document name in EDGAR's archive: 1 to 128 characters of `A-Z`, `a-z`,
/// `0-9`, `.`, `_`, and `-`, never `..`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentName(String);

impl DocumentName {
    pub fn parse(text: &str) -> Result<Self, SourceError> {
        let _ = text;
        Err(SourceError::Unimplemented)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One row of EDGAR's submissions index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgarIndexEntry {
    pub cik: Cik,
    pub accession: VendorId,
    pub form: FormType,
    pub accepted_at: UtcNanos,
    pub primary_document: DocumentName,
}

/// EDGAR's submissions index for one filer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgarIndex {
    pub entries: Vec<EdgarIndexEntry>,
    pub refused: Vec<(usize, SourceError)>,
}

/// Parses `data.sec.gov/submissions/CIK##########.json`'s `filings.recent` columns. The filer's
/// own `tickers` list is never read: tags come from the versioned CIK table (§4.3).
pub fn edgar_submissions(ctx: &IngestContext<'_>, body: &[u8]) -> Result<EdgarIndex, SourceError> {
    let _ = (ctx, body);
    Err(SourceError::Unimplemented)
}

/// One filing's draft from its index row and its primary document's raw bytes.
pub fn edgar_filing(
    ctx: &IngestContext<'_>,
    entry: &EdgarIndexEntry,
    document: &[u8],
) -> Result<Draft, SourceError> {
    let _ = (ctx, entry, document, Cik::get);
    Err(SourceError::Unimplemented)
}
