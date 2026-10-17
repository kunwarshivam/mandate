//! The start-up lock check, and the writer that only a passed check can build.
//!
//! [`check_s3`] reads the bucket's settings and runs the probe with the writer's own credential:
//! it keeps one probe object at `<prefix>_mandate/lockcheck`, writing a new version only when there
//! is none or its retention has lapsed, so a restart adds nothing that stays locked for six years;
//! then it tries, with that credential, to move the probe's retain-until date earlier, to delete
//! the probe's version with the governance bypass, and to write the bucket's own lock
//! configuration back. [`crate::lock::judge_s3`] judges what it found. Only an accepted judgement
//! yields a [`LockCertificate`], and only a certificate builds a [`LockedWriter`], so the cold
//! exporter and the artifact writer cannot write to a store the check has not passed.
//!
//! [`check_r2`] reads R2's bucket lock rules through Cloudflare's API and judges them with
//! [`crate::lock::judge_r2`], which refuses them for §6.2 (DEC-973).

use jiff::Timestamp;

use crate::lock::{LockMode, Refused, S3Evidence};
use crate::s3::{Rejected, S3, S3Error};

/// The probe object's key, under the checked prefix.
pub const PROBE_KEY: &str = "_mandate/lockcheck";

/// How long the writer retains what it writes: the six years §6.2 needs, plus one day so a clock
/// behind the store's, or the truncation of the date to whole seconds, never leaves an object short.
pub const WRITE_RETENTION_DAYS: u32 = crate::lock::REQUIRED_RETENTION_DAYS + 1;

/// Why the check did not pass.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CheckError {
    /// The store is weaker than §6.2.
    #[error(transparent)]
    Refused(#[from] Refused),
    /// The store gave no usable answer; nothing was judged, and nothing may start.
    #[error(transparent)]
    Store(#[from] S3Error),
    /// Not built yet (E21-40).
    #[error("not implemented: {story}")]
    Unimplemented {
        /// The story that builds it.
        story: &'static str,
    },
}

/// Proof that the lock check passed for one bucket and prefix. It has no public constructor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockCertificate {
    base: String,
    bucket: String,
    prefix: String,
    checked_at: Timestamp,
}

impl LockCertificate {
    /// The prefix the check covered.
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// When the check passed.
    pub fn checked_at(&self) -> Timestamp {
        self.checked_at
    }
}

/// Gathers the evidence for objects under `prefix`, with `store`'s credential as the writer's.
pub async fn gather_s3(store: &S3, prefix: &str) -> Result<S3Evidence, S3Error> {
    let _ = (store, prefix);
    Err(S3Error::Transport)
}

/// Runs the check for objects under `prefix`; a certificate only when the store is at least as
/// strong as §6.2.
pub async fn check_s3(store: &S3, prefix: &str) -> Result<LockCertificate, CheckError> {
    let _ = (store, prefix);
    Err(CheckError::Unimplemented { story: "E21-40" })
}

/// Why a locked write failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WriteError {
    /// The certificate is for another endpoint or bucket.
    #[error("the lock certificate is for another store")]
    CertificateMismatch,
    /// The store declined the write.
    #[error("the object store declined the write with {} {}", .0.status, .0.code)]
    Rejected(Rejected),
    /// The store gave no usable answer.
    #[error(transparent)]
    Store(#[from] S3Error),
    /// Not built yet (E21-40).
    #[error("not implemented: {story}")]
    Unimplemented {
        /// The story that builds it.
        story: &'static str,
    },
}

/// Writes objects under the certified prefix, each with its own compliance retention of
/// [`WRITE_RETENTION_DAYS`] from the time it is written.
#[derive(Debug, Clone)]
pub struct LockedWriter {
    store: S3,
    certificate: LockCertificate,
}

impl LockedWriter {
    /// A writer for `store`, which must be the store `certificate` was issued for.
    pub fn new(store: S3, certificate: LockCertificate) -> Result<Self, WriteError> {
        let _ = (store, certificate);
        Err(WriteError::Unimplemented { story: "E21-40" })
    }

    /// Writes `body` at the certified prefix followed by `key`; the new version's identifier.
    pub async fn put(&self, key: &str, body: &[u8]) -> Result<Option<String>, WriteError> {
        let _ = (
            key,
            body,
            &self.store,
            &self.certificate,
            LockMode::Compliance,
        );
        Err(WriteError::Unimplemented { story: "E21-40" })
    }
}
