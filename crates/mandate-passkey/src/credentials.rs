//! A workspace's passkeys, folded from its control stream (journal spec §9.31, [DEC-2108] items 2
//! and 8, [DEC-2121]).
//!
//! [`credential_record`] reads one control-stream envelope into a [`CredentialRecord`], or `None`
//! for a record §9.31 does not add. [`CredentialFold`] folds them in stream order. A member's
//! passkeys are the credentials they enrolled and have not removed, in enrolment order; a
//! credential id, once enrolled, is never free again, so [`CredentialFold::admits`] refuses it.
//!
//! Stream rule 251 is the writer's, and the fold does not trust it ([DEC-2121] item 2). A record
//! that breaks it breaks its own member and, when it names a credential another member enrolled,
//! that member too. A broken member has no passkey that counts, from then on: every read of
//! theirs answers [`CredentialError::Broken`], so their step-up is `step_up_missing` and never passes (§9.31).
//!
//! The fold reads no clock and judges no cool-off: `cool_off_ends` is carried for the verifier
//! and the list route. It holds no public key either, only the key's artifact reference, which
//! the verifier resolves. The counter is the enrolment's until `StepUpAsserted` (journal spec
//! §9.32) is registered.
//!
//! [DEC-2108]: ../../../docs/project/decisions/DEC-2108.md
//! [DEC-2121]: ../../../docs/project/decisions/DEC-2121.md

use mandate_canon::Value;
use mandate_identity::PrincipalId;
use mandate_time::UtcNanos;

/// A key's algorithm, §9.31's `passkey_alg` ([DEC-660]).
///
/// [DEC-660]: ../../../docs/project/decisions/DEC-660.md
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasskeyAlg {
    /// `es256`: ECDSA over P-256 with SHA-256.
    Es256,
    /// `eddsa`: EdDSA over Ed25519.
    EdDsa,
}

/// A `CredentialEnrolled`, as the fold keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enrolment {
    /// The record's `seq` on the control stream.
    pub seq: u64,
    /// The member's principal.
    pub member: PrincipalId,
    /// The credential id, unpadded base64url, as the record carries it.
    pub credential: String,
    /// The COSE public key's artifact reference.
    pub public_key: String,
    /// The key's algorithm.
    pub alg: PasskeyAlg,
    /// The counter the registration carried.
    pub sign_count: u32,
    /// When the credential may first step up, or `None` for no cool-off.
    pub cool_off_ends: Option<UtcNanos>,
}

/// A `CredentialRemoved`, as the fold reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removal {
    /// The record's `seq` on the control stream.
    pub seq: u64,
    /// The member who removed it.
    pub member: PrincipalId,
    /// The credential removed.
    pub credential: String,
}

/// One of §9.31's two records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialRecord {
    /// A `CredentialEnrolled`.
    Enrolled(Enrolment),
    /// A `CredentialRemoved`.
    Removed(Removal),
}

/// The story whose tests are pending on this module's stubs (DEC-77).
pub const STORY: &str = "E9-4";

/// Why a credential read gives no answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CredentialError {
    /// A §9.31 record that does not map: the envelope member at fault, by its path, such as
    /// `payload.credential` or `schema_version`.
    #[error("the credential record at seq {seq} does not map at {member}")]
    Unreadable { seq: u64, member: &'static str },
    /// The member's records break stream rule 251 (journal spec §9.31), first at `seq`: none of
    /// their passkeys counts.
    #[error("rule 251 breaks at seq {seq}")]
    Broken { seq: u64 },
    /// The record offered for append would break rule 251.
    #[error("the record would break rule 251")]
    Breaks,
    /// The stub's own report until its story lands (DEC-77).
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
}

/// The §9.31 record in the control-stream `envelope` at `seq`, or `None` for any other event
/// type. A `CredentialEnrolled` or `CredentialRemoved` maps whole at schema version 1, or not at
/// all.
///
/// # Errors
/// [`CredentialError::Unreadable`] for a §9.31 record at another schema version, or with a member
/// that is absent or not of its type, and at `event_type` for a `StepUpAsserted`, which the fold
/// does not read yet.
pub fn credential_record(
    seq: u64,
    envelope: &Value,
) -> Result<Option<CredentialRecord>, CredentialError> {
    let _ = (seq, envelope);
    Err(CredentialError::Unimplemented { story: STORY })
}

/// The passkeys of one control stream: §9.31's records, in stream order, folded on each read.
#[derive(Debug, Clone, Default)]
pub struct CredentialFold {
    records: Vec<CredentialRecord>,
}

impl CredentialFold {
    /// The fold of `records`, which are in stream order.
    pub fn new(records: impl IntoIterator<Item = CredentialRecord>) -> Self {
        Self {
            records: records.into_iter().collect(),
        }
    }

    /// `member`'s passkeys: enrolled and not removed, in enrolment order. A member with no record
    /// holds none.
    ///
    /// # Errors
    /// [`CredentialError::Broken`] when a record breaks rule 251 for `member`.
    pub fn passkeys(&self, member: PrincipalId) -> Result<Vec<&Enrolment>, CredentialError> {
        let _ = (&self.records, member);
        Err(CredentialError::Unimplemented { story: STORY })
    }

    /// The enrolment of `credential` while it is held, enrolled and not removed, or `None`. Who
    /// holds it is the enrolment's `member`.
    ///
    /// # Errors
    /// [`CredentialError::Broken`] when the credential's member is broken, held or not.
    pub fn held(&self, credential: &str) -> Result<Option<&Enrolment>, CredentialError> {
        let _ = credential;
        Err(CredentialError::Unimplemented { story: STORY })
    }

    /// Whether appending `next` after the folded records keeps rule 251: the writer's check before
    /// it appends. An id ever enrolled on the stream, removed or not, is never enrolled again.
    ///
    /// # Errors
    /// [`CredentialError::Breaks`] when `next` would break rule 251.
    pub fn admits(&self, next: &CredentialRecord) -> Result<(), CredentialError> {
        let _ = next;
        Err(CredentialError::Unimplemented { story: STORY })
    }
}
