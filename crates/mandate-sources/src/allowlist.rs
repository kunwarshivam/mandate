//! The vetted-source allowlist (DEC-101, E17-7, data-plane spec §4.1 and §4.2 step 1): versioned
//! configuration, one file a version under `config/sources/`, and the only constructor of a
//! [`PermittedUrl`] (DP-9, DEC-433 item 11).
//!
//! A source is in effect only while its entry says `counsel_confirmed = true` (DEC-433 item 20,
//! DEC-1130 item 2), or names the founder's approval and the decision that records it
//! (`approved_by`, `approved_in`; DEC-79, DEC-2276): an entry with neither is listed for review,
//! permits no fetch, and contributes nothing to [`Allowlist::research_allowlist`].

use std::collections::{BTreeMap, BTreeSet};

use mandate_research::{AllowlistVersion, SourceAllowlist, SourceId};
use serde::Deserialize;

use crate::SourceError;

const HTTPS: &str = "https://";

/// The only bytes a permitted URL may hold after its scheme: no `%`, so nothing is decoded into a
/// traversal; no `@`, `#`, `\`, or whitespace; no `..` and no `//` (checked beside it).
fn url_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"-._~/?&=:,+".contains(&b)
}

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
        let rest = text.strip_prefix(HTTPS).ok_or(SourceError::BadEndpoint)?;
        let slash = rest.find('/').ok_or(SourceError::BadEndpoint)?;
        let (host, path_prefix) = rest.split_at(slash);
        let host_ok = !host.is_empty()
            && !host.contains("..")
            && host
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b".-".contains(&b));
        let path_ok = !path_prefix.contains("..")
            && !path_prefix.contains("//")
            && path_prefix
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._~/".contains(&b));
        if !host_ok || !path_ok {
            return Err(SourceError::BadEndpoint);
        }
        Ok(Self {
            host: host.to_owned(),
            path_prefix: path_prefix.to_owned(),
        })
    }

    /// Whether `host` and `path_and_query` lie under this endpoint: the same host, and the path
    /// prefix followed by nothing, or by `/` or `?` when the prefix does not end in `/`.
    fn covers(&self, host: &str, path_and_query: &str) -> bool {
        let Some(after) = path_and_query.strip_prefix(self.path_prefix.as_str()) else {
            return false;
        };
        host == self.host
            && (self.path_prefix.ends_with('/')
                || after.is_empty()
                || after.starts_with('/')
                || after.starts_with('?'))
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn path_prefix(&self) -> &str {
        &self.path_prefix
    }
}

/// Who approved a source that counsel has not confirmed (DEC-2276).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Approver {
    /// The founder, whose call it is under DEC-79 (legal).
    Founder,
}

/// A source's approval other than counsel's: who gave it, and the decision that records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approval {
    pub by: Approver,
    /// The decision file's id, `DEC-<n>`.
    pub decision: String,
}

/// One source on the allowlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceEntry {
    id: SourceId,
    kind: SourceKind,
    licensed: bool,
    counsel_confirmed: bool,
    approval: Option<Approval>,
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

    /// The approval the entry records besides counsel's, if any (DEC-2276).
    pub fn approval(&self) -> Option<&Approval> {
        self.approval.as_ref()
    }

    /// Whether the source is in effect: counsel confirmed it, or the entry records the founder's
    /// approval and its decision.
    pub fn in_effect(&self) -> bool {
        self.counsel_confirmed || self.approval.is_some()
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
    #[serde(default)]
    approved_by: Option<Approver>,
    #[serde(default)]
    approved_in: Option<String>,
    min_interval_ms: u64,
    max_response_bytes: u64,
    endpoints: Vec<String>,
}

/// `DEC-` followed by one or more digits, with no leading zero.
fn decision_id(text: &str) -> bool {
    text.strip_prefix("DEC-").is_some_and(|n| {
        !n.is_empty() && !n.starts_with('0') && n.bytes().all(|b| b.is_ascii_digit())
    })
}

impl Allowlist {
    /// Parses one allowlist file. Unknown keys, a version of 0, a repeated source, a source with
    /// no endpoint, a zero response cap, an endpoint that is not `https://`, and an approver
    /// without the decision that records it, or a decision without an approver, are refused.
    pub fn parse(text: &str) -> Result<Self, SourceError> {
        let file: FileShape = toml::from_str(text).map_err(|_| SourceError::AllowlistMalformed)?;
        if file.version == 0 {
            return Err(SourceError::AllowlistMalformed);
        }
        let mut entries = BTreeMap::new();
        for shape in file.source {
            let id = SourceId::new(&shape.id).map_err(|_| SourceError::AllowlistMalformed)?;
            if shape.max_response_bytes == 0 {
                return Err(SourceError::AllowlistMalformed);
            }
            if shape.endpoints.is_empty() {
                return Err(SourceError::BadEndpoint);
            }
            let endpoints = shape
                .endpoints
                .iter()
                .map(|e| Endpoint::parse(e))
                .collect::<Result<Vec<_>, _>>()?;
            let approval = match (shape.approved_by, shape.approved_in) {
                (None, None) => None,
                (Some(by), Some(decision)) if decision_id(&decision) => {
                    Some(Approval { by, decision })
                }
                _ => return Err(SourceError::AllowlistMalformed),
            };
            let entry = SourceEntry {
                id: id.clone(),
                kind: shape.kind,
                licensed: shape.licensed,
                counsel_confirmed: shape.counsel_confirmed,
                approval,
                min_interval_ms: shape.min_interval_ms,
                max_response_bytes: shape.max_response_bytes,
                endpoints,
            };
            if entries.insert(id, entry).is_some() {
                return Err(SourceError::DuplicateSource);
            }
        }
        Ok(Self {
            version: AllowlistVersion(file.version),
            entries,
        })
    }

    pub fn version(&self) -> AllowlistVersion {
        self.version
    }

    /// Every entry, confirmed or not.
    pub fn entries(&self) -> impl Iterator<Item = &SourceEntry> {
        self.entries.values()
    }

    /// The entry for `id` if it is in effect: listed, and confirmed by counsel or approved by the
    /// founder in a recorded decision.
    pub fn active(&self, id: &SourceId) -> Option<&SourceEntry> {
        self.entries.get(id).filter(|e| e.in_effect())
    }

    /// The sources in effect, as admission check 14 reads them (`mandate-research`).
    pub fn research_allowlist(&self) -> SourceAllowlist {
        SourceAllowlist {
            version: self.version,
            sources: self
                .entries
                .values()
                .filter(|e| e.in_effect())
                .map(|e| e.id.clone())
                .collect::<BTreeSet<_>>(),
        }
    }

    /// `url` as a [`PermittedUrl`] if `source` is in effect and `url` lies under one of its
    /// endpoints; refused otherwise, before any byte is sent (DP-9).
    pub fn permit(&self, source: &SourceId, url: &str) -> Result<PermittedUrl, SourceError> {
        let entry = self
            .active(source)
            .ok_or(SourceError::SourceNotAllowlisted)?;
        let rest = url
            .strip_prefix(HTTPS)
            .filter(|rest| {
                !rest.contains("..") && !rest.contains("//") && rest.bytes().all(url_byte)
            })
            .ok_or(SourceError::EndpointNotAllowlisted)?;
        let (host, path_and_query) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
        if !entry
            .endpoints
            .iter()
            .any(|e| e.covers(host, path_and_query))
        {
            return Err(SourceError::EndpointNotAllowlisted);
        }
        Ok(PermittedUrl {
            source: source.clone(),
            url: url.to_owned(),
            max_response_bytes: entry.max_response_bytes,
        })
    }
}
