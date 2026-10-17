//! The vetted-source allowlist (DEC-101, E17-7, data-plane spec §4.1 and §4.2 step 1): versioned
//! configuration, one file a version under `config/sources/`, and the only constructor of a
//! [`PermittedUrl`] (DP-9, DEC-433 item 11).
//!
//! A source is in effect only while its entry says `counsel_confirmed = true` (DEC-433 item 20,
//! DEC-996 item 2): an unconfirmed entry is listed for review, permits no fetch, and contributes
//! nothing to [`Allowlist::research_allowlist`].

use std::collections::{BTreeMap, BTreeSet};

use mandate_research::{AllowlistVersion, SourceAllowlist, SourceId};
use serde::Deserialize;

use crate::SourceError;

/// What a source delivers, which decides the parser, the store, and the read it feeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    News,
    Filings,
}

/// One allowlisted endpoint: an HTTPS host and a path prefix that every URL must start with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    host: String,
    path_prefix: String,
}

impl Endpoint {
    /// Parses `https://<host><path prefix>`: a lowercase DNS host with no port or user info, and a
    /// path prefix starting with `/`.
    pub fn parse(text: &str) -> Result<Self, SourceError> {
        let _ = text;
        Err(SourceError::Unimplemented)
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn path_prefix(&self) -> &str {
        &self.path_prefix
    }
}

/// One source on the allowlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceEntry {
    id: SourceId,
    kind: SourceKind,
    licensed: bool,
    counsel_confirmed: bool,
    min_interval_ms: u64,
    max_response_bytes: u64,
    endpoints: Vec<Endpoint>,
}

impl SourceEntry {
    pub fn id(&self) -> &SourceId {
        &self.id
    }

    pub fn kind(&self) -> SourceKind {
        self.kind
    }

    /// Whether the source's text is licensed rather than public (agent harness spec §10.2).
    pub fn licensed(&self) -> bool {
        self.licensed
    }

    /// Whether counsel confirmed the source's terms allow what DEC-433 item 20 keeps.
    pub fn counsel_confirmed(&self) -> bool {
        self.counsel_confirmed
    }

    /// The least time between two requests to the source (the SEC's fair-access rules).
    pub fn min_interval_ms(&self) -> u64 {
        self.min_interval_ms
    }

    /// The largest response body the source may send.
    pub fn max_response_bytes(&self) -> u64 {
        self.max_response_bytes
    }

    pub fn endpoints(&self) -> &[Endpoint] {
        &self.endpoints
    }
}

/// One version of the allowlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allowlist {
    version: AllowlistVersion,
    entries: BTreeMap<SourceId, SourceEntry>,
}

/// A URL a fetcher may send a request to: built only by [`Allowlist::permit`], so a
/// [`crate::fetch::Transport`] cannot be handed anything else (DP-9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermittedUrl {
    source: SourceId,
    url: String,
    max_response_bytes: u64,
}

impl PermittedUrl {
    pub fn source(&self) -> &SourceId {
        &self.source
    }

    pub fn as_str(&self) -> &str {
        &self.url
    }

    /// The source's response cap, which the transport enforces while it reads.
    pub fn max_response_bytes(&self) -> u64 {
        self.max_response_bytes
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileShape {
    version: u32,
    source: Vec<EntryShape>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryShape {
    id: String,
    kind: SourceKind,
    licensed: bool,
    counsel_confirmed: bool,
    min_interval_ms: u64,
    max_response_bytes: u64,
    endpoints: Vec<String>,
}

impl Allowlist {
    /// Parses one allowlist file. Unknown keys, a version of 0, a repeated source, a source with
    /// no endpoint, a zero response cap, and an endpoint that is not `https://` are refused.
    pub fn parse(text: &str) -> Result<Self, SourceError> {
        let _ = text;
        let _: Option<FileShape> = None;
        let _: Option<EntryShape> = None;
        Err(SourceError::Unimplemented)
    }

    pub fn version(&self) -> AllowlistVersion {
        self.version
    }

    /// Every entry, confirmed or not.
    pub fn entries(&self) -> impl Iterator<Item = &SourceEntry> {
        self.entries.values()
    }

    /// The entry for `id` if it is in effect: listed and confirmed by counsel.
    pub fn active(&self, id: &SourceId) -> Option<&SourceEntry> {
        let _ = id;
        None
    }

    /// The sources in effect, as admission check 14 reads them (`mandate-research`).
    pub fn research_allowlist(&self) -> SourceAllowlist {
        SourceAllowlist {
            version: self.version,
            sources: BTreeSet::new(),
        }
    }

    /// `url` as a [`PermittedUrl`] if `source` is in effect and `url` lies under one of its
    /// endpoints; refused otherwise, before any byte is sent (DP-9).
    pub fn permit(&self, source: &SourceId, url: &str) -> Result<PermittedUrl, SourceError> {
        let _ = (source, url);
        Err(SourceError::Unimplemented)
    }
}
