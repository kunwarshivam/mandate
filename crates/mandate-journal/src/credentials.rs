//! The control stream's `CredentialEnrolled` and `CredentialRemoved`, journal spec v0.70 §9.31
//! (DEC-2108 items 4, 5 and 8), closed at schema version 1 with the consistency rules that read
//! them: 249 and 250, each refusing at its member, in rule order. Every rule only refuses a draft.
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
use crate::{Invalid, InvalidReason, StreamType};

const ENROLLED: &str = "CredentialEnrolled";
const REMOVED: &str = "CredentialRemoved";

/// The control-stream event types §9.31 adds to [`crate::control::governs`].
pub(crate) const CONTROL: [&str; 2] = [ENROLLED, REMOVED];

/// A cool-off's length when one is owed (identity spec §10.1).
const DAY_SECS: i64 = 86_400;

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
        (REMOVED, 1, StreamType::Control) => Some(&CREDENTIAL_REMOVED),
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

static STEP_UP_RECORD: Ty = Ty::Record(STEP_UP);

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

static CREDENTIAL_REMOVED: Ty = Ty::Record(&[
    ("member", Ty::Ulid),
    ("credential", Ty::Text(credential_id)),
    ("step_up", Ty::Record(STEP_UP)),
]);

/// Rules 249 and 250 on a credential record its schema has normalized, the first that fails
/// reported. `actor` and `event_time` are the envelope's.
pub(crate) fn rules(
    event_type: &str,
    payload: &Value,
    actor: Option<&Value>,
    event_time: Option<&Value>,
) -> Result<(), Invalid> {
    match event_type {
        ENROLLED => rules_249(payload, actor, event_time),
        REMOVED => rules_250(payload, actor),
        _ => Ok(()),
    }
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
    // §9.31's `sign_count` is the authenticator's 32-bit counter, so anything past it is refused
    // here rather than by the type: `Ty::Int` bounds nothing.
    ensure(
        payload
            .get("sign_count")
            .and_then(Value::as_int)
            .is_some_and(|n| n <= u64::from(u32::MAX)),
        "payload.sign_count",
    )?;
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
        let owed = at.and_then(|t| UtcNanos::from_parts(t.secs().checked_add(DAY_SECS)?, t.nanos()).ok());
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

        assert!(!credential_id(&ok[..21]), "21 characters: under the floor");
        assert!(!credential_id(&"A".repeat(1366)), "1366 characters: over the ceiling");
        assert!(
            !credential_id(&"A".repeat(25)),
            "25 characters: no unpadded base64url has a length of 1 modulo 4"
        );
        assert!(!credential_id(&format!("{ok}=")), "padded");

        let mut bad_alphabet = ok.to_owned();
        bad_alphabet.replace_range(7..8, "+");
        assert!(!credential_id(&bad_alphabet), "`+` is base64's, not base64url's");

        let mut too_long_and_bad = "A".repeat(1366);
        too_long_and_bad.replace_range(4..5, "+");
        assert!(
            !credential_id(&too_long_and_bad),
            "a bad alphabet that is also too long: both clauses refuse it, so neither alone decides"
        );
    }
}
