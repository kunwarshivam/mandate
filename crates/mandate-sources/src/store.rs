//! The news store and the filings store (data-plane spec §4.4, §4.5, §5.2): raw bytes kept by
//! digest, records appended and never revised (DP-2), and as-of reads that filter on knowledge
//! time (DP-1).
//!
//! Both are in memory. Where records live in a cell is the workspace data service's (lane M2,
//! E2-6); this store moves onto its storage interface once that is on `main` (DEC-996 item 8).

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::Digest;
use mandate_domain::AssetId;
use mandate_research::SourceId;
use mandate_time::UtcNanos;

use crate::SourceError;
use crate::allowlist::SourceKind;
use crate::record::{Draft, ItemId, SourceRecord, VendorId};

/// What an ingestion did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ingested {
    /// A new record, or a new version of a vendor's item.
    Stored(ItemId),
    /// An exact copy of a stored record from the same source: stored once (§4.4).
    Duplicate(ItemId),
}

/// An as-of read of one store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreQuery<'a> {
    pub as_of: UtcNanos,
    /// Only records whose knowledge time is after this instant.
    pub since: UtcNanos,
    /// Only records from these sources.
    pub sources: &'a BTreeSet<SourceId>,
    /// Only records tagged with one of these instruments.
    pub instruments: &'a BTreeSet<AssetId>,
    pub limit: usize,
}

/// One store's answer: newest knowledge time first, and how many matching records the limit cut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreAnswer<'a> {
    pub records: Vec<&'a SourceRecord>,
    pub dropped: usize,
}

/// The records of one kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemStore {
    kind: SourceKind,
    raw: BTreeMap<Digest, Vec<u8>>,
    records: Vec<SourceRecord>,
    versions: BTreeMap<(SourceId, VendorId), Vec<usize>>,
    copies: BTreeMap<(SourceId, Digest), usize>,
}

impl ItemStore {
    /// The news store.
    pub fn news() -> Self {
        Self::of(SourceKind::News)
    }

    /// The filings store.
    pub fn filings() -> Self {
        Self::of(SourceKind::Filings)
    }

    fn of(kind: SourceKind) -> Self {
        Self {
            kind,
            raw: BTreeMap::new(),
            records: Vec::new(),
            versions: BTreeMap::new(),
            copies: BTreeMap::new(),
        }
    }

    pub fn kind(&self) -> SourceKind {
        self.kind
    }

    /// Keeps `bytes` under their SHA-256 digest, before anything parses them (§4.2 step 2).
    pub fn put_raw(&mut self, bytes: &[u8]) -> Digest {
        let _ = bytes;
        Digest::ZERO
    }

    /// The raw bytes stored under `digest`.
    pub fn raw(&self, digest: &Digest) -> Option<&[u8]> {
        let _ = digest;
        None
    }

    /// Appends `draft`. An exact copy from the same source is a [`Ingested::Duplicate`]; a vendor
    /// update with a later version is a new record that supersedes the latest and joins its group;
    /// anything else under a stored vendor id is refused [`SourceError::RevisedInPlace`].
    pub fn ingest(&mut self, draft: Draft) -> Result<Ingested, SourceError> {
        let _ = draft;
        Err(SourceError::Unimplemented)
    }

    /// Every record, in the order stored.
    pub fn records(&self) -> &[SourceRecord] {
        &self.records
    }

    /// The records known by `as_of`: for each vendor item, only its latest version known by then.
    pub fn as_of(&self, query: &StoreQuery<'_>) -> StoreAnswer<'_> {
        let _ = query;
        StoreAnswer {
            records: Vec::new(),
            dropped: 0,
        }
    }
}
