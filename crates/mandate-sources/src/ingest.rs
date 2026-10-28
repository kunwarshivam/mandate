//! Vendor payloads to [`Draft`]s (data-plane spec §4.2 steps 3 and 5): structured fields only,
//! every one checked against its allowed form, and text kept as bounded [`UntrustedText`] that no
//! typed field is derived from (DP-8).
//!
//! The caller stores the raw response by digest first ([`crate::store::ItemStore::put_raw`],
//! §4.2 step 2). A response larger than its source's cap is refused whole; one bad item in a news
//! page is refused and counted, and the rest are kept (§3.1's rule: refused and counted, never
//! completed).

use std::collections::BTreeSet;

use mandate_canon::Digest;
use mandate_domain::AssetId;
use mandate_research::{ContentHash, SourceId};
use mandate_time::UtcNanos;
use serde::Deserialize;
use serde_json::value::RawValue;

use crate::SourceError;
use crate::allowlist::{Allowlist, SourceEntry, SourceKind};
use crate::record::{Capture, Draft, FilingFields, FormType, GroupKey, ItemText, VendorId};
use crate::symbols::{Cik, CikTable, Symbol, SymbolTable};
use crate::text::UntrustedText;

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

/// A vendor's continuation token: 1 to 256 characters of `A-Z`, `a-z`, `0-9`, `/`, `=`, `_`, and
/// `-`, so it can ride in a query string unchanged. No `+`, which a server decodes as a space and
/// `%` cannot escape here (DEC-1130 item 3): a page whose token holds one ends the paging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageToken(String);

impl PageToken {
    pub fn parse(text: &str) -> Result<Self, SourceError> {
        let allowed = |b: u8| b.is_ascii_alphanumeric() || b"/=_-".contains(&b);
        if !(1..=256).contains(&text.len()) || !text.bytes().all(allowed) {
            return Err(SourceError::BadField);
        }
        Ok(Self(text.to_owned()))
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
    source_for(ctx, SourceKind::News, body)?;
    let page: NewsPageShape<'_> =
        serde_json::from_slice(body).map_err(|_| SourceError::Malformed)?;
    let mut drafts = Vec::new();
    let mut refused = Vec::new();
    for (position, item) in page.news.iter().enumerate() {
        match news_draft(ctx, item) {
            Ok(draft) => drafts.push(draft),
            Err(e) => refused.push((position, e)),
        }
    }
    Ok(NewsPage {
        drafts,
        refused,
        next_page_token: page.next_page_token.and_then(|t| PageToken::parse(&t).ok()),
    })
}

#[derive(Deserialize)]
struct NewsPageShape<'a> {
    #[serde(borrow)]
    news: Vec<&'a RawValue>,
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
struct NewsShape {
    id: u64,
    #[serde(default)]
    headline: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    url: String,
    created_at: String,
    updated_at: String,
    #[serde(default)]
    symbols: Vec<String>,
}

fn news_draft(ctx: &IngestContext<'_>, item: &RawValue) -> Result<Draft, SourceError> {
    let raw = item.get();
    let shape: NewsShape = serde_json::from_str(raw).map_err(|_| SourceError::Malformed)?;
    let published = vendor_time(&shape.created_at)?;
    let updated = vendor_time(&shape.updated_at)?;
    not_future(published, ctx.received_at)?;
    not_future(updated, ctx.received_at)?;
    let knowledge_time = known_at(ctx, published.max(updated));
    let symbols = shape
        .symbols
        .iter()
        .map(|s| Symbol::parse(s))
        .collect::<Result<Vec<_>, _>>()?;
    let body_group = GroupKey(Digest::of_parts(&[
        b"news\0",
        normalized(&shape.headline).as_bytes(),
        b"\n",
        normalized(&shape.summary).as_bytes(),
    ]));
    Ok(Draft {
        source: ctx.source.clone(),
        allowlist_version: ctx.allowlist.version(),
        kind: SourceKind::News,
        vendor_id: VendorId::parse(&shape.id.to_string())?,
        vendor_version: updated,
        published_at: published,
        received_at: ctx.received_at,
        knowledge_time,
        content: ContentHash::new(Digest::of(raw.as_bytes())),
        length_bytes: byte_length(raw.as_bytes())?,
        tags: tags(ctx, &symbols, knowledge_time)?,
        tickers: symbols,
        body_group,
        filing: None,
        text: ItemText {
            headline: text(&shape.headline, STORED_HEADLINE_BYTES),
            summary: text(&shape.summary, STORED_SUMMARY_BYTES),
            link: text(&shape.url, STORED_LINK_BYTES),
            excerpt: None,
        },
    })
}

/// The source's entry, if it is in effect, of `kind`, and `body` is within its cap.
fn source_for<'a>(
    ctx: &IngestContext<'a>,
    kind: SourceKind,
    body: &[u8],
) -> Result<&'a SourceEntry, SourceError> {
    let entry = ctx
        .allowlist
        .active(ctx.source)
        .ok_or(SourceError::SourceNotAllowlisted)?;
    if entry.kind() != kind {
        return Err(SourceError::WrongKind);
    }
    if byte_length(body)? > entry.max_response_bytes() {
        return Err(SourceError::ResponseTooLarge);
    }
    Ok(entry)
}

fn byte_length(bytes: &[u8]) -> Result<u64, SourceError> {
    u64::try_from(bytes.len()).map_err(|_| SourceError::ResponseTooLarge)
}

fn vendor_time(text: &str) -> Result<UtcNanos, SourceError> {
    UtcNanos::parse_rfc3339(text).map_err(|_| SourceError::Malformed)
}

/// Refuses a source time more than [`FUTURE_SKEW_MS`] after receipt (§3.4, §8 "Future-dated
/// items").
fn not_future(source_time: UtcNanos, received_at: UtcNanos) -> Result<(), SourceError> {
    let skew_ns = FUTURE_SKEW_MS
        .checked_mul(1_000_000)
        .ok_or(SourceError::Time)?;
    let nanos = received_at
        .nanos()
        .checked_add(skew_ns)
        .ok_or(SourceError::Time)?;
    let carry = i64::from(nanos / 1_000_000_000);
    let latest = UtcNanos::from_parts(
        received_at
            .secs()
            .checked_add(carry)
            .ok_or(SourceError::Time)?,
        nanos % 1_000_000_000,
    )
    .map_err(|_| SourceError::Time)?;
    if source_time > latest {
        return Err(SourceError::FutureDated);
    }
    Ok(())
}

/// Knowledge time (§5.2): receipt for a live capture, the source's own time for a backfill.
fn known_at(ctx: &IngestContext<'_>, source_time: UtcNanos) -> UtcNanos {
    match ctx.capture {
        Capture::Live => ctx.received_at,
        Capture::Backfill => source_time,
    }
}

/// The instruments `symbols` name at `at`; an unknown ticker is dropped (§4.3).
fn tags(
    ctx: &IngestContext<'_>,
    symbols: &[Symbol],
    at: UtcNanos,
) -> Result<BTreeSet<AssetId>, SourceError> {
    if symbols.len() > MAX_TAGS {
        return Err(SourceError::TooManyTags);
    }
    Ok(symbols
        .iter()
        .filter_map(|s| ctx.symbols.resolve(s, at).cloned())
        .collect())
}

fn text(raw: &str, cap: usize) -> Option<UntrustedText> {
    (!raw.is_empty()).then(|| UntrustedText::bounded(raw, cap))
}

/// The syndication digest's input (§4.4): lowercased, whitespace runs collapsed, trimmed. It only
/// groups copies; it never yields a typed field.
fn normalized(raw: &str) -> String {
    raw.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// A filing's primary document name in EDGAR's archive: 1 to 128 characters of `A-Z`, `a-z`,
/// `0-9`, `.`, `_`, and `-`, never `..`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentName(String);

impl DocumentName {
    pub fn parse(text: &str) -> Result<Self, SourceError> {
        let allowed = |b: u8| b.is_ascii_alphanumeric() || b"._-".contains(&b);
        if !(1..=128).contains(&text.len()) || text.contains("..") || !text.bytes().all(allowed) {
            return Err(SourceError::BadField);
        }
        Ok(Self(text.to_owned()))
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
    source_for(ctx, SourceKind::Filings, body)?;
    let shape: SubmissionsShape =
        serde_json::from_slice(body).map_err(|_| SourceError::Malformed)?;
    let cik = Cik::parse(&shape.cik).map_err(|_| SourceError::Malformed)?;
    let recent = shape.filings.recent;
    let rows = recent.accession.len();
    if [
        recent.accepted.len(),
        recent.form.len(),
        recent.primary.len(),
    ]
    .iter()
    .any(|n| *n != rows)
    {
        return Err(SourceError::Malformed);
    }
    let mut entries = Vec::new();
    let mut refused = Vec::new();
    let columns = recent
        .accession
        .iter()
        .zip(&recent.accepted)
        .zip(&recent.form)
        .zip(&recent.primary);
    for (position, (((accession, accepted), form), primary)) in columns.enumerate() {
        match index_entry(ctx, cik, accession, accepted, form, primary) {
            Ok(entry) => entries.push(entry),
            Err(e) => refused.push((position, e)),
        }
    }
    Ok(EdgarIndex { entries, refused })
}

#[derive(Deserialize)]
struct SubmissionsShape {
    cik: String,
    filings: FilingsShape,
}

#[derive(Deserialize)]
struct FilingsShape {
    recent: RecentShape,
}

#[derive(Deserialize)]
struct RecentShape {
    #[serde(rename = "accessionNumber")]
    accession: Vec<String>,
    #[serde(rename = "acceptanceDateTime")]
    accepted: Vec<String>,
    form: Vec<String>,
    #[serde(rename = "primaryDocument")]
    primary: Vec<String>,
}

fn index_entry(
    ctx: &IngestContext<'_>,
    cik: Cik,
    accession: &str,
    accepted: &str,
    form: &str,
    primary: &str,
) -> Result<EdgarIndexEntry, SourceError> {
    let accepted_at = vendor_time(accepted)?;
    not_future(accepted_at, ctx.received_at)?;
    Ok(EdgarIndexEntry {
        cik,
        accession: accession_number(accession)?,
        form: FormType::parse(form)?,
        accepted_at,
        primary_document: DocumentName::parse(primary)?,
    })
}

/// An accession number is exactly `##########-##-######`.
fn accession_number(text: &str) -> Result<VendorId, SourceError> {
    let shape_ok = text.len() == 20
        && text.bytes().enumerate().all(|(i, b)| match i {
            10 | 13 => b == b'-',
            _ => b.is_ascii_digit(),
        });
    if !shape_ok {
        return Err(SourceError::BadField);
    }
    VendorId::parse(text)
}

/// One filing's draft from its index row and its primary document's raw bytes.
pub fn edgar_filing(
    ctx: &IngestContext<'_>,
    entry: &EdgarIndexEntry,
    document: &[u8],
) -> Result<Draft, SourceError> {
    source_for(ctx, SourceKind::Filings, document)?;
    not_future(entry.accepted_at, ctx.received_at)?;
    let knowledge_time = known_at(ctx, entry.accepted_at);
    let symbols: Vec<Symbol> = ctx
        .ciks
        .symbols(entry.cik, knowledge_time)
        .into_iter()
        .cloned()
        .collect();
    let head = document.get(..STORED_EXCERPT_BYTES).unwrap_or(document);
    Ok(Draft {
        source: ctx.source.clone(),
        allowlist_version: ctx.allowlist.version(),
        kind: SourceKind::Filings,
        vendor_id: entry.accession.clone(),
        vendor_version: entry.accepted_at,
        published_at: entry.accepted_at,
        received_at: ctx.received_at,
        knowledge_time,
        content: ContentHash::new(Digest::of(document)),
        length_bytes: byte_length(document)?,
        tags: tags(ctx, &symbols, knowledge_time)?,
        tickers: symbols,
        body_group: GroupKey(Digest::of_parts(&[
            b"filing\0",
            entry.accession.as_str().as_bytes(),
        ])),
        filing: Some(FilingFields {
            cik: entry.cik,
            accession: entry.accession.clone(),
            form: entry.form.clone(),
        }),
        text: ItemText {
            excerpt: text(&String::from_utf8_lossy(head), STORED_EXCERPT_BYTES),
            ..ItemText::default()
        },
    })
}
