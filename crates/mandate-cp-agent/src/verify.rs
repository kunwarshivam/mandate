//! The checks of control-plane design CP-5 (a) and (c): the kind is in the closed list, the
//! signature verifies against a key the installed release pins for that kind, and the body is its
//! closed schema. A message reaches workspace services only as a [`Verified`] or a [`Refusal`].

use std::collections::BTreeMap;

use mandate_canon::{Digest, Value, parse, to_canonical};
use ring::signature::{ED25519, UnparsedPublicKey};

use crate::instruction::{self, Body, Ident, Instruction};

/// The kinds an inbound message may claim: design §3.8's five instructions, and `Unknown` for
/// anything else, which is refused `out_of_list`. `Receipt` is not an instruction and never
/// reaches this check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MessageKind {
    License,
    ReleaseManifest,
    ReleaseWithdrawn,
    CatalogEntry,
    DataBundle,
    Unknown,
}

impl MessageKind {
    /// The spelling journal spec §9.17's `ControlPlaneMessageRefused.message` records.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::License => "license",
            Self::ReleaseManifest => "release_manifest",
            Self::ReleaseWithdrawn => "release_withdrawn",
            Self::CatalogEntry => "catalog_entry",
            Self::DataBundle => "data_bundle",
            Self::Unknown => "unknown",
        }
    }

    fn parse(text: &str) -> Self {
        [
            Self::License,
            Self::ReleaseManifest,
            Self::ReleaseWithdrawn,
            Self::CatalogEntry,
            Self::DataBundle,
        ]
        .into_iter()
        .find(|k| k.as_str() == text)
        .unwrap_or(Self::Unknown)
    }
}

/// The signing keys of design §6 (DEC-440 item 5), one per kind, each held offline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyKind {
    License,
    Release,
    Registry,
    Data,
}

/// Why an inbound message was refused, as journal spec §9.17 spells it. `Replayed` is workspace
/// services' (CP-9), never this crate's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RefusalReason {
    Unsigned,
    BadSignature,
    UnknownKey,
    Replayed,
    OutOfList,
    Malformed,
    NotAddressed,
}

impl RefusalReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unsigned => "unsigned",
            Self::BadSignature => "bad_signature",
            Self::UnknownKey => "unknown_key",
            Self::Replayed => "replayed",
            Self::OutOfList => "out_of_list",
            Self::Malformed => "malformed",
            Self::NotAddressed => "not_addressed",
        }
    }
}

/// The public keys the installed release pins, by key ID. Built from the release, never from
/// the link (CP-5 (a)).
#[derive(Debug, Clone, Default)]
pub struct PinnedKeys(BTreeMap<String, (KeyKind, Vec<u8>)>);

impl PinnedKeys {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pins `public_key`, an Ed25519 public key, as `kind` under `key_id`.
    pub fn pin(mut self, key_id: &Ident, kind: KeyKind, public_key: &[u8]) -> Self {
        self.0
            .insert(key_id.as_str().to_owned(), (kind, public_key.to_vec()));
        self
    }
}

/// Which deployment this is, which a license must name (`not_addressed` otherwise). A managed cell
/// hosts many organizations, so whether a license's organization has a workspace here is workspace
/// services' check, not this one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    pub deployment_id: Ident,
}

/// An instruction that passed every check here. Its fields are private and only [`crate::receive`]
/// builds one, so workspace services can never be handed an unverified instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    instruction: Instruction,
    key_id: Ident,
    document_digest: Digest,
}

impl Verified {
    pub fn instruction(&self) -> &Instruction {
        &self.instruction
    }

    /// The kind it was received as.
    pub fn message_kind(&self) -> MessageKind {
        match self.instruction {
            Instruction::License(_) => MessageKind::License,
            Instruction::ReleaseManifest(_) => MessageKind::ReleaseManifest,
            Instruction::ReleaseWithdrawn(_) => MessageKind::ReleaseWithdrawn,
            Instruction::CatalogEntry(_) => MessageKind::CatalogEntry,
            Instruction::DataBundle(_) => MessageKind::DataBundle,
        }
    }

    /// The pinned key that verified it.
    pub fn key_id(&self) -> &Ident {
        &self.key_id
    }

    /// SHA-256 of the bytes as received.
    pub fn document_digest(&self) -> Digest {
        self.document_digest
    }
}

/// A refused message: what it claimed, why, the sequence it claimed if one could be read, and the
/// digest of the bytes received. Every member is one §9.17 records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refusal {
    pub message: MessageKind,
    pub reason: RefusalReason,
    pub sequence: Option<u64>,
    pub document_digest: Digest,
}

/// The bytes a signature covers: a domain tag, the kind, the key ID, and the canonical body, each
/// separated by a zero byte, so a signature for one kind or key never verifies for another.
pub fn signed_bytes(kind: MessageKind, key_id: &str, body: &Value) -> Vec<u8> {
    let mut out = b"mandate-cp/1\0".to_vec();
    out.extend_from_slice(kind.as_str().as_bytes());
    out.push(0);
    out.extend_from_slice(key_id.as_bytes());
    out.push(0);
    out.extend_from_slice(&to_canonical(body));
    out
}

/// Which key kinds may sign each instruction (design §3.8's "Signed by" column, with a catalog
/// entry's kind choosing between the release and registry keys).
fn signer(kind: MessageKind, body: &Value) -> Option<KeyKind> {
    match kind {
        MessageKind::License => Some(KeyKind::License),
        MessageKind::ReleaseManifest | MessageKind::ReleaseWithdrawn => Some(KeyKind::Release),
        MessageKind::DataBundle => Some(KeyKind::Data),
        MessageKind::CatalogEntry => match body.get("kind").and_then(Value::as_str) {
            Some("model_registry_entry") => Some(KeyKind::Registry),
            _ => Some(KeyKind::Release),
        },
        MessageKind::Unknown => None,
    }
}

/// Checks `bytes`, one message as received, for `site` against `keys`.
pub(crate) fn check(bytes: &[u8], _keys: &PinnedKeys, _site: &Site) -> Result<Verified, Refusal> {
    Err(Refusal {
        message: MessageKind::Unknown,
        reason: RefusalReason::Malformed,
        sequence: None,
        document_digest: Digest::of(bytes),
    })
}

/// Lowercase hex, two characters a byte.
fn hex(text: &str) -> Option<Vec<u8>> {
    let (pairs, rest) = text.as_bytes().as_chunks::<2>();
    if !rest.is_empty() {
        return None;
    }
    pairs
        .iter()
        .map(|[hi, lo]| {
            let digit = |c: u8| match c {
                b'0'..=b'9' => c.checked_sub(b'0'),
                b'a'..=b'f' => c.checked_sub(b'a').and_then(|v| v.checked_add(10)),
                _ => None,
            };
            digit(*hi)?.checked_mul(16)?.checked_add(digit(*lo)?)
        })
        .collect()
}
