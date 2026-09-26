//! Content-addressed artifacts (journal spec §6.3, backlog E5-2). Large items (prompts and
//! responses, data snapshots, reports, configuration objects) are stored under `sha256:{hex}` of
//! their stored bytes, events carry only that reference in `artifact_refs`, and every checked read
//! re-hashes, so the journal stays small and each artifact is as verifiable as the event naming it.
//!
//! Backends implement [`ArtifactSource`] and [`ArtifactStore`]: the map here for pure code and
//! tests, and `mandate-artifacts-fs` on disk (DEC-107).

use std::collections::BTreeMap;
use std::fmt;

use mandate_canon::Digest;

use crate::schema::parse_digest_ref;

/// `sha256:{hex}` of an artifact's stored bytes: its address and its integrity check at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArtifactRef(Digest);

impl ArtifactRef {
    /// The reference `bytes` are stored under.
    pub fn of(bytes: &[u8]) -> Self {
        let _ = bytes;
        Self(Digest::ZERO)
    }

    pub fn from_digest(digest: Digest) -> Self {
        Self(digest)
    }

    /// Accepts exactly `sha256:` and 64 lowercase hex characters, the form `artifact_refs` holds.
    pub fn parse(text: &str) -> Option<Self> {
        parse_digest_ref(text).map(Self)
    }

    pub fn digest(&self) -> Digest {
        self.0
    }
}

impl fmt::Display for ArtifactRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "sha256:")
    }
}

/// Why an artifact could not be stored or read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ArtifactError {
    #[error("no artifact is stored under this reference")]
    Missing,
    /// The stored bytes no longer hash to their reference. The object is left in place as evidence;
    /// nothing is repaired in place (journal spec §11).
    #[error("the stored bytes do not hash to their reference")]
    Corrupt,
    /// Storage backends only: the store could not be read or written; retry with the same bytes.
    #[error("the artifact store could not be read or written")]
    Unavailable,
}

impl ArtifactError {
    /// Stable reason code (ADR-0001 ES-09); the first two are the verification codes of spec §11.
    pub fn code(self) -> &'static str {
        match self {
            Self::Missing | Self::Corrupt | Self::Unavailable => "",
        }
    }
}

/// Where artifact bytes are read from, by reference.
pub trait ArtifactSource {
    /// The bytes stored under `reference` exactly as stored, not yet re-hashed. Callers that act on
    /// the bytes use [`get_artifact`], which checks them.
    fn read_artifact(&self, reference: &ArtifactRef) -> Result<Vec<u8>, ArtifactError>;
}

/// Where artifact bytes are written: write-once and addressed by content.
pub trait ArtifactStore: ArtifactSource {
    /// Stores `bytes` under [`ArtifactRef::of`]`(bytes)` and returns that reference. Putting bytes
    /// that are already stored changes nothing. An object is never overwritten: if the one already
    /// under the reference does not re-hash, this returns `Corrupt` and leaves it untouched.
    fn put_artifact(&mut self, bytes: &[u8]) -> Result<ArtifactRef, ArtifactError>;
}

/// The checked read: the bytes stored under `reference` if they hash to it, `Corrupt` if not.
pub fn get_artifact<S: ArtifactSource + ?Sized>(
    source: &S,
    reference: &ArtifactRef,
) -> Result<Vec<u8>, ArtifactError> {
    let _ = (source, reference);
    Err(ArtifactError::Unavailable)
}

/// `Corrupt` unless `bytes` hash to `reference`.
pub fn check_artifact(reference: &ArtifactRef, bytes: &[u8]) -> Result<(), ArtifactError> {
    let _ = (reference, bytes);
    Err(ArtifactError::Unavailable)
}

/// An in-memory store keyed by digest. Values can be replaced through the map itself, which is how
/// tests plant corrupt objects.
impl ArtifactSource for BTreeMap<Digest, Vec<u8>> {
    fn read_artifact(&self, reference: &ArtifactRef) -> Result<Vec<u8>, ArtifactError> {
        self.get(&reference.digest())
            .cloned()
            .ok_or(ArtifactError::Missing)
    }
}

impl ArtifactStore for BTreeMap<Digest, Vec<u8>> {
    fn put_artifact(&mut self, bytes: &[u8]) -> Result<ArtifactRef, ArtifactError> {
        let _ = bytes;
        Err(ArtifactError::Unavailable)
    }
}
