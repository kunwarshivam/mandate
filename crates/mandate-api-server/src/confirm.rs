//! `cli_confirm` step-up for a paper grant made in the web app
//! ([DEC-2104](../../../docs/project/decisions/DEC-2104.md), DEC-2103 item 7, decided by the founder
//! in DEC-2605 row 5): the code the web app shows for a challenge, and the port that verifies a
//! grant's `cli_confirm` evidence against the challenge this server holds and the owner's
//! `StepUpConfirmed` on the control stream ([journal spec](../../../docs/specs/journal.md) §9.29).
//!
//! The owner confirms on the host, where only the CLI appends to the control stream, so a
//! confirmation is one the browser cannot make (DEC-2104 item 1). A confirmation is not step-up
//! evidence: only this port reads it, and the grant's own evidence is still judged by the runtime
//! as mandate spec §6.1 says.

use std::sync::Arc;

use mandate_api::envelope::{StepUpEvidence, StepUpMethod};
use mandate_api::problem::ProblemCode;
use mandate_canon::{Digest, Value, parse};
use mandate_identity::{PrincipalId, StepUpActionKind, WorkspaceId};
use mandate_journal::StoredEvent;
use mandate_time::UtcNanos;

use crate::challenges::Challenges;
use crate::stepup::{Action, Judged, StepUp, journaled};
use crate::subject::NamesPrincipal;

/// How many characters the code has: 40 bits of Crockford base 32 (DEC-2104 item 1).
pub const CODE_LEN: usize = 8;

/// How many bytes the code is made from: its 40 bits.
const CODE_BYTES: usize = 5;

/// Crockford's base-32 digits, upper case.
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// A Crockford digit's width in bits.
const DIGIT_BITS: u32 = 5;

/// The record a CLI confirmation is (journal spec §9.29).
const CONFIRMED: &str = "StepUpConfirmed";

/// The code the web app shows for a challenge whose `webauthn_challenge` is `bytes`: their first
/// 40 bits as [`CODE_LEN`] Crockford base-32 characters, upper case, without the display hyphen.
/// Fewer than 5 bytes has no code.
pub fn code(bytes: &[u8]) -> Option<String> {
    let mut word = [0u8; 8];
    word.get_mut(8 - CODE_BYTES..)?
        .copy_from_slice(bytes.get(..CODE_BYTES)?);
    let bits = u64::from_be_bytes(word);
    (0..CODE_LEN)
        .rev()
        .map(|digit| {
            let shift = u32::try_from(digit).ok()?.checked_mul(DIGIT_BITS)?;
            let index = usize::try_from((bits >> shift) & 31).ok()?;
            CROCKFORD.get(index).copied().map(char::from)
        })
        .collect()
}

/// The port for `cli_confirm` evidence (DEC-2104 item 6), over the challenges this server holds
/// and the subject links that must still name the confirming principal.
pub struct CliConfirm {
    pub challenges: Arc<Challenges>,
    pub links: Arc<dyn NamesPrincipal>,
}

impl StepUp for CliConfirm {
    /// DEC-2104 item 6's nine checks in order, with DEC-2105's bounds, then the challenge dropped
    /// so it verifies once, for an approval and, until E9-4's passkey step-up does, an OAuth
    /// connect (DEC-2125); any other kind is `step_up_method`. Evidence of another method has no
    /// confirmation to judge and is missing: no passkey is enrolled before E9-4 S3.
    fn verify(
        &self,
        evidence: &StepUpEvidence,
        action: &Action,
        judged: &Judged<'_>,
    ) -> Result<(), ProblemCode> {
        if !matches!(
            action.kind,
            StepUpActionKind::Approve | StepUpActionKind::Connection
        ) {
            return Err(ProblemCode::StepUpMethod);
        }
        if evidence.method != StepUpMethod::CliConfirm {
            return Err(ProblemCode::StepUpMissing);
        }
        let presented = journaled(Some(evidence)).ok_or(ProblemCode::StepUpMissing)?;
        let id = member(&presented, "assertion_id").ok_or(ProblemCode::StepUpMissing)?;
        let record = self.challenges.held(id).ok_or(ProblemCode::StepUpMissing)?;
        let bound = record.principal_id() == &judged.tenant.principal()
            && record.workspace_id() == &judged.tenant.workspace()
            && record.action().kind == action.kind
            && record.action().digest == action.digest;
        if !bound {
            return Err(ProblemCode::StepUpMismatch);
        }
        let authenticated = member(&presented, "authenticated_at")
            .and_then(|text| UtcNanos::parse(text).ok())
            .ok_or(ProblemCode::StepUpStale)?;
        let current = record.issued_at() <= judged.now && judged.now < record.expires_at();
        if !current || authenticated != record.issued_at() {
            return Err(ProblemCode::StepUpStale);
        }
        let rows = judged
            .control
            .iter()
            .map(|row| parse(&row.body).map(|event| (row, event)))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| ProblemCode::StepUpMissing)?;
        let reused = rows.iter().any(|(_, event)| {
            event
                .get("payload")
                .and_then(|payload| payload.get("step_up"))
                .and_then(|step_up| step_up.get("assertion_id"))
                .and_then(Value::as_str)
                == Some(id)
        });
        if reused {
            return Err(ProblemCode::StepUpReused);
        }
        let mut confirmations = rows.iter().filter_map(|(row, event)| {
            let payload = event.get("payload")?;
            let names = row.event_type == CONFIRMED && member(payload, "challenge") == Some(id);
            names.then_some(payload)
        });
        let (Some(confirmation), None) = (confirmations.next(), confirmations.next()) else {
            return Err(ProblemCode::StepUpMissing);
        };
        let principal = judged.tenant.principal();
        let digest = format!("sha256:{}", record.action().digest);
        let principal_text = principal
            .to_ulid_text()
            .map_err(|_| ProblemCode::StepUpMismatch)?;
        let matches = member(confirmation, "action_kind") == Some(record.action().kind.code())
            && member(confirmation, "action_digest") == Some(digest.as_str())
            && member(confirmation, "principal") == Some(principal_text.as_str());
        if !matches {
            return Err(ProblemCode::StepUpMismatch);
        }
        if self.links.names(principal) != Ok(true) {
            return Err(ProblemCode::StepUpMissing);
        }
        let second = confirmation
            .get("confirmed_at")
            .and_then(Value::as_int)
            .and_then(|secs| i64::try_from(secs).ok())
            .ok_or(ProblemCode::StepUpStale)?;
        let ends = second
            .checked_add(1)
            .and_then(|end| UtcNanos::from_parts(end, 0).ok())
            .ok_or(ProblemCode::StepUpStale)?;
        if second < record.issued_at().secs() || ends > record.expires_at() {
            return Err(ProblemCode::StepUpStale);
        }
        let shown = record
            .canonical()
            .ok()
            .and_then(|canonical| code(Digest::of(&canonical).as_bytes()))
            .ok_or(ProblemCode::StepUpMissing)?;
        let typed = member(confirmation, "code").unwrap_or_default();
        if !same(typed.as_bytes(), shown.as_bytes()) {
            return Err(ProblemCode::StepUpMissing);
        }
        if !self.challenges.take(id) {
            return Err(ProblemCode::StepUpMissing);
        }
        Ok(())
    }
}

/// Why [`passkey_held`] gives no answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unanswered {
    /// The stream's credential records cannot be read, or the member's records break rule 251 or
    /// 254, so the passkey routes would list nothing: `journal_unavailable`.
    Journal,
    /// The check is a stub until its story implements it (DEC-77).
    Unimplemented { story: &'static str },
}

/// Whether `principal` holds a passkey past its cool-off at `now` on `workspace`'s control stream
/// as the request read it ([DEC-2126](../../../docs/project/decisions/DEC-2126.md) item 6),
/// judged as the step-up verifier counts one: from the same fold, a `CredentialEnrolled` version 2
/// passkey whose cool-off has ended or that has none. A version 1 enrolment never counts (DEC-2122
/// item 3), so it holds nothing here. Such a member's `cli_confirm` for a connect has ended, and a
/// paper connect needs their passkey.
///
/// # Errors
/// [`Unanswered`].
pub fn passkey_held(
    _workspace: WorkspaceId,
    _principal: PrincipalId,
    _control: &[StoredEvent],
    _now: UtcNanos,
) -> Result<bool, Unanswered> {
    Err(Unanswered::Unimplemented { story: "E9-4" })
}

/// `value`'s text member `name`.
fn member<'v>(value: &'v Value, name: &str) -> Option<&'v str> {
    value.get(name).and_then(Value::as_str)
}

/// Whether `typed` is `shown`, compared over every byte whatever differs first (DEC-2104 item 6
/// check 9), so the time taken says nothing of how much of a code was right.
fn same(typed: &[u8], shown: &[u8]) -> bool {
    let differs = typed
        .iter()
        .zip(shown)
        .fold(0u8, |acc, (a, b)| acc | (a ^ b));
    typed.len() == shown.len() && differs == 0
}
