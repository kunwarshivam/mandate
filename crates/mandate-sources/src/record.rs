//! A normalized news or filing record (data-plane spec §4.5): its typed fields, every one taken
//! from a structured vendor field, the platform's clock, or a digest, and its text, kept apart as
//! [`ItemText`] so no typed field can be derived from text (DP-8).

use std::collections::BTreeSet;

use mandate_canon::Digest;
use mandate_domain::AssetId;
use mandate_research::{AllowlistVersion, ContentHash, SourceId};
use mandate_time::UtcNanos;

use crate::SourceError;
use crate::allowlist::SourceKind;
use crate::symbols::{Cik, Symbol};
use crate::text::UntrustedText;

/// How an item reached the platform, which decides its knowledge time (data-plane spec §5.2,
/// DEC-433 item 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capture {
    /// Polled as it was published: known when received.
    Live,
    /// Downloaded after the fact: known when its source first published it.
    Backfill,
}

/// A vendor's own item id or an accession number: 1 to 64 characters of `A-Z`, `a-z`, `0-9`, `.`,
/// `_`, and `-`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VendorId(String);

impl VendorId {
    pub fn parse(text: &str) -> Result<Self, SourceError> {
        let allowed = |b: u8| b.is_ascii_alphanumeric() || b"._-".contains(&b);
        if !(1..=64).contains(&text.len()) || !text.bytes().all(allowed) {
            return Err(SourceError::BadField);
        }
        Ok(Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An SEC form type: 1 to 16 characters of `A-Z`, `0-9`, `-`, `/`, and space (`10-K`, `10-K/A`,
/// `8-K`, `SC 13G`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FormType(String);

impl FormType {
    pub fn parse(text: &str) -> Result<Self, SourceError> {
        let allowed = |b: u8| b.is_ascii_uppercase() || b.is_ascii_digit() || b"-/ ".contains(&b);
        if !(1..=16).contains(&text.len()) || !text.bytes().all(allowed) {
            return Err(SourceError::BadField);
        }
        Ok(Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A record's id: `<source>/<vendor id>@<vendor version>`, unique per version.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemId(String);

impl ItemId {
    pub(crate) fn of(source: &SourceId, vendor_id: &VendorId, vendor_version: UtcNanos) -> Self {
        Self(format!(
            "{}/{}@{}",
            source.as_str(),
            vendor_id.as_str(),
            vendor_version
        ))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The story group an item belongs to (data-plane spec §4.4): copies in one group count as one
/// source for corroboration (DEC-433 item 14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GroupKey(pub Digest);

/// A filing's structured fields from EDGAR's index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilingFields {
    pub cik: Cik,
    pub accession: VendorId,
    pub form: FormType,
}

/// Every typed field of a record. Nothing here comes from free text: the order path, the
/// admission checks, and the drift detector read only these (DP-8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedFields {
    pub item_id: ItemId,
    pub source: SourceId,
    pub allowlist_version: AllowlistVersion,
    pub kind: SourceKind,
    pub vendor_id: VendorId,
    /// The vendor's update time; a later one is a new version (§4.4).
    pub vendor_version: UtcNanos,
    /// Published or accepted time: displayed only, never a filter (agent harness spec HI-13).
    pub published_at: UtcNanos,
    pub received_at: UtcNanos,
    /// When the platform could first have known the item (§5.2); every as-of read filters on it.
    pub knowledge_time: UtcNanos,
    /// SHA-256 of the item's raw bytes.
    pub content: ContentHash,
    pub length_bytes: u64,
    /// Instruments the vendor's structured fields name, resolved as of the knowledge time.
    pub tags: BTreeSet<AssetId>,
    pub group: GroupKey,
    /// The version this one replaces, if it is a vendor update.
    pub supersedes: Option<ItemId>,
    pub filing: Option<FilingFields>,
}

/// A record's text, all of it untrusted (DP-8).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ItemText {
    pub headline: Option<UntrustedText>,
    pub summary: Option<UntrustedText>,
    pub link: Option<UntrustedText>,
    pub excerpt: Option<UntrustedText>,
}

/// A parsed item before the store gives it an id, a group, and a predecessor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    pub(crate) source: SourceId,
    pub(crate) allowlist_version: AllowlistVersion,
    pub(crate) kind: SourceKind,
    pub(crate) vendor_id: VendorId,
    pub(crate) vendor_version: UtcNanos,
    pub(crate) published_at: UtcNanos,
    pub(crate) received_at: UtcNanos,
    pub(crate) knowledge_time: UtcNanos,
    pub(crate) content: ContentHash,
    pub(crate) length_bytes: u64,
    pub(crate) tags: BTreeSet<AssetId>,
    /// The tickers the item names, as parsed: a news item's from the vendor's own `symbols`, in
    /// the vendor's order, unknown ones included (DEC-2280 item 2); a filing's from the CIK table.
    pub(crate) tickers: Vec<Symbol>,
    pub(crate) body_group: GroupKey,
    pub(crate) filing: Option<FilingFields>,
    pub(crate) text: ItemText,
}

impl Draft {
    pub fn source(&self) -> &SourceId {
        &self.source
    }

    pub fn allowlist_version(&self) -> AllowlistVersion {
        self.allowlist_version
    }

    pub fn vendor_version(&self) -> UtcNanos {
        self.vendor_version
    }

    pub fn published_at(&self) -> UtcNanos {
        self.published_at
    }

    pub fn received_at(&self) -> UtcNanos {
        self.received_at
    }

    pub fn content(&self) -> &ContentHash {
        &self.content
    }

    /// The tickers the item names, as parsed (DEC-2280 item 2).
    pub fn tickers(&self) -> &[Symbol] {
        &self.tickers
    }

    /// The item's text, all of it untrusted (DP-8).
    pub fn text(&self) -> &ItemText {
        &self.text
    }

    pub fn vendor_id(&self) -> &VendorId {
        &self.vendor_id
    }

    pub fn knowledge_time(&self) -> UtcNanos {
        self.knowledge_time
    }

    pub fn tags(&self) -> &BTreeSet<AssetId> {
        &self.tags
    }
}

/// A stored record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRecord {
    pub(crate) typed: TypedFields,
    pub(crate) text: ItemText,
}

impl SourceRecord {
    pub fn typed(&self) -> &TypedFields {
        &self.typed
    }

    pub fn text(&self) -> &ItemText {
        &self.text
    }
}
