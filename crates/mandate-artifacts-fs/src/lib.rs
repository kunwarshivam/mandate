#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]
//! Filesystem backend for the content-addressed artifact store (journal spec §6.3, backlog E5-2,
//! DEC-107). The pure core (`mandate-journal`) defines references, the store traits, and the
//! checked read; this crate keeps the filesystem I/O out of it (ADR-0001 ES-02).
//!
//! Layout, part of the on-disk format: `<root>/sha256/<first two hex>/<64 hex>` holds the object
//! whose SHA-256 is that hex, read-only once written; `<root>/tmp/` holds writes in progress.
//!
//! A put writes a temporary file, flushes it to disk, and hard-links it into place, which fails
//! rather than replace an existing object. Readers therefore see a whole object or none, a crash
//! leaves at most a temporary file, and concurrent puts of the same bytes all succeed with one
//! object. Durability relies on POSIX directory `fsync`.

use std::path::PathBuf;

use mandate_journal::{ArtifactError, ArtifactRef, ArtifactSource, ArtifactStore};

/// An artifact store rooted at a directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsArtifactStore {
    root: PathBuf,
}

impl FsArtifactStore {
    /// Opens the store at `root`, creating it and its object and temporary directories if they are
    /// missing; `Unavailable` if they cannot be created.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, ArtifactError> {
        Ok(Self { root: root.into() })
    }

    /// Where the object for `reference` lives.
    pub fn object_path(&self, reference: &ArtifactRef) -> PathBuf {
        let _ = reference;
        self.root.clone()
    }
}

impl ArtifactSource for FsArtifactStore {
    fn read_artifact(&self, reference: &ArtifactRef) -> Result<Vec<u8>, ArtifactError> {
        let _ = reference;
        Err(ArtifactError::Unavailable)
    }
}

/// A temporary file that cannot be removed after the link is left behind: it is never read, and
/// the put's outcome is the link's.
impl ArtifactStore for FsArtifactStore {
    fn put_artifact(&mut self, bytes: &[u8]) -> Result<ArtifactRef, ArtifactError> {
        let _ = bytes;
        Err(ArtifactError::Unavailable)
    }
}
