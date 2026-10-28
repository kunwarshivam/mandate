//! The control stream's `CredentialEnrolled` and `CredentialRemoved`, journal spec v0.70 §9.31
//! (DEC-2108 items 4, 5 and 8), closed at schema version 1 with the consistency rules that read
//! them: 249 and 250, each refusing at its member, in rule order. Every rule only refuses a draft.
//! v0.71 (DEC-2122) closes `CredentialEnrolled` at version 2 too, with `backup_eligible`, which
//! rule 249 reads as at version 1, and §9.32's `StepUpAsserted` at version 1, with rule 252 at
//! append and batch rule 253 in [`check_batch`]. Stream rule 254 is the writer's, as 251 is.
//!
//! v0.112 (DEC-2146) closes §9.38's `StepUpCoolOffWaived` at version 1, with rule 316 at append
//! and batch rule 317 in [`check_batch`], judged after rule 253. Stream rule 318 is the writer's, as
//! 254 is.
//!
//! §9.31's stream rule, 251, is the writer's: the API checks it against its own fold before it
//! appends, and the verifier's fold fails closed on it, so `append` neither sees the stream nor
//! judges it (the module's doc in the spec says so in as many words).
//!
//! No cross-record check lives here. A credential names no workspace: the stream it is on binds it
//! to its workspace and `member` binds it to its member, which the API's verifier checks against
//! the session's principal (identity spec §7.2 step 4).

use mandate_canon::Value;
use mandate_time::UtcNanos;

use crate::schema::Ty;
use crate::{Draft, Invalid, InvalidReason, StreamType};

const ENROLLED: &str = "CredentialEnrolled";
const REMOVED: &str = "CredentialRemoved";
const ASSERTED: &str = "StepUpAsserted";
const WAIVED: &str = "StepUpCoolOffWaived";

/// The control-stream event types §9.31, §9.32 and §9.38 add to [`crate::control::governs`].
pub(crate) const CONTROL: [&str; 4] = [ENROLLED, REMOVED, ASSERTED, WAIVED];

/// A cool-off's length when one is owed (identity spec §10.1).
const DAY_SECS: i64 = 86_400;

/// How long before its event a passkey step-up still counts (identity spec §7.1).
const STEP_UP_WINDOW_SECS: i64 = 300;

/// §9.31's `credential_id`: a WebAuthn credential id as unpadded base64url, 22 to 1364 characters,
/// which is 16 to 1023 bytes. `A-Z a-z 0-9 - _` with no `=` padding, and a length no unpadded
/// base64url can have (`length % 4 == 1`), which is a block cut one character over.
fn credential_id(text: &str) -> bool {
    (22..=1364).contains(&text.len())
        && text.len() % 4 != 1
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The §9.31 schema of `event_type` at `schema_version` on `stream`, or `None` when this module
/// registers none there.
pub(crate) fn schema(
    event_type: &str,
    schema_version: u64,
    stream: StreamType,
) -> Option<&'static Ty> {
    match (event_type, schema_version, stream) {
        (ENROLLED, 1, StreamType::Control) => Some(&CREDENTIAL_ENROLLED),
        (ENROLLED, 2, StreamType::Control) => Some(&CREDENTIAL_ENROLLED_V2),
        (REMOVED, 1, StreamType::Control) => Some(&CREDENTIAL_REMOVED),
        (ASSERTED, 1, StreamType::Control) => Some(&STEP_UP_ASSERTED),
        (WAIVED, 1, StreamType::Control) => Some(&STEP_UP_COOL_OFF_WAIVED),
        _ => None,
    }
}

/// §9.31's step-up, on both records: the assertion, when it was authenticated (a timestamp, as
/// §9.2 has it, not §9.7's integer risk-clock seconds), and its method — which rule 249 and rule
/// 250 hold to `passkey`.
const STEP_UP: &[(&str, Ty)] = &[
    ("assertion_id", Ty::Str),
    ("authenticated_at", Ty::Timestamp),
    ("method", Ty::Str),
];

pub(crate) const STEP_UP_RECORD: Ty = Ty::Record(STEP_UP);

static CREDENTIAL_ENROLLED: Ty = Ty::Record(&[
    ("member", Ty::Ulid),
    ("kind", Ty::Str),
    ("credential", Ty::Text(credential_id)),
    ("public_key", Ty::DigestRef),
    ("alg", Ty::OneOf(&["es256", "eddsa"])),
    ("sign_count", Ty::Int),
    ("cool_off_ends", Ty::Nullable(&Ty::Timestamp)),
    ("step_up", Ty::Nullable(&STEP_UP_RECORD)),
]);

/// Version 2 (v0.71): version 1's members, with the registration's backup-eligible flag after
/// `sign_count`.
static CREDENTIAL_ENROLLED_V2: Ty = Ty::Record(&[
    ("member", Ty::Ulid),
    ("kind", Ty::Str),
    ("credential", Ty::Text(credential_id)),
    ("public_key", Ty::DigestRef),
    ("alg", Ty::OneOf(&["es256", "eddsa"])),
    ("sign_count", Ty::Int),
    ("backup_eligible", Ty::Bool),
    ("cool_off_ends", Ty::Nullable(&Ty::Timestamp)),
    ("step_up", Ty::Nullable(&STEP_UP_RECORD)),
]);

/// §9.32's `StepUpAsserted`: the challenge answered, the credential, the counter the assertion
/// carried, and the raw assertion as an artifact.
static STEP_UP_ASSERTED: Ty = Ty::Record(&[
    ("challenge", Ty::Ulid),
    ("credential", Ty::Text(credential_id)),
    ("sign_count", Ty::Int),
    ("assertion", Ty::DigestRef),
]);

/// §9.38's `StepUpCoolOffWaived`: the challenge and credential of the step-up a test cell counted
/// inside the credential's enrolment cool-off, the cool-off's end, and the cell's tier.
static STEP_UP_COOL_OFF_WAIVED: Ty = Ty::Record(&[
    ("challenge", Ty::Ulid),
    ("credential", Ty::Text(credential_id)),
    ("cool_off_ends", Ty::Timestamp),
    ("tier", Ty::OneOf(&["dev", "ci"])),
]);

static CREDENTIAL_REMOVED: Ty = Ty::Record(&[
    ("member", Ty::Ulid),
    ("credential", Ty::Text(credential_id)),
    ("step_up", Ty::Record(STEP_UP)),
]);

/// The envelope members the rules of this module read.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Read<'a> {
    pub(crate) actor: Option<&'a Value>,
    pub(crate) event_time: Option<&'a Value>,
    pub(crate) causation_id: Option<&'a Value>,
    pub(crate) environment: Option<&'a Value>,
}

/// Rules 249, 250, 252 and 316 on a record its schema has normalized, the first that fails
/// reported.
pub(crate) fn rules(event_type: &str, payload: &Value, read: Read<'_>) -> Result<(), Invalid> {
    match event_type {
        ENROLLED => rules_249(payload, read.actor, read.event_time),
        REMOVED => rules_250(payload, read.actor),
        ASSERTED => rules_252(payload, read.actor, read.causation_id),
        WAIVED => rules_316(read),
        _ => Ok(()),
    }
}

/// Rule 316: the waiver names the event the step-up authorized as its cause, its actor is a user,
/// and it is written in `paper`, since a passkey step-up in `live` is refused before its evidence
/// is read (DEC-2146 item 3).
fn rules_316(read: Read<'_>) -> Result<(), Invalid> {
    ensure(
        read.causation_id.is_some_and(|c| *c != Value::Null),
        "causation_id",
    )?;
    ensure(
        read.actor
            .and_then(|a| a.get("kind"))
            .and_then(Value::as_str)
            == Some("user"),
        "actor.kind",
    )?;
    ensure(
        read.environment.and_then(Value::as_str) == Some("paper"),
        "environment",
    )
}

/// Rule 252: the counter fits 32 bits, the record names the event it authorized as its cause, and
/// its actor is a user.
fn rules_252(
    payload: &Value,
    actor: Option<&Value>,
    causation_id: Option<&Value>,
) -> Result<(), Invalid> {
    ensure(counts_in_32_bits(payload), "payload.sign_count")?;
    ensure(
        causation_id.is_some_and(|c| *c != Value::Null),
        "causation_id",
    )?;
    ensure(
        actor.and_then(|a| a.get("kind")).and_then(Value::as_str) == Some("user"),
        "actor.kind",
    )
}

/// Batch rule 253 (§9.32): each `StepUpAsserted` follows, immediately, the event its cause names
/// (else its `event_type`), by the same actor (`actor.id`), whose step-up is a passkey's
/// (`payload.step_up`) over the challenge the assertion answered (`payload.challenge`). Each draft
/// has already passed `Draft::parse`, so rule 252 has held. Then batch rule 317 (§9.38) over every
/// waiver, in [`check_waivers`].
pub(crate) fn check_batch(drafts: &[Draft]) -> Result<(), (usize, Invalid)> {
    for (i, draft) in drafts.iter().enumerate() {
        if draft.event_type() != ASSERTED {
            continue;
        }
        let refuse = |path: &str| Err((i, Invalid::new(InvalidReason::Schema, path)));
        let cause = at(draft, &["causation_id"]).and_then(Value::as_str);
        let before = i
            .checked_sub(1)
            .and_then(|j| drafts.get(j))
            .filter(|b| Some(b.event_id()) == cause);
        let Some(before) = before else {
            return refuse("event_type");
        };
        let actor = |d| at(d, &["actor", "id"]).and_then(Value::as_str);
        if actor(before) != actor(draft) {
            return refuse("actor.id");
        }
        let method = at(before, &["payload", "step_up", "method"]).and_then(Value::as_str);
        if method != Some("passkey") {
            return refuse("payload.step_up");
        }
        let assertion_id =
            at(before, &["payload", "step_up", "assertion_id"]).and_then(Value::as_str);
        if assertion_id != at(draft, &["payload", "challenge"]).and_then(Value::as_str) {
            return refuse("payload.challenge");
        }
    }
    check_waivers(drafts)
}

/// A draft's member at `path`, dotted from the envelope.
fn at<'a>(draft: &'a Draft, path: &[&str]) -> Option<&'a Value> {
    let (first, rest) = path.split_first()?;
    rest.iter()
        .try_fold(draft.fields().get(*first)?, |node, name| node.get(name))
}

/// Batch rule 317 (§9.38): each `StepUpCoolOffWaived` follows, immediately, a `StepUpAsserted`
/// (else its `event_type`) with the same cause (`causation_id`), actor (`actor.id`), challenge
/// (`payload.challenge`), credential (`payload.credential`) and instant (`event_time`). Each draft has already passed
/// `Draft::parse`, so rule 316 has held.
fn check_waivers(drafts: &[Draft]) -> Result<(), (usize, Invalid)> {
    for (i, draft) in drafts.iter().enumerate() {
        if draft.event_type() != WAIVED {
            continue;
        }
        let refuse = |path: &str| Err((i, Invalid::new(InvalidReason::Schema, path)));
        let before = i
            .checked_sub(1)
            .and_then(|j| drafts.get(j))
            .filter(|b| b.event_type() == ASSERTED);
        let Some(before) = before else {
            return refuse("event_type");
        };
        for (path, member) in [
            ("causation_id", &["causation_id"][..]),
            ("actor.id", &["actor", "id"][..]),
            ("payload.challenge", &["payload", "challenge"][..]),
            ("payload.credential", &["payload", "credential"][..]),
            ("event_time", &["event_time"][..]),
        ] {
            if at(before, member) != at(draft, member) {
                return refuse(path);
            }
        }
    }
    Ok(())
}

/// Rule 249: `kind` is `passkey`, the actor is the member themselves, a step-up's method is
/// `passkey`, and `cool_off_ends` is non-null exactly when there is no step-up — and then is
/// `event_time` plus 86 400 s. So a first enrolment always waits its cool-off and a stepped-up one
/// never does.
fn rules_249(
    payload: &Value,
    actor: Option<&Value>,
    event_time: Option<&Value>,
) -> Result<(), Invalid> {
    ensure(
        payload.get("kind").and_then(Value::as_str) == Some("passkey"),
        "payload.kind",
    )?;
    ensure(counts_in_32_bits(payload), "payload.sign_count")?;
    own_record(payload, actor)?;
    let stepped = payload.get("step_up").is_some_and(|s| *s != Value::Null);
    if stepped {
        ensure(
            payload
                .get("step_up")
                .and_then(|s| s.get("method"))
                .and_then(Value::as_str)
                == Some("passkey"),
            "payload.step_up.method",
        )?;
    }
    let cool_off = payload.get("cool_off_ends");
    let present = cool_off.is_some_and(|c| *c != Value::Null);
    ensure(present != stepped, "payload.cool_off_ends")?;
    if present {
        let at = event_time
            .and_then(Value::as_str)
            .and_then(|t| UtcNanos::parse(t).ok());
        let owed =
            at.and_then(|t| UtcNanos::from_parts(t.secs().checked_add(DAY_SECS)?, t.nanos()).ok());
        ensure(
            owed.is_some() && owed == instant(payload, "cool_off_ends"),
            "payload.cool_off_ends",
        )?;
    }
    Ok(())
}

/// Rule 250: the actor is the member themselves, and the step-up's method is `passkey`. A removal
/// always carries one, so the schema has already refused a null.
fn rules_250(payload: &Value, actor: Option<&Value>) -> Result<(), Invalid> {
    own_record(payload, actor)?;
    ensure(
        payload
            .get("step_up")
            .and_then(|s| s.get("method"))
            .and_then(Value::as_str)
            == Some("passkey"),
        "payload.step_up.method",
    )
}

/// The actor binding both rules share: a `user` whose `id` is the payload's `member`, so no one
/// enrols or removes for another. The kind is checked first, then the id.
fn own_record(payload: &Value, actor: Option<&Value>) -> Result<(), Invalid> {
    ensure(
        actor.and_then(|a| a.get("kind")).and_then(Value::as_str) == Some("user"),
        "actor.kind",
    )?;
    ensure(
        actor.and_then(|a| a.get("id")).and_then(Value::as_str)
            == payload.get("member").and_then(Value::as_str),
        "actor.id",
    )
}

/// §9.31's `sign_count` is the authenticator's 32-bit counter, so anything past it is refused by
/// rule 249 rather than by the type: `Ty::Int` bounds nothing.
fn counts_in_32_bits(payload: &Value) -> bool {
    payload
        .get("sign_count")
        .and_then(Value::as_int)
        .is_some_and(|n| n <= u64::from(u32::MAX))
}

/// Rules 267 and 268's evidence clauses (journal spec v0.84, DEC-2131): the record's `step_up` is a
/// passkey's (`schema` at `payload.step_up.method`), authenticated 0 to 300 seconds before the
/// envelope's `event_time`, compared to the nanosecond (`schema` at
/// `payload.step_up.authenticated_at`), the method judged first.
pub(crate) fn passkey_step_up(payload: &Value, event_time: Option<&Value>) -> Result<(), Invalid> {
    let step_up = payload.get("step_up").unwrap_or(&Value::Null);
    ensure(
        step_up.get("method").and_then(Value::as_str) == Some("passkey"),
        "payload.step_up.method",
    )?;
    let event = event_time
        .and_then(Value::as_str)
        .and_then(|t| UtcNanos::parse(t).ok());
    let fresh = instant(step_up, "authenticated_at")
        .zip(event)
        .is_some_and(|(at, event)| {
            UtcNanos::from_parts(
                event.secs().saturating_sub(STEP_UP_WINDOW_SECS),
                event.nanos(),
            )
            .is_ok_and(|earliest| earliest <= at && at <= event)
        });
    ensure(fresh, "payload.step_up.authenticated_at")
}

fn instant(payload: &Value, member: &str) -> Option<UtcNanos> {
    payload
        .get(member)
        .and_then(Value::as_str)
        .and_then(|t| UtcNanos::parse(t).ok())
}

fn ensure(holds: bool, path: &str) -> Result<(), Invalid> {
    if holds {
        Ok(())
    } else {
        Err(Invalid::new(InvalidReason::Schema, path))
    }
}

#[cfg(test)]
mod tests {
    use super::credential_id;

    /// §9.31's `credential_id` is four conditions, each refusing on its own. Pinned here because
    /// the vectors exercise them one at a time, so no vector had a good alphabet with a bad length
    /// — which is the case that tells `&&` from `||` in the predicate, and the mutation gate found
    /// the `||` mutant surviving.
    #[test]
    fn a_credential_id_needs_every_clause_of_its_shape() {
        let ok = "AAECAwQFBgcICQoLDA0ODw";
        assert!(credential_id(ok), "16 bytes: the base draft");
        assert!(credential_id(&"A".repeat(1364)), "1023 bytes: the longest");

        for (class, sample) in [
            ("an upper-case letter", "AAECAwQFBgcICQoLDA0ODw"),
            ("a lower-case letter", "aaecAwQFBgcICQoLDA0ODw"),
            ("a digit", "00ECAwQFBgcICQoLDA0ODw"),
            ("a hyphen", "AA-CAwQFBgcICQoLDA0ODw"),
            ("an underscore", "AA_CAwQFBgcICQoLDA0ODw"),
        ] {
            assert!(credential_id(sample), "{class} is in base64url's alphabet");
        }

        assert!(!credential_id(&ok[..21]), "21 characters: under the floor");
        assert!(
            !credential_id(&"A".repeat(1366)),
            "1366 characters: over the ceiling"
        );
        assert!(
            !credential_id(&"A".repeat(25)),
            "25 characters: no unpadded base64url has a length of 1 modulo 4"
        );
        assert!(!credential_id(&format!("{ok}=")), "padded");

        let mut bad_alphabet = ok.to_owned();
        bad_alphabet.replace_range(7..8, "+");
        assert!(
            !credential_id(&bad_alphabet),
            "`+` is base64's, not base64url's"
        );

        let mut too_long_and_bad = "A".repeat(1366);
        too_long_and_bad.replace_range(4..5, "+");
        assert!(
            !credential_id(&too_long_and_bad),
            "a bad alphabet that is also too long: both clauses refuse it, so neither alone decides"
        );
    }
}
