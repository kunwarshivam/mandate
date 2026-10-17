//! The five inbound instructions of control-plane design §3.8, each a closed body: every member
//! listed is present, no other member is admitted, and no member is free text (CP-1). The members
//! are those journal spec §9.17 records, so a [`crate::Verified`] maps onto its record member for
//! member.

use std::collections::BTreeSet;

use mandate_canon::{Digest, Object, Value};
use mandate_time::UtcNanos;

/// An opaque identifier in journal spec §2's grammar, `[A-Za-z0-9_-]+`, at most 64 bytes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ident(String);

impl Ident {
    pub fn parse(text: &str) -> Option<Self> {
        let grammar = text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        (grammar && !text.is_empty() && text.len() <= 64).then(|| Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A signed document's sequence number, at least 1 (journal spec §9.17 rule 139).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Sequence(u64);

impl Sequence {
    pub fn new(n: u64) -> Option<Self> {
        (n >= 1).then_some(Self(n))
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

/// A license's feature flags, the closed list of design §3.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Feature {
    ConnectedClients,
    Hybrid,
}

impl Feature {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ConnectedClients => "connected_clients",
            Self::Hybrid => "hybrid",
        }
    }
}

/// Why a release was withdrawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WithdrawalReason {
    Defect,
    Security,
    Superseded,
}

impl WithdrawalReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Defect => "defect",
            Self::Security => "security",
            Self::Superseded => "superseded",
        }
    }
}

/// What a catalog entry registers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CatalogKind {
    ConnectorPackage,
    ModelRegistryEntry,
}

impl CatalogKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ConnectorPackage => "connector_package",
            Self::ModelRegistryEntry => "model_registry_entry",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct License {
    pub license_id: Ident,
    pub sequence: Sequence,
    pub org_id: Ident,
    pub deployment_ids: Vec<Ident>,
    pub max_workspaces: u64,
    pub max_agents_deployed: u64,
    pub features: BTreeSet<Feature>,
    pub not_before: UtcNanos,
    pub not_after: UtcNanos,
    pub grace_days: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseManifest {
    pub release_id: Ident,
    pub sequence: Sequence,
    pub images: Vec<(Ident, Digest)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseWithdrawn {
    pub release_id: Ident,
    pub reason: WithdrawalReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    pub kind: CatalogKind,
    pub entry_id: Ident,
    pub content_hash: Digest,
    pub sequence: Sequence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataBundle {
    pub dataset: Ident,
    pub bundle_digest: Digest,
    pub sequence: Sequence,
}

/// One verified instruction's body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Instruction {
    License(License),
    ReleaseManifest(ReleaseManifest),
    ReleaseWithdrawn(ReleaseWithdrawn),
    CatalogEntry(CatalogEntry),
    DataBundle(DataBundle),
}

impl Instruction {
    /// The document's sequence, for the instructions that carry one (CP-9).
    pub fn sequence(&self) -> Option<Sequence> {
        match self {
            Self::License(l) => Some(l.sequence),
            Self::ReleaseManifest(m) => Some(m.sequence),
            Self::CatalogEntry(c) => Some(c.sequence),
            Self::DataBundle(d) => Some(d.sequence),
            Self::ReleaseWithdrawn(_) => None,
        }
    }
}

/// A body's members, read once each; [`Body::closed`] refuses any member not read.
pub(crate) struct Body<'a> {
    members: &'a Object,
    read: BTreeSet<&'static str>,
}

impl<'a> Body<'a> {
    pub(crate) fn new(value: &'a Value) -> Option<Self> {
        value.as_object().map(|members| Self {
            members,
            read: BTreeSet::new(),
        })
    }

    fn member(&mut self, name: &'static str) -> Option<&'a Value> {
        self.read.insert(name);
        self.members.get(name)
    }

    fn ident(&mut self, name: &'static str) -> Option<Ident> {
        self.member(name)
            .and_then(Value::as_str)
            .and_then(Ident::parse)
    }

    fn int(&mut self, name: &'static str) -> Option<u64> {
        self.member(name).and_then(Value::as_int)
    }

    fn sequence(&mut self) -> Option<Sequence> {
        self.int("sequence").and_then(Sequence::new)
    }

    fn digest(&mut self, name: &'static str) -> Option<Digest> {
        self.member(name)
            .and_then(Value::as_str)
            .and_then(Digest::from_hex)
    }

    fn time(&mut self, name: &'static str) -> Option<UtcNanos> {
        let text = self.member(name).and_then(Value::as_str)?;
        UtcNanos::parse(text).ok()
    }

    fn texts(&mut self, name: &'static str) -> Option<Vec<&'a str>> {
        let items = self.member(name).and_then(Value::as_array)?;
        items.iter().map(Value::as_str).collect()
    }

    fn one_of<T: Copy>(&mut self, name: &'static str, options: &[(&str, T)]) -> Option<T> {
        let text = self.member(name).and_then(Value::as_str)?;
        options.iter().find(|(o, _)| *o == text).map(|(_, v)| *v)
    }

    fn closed<T>(self, value: T) -> Option<T> {
        let read = self.read;
        self.members
            .keys()
            .all(|k| read.contains(k.as_str()))
            .then_some(value)
    }
}

/// Strictly ascending by bytes, so a repeat is refused as well as a swap (journal spec rule 138).
fn ascending(items: &[&str]) -> bool {
    items.windows(2).all(|w| matches!(w, [a, b] if a < b))
}

const FEATURES: [(&str, Feature); 2] = [
    ("connected_clients", Feature::ConnectedClients),
    ("hybrid", Feature::Hybrid),
];

pub(crate) fn license(mut b: Body<'_>) -> Option<Instruction> {
    let deployments = b.texts("deployment_ids")?;
    let features = b.texts("features")?;
    let license = License {
        license_id: b.ident("license_id")?,
        sequence: b.sequence()?,
        org_id: b.ident("org_id")?,
        deployment_ids: deployments
            .iter()
            .map(|d| Ident::parse(d))
            .collect::<Option<_>>()?,
        max_workspaces: b.int("max_workspaces")?,
        max_agents_deployed: b.int("max_agents_deployed")?,
        features: features
            .iter()
            .map(|f| FEATURES.iter().find(|(n, _)| n == f).map(|(_, v)| *v))
            .collect::<Option<_>>()?,
        not_before: b.time("not_before")?,
        not_after: b.time("not_after")?,
        grace_days: b.int("grace_days")?,
    };
    let shaped = !deployments.is_empty()
        && ascending(&deployments)
        && ascending(&features)
        && license.not_before < license.not_after;
    b.closed(Instruction::License(license)).filter(|_| shaped)
}

pub(crate) fn release_manifest(mut b: Body<'_>) -> Option<Instruction> {
    let release_id = b.ident("release_id")?;
    let sequence = b.sequence()?;
    let images = b.member("images").and_then(Value::as_array)?;
    let images = images
        .iter()
        .map(|image| {
            let mut i = Body::new(image)?;
            let pair = (i.ident("component")?, i.digest("digest")?);
            i.closed(pair)
        })
        .collect::<Option<Vec<_>>>()?;
    let components: Vec<&str> = images.iter().map(|(c, _)| c.as_str()).collect();
    let shaped = !images.is_empty() && ascending(&components);
    let manifest = ReleaseManifest {
        release_id,
        sequence,
        images,
    };
    b.closed(Instruction::ReleaseManifest(manifest))
        .filter(|_| shaped)
}

pub(crate) fn release_withdrawn(mut b: Body<'_>) -> Option<Instruction> {
    let withdrawn = ReleaseWithdrawn {
        release_id: b.ident("release_id")?,
        reason: b.one_of(
            "reason",
            &[
                ("defect", WithdrawalReason::Defect),
                ("security", WithdrawalReason::Security),
                ("superseded", WithdrawalReason::Superseded),
            ],
        )?,
    };
    b.closed(Instruction::ReleaseWithdrawn(withdrawn))
}

pub(crate) fn catalog_entry(mut b: Body<'_>) -> Option<Instruction> {
    let entry = CatalogEntry {
        kind: b.one_of(
            "kind",
            &[
                ("connector_package", CatalogKind::ConnectorPackage),
                ("model_registry_entry", CatalogKind::ModelRegistryEntry),
            ],
        )?,
        entry_id: b.ident("entry_id")?,
        content_hash: b.digest("content_hash")?,
        sequence: b.sequence()?,
    };
    b.closed(Instruction::CatalogEntry(entry))
}

pub(crate) fn data_bundle(mut b: Body<'_>) -> Option<Instruction> {
    let bundle = DataBundle {
        dataset: b.ident("dataset")?,
        bundle_digest: b.digest("bundle_digest")?,
        sequence: b.sequence()?,
    };
    b.closed(Instruction::DataBundle(bundle))
}
