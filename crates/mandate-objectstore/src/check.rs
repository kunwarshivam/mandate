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

use crate::lock::{
    LockMode, ProbeEvidence, REQUIRED_RETENTION_SECS, Refused, S3Evidence, judge_s3,
};
use crate::s3::{ObjectHead, Outcome, Rejected, S3, S3Error, is_plain_key};

const PROBE_BODY: &[u8] = b"mandate lock check probe\n";
const DAY_HOURS: i64 = 24;

/// The probe object's key, under the checked prefix.
pub const PROBE_KEY: &str = "_mandate/lockcheck";

const RESERVED_SEGMENT: &str = "_mandate";

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
    if !is_prefix(prefix) {
        return Err(S3Error::InvalidKey);
    }
    let (bucket, configuration) = store.bucket_lock().await?;
    let key = format!("{prefix}{PROBE_KEY}");
    let now = store.now();
    let existing = store.head_object(&key, None).await?;
    let head = match existing {
        Some(head) if still_locked(&head, now) => head,
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
    let rule_write_refused = match &configuration {
        Some(configuration) => permission_refused(
            store.put_lock_configuration(&configuration.xml).await?,
            "PutObjectLockConfiguration",
        )?,
        None => true,
    };
    Ok(S3Evidence {
        bucket,
        probe: ProbeEvidence {
            mode: retention.map(|(mode, _)| mode),
            retained_secs,
            shorten_refused: lock_refused(shorten, "PutObjectRetention")?,
            delete_refused: lock_refused(delete, "DeleteObject")?,
            rule_write_refused,
        },
    })
}

/// Whether a prefix is one the check and the writer accept: a plain key ending in `/`.
fn is_prefix(prefix: &str) -> bool {
    prefix.strip_suffix('/').is_some_and(is_plain_key)
}

/// Whether a write the lock should stop was stopped by the lock or a permission: `AccessDenied`
/// (403, as S3 answers both), or the 400 `InvalidRequest` and 409 `ObjectLocked` stores give for a
/// locked version. Done is `false`; any other rejection, a malformed request, a missing version or
/// an unimplemented call, is no evidence, and the check fails closed on it.
fn lock_refused(outcome: Outcome, call: &'static str) -> Result<bool, S3Error> {
    match outcome {
        Outcome::Done => Ok(false),
        Outcome::Rejected(rejected) => {
            let enforced = rejected.status == 403
                || (rejected.status == 400 && rejected.code == "InvalidRequest")
                || (rejected.status == 409 && rejected.code == "ObjectLocked");
            if enforced {
                Ok(true)
            } else {
                Err(unusable(rejected, call))
            }
        }
    }
}

/// Whether a write only a permission should stop was stopped by one: 403 only.
fn permission_refused(outcome: Outcome, call: &'static str) -> Result<bool, S3Error> {
    match outcome {
        Outcome::Done => Ok(false),
        Outcome::Rejected(rejected) if rejected.status == 403 => Ok(true),
        Outcome::Rejected(rejected) => Err(unusable(rejected, call)),
    }
}

fn unusable(Rejected { status, code }: Rejected, call: &'static str) -> S3Error {
    S3Error::Rejected { call, status, code }
}

/// Whether a probe version may be reused: compliance mode, retained at least six years from its
/// creation, and until a time after `now`.
fn still_locked(head: &ObjectHead, now: Timestamp) -> bool {
    head.retention.is_some_and(|(mode, until)| {
        mode == LockMode::Compliance
            && until > now
            && u64::try_from(until.duration_since(head.last_modified).as_secs())
                .is_ok_and(|secs| secs >= REQUIRED_RETENTION_SECS)
    })
}

/// Writes a new probe version with compliance retention, or, on a store that refuses the lock
/// headers (400, or 501 for a store that does not implement them), without them, so the rest of
/// the probe still runs and reports what the store allows. Any other rejection is an error.
async fn write_probe(store: &S3, key: &str, now: Timestamp) -> Result<(), S3Error> {
    let until = retain_until(now)?;
    let locked = store
        .put_object(key, PROBE_BODY, Some((LockMode::Compliance, until)))
        .await?;
    if let Err(rejected) = locked {
        if rejected.status != 400 && rejected.status != 501 {
            return Err(unusable(rejected, "PutObject"));
        }
        store
            .put_object(key, PROBE_BODY, None)
            .await?
            .map_err(|rejected| unusable(rejected, "PutObject"))?;
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
/// `prefix` must be a plain key ending in `/` ([`crate::s3::is_plain_key`]).
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
    /// The key is in the `_mandate/` namespace the check keeps for its probe.
    #[error("the key is reserved for the lock check")]
    ReservedKey,
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
    /// Keys are plain relative keys ([`crate::s3::is_plain_key`]); the `_mandate/` namespace is
    /// the check's.
    pub async fn put(&self, key: &str, body: &[u8]) -> Result<Option<String>, WriteError> {
        if key.split('/').next() == Some(RESERVED_SEGMENT) {
            return Err(WriteError::ReservedKey);
        }
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

    fn at(second: i64) -> Timestamp {
        Timestamp::from_second(second).unwrap_or(Timestamp::UNIX_EPOCH)
    }

    fn head(mode: LockMode, created: i64, until: i64) -> ObjectHead {
        ObjectHead {
            version_id: None,
            last_modified: at(created),
            retention: Some((mode, at(until))),
        }
    }

    #[test]
    fn a_probe_is_reused_only_while_its_compliance_retention_holds() {
        let six = i64::try_from(REQUIRED_RETENTION_SECS).unwrap_or(i64::MAX);
        let created = 1_700_000_000;
        let now = 1_800_000_000;
        let until = created + six;
        assert!(still_locked(
            &head(LockMode::Compliance, created, until),
            at(now)
        ));
        assert!(!still_locked(
            &head(LockMode::Compliance, created, until - 1),
            at(now)
        ));
        assert!(!still_locked(
            &head(LockMode::Compliance, created, until),
            at(until)
        ));
        assert!(still_locked(
            &head(LockMode::Compliance, created, until),
            at(until - 1)
        ));
        assert!(!still_locked(
            &head(LockMode::Governance, created, until),
            at(now)
        ));
        let mut none = head(LockMode::Compliance, created, until);
        none.retention = None;
        assert!(!still_locked(&none, at(now)));
        assert!(!still_locked(
            &head(LockMode::Compliance, until, created),
            at(0)
        ));
    }

    fn rejected(status: u16, code: &str) -> Outcome {
        Outcome::Rejected(Rejected {
            status,
            code: code.to_owned(),
        })
    }

    #[test]
    fn only_a_lock_or_a_permission_counts_as_refused() {
        assert_eq!(lock_refused(Outcome::Done, "c"), Ok(false));
        for (status, code) in [
            (403, "AccessDenied"),
            (400, "InvalidRequest"),
            (409, "ObjectLocked"),
        ] {
            assert_eq!(
                lock_refused(rejected(status, code), "c"),
                Ok(true),
                "{status} {code}"
            );
        }
        for (status, code) in [
            (400, "MalformedXML"),
            (400, "InvalidArgument"),
            (404, "NoSuchVersion"),
            (404, "NoSuchKey"),
            (409, "InvalidRequest"),
            (411, "MissingContentLength"),
            (501, "NotImplemented"),
        ] {
            assert_eq!(
                lock_refused(rejected(status, code), "c"),
                Err(S3Error::Rejected {
                    call: "c",
                    status,
                    code: code.to_owned()
                }),
                "{status} {code}"
            );
        }
        assert_eq!(permission_refused(Outcome::Done, "c"), Ok(false));
        assert_eq!(
            permission_refused(rejected(403, "AccessDenied"), "c"),
            Ok(true)
        );
        for (status, code) in [
            (400, "MalformedXML"),
            (400, "InvalidRequest"),
            (501, "NotImplemented"),
        ] {
            assert!(
                permission_refused(rejected(status, code), "c").is_err(),
                "{status} {code}"
            );
        }
    }

    #[test]
    fn a_prefix_is_a_plain_key_ending_in_a_slash() {
        for good in ["cold/", "cold/segments/", "a/"] {
            assert!(is_prefix(good), "{good}");
        }
        for bad in [
            "", "/", "cold", "/cold/", "../", "cold/../", "cold//", "./cold/", "co\nld/",
        ] {
            assert!(!is_prefix(bad), "{bad:?}");
        }
    }
}
