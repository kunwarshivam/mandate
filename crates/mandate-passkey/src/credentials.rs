//! A workspace's passkeys, folded from its control stream (journal spec §9.31, [DEC-2108] items 2
//! and 8, [DEC-2121]).
//!
//! [`credential_record`] reads one control-stream envelope into a [`CredentialRecord`], or `None`
//! for a record §9.31 does not add. [`CredentialFold`] folds them in stream order. A member's
//! passkeys are the credentials they enrolled and have not removed, in enrolment order; a
//! credential id, once enrolled, is never free again, so [`CredentialFold::admits`] refuses it.
//!
//! Two readers, two views ([DEC-2121] items 2 to 4):
//!
//! - **Step-up** reads [`CredentialFold::passkeys`], which fails closed. Stream rule 251 is the
//!   writer's, and the fold does not trust it. A record that breaks it breaks its own member and,
//!   when it names a credential another member enrolled, that member too. From then on a broken
//!   member's passkeys answer [`CredentialError::Broken`], so their step-up is `step_up_missing`
//!   and never passes (§9.31). Until the fold reads `StepUpAsserted` (journal spec §9.32, planned),
//!   a stream that holds one answers [`CredentialError::AssertionUnread`]: the enrolment's counter
//!   is never taken for the last.
//! - **The reduction-only opening** (route 2, DEC-2117) reads [`CredentialFold::held`], which never
//!   refuses: risk reduction is never denied (`AGENTS.md` rule 13). It replays only the records
//!   that keep rule 251, so the first enrolment of an id wins, and it reads no counter, so a
//!   `StepUpAsserted` changes nothing.
//!
//! The fold reads no clock and judges no cool-off: `cool_off_ends` is carried for the verifier
//! and the list route. It holds no public key either, only the key's artifact reference, which
//! the verifier resolves.
//!
//! [DEC-2108]: ../../../docs/project/decisions/DEC-2108.md
//! [DEC-2121]: ../../../docs/project/decisions/DEC-2121.md

use std::collections::BTreeMap;

use mandate_canon::{Value, decode_ulid};
use mandate_identity::PrincipalId;
use mandate_time::UtcNanos;

/// §9.31's two record types.
const ENROLLED: &str = "CredentialEnrolled";
const REMOVED: &str = "CredentialRemoved";
/// §9.32's record of a later counter (planned), which the fold maps but does not read yet
/// (DEC-2121 item 4).
const ASSERTED: &str = "StepUpAsserted";

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

/// One of §9.31's two records, or a §9.32 `StepUpAsserted` the fold does not read yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialRecord {
    /// A `CredentialEnrolled`.
    Enrolled(Enrolment),
    /// A `CredentialRemoved`.
    Removed(Removal),
    /// A `StepUpAsserted` at this `seq`, kept unread so step-up fails closed on it.
    Asserted { seq: u64 },
}

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
    /// The stream holds a `StepUpAsserted` at `seq`, whose counter the fold does not read yet,
    /// so no passkey's last counter is known.
    #[error("the step-up assertion at seq {seq} is not read yet")]
    AssertionUnread { seq: u64 },
}

/// The §9.31 record in the control-stream `envelope` at `seq`, a `StepUpAsserted` as
/// [`CredentialRecord::Asserted`], or `None` for any other event type. A `CredentialEnrolled` or
/// `CredentialRemoved` maps whole at schema version 1, or not at all.
///
/// # Errors
/// [`CredentialError::Unreadable`] for a §9.31 record at another schema version, or with a member
/// that is absent or not of its type.
pub fn credential_record(
    seq: u64,
    envelope: &Value,
) -> Result<Option<CredentialRecord>, CredentialError> {
    let at = |member| CredentialError::Unreadable { seq, member };
    let event_type = envelope
        .get("event_type")
        .and_then(Value::as_str)
        .ok_or(at("event_type"))?;
    if event_type == ASSERTED {
        return Ok(Some(CredentialRecord::Asserted { seq }));
    }
    if event_type != ENROLLED && event_type != REMOVED {
        return Ok(None);
    }
    if envelope.get("schema_version").and_then(Value::as_int) != Some(1) {
        return Err(at("schema_version"));
    }
    let payload = envelope.get("payload").ok_or(at("payload"))?;
    let text = |name: &str, path| payload.get(name).and_then(Value::as_str).ok_or(at(path));
    let member = decode_ulid(text("member", "payload.member")?)
        .map(PrincipalId)
        .map_err(|_| at("payload.member"))?;
    let credential = text("credential", "payload.credential")?;
    if !credential_id(credential) {
        return Err(at("payload.credential"));
    }
    let credential = credential.to_owned();
    if event_type == REMOVED {
        return Ok(Some(CredentialRecord::Removed(Removal {
            seq,
            member,
            credential,
        })));
    }
    let public_key = text("public_key", "payload.public_key")?;
    if !artifact_ref(public_key) {
        return Err(at("payload.public_key"));
    }
    let alg = match text("alg", "payload.alg")? {
        "es256" => PasskeyAlg::Es256,
        "eddsa" => PasskeyAlg::EdDsa,
        _ => return Err(at("payload.alg")),
    };
    let sign_count = payload
        .get("sign_count")
        .and_then(Value::as_int)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or(at("payload.sign_count"))?;
    let cool_off_ends = match payload.get("cool_off_ends") {
        Some(Value::Null) => None,
        Some(Value::Str(t)) => Some(UtcNanos::parse(t).map_err(|_| at("payload.cool_off_ends"))?),
        _ => return Err(at("payload.cool_off_ends")),
    };
    Ok(Some(CredentialRecord::Enrolled(Enrolment {
        seq,
        member,
        credential,
        public_key: public_key.to_owned(),
        alg,
        sign_count,
        cool_off_ends,
    })))
}

/// §9.31's `credential_id`: unpadded base64url of 16 to 1023 bytes, so 22 to 1364 characters and
/// never a length whose remainder by 4 is 1.
fn credential_id(text: &str) -> bool {
    (22..=1364).contains(&text.len())
        && text.len() % 4 != 1
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// §9.1's `ref`: `sha256:` and 64 lowercase hex digits.
fn artifact_ref(text: &str) -> bool {
    text.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
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
    /// [`CredentialError::AssertionUnread`] when the stream holds a `StepUpAsserted`, first, and
    /// then [`CredentialError::Broken`] when a record breaks rule 251 for `member`.
    pub fn passkeys(&self, member: PrincipalId) -> Result<Vec<&Enrolment>, CredentialError> {
        let folded = self.fold();
        if let Some(seq) = folded.asserted {
            return Err(CredentialError::AssertionUnread { seq });
        }
        if let Some(seq) = folded.broken.get(&member) {
            return Err(CredentialError::Broken { seq: *seq });
        }
        Ok(folded
            .held
            .into_iter()
            .filter(|e| e.member == member)
            .collect())
    }

    /// The enrolment of `credential` while it is held, or `None`: route 2's view. Only the records
    /// that keep rule 251 count, so the first enrolment of an id wins and a breaking record,
    /// enrolment or removal, changes nothing. A broken member still holds what they hold here, and
    /// a `StepUpAsserted` is skipped. Who holds it is the enrolment's `member`. It has no error to
    /// return: this view never refuses (`AGENTS.md` rule 13).
    pub fn held(&self, credential: &str) -> Option<&Enrolment> {
        self.fold()
            .held
            .into_iter()
            .find(|e| e.credential == credential)
    }

    /// Whether appending `next` after the folded records keeps rule 251: the writer's check before
    /// it appends. An id ever enrolled on the stream, removed or not, is never enrolled again. A
    /// record that keeps the rule is admitted from a broken member too, whom step-up still
    /// discounts, and a `StepUpAsserted` is not a record the rule reads.
    ///
    /// # Errors
    /// [`CredentialError::Breaks`] when `next` would break rule 251.
    pub fn admits(&self, next: &CredentialRecord) -> Result<(), CredentialError> {
        let Some((credential, member)) = next.names() else {
            return Ok(());
        };
        if keeps_rule_251(self.fold().history.get(credential), next, member) {
            Ok(())
        } else {
            Err(CredentialError::Breaks)
        }
    }

    /// The records replayed: each credential's history, the members broken and from which `seq`,
    /// the first `StepUpAsserted`, and the enrolments the records that keep rule 251 leave held,
    /// in enrolment order.
    fn fold(&self) -> Folded<'_> {
        let mut folded = Folded::default();
        for record in &self.records {
            let Some((credential, member)) = record.names() else {
                folded.asserted = folded.asserted.or(Some(record.seq()));
                continue;
            };
            let history = folded.history.get(credential);
            if !keeps_rule_251(history, record, member) {
                let first = history.and_then(|h| h.enroller);
                for broken in std::iter::once(member).chain(first) {
                    folded.broken.entry(broken).or_insert(record.seq());
                }
            } else if let CredentialRecord::Enrolled(enrolment) = record {
                folded.held.push(enrolment);
            } else {
                folded.held.retain(|e| e.credential != credential);
            }
            let entry = folded.history.entry(credential).or_default();
            entry.records = entry.records.saturating_add(1);
            if entry.records == 1 {
                entry.enroller = matches!(record, CredentialRecord::Enrolled(_)).then_some(member);
            }
        }
        folded
    }
}

/// One credential's records so far: how many, and who enrolled it when its first was an
/// enrolment.
#[derive(Debug, Clone, Copy, Default)]
struct History {
    records: u32,
    enroller: Option<PrincipalId>,
}

/// What a replay of the records finds.
#[derive(Debug, Default)]
struct Folded<'a> {
    history: BTreeMap<&'a str, History>,
    broken: BTreeMap<PrincipalId, u64>,
    asserted: Option<u64>,
    held: Vec<&'a Enrolment>,
}

/// Rule 251 for `record` after its credential's `history`: an enrolment must be the credential's
/// first record, and a removal its second, by the member who enrolled it.
fn keeps_rule_251(
    history: Option<&History>,
    record: &CredentialRecord,
    member: PrincipalId,
) -> bool {
    match (record, history) {
        (CredentialRecord::Enrolled(_), None) => true,
        (CredentialRecord::Removed(_), Some(h)) => h.records == 1 && h.enroller == Some(member),
        _ => false,
    }
}

impl CredentialRecord {
    /// The credential the record names, and its member, or `None` for an `Asserted` record.
    fn names(&self) -> Option<(&str, PrincipalId)> {
        match self {
            Self::Enrolled(e) => Some((&e.credential, e.member)),
            Self::Removed(r) => Some((&r.credential, r.member)),
            Self::Asserted { .. } => None,
        }
    }

    /// The record's `seq`.
    fn seq(&self) -> u64 {
        match self {
            Self::Enrolled(e) => e.seq,
            Self::Removed(r) => r.seq,
            Self::Asserted { seq } => *seq,
        }
    }
}
