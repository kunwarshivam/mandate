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
//!   and never passes (§9.31). It reads `StepUpAsserted` (journal spec §9.32) under stream rule
//!   254, which it does not trust either: an assertion of a credential its own member holds,
//!   with a counter above the last kept one (or 0 over 0) and an unused challenge, raises
//!   [`CredentialFold::last_count`]; any other disables step-up for the asserting member and the
//!   credential's holder (DEC-2323). A version 1 enrolment carries no `backup_eligible`, and the
//!   verifier never counts it (DEC-2122 item 3).
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

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::{Value, decode_ulid};
use mandate_identity::PrincipalId;
use mandate_time::UtcNanos;

/// §9.31's two record types.
const ENROLLED: &str = "CredentialEnrolled";
const REMOVED: &str = "CredentialRemoved";
/// §9.32's record of an assertion's counter, which step-up's view reads under rule 254
/// (DEC-2323).
const ASSERTED: &str = "StepUpAsserted";
/// §9.38's record of a test cell's waived cool-off, which step-up's view reads under rule 318
/// (DEC-2146).
const WAIVED: &str = "StepUpCoolOffWaived";

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
    /// The registration's backup-eligible flag at version 2, or `None` for a version 1 enrolment,
    /// which carries none and which the verifier never counts (DEC-2122 item 3).
    pub backup_eligible: Option<bool>,
}

/// A `StepUpAsserted` (journal spec §9.32), as the fold reads it: whose assertion it was, of which
/// credential, over which challenge, and the counter it carried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assertion {
    /// The record's `seq` on the control stream.
    pub seq: u64,
    /// The member who asserted, the record's actor.
    pub member: PrincipalId,
    /// The credential asserted with.
    pub credential: String,
    /// The step-up challenge the assertion answered, its ULID text.
    pub challenge: String,
    /// The signature counter the assertion carried.
    pub sign_count: u32,
    /// The record's `event_time`, which rule 318's converse judges against the credential's
    /// `cool_off_ends` (journal spec §9.38).
    pub event_time: UtcNanos,
}

/// A `StepUpCoolOffWaived` (journal spec §9.38, [DEC-2146]), as the fold reads it: whose waiver it
/// was, of which credential and challenge, the `cool_off_ends` it waived, and when.
///
/// [DEC-2146]: ../../../docs/project/decisions/DEC-2146.md
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waiver {
    /// The record's `seq` on the control stream.
    pub seq: u64,
    /// The member who stepped up, the record's actor.
    pub member: PrincipalId,
    /// The credential whose cool-off was waived.
    pub credential: String,
    /// The waived step-up's challenge, its ULID text.
    pub challenge: String,
    /// The enrolment's `cool_off_ends` the waiver names.
    pub cool_off_ends: UtcNanos,
    /// The record's `event_time`.
    pub event_time: UtcNanos,
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

/// One of §9.31's two records, or §9.32's `StepUpAsserted`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialRecord {
    /// A `CredentialEnrolled`.
    Enrolled(Enrolment),
    /// A `CredentialRemoved`.
    Removed(Removal),
    /// A `StepUpAsserted`.
    Asserted(Assertion),
    /// A `StepUpCoolOffWaived`.
    Waived(Waiver),
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
        return assertion(seq, envelope).map(Some);
    }
    if event_type == WAIVED {
        return waiver(seq, envelope).map(Some);
    }
    if event_type != ENROLLED && event_type != REMOVED {
        return Ok(None);
    }
    let version_2 = match envelope.get("schema_version").and_then(Value::as_int) {
        Some(1) => false,
        Some(2) if event_type == ENROLLED => true,
        _ => return Err(at("schema_version")),
    };
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
    let backup_eligible = if version_2 {
        match payload.get("backup_eligible") {
            Some(Value::Bool(flag)) => Some(*flag),
            _ => return Err(at("payload.backup_eligible")),
        }
    } else {
        None
    };
    Ok(Some(CredentialRecord::Enrolled(Enrolment {
        seq,
        member,
        credential,
        public_key: public_key.to_owned(),
        alg,
        sign_count,
        cool_off_ends,
        backup_eligible,
    })))
}

/// A `StepUpAsserted` at schema version 1 (journal spec §9.32), its member read from its actor,
/// mapped whole or not at all.
fn assertion(seq: u64, envelope: &Value) -> Result<CredentialRecord, CredentialError> {
    let at = |member| CredentialError::Unreadable { seq, member };
    if envelope.get("schema_version").and_then(Value::as_int) != Some(1) {
        return Err(at("schema_version"));
    }
    let member = envelope
        .get("actor")
        .and_then(|a| a.get("id"))
        .and_then(Value::as_str)
        .and_then(|id| decode_ulid(id).ok())
        .map(PrincipalId)
        .ok_or(at("actor.id"))?;
    let payload = envelope.get("payload").ok_or(at("payload"))?;
    let text = |name: &str, path| payload.get(name).and_then(Value::as_str).ok_or(at(path));
    let challenge = text("challenge", "payload.challenge")?;
    if decode_ulid(challenge).is_err() {
        return Err(at("payload.challenge"));
    }
    let credential = text("credential", "payload.credential")?;
    if !credential_id(credential) {
        return Err(at("payload.credential"));
    }
    let sign_count = payload
        .get("sign_count")
        .and_then(Value::as_int)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or(at("payload.sign_count"))?;
    if !artifact_ref(text("assertion", "payload.assertion")?) {
        return Err(at("payload.assertion"));
    }
    let event_time = envelope
        .get("event_time")
        .and_then(Value::as_str)
        .and_then(|t| UtcNanos::parse(t).ok())
        .ok_or(at("event_time"))?;
    Ok(CredentialRecord::Asserted(Assertion {
        seq,
        member,
        credential: credential.to_owned(),
        challenge: challenge.to_owned(),
        sign_count,
        event_time,
    }))
}

/// A `StepUpCoolOffWaived` at schema version 1 (journal spec §9.38) in the control-stream
/// `envelope` at `seq`, its member read from its actor, mapped whole or not at all.
///
/// # Errors
/// [`CredentialError::Unreadable`] naming the member at fault.
pub fn waiver(seq: u64, envelope: &Value) -> Result<CredentialRecord, CredentialError> {
    let at = |member| CredentialError::Unreadable { seq, member };
    if envelope.get("schema_version").and_then(Value::as_int) != Some(1) {
        return Err(at("schema_version"));
    }
    let member = envelope
        .get("actor")
        .and_then(|a| a.get("id"))
        .and_then(Value::as_str)
        .and_then(|id| decode_ulid(id).ok())
        .map(PrincipalId)
        .ok_or(at("actor.id"))?;
    let payload = envelope.get("payload").ok_or(at("payload"))?;
    let text = |name: &str, path| payload.get(name).and_then(Value::as_str).ok_or(at(path));
    let challenge = text("challenge", "payload.challenge")?;
    if decode_ulid(challenge).is_err() {
        return Err(at("payload.challenge"));
    }
    let credential = text("credential", "payload.credential")?;
    if !credential_id(credential) {
        return Err(at("payload.credential"));
    }
    let cool_off_ends = UtcNanos::parse(text("cool_off_ends", "payload.cool_off_ends")?)
        .map_err(|_| at("payload.cool_off_ends"))?;
    let event_time = envelope
        .get("event_time")
        .and_then(Value::as_str)
        .and_then(|t| UtcNanos::parse(t).ok())
        .ok_or(at("event_time"))?;
    Ok(CredentialRecord::Waived(Waiver {
        seq,
        member,
        credential: credential.to_owned(),
        challenge: challenge.to_owned(),
        cool_off_ends,
        event_time,
    }))
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
    /// [`CredentialError::Broken`] when a record breaks rule 251 or rule 254 for `member`.
    pub fn passkeys(&self, member: PrincipalId) -> Result<Vec<&Enrolment>, CredentialError> {
        let folded = self.fold();
        if let Some(seq) = folded.broken.get(&member) {
            return Err(CredentialError::Broken { seq: *seq });
        }
        Ok(folded
            .held
            .into_iter()
            .filter(|e| e.member == member)
            .collect())
    }

    /// The last counter of `credential`, step-up's view: the greatest an assertion that keeps rule
    /// 254 carried, else the enrolment's; `None` when it is not held.
    ///
    /// # Errors
    /// [`CredentialError::Broken`] when a record breaks rule 251 or rule 254 for its holder.
    pub fn last_count(&self, credential: &str) -> Result<Option<u32>, CredentialError> {
        let folded = self.fold();
        let Some(holder) = folded.holder(credential) else {
            return Ok(None);
        };
        if let Some(seq) = folded.broken.get(&holder) {
            return Err(CredentialError::Broken { seq: *seq });
        }
        Ok(folded.last.get(credential).copied())
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

    /// Whether appending `next` after the folded records keeps its stream rule: the writer's check
    /// before it appends. An enrolment or removal is judged by rule 251: an id ever enrolled on the
    /// stream, removed or not, is never enrolled again. A `StepUpAsserted` is judged by rule 254: a
    /// credential its own member holds, a counter above the last kept one (or 0 over 0), and an
    /// unused challenge. A record that keeps its rule is admitted from a broken member too, whom
    /// step-up still discounts.
    ///
    /// # Errors
    /// [`CredentialError::Breaks`] when `next` would break its stream rule.
    pub fn admits(&self, next: &CredentialRecord) -> Result<(), CredentialError> {
        let folded = self.fold();
        let keeps = match (next, next.names()) {
            (CredentialRecord::Asserted(assertion), _) => folded.keeps_rule_254(assertion),
            (_, Some((credential, member))) => {
                keeps_rule_251(folded.history.get(credential), next, member)
            }
            (_, None) => false,
        };
        if keeps {
            Ok(())
        } else {
            Err(CredentialError::Breaks)
        }
    }

    /// The `seq` of the first record that breaks stream rule 318 (journal spec §9.38), or `None`:
    /// a waiver that names a credential its actor does not hold, a `cool_off_ends` other than that
    /// enrolment's or not after the waiver's `event_time`, or a challenge already waived; or an
    /// assertion inside its credential's cool-off that the next record does not waive.
    ///
    /// # Errors
    /// None: every record the fold holds has already mapped.
    pub fn first_waiver_break(&self) -> Result<Option<u64>, CredentialError> {
        Ok(self.fold().waiver_break)
    }

    /// The records replayed: each credential's history, the members broken and from which `seq`,
    /// the enrolments the records that keep rule 251 leave held, in enrolment order, and each held
    /// credential's last kept counter with the challenges its assertions used.
    fn fold(&self) -> Folded<'_> {
        let mut folded = Folded::default();
        for record in &self.records {
            folded.settle_owed(record);
            match record {
                CredentialRecord::Asserted(assertion) => {
                    folded.assert(assertion);
                    continue;
                }
                CredentialRecord::Waived(waiver) => {
                    folded.waive(waiver);
                    continue;
                }
                CredentialRecord::Enrolled(_) | CredentialRecord::Removed(_) => {}
            }
            let Some((credential, member)) = record.names() else {
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
                folded.last.insert(credential, enrolment.sign_count);
            } else {
                folded.held.retain(|e| e.credential != credential);
                folded.last.remove(credential);
            }
            let entry = folded.history.entry(credential).or_default();
            entry.records = entry.records.saturating_add(1);
            if entry.records == 1 {
                entry.enroller = matches!(record, CredentialRecord::Enrolled(_)).then_some(member);
            }
        }
        if let Some(owed) = folded.owed.take() {
            folded.break_318(owed.member, &owed.credential, owed.seq);
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
    held: Vec<&'a Enrolment>,
    last: BTreeMap<&'a str, u32>,
    challenges: BTreeSet<&'a str>,
    /// Rule 318: the challenges waived so far, the assertion inside its credential's cool-off the
    /// very next record must waive, and the `seq` of the first record that broke the rule.
    waived: BTreeSet<&'a str>,
    owed: Option<&'a Assertion>,
    waiver_break: Option<u64>,
}

impl<'a> Folded<'a> {
    /// Who holds `credential` now, among the records that keep rule 251.
    fn holder(&self, credential: &str) -> Option<PrincipalId> {
        self.held
            .iter()
            .find(|e| e.credential == credential)
            .map(|e| e.member)
    }

    /// Rule 254 for `assertion` (journal spec §9.32): a credential its own member holds, a counter
    /// above the last kept one or 0 over 0, and a challenge no kept assertion used.
    fn keeps_rule_254(&self, assertion: &Assertion) -> bool {
        let own = self.holder(&assertion.credential) == Some(assertion.member);
        let rises = self
            .last
            .get(assertion.credential.as_str())
            .is_some_and(|last| {
                assertion.sign_count > *last || (assertion.sign_count == 0 && *last == 0)
            });
        own && rises && !self.challenges.contains(assertion.challenge.as_str())
    }

    /// The enrolment of `credential` while it is held, among the records that keep rule 251.
    fn enrolment(&self, credential: &str) -> Option<&'a Enrolment> {
        self.held
            .iter()
            .copied()
            .find(|e| e.credential == credential)
    }

    /// Rule 318's converse, before `next` is folded: an owed assertion is settled only by a waiver
    /// at the very next `seq` naming its challenge; any other next record breaks the rule at the
    /// assertion.
    fn settle_owed(&mut self, next: &CredentialRecord) {
        let Some(owed) = self.owed.take() else {
            return;
        };
        let waives = matches!(next, CredentialRecord::Waived(w)
            if owed.seq.checked_add(1) == Some(w.seq) && w.challenge == owed.challenge);
        if !waives {
            self.break_318(owed.member, &owed.credential, owed.seq);
        }
    }

    /// Rule 318 broken at `seq` by `member`'s record naming `credential`: step-up is disabled for
    /// the member and the credential's holder from the earliest `seq` either is broken at.
    fn break_318(&mut self, member: PrincipalId, credential: &str, seq: u64) {
        self.waiver_break = Some(self.waiver_break.map_or(seq, |first| first.min(seq)));
        let holder = self.holder(credential);
        for broken in std::iter::once(member).chain(holder) {
            let first = self.broken.entry(broken).or_insert(seq);
            *first = (*first).min(seq);
        }
    }

    /// Folds one waiver under rule 318: the actor holds the credential, the waiver names that
    /// enrolment's `cool_off_ends`, which is after the waiver's `event_time`, and no waiver named
    /// its challenge before; otherwise the rule breaks at the waiver.
    fn waive(&mut self, waiver: &'a Waiver) {
        let keeps = self.enrolment(&waiver.credential).is_some_and(|e| {
            e.member == waiver.member && e.cool_off_ends == Some(waiver.cool_off_ends)
        }) && waiver.cool_off_ends > waiver.event_time
            && !self.waived.contains(waiver.challenge.as_str());
        if !keeps {
            self.break_318(waiver.member, &waiver.credential, waiver.seq);
        }
        self.waived.insert(&waiver.challenge);
    }

    /// Folds one assertion: a kept one raises its credential's last counter and uses its
    /// challenge; a breaking one disables step-up for its member and the credential's holder. An
    /// assertion of the member's own credential inside its cool-off is owed a waiver by the next
    /// record (rule 318's converse).
    fn assert(&mut self, assertion: &'a Assertion) {
        if self.enrolment(&assertion.credential).is_some_and(|e| {
            e.member == assertion.member
                && e.cool_off_ends
                    .is_some_and(|ends| ends > assertion.event_time)
        }) {
            self.owed = Some(assertion);
        }
        if self.keeps_rule_254(assertion) {
            self.last
                .insert(&assertion.credential, assertion.sign_count);
            self.challenges.insert(&assertion.challenge);
            return;
        }
        let holder = self.holder(&assertion.credential);
        for broken in std::iter::once(assertion.member).chain(holder) {
            self.broken.entry(broken).or_insert(assertion.seq);
        }
    }
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
            Self::Asserted(_) | Self::Waived(_) => None,
        }
    }

    /// The record's `seq`.
    fn seq(&self) -> u64 {
        match self {
            Self::Enrolled(e) => e.seq,
            Self::Removed(r) => r.seq,
            Self::Asserted(a) => a.seq,
            Self::Waived(w) => w.seq,
        }
    }
}
