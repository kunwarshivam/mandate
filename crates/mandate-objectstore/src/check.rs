//! The start-up lock check, and the writer that only a passed check can build.
//!
//! [`check_s3`] reads the bucket's settings and runs the probe with the writer's own credential:
//! it keeps one probe object at `<prefix>_mandate/lockcheck`, writing a new version only when there
//! is none, or the latest is not under compliance retention, or that retention has lapsed, so a
//! restart adds nothing that stays locked for six years;
//! then it tries, with that credential, to move the probe's retain-until date earlier, to delete
//! the probe's version with the governance bypass, and to write the bucket's own lock
//! configuration back. [`crate::lock::judge_s3`] judges what it found. Only an accepted judgement
//! yields a [`LockCertificate`], and only a certificate builds a [`LockedWriter`], so the cold
//! exporter and the artifact writer cannot write to a store the check has not passed.
//!
//! [`check_r2`](crate::r2::check_r2) reads R2's bucket lock rules through Cloudflare's API and judges them with
//! [`crate::lock::judge_r2`], which refuses them for §6.2 (DEC-973).

use jiff::{SignedDuration, Timestamp};

use crate::lock::{LockMode, ProbeEvidence, Refused, S3Evidence, judge_s3};
use crate::s3::{Outcome, Rejected, S3, S3Error};

const PROBE_BODY: &[u8] = b"mandate lock check probe\n";
const DAY_HOURS: i64 = 24;

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
}

/// Proof that the lock check passed for one bucket and prefix. It has no public constructor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockCertificate {
    base: String,
    bucket: String,
    prefix: String,
}

impl LockCertificate {
    /// The prefix the check covered.
    pub fn prefix(&self) -> &str {
        &self.prefix
    }
}

/// Gathers the evidence for objects under `prefix`, with `store`'s credential as the writer's.
pub async fn gather_s3(store: &S3, prefix: &str) -> Result<S3Evidence, S3Error> {
    let (bucket, configuration) = store.bucket_lock().await?;
    let key = format!("{prefix}{PROBE_KEY}");
    let now = store.now();
    let existing = store.head_object(&key, None).await?;
    let head = match existing {
        Some(head) if still_locked(head.retention, now) => head,
        _ => {
            write_probe(store, &key, now).await?;
            store
                .head_object(&key, None)
                .await?
                .ok_or(S3Error::Malformed { call: "HeadObject" })?
        }
    };
    let version = head.version_id.as_deref().unwrap_or("null");
    let retention = store.get_object_retention(&key, version).await?;
    let retained_secs = retention.and_then(|(_, until)| {
        u64::try_from(until.duration_since(head.last_modified).as_secs()).ok()
    });
    let earlier = now
        .checked_add(SignedDuration::from_hours(DAY_HOURS))
        .map_err(|_| S3Error::Malformed { call: "the clock" })?;
    let shorten = store
        .put_object_retention(&key, version, LockMode::Compliance, earlier)
        .await?;
    let delete = store.delete_object_version(&key, version, true).await?;
    let rule_write = match &configuration {
        Some(configuration) => store.put_lock_configuration(&configuration.xml).await?,
        None => Outcome::Rejected(Rejected {
            status: 0,
            code: "no lock configuration to write back".to_owned(),
        }),
    };
    Ok(S3Evidence {
        bucket,
        probe: ProbeEvidence {
            mode: retention.map(|(mode, _)| mode),
            retained_secs,
            shorten_refused: shorten != Outcome::Done,
            delete_refused: delete != Outcome::Done,
            rule_write_refused: rule_write != Outcome::Done,
        },
    })
}

/// Whether a probe version's retention still holds: compliance mode, until a time after `now`.
fn still_locked(retention: Option<(LockMode, Timestamp)>, now: Timestamp) -> bool {
    retention.is_some_and(|(mode, until)| mode == LockMode::Compliance && until > now)
}

/// Writes a new probe version with compliance retention, or, on a store that refuses the lock
/// headers, without them, so the rest of the probe still runs and reports what the store allows.
async fn write_probe(store: &S3, key: &str, now: Timestamp) -> Result<(), S3Error> {
    let until = retain_until(now)?;
    let locked = store
        .put_object(key, PROBE_BODY, Some((LockMode::Compliance, until)))
        .await?;
    if locked.is_err() {
        store
            .put_object(key, PROBE_BODY, None)
            .await?
            .map_err(|Rejected { status, code }| S3Error::Rejected {
                call: "PutObject",
                status,
                code,
            })?;
    }
    Ok(())
}

fn retain_until(now: Timestamp) -> Result<Timestamp, S3Error> {
    now.checked_add(SignedDuration::from_hours(
        i64::from(WRITE_RETENTION_DAYS).saturating_mul(DAY_HOURS),
    ))
    .map_err(|_| S3Error::Malformed { call: "the clock" })
}

/// Runs the check for objects under `prefix`; a certificate only when the store is at least as
/// strong as §6.2.
pub async fn check_s3(store: &S3, prefix: &str) -> Result<LockCertificate, CheckError> {
    let evidence = gather_s3(store, prefix).await?;
    judge_s3(&evidence)?;
    Ok(certify(store, prefix))
}

pub(crate) fn certify(store: &S3, prefix: &str) -> LockCertificate {
    LockCertificate {
        base: store.endpoint().base().to_owned(),
        bucket: store.endpoint().bucket().to_owned(),
        prefix: prefix.to_owned(),
    }
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
        let endpoint = store.endpoint();
        if endpoint.base() != certificate.base || endpoint.bucket() != certificate.bucket {
            return Err(WriteError::CertificateMismatch);
        }
        Ok(Self { store, certificate })
    }

    /// Writes `body` at the certified prefix followed by `key`; the new version's identifier.
    pub async fn put(&self, key: &str, body: &[u8]) -> Result<Option<String>, WriteError> {
        let until = retain_until(self.store.now())?;
        let full_key = format!("{}{key}", self.certificate.prefix);
        self.store
            .put_object(&full_key, body, Some((LockMode::Compliance, until)))
            .await?
            .map_err(WriteError::Rejected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_probe_is_reused_only_while_its_compliance_retention_holds() {
        let now = Timestamp::from_second(1_800_000_000).unwrap_or(Timestamp::UNIX_EPOCH);
        let after = Timestamp::from_second(1_800_000_001).unwrap_or(Timestamp::UNIX_EPOCH);
        let before = Timestamp::from_second(1_799_999_999).unwrap_or(Timestamp::UNIX_EPOCH);
        assert!(still_locked(Some((LockMode::Compliance, after)), now));
        assert!(!still_locked(Some((LockMode::Compliance, now)), now));
        assert!(!still_locked(Some((LockMode::Compliance, before)), now));
        assert!(!still_locked(Some((LockMode::Governance, after)), now));
        assert!(!still_locked(None, now));
    }
}
