//! The build digest: the SHA-256 of an executable's bytes (DEC-970 item 4).

use std::io;
use std::path::Path;

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
    /// The tests-first stub (DEC-921): the body lands in the implementation commit.
    #[error("unimplemented ({story})")]
    Unimplemented { story: &'static str },
}

const STORY: &str = "E21-8";

impl BuildDigest {
    /// The digest of `bytes`.
    pub fn of_bytes(bytes: &[u8]) -> Self {
        let _ = bytes;
        Self(Digest::ZERO)
    }

    /// The digest of the file at `path`.
    pub fn of_file(path: &Path) -> Result<Self, ReleaseError> {
        let _ = path;
        Err(ReleaseError::Unimplemented { story: STORY })
    }

    /// The digest of the executable this process is running.
    pub fn running() -> Result<Self, ReleaseError> {
        Err(ReleaseError::Unimplemented { story: STORY })
    }

    /// Reads a `sha256:<hex>` reference, the form [`BuildDigest::reference`] writes.
    pub fn from_reference(reference: &str) -> Result<Self, ReleaseError> {
        let _ = reference;
        Err(ReleaseError::Unimplemented { story: STORY })
    }

    /// The 64 lowercase hex digits, as `SHA256SUMS` writes them.
    pub fn to_hex(&self) -> String {
        String::new()
    }

    /// The `sha256:<hex>` content reference the envelope's `actor.build` member holds.
    pub fn reference(&self) -> String {
        String::new()
    }
}
