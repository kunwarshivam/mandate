//! The build digest: the SHA-256 of an executable's bytes (DEC-970 item 4).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use mandate_canon::Digest;

/// The SHA-256 of an executable's bytes, the digest the release's `SHA256SUMS` lists for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BuildDigest(Digest);

/// Why a build digest could not be taken or read. It names no path and no file content, only the
/// kind of failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReleaseError {
    /// The executable could not be read.
    #[error("the executable could not be read: {0:?}")]
    Unreadable(io::ErrorKind),
    /// A reference that is not `sha256:` followed by 64 lowercase hex digits.
    #[error("not a build digest reference")]
    NotAReference,
}

/// The prefix of the `sha256:<hex>` content reference (journal spec §3).
const PREFIX: &str = "sha256:";

impl BuildDigest {
    /// The digest of `bytes`.
    pub fn of_bytes(bytes: &[u8]) -> Self {
        Self(Digest::of(bytes))
    }

    /// The digest of the file at `path`.
    pub fn of_file(path: &Path) -> Result<Self, ReleaseError> {
        fs::read(path)
            .map(|bytes| Self::of_bytes(&bytes))
            .map_err(|error| ReleaseError::Unreadable(error.kind()))
    }

    /// The digest of the executable this process is running. On Linux it reads `/proc/self/exe`,
    /// which names the running file even after the path it was started from is replaced, so a
    /// release installed over a running one cannot make it journal the new release's digest.
    pub fn running() -> Result<Self, ReleaseError> {
        Self::of_file(&running_executable()?)
    }

    /// Reads a `sha256:<hex>` reference, the form [`BuildDigest::reference`] writes.
    pub fn from_reference(reference: &str) -> Result<Self, ReleaseError> {
        reference
            .strip_prefix(PREFIX)
            .and_then(Digest::from_hex)
            .map(Self)
            .ok_or(ReleaseError::NotAReference)
    }

    /// The 64 lowercase hex digits, as `SHA256SUMS` writes them.
    pub fn to_hex(&self) -> String {
        self.0.to_hex()
    }

    /// The `sha256:<hex>` content reference the envelope's `actor.build` member holds.
    pub fn reference(&self) -> String {
        format!("{PREFIX}{}", self.0.to_hex())
    }
}

/// The file this process runs: `/proc/self/exe` on Linux, elsewhere the path it was started from.
pub(crate) fn running_executable() -> Result<PathBuf, ReleaseError> {
    if cfg!(target_os = "linux") {
        return Ok(PathBuf::from("/proc/self/exe"));
    }
    std::env::current_exe().map_err(|error| ReleaseError::Unreadable(error.kind()))
}
