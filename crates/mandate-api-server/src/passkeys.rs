//! The passkey routes (workspace API spec §3.6, [DEC-2108] items 4 and 5, [DEC-2115],
//! [DEC-2118], [DEC-2124]): `POST /v1/workspaces/{ws}/passkeys/registration-challenges`,
//! `POST /v1/workspaces/{ws}/passkeys`, and `GET /v1/workspaces/{ws}/passkeys`.
//!
//! Both serve §3.7's row "Enrol or remove one's own passkey", its `own` cell, and read the
//! member's passkeys from one fold of the control stream, `CredentialFold::passkeys`; a member
//! whose records break rule 251 or 254 is answered `journal_unavailable` (DEC-2124 items 1 and 2).
//!
//! [DEC-2108]: ../../../docs/project/decisions/DEC-2108.md
//! [DEC-2115]: ../../../docs/project/decisions/DEC-2115.md
//! [DEC-2118]: ../../../docs/project/decisions/DEC-2118.md
//! [DEC-2124]: ../../../docs/project/decisions/DEC-2124.md

use std::sync::Arc;

use axum::body::Body;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::header::CACHE_CONTROL;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use mandate_api::envelope::{StepUpAssertion, StepUpEvidence};
use mandate_api::problem::{Effect, Problem, ProblemCode, Violation};
use mandate_api::wire::{decode, encode};
use mandate_authn::Request;
use mandate_canon::{Digest, Int, Value, encode_ulid, to_canonical};
use mandate_identity::{AssertionId, Permission, PrincipalId};
use mandate_journal::StoredEvent;
use mandate_passkey::credentials::{CredentialFold, CredentialRecord, Enrolment, PasskeyAlg};
use mandate_passkey::{Challenge, Credential, PublicKey, RelyingParty};
use mandate_time::UtcNanos;

use crate::backtests::paper;
use crate::challenges::{Registration, Withheld};
use crate::commands::{Event, Settled, Writer, envelope, settle_at};
use crate::http::{App, invalid, problem, problem_body};
use crate::members::credential_records;
use crate::reduction::rp_id;
use crate::routes::{
    OwnReader, Refused, Unreadable, enveloped, object, own_authorized, path_refused, session,
    string, watermark,
};
use crate::writes::{derived_for, strict};

/// §3.4's operation name for the registration challenge route (DEC-2108 item 4).
pub const CHALLENGE_OPERATION: &str = "passkey_registration_challenge";

/// The list route's operation name (DEC-2115 item 3).
pub const LIST_OPERATION: &str = "passkeys";

/// How long a registration challenge is held, and how recently a member with no passkey must have
/// signed in to ask for one, in seconds (DEC-2108 items 4 and 5, DEC-2124 item 4).
pub const FRESH_S: i64 = 300;

/// How long a dropped or expired registration challenge's id is kept as a tombstone, in seconds
/// (DEC-2118 item 1).
pub const TOMBSTONE_S: i64 = 86_400;

/// The most tombstones one principal keeps; past it the oldest is dropped (DEC-2118 item 1).
pub const TOMBSTONES_PER_PRINCIPAL: usize = 64;

/// `POST /v1/workspaces/{ws}/passkeys/registration-challenges` (DEC-2108 item 4, DEC-2115 item 1,
/// DEC-2118 item 1, DEC-2124): the session, the CSRF mark, the key, the strict empty body, the
/// row, then `live`; then the member's passkeys, a first enrolment's fresh sign-in, and the store.
pub async fn registration_challenge(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
    body: Body,
) -> Response {
    let Ok(Path(workspace)) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let (gate, body) = match strict(&app, &headers, body).await {
        Ok(admitted) => admitted,
        Err(refusal) => return refusal.response(),
    };
    if !empty_object(&body) {
        return Refused::Invalid.response();
    }
    let reader = match own_authorized(
        &app,
        &gate.admitted,
        gate.now,
        &workspace,
        Permission::OwnPasskey,
        Unreadable::Command,
    )
    .await
    {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    if !paper(&reader.control) {
        return problem(
            StatusCode::CONFLICT,
            problem_body(ProblemCode::LiveUnavailable),
        );
    }
    let principal = gate.admitted.principal;
    let Some(passkeys) = own_passkeys(&app, &reader, principal) else {
        return Refused::JournalUnavailable.response();
    };
    let fresh = later(gate.admitted.signed_in, FRESH_S).is_some_and(|ends| gate.now <= ends);
    if passkeys.is_empty() && !fresh {
        return stale_session();
    }
    let (Some((_, id)), Some(rp_id)) = (
        derived_for(&gate, &reader.workspace_text, CHALLENGE_OPERATION),
        rp_id(&app.settings.app_origin),
    ) else {
        return Refused::JournalUnavailable.response();
    };
    let held = app
        .challenges
        .register(principal, reader.workspace, AssertionId(id), gate.now);
    match held {
        Ok(registration) => challenged(&reader, &registration, &rp_id, &passkeys),
        Err(Withheld::Conflict) => problem(
            StatusCode::CONFLICT,
            problem_body(ProblemCode::IdempotencyConflict),
        ),
        Err(Withheld::Full) => problem(
            StatusCode::TOO_MANY_REQUESTS,
            problem_body(ProblemCode::RateLimited),
        ),
        Err(Withheld::Refused) => Refused::Forbidden.response(),
        Err(Withheld::OutOfRange | Withheld::NoRandom) => Refused::JournalUnavailable.response(),
    }
}

/// `GET /v1/workspaces/{ws}/passkeys` (DEC-2115 item 3, DEC-2124 items 1 to 3): the member's own
/// passkeys from the one fold, dated by their rows, with no step-up.
pub async fn list(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
) -> Response {
    let Ok(Path(workspace)) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let (admitted, now) = match session(&app, &headers, Request::Other) {
        Ok(admitted) => admitted,
        Err(refusal) => return refusal.response(),
    };
    let reader = match own_authorized(
        &app,
        &admitted,
        now,
        &workspace,
        Permission::OwnPasskey,
        Unreadable::Command,
    )
    .await
    {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    let Some(passkeys) = own_passkeys(&app, &reader, admitted.principal) else {
        return Refused::JournalUnavailable.response();
    };
    let listed = passkeys
        .iter()
        .map(|enrolment| listed(enrolment, &reader.control))
        .collect();
    let members = vec![
        ("passkeys", Value::Array(listed)),
        ("as_of", as_of(&reader.control)),
    ];
    no_store(enveloped(StatusCode::OK, now, members))
}

/// The enrolment route's operation name (DEC-2108 item 5).
pub const ENROL_OPERATION: &str = "passkey_enrol";

/// `POST /v1/workspaces/{ws}/passkeys` (DEC-2108 items 4 and 5, DEC-2118 items 3 and 5, DEC-2122,
/// DEC-2124): the session, the CSRF mark, the key, the strict body, the row, then `live`; then the
/// registration challenge, taken whatever follows, the member's passkeys, the step-up a first or a
/// later enrolment needs, the registration response, and a passkey id the stream never enrolled;
/// then the key's artifact and `CredentialEnrolled` version 2, and the `201`.
///
/// A later enrolment's step-up never counts yet: an `own` row's authorization yields no tenant
/// context for the port to judge it under, so evidence is `step_up_missing` until E9-4 S4 binds
/// one, and only a first enrolment commits. That only refuses, so it adds no risk.
pub async fn enrol(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
    body: Body,
) -> Response {
    let Ok(Path(workspace)) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let (gate, body) = match strict(&app, &headers, body).await {
        Ok(admitted) => admitted,
        Err(refusal) => return refusal.response(),
    };
    let Some(request) = Enrol::parse(&body) else {
        return Refused::Invalid.response();
    };
    let reader = match own_authorized(
        &app,
        &gate.admitted,
        gate.now,
        &workspace,
        Permission::OwnPasskey,
        Unreadable::Command,
    )
    .await
    {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    if !paper(&reader.control) {
        return problem(
            StatusCode::CONFLICT,
            problem_body(ProblemCode::LiveUnavailable),
        );
    }
    let principal = gate.admitted.principal;
    let Some(registration) = app.challenges.take_registration(
        &request.challenge_id,
        principal,
        reader.workspace,
        gate.now,
    ) else {
        return challenge_refused();
    };
    let Ok(records) = credential_records(app.hosted.workspace, &reader.control) else {
        return Refused::JournalUnavailable.response();
    };
    let fold = CredentialFold::new(records);
    let Ok(passkeys) = fold.passkeys(principal) else {
        return Refused::JournalUnavailable.response();
    };
    let fresh = later(gate.admitted.signed_in, FRESH_S).is_some_and(|ends| gate.now <= ends);
    match (request.stepped_up, passkeys.is_empty()) {
        (false, true) if !fresh => return stale_session(),
        (false, true) => {}
        (true, true) => return Refused::Invalid.response(),
        (false, false) => {
            return problem(
                StatusCode::UNAUTHORIZED,
                problem_body(ProblemCode::StepUpRequired),
            );
        }
        (true, false) => {
            return problem(
                StatusCode::UNAUTHORIZED,
                problem_body(ProblemCode::StepUpMissing),
            );
        }
    }
    let Some(rp_id) = rp_id(&app.settings.app_origin) else {
        return Refused::JournalUnavailable.response();
    };
    let rp = RelyingParty {
        rp_id,
        origin: app.settings.app_origin.clone(),
    };
    let Ok(challenge) = Challenge::new(&registration.challenge) else {
        return Refused::JournalUnavailable.response();
    };
    let registered = mandate_passkey::Registration {
        attestation_object: &request.attestation_object,
        client_data_json: &request.client_data_json,
    };
    let Ok(credential) = mandate_passkey::enrol(&rp, &challenge, &registered) else {
        return Refused::Invalid.response();
    };
    if URL_SAFE_NO_PAD.encode(&credential.id) != request.passkey_id {
        return Refused::Invalid.response();
    }
    let key = cose_bytes(&credential.public_key);
    let (alg, alg_name) = match credential.public_key {
        PublicKey::Es256 { .. } => (PasskeyAlg::Es256, "es256"),
        PublicKey::Ed25519 { .. } => (PasskeyAlg::EdDsa, "eddsa"),
    };
    let public_key = format!("sha256:{}", Digest::of(&key).to_hex());
    let Some(cool_off_ends) = later(gate.now, COOL_OFF_S) else {
        return Refused::JournalUnavailable.response();
    };
    let enrolment = Enrolment {
        seq: reader
            .control
            .last()
            .map_or(1, |row| row.seq.saturating_add(1)),
        member: principal,
        credential: request.passkey_id.clone(),
        public_key: public_key.clone(),
        alg,
        sign_count: credential.sign_count,
        cool_off_ends: Some(cool_off_ends),
        backup_eligible: Some(credential.backup_eligible),
    };
    if fold.admits(&CredentialRecord::Enrolled(enrolment)).is_err() {
        return Refused::Invalid.response();
    }
    let (Some((event_id, id)), Some(objects)) = (
        derived_for(&gate, &reader.workspace_text, ENROL_OPERATION),
        app.objects.as_ref(),
    ) else {
        return Refused::JournalUnavailable.response();
    };
    let Some(draft) = enrolled_draft(
        &reader,
        principal,
        &id,
        &request.passkey_id,
        (&public_key, alg_name),
        &credential,
        cool_off_ends,
        gate.now,
    ) else {
        return Refused::JournalUnavailable.response();
    };
    if objects.put(&key).is_err() {
        return Refused::JournalUnavailable.response();
    }
    let stream = format!("ctl:{}", reader.workspace_text);
    let head = reader.control.last().map(|row| row.seq);
    match settle_at(app.journal.as_ref(), &stream, &id, &draft, gate.now, head).await {
        Settled::Stored(_) => {}
        Settled::Conflict => {
            return problem(
                StatusCode::CONFLICT,
                problem_body(ProblemCode::IdempotencyConflict),
            );
        }
        Settled::Unknown | Settled::Unavailable => {
            return Refused::JournalUnavailable.response();
        }
    }
    let Ok(event_id) = encode(&event_id) else {
        return Refused::JournalUnavailable.response();
    };
    let members = vec![
        (
            "event_id",
            mandate_canon::parse(&event_id).unwrap_or(Value::Null),
        ),
        ("passkey_id", string(&request.passkey_id)),
        ("alg", string(alg_name)),
        ("cool_off_ends", string(&cool_off_ends.to_string())),
    ];
    no_store(enveloped(StatusCode::CREATED, gate.now, members))
}

/// A first passkey's cool-off, in seconds (journal spec §9.31 rule 249).
pub const COOL_OFF_S: i64 = 86_400;

/// `commands/passkey-enrol-request`, strict: exactly its six members, the registration response's
/// two byte strings unpadded base64url, and the step-up and its assertion both `null` or both
/// set, each of its own shape (DEC-2118 item 3).
struct Enrol {
    challenge_id: String,
    passkey_id: String,
    attestation_object: Vec<u8>,
    client_data_json: Vec<u8>,
    /// Whether the request carries step-up and its assertion.
    stepped_up: bool,
}

/// The longest passkey id §9.31 admits, in characters.
const PASSKEY_ID_MAX: usize = 1_364;

/// The shortest passkey id §9.31 admits, in characters.
const PASSKEY_ID_MIN: usize = 22;

impl Enrol {
    /// The request `body` holds, or `None` for anything else.
    fn parse(body: &[u8]) -> Option<Self> {
        let Ok(Value::Object(members)) = mandate_canon::parse(body) else {
            return None;
        };
        if members.len() != 6 {
            return None;
        }
        let text = |name: &str| members.get(name)?.as_str().map(str::to_owned);
        let bytes = |name: &str| unpadded(&text(name)?);
        let passkey_id = text("passkey_id")?;
        unpadded(&passkey_id)?;
        if !(PASSKEY_ID_MIN..=PASSKEY_ID_MAX).contains(&passkey_id.len()) {
            return None;
        }
        let step_up = members.get("step_up")?;
        let assertion = members.get("step_up_assertion")?;
        let stepped_up = match (step_up, assertion) {
            (Value::Null, Value::Null) => false,
            (Value::Null, _) | (_, Value::Null) => return None,
            (step_up, assertion) => {
                decode::<StepUpEvidence>(&to_canonical(step_up)).ok()?;
                decode::<StepUpAssertion>(&to_canonical(assertion)).ok()?;
                true
            }
        };
        Some(Self {
            challenge_id: text("challenge_id")?,
            passkey_id,
            attestation_object: bytes("attestation_object")?,
            client_data_json: bytes("client_data_json")?,
            stepped_up,
        })
    }
}

/// The bytes `text` holds as unpadded base64url, written canonically; `None` for anything else.
fn unpadded(text: &str) -> Option<Vec<u8>> {
    let bytes = URL_SAFE_NO_PAD.decode(text).ok()?;
    (URL_SAFE_NO_PAD.encode(&bytes) == text).then_some(bytes)
}

/// `key` as a COSE_Key (RFC 9053) in CTAP2's canonical form, the bytes an enrolment stores: an
/// EC2 P-256 key's `kty`, `alg` ES256, `crv`, `x` and `y`, or an OKP Ed25519 key's `kty`, `alg`
/// EdDSA, `crv` and `x`.
fn cose_bytes(key: &PublicKey) -> Vec<u8> {
    match key {
        PublicKey::Es256 { x, y } => [
            &[0xa5, 0x01, 0x02, 0x03, 0x26, 0x20, 0x01, 0x21, 0x58, 0x20][..],
            &x[..],
            &[0x22, 0x58, 0x20][..],
            &y[..],
        ]
        .concat(),
        PublicKey::Ed25519 { x } => [
            &[0xa4, 0x01, 0x01, 0x03, 0x27, 0x20, 0x06, 0x21, 0x58, 0x20][..],
            &x[..],
        ]
        .concat(),
    }
}

/// The `CredentialEnrolled` version 2 a first enrolment appends (journal spec §9.31): the member,
/// the passkey, its key's artifact reference and algorithm, the registration's counter and
/// backup-eligible flag, its cool-off's end, and no step-up.
#[expect(
    clippy::too_many_arguments,
    reason = "each is one member of the record, named at the one call"
)]
fn enrolled_draft(
    reader: &OwnReader,
    principal: PrincipalId,
    id: &str,
    passkey_id: &str,
    (public_key, alg): (&str, &str),
    credential: &Credential,
    cool_off_ends: UtcNanos,
    now: UtcNanos,
) -> Option<Vec<u8>> {
    let member = encode_ulid(principal.0);
    let payload = object(vec![
        ("member", string(&member)),
        ("kind", string("passkey")),
        ("credential", string(passkey_id)),
        ("public_key", string(public_key)),
        ("alg", string(alg)),
        (
            "sign_count",
            Value::Int(Int::new(u64::from(credential.sign_count))?),
        ),
        ("backup_eligible", Value::Bool(credential.backup_eligible)),
        ("cool_off_ends", string(&cool_off_ends.to_string())),
        ("step_up", Value::Null),
    ]);
    let event = Event {
        event_type: "CredentialEnrolled",
        schema_version: 2,
        payload,
        artifact_refs: vec![string(public_key)],
        config_refs: Vec::new(),
    };
    let writer = Writer {
        workspace: &reader.workspace_text,
        principal: &member,
        event_id: id,
        environment: "paper",
        now,
    };
    envelope(&writer, &event)
}

/// `principal`'s passkeys on the control stream, from `CredentialFold::passkeys`, or `None` when
/// the rows do not read or the fold refuses them (DEC-2124 items 1 and 2).
fn own_passkeys(app: &App, reader: &OwnReader, principal: PrincipalId) -> Option<Vec<Enrolment>> {
    let records = credential_records(app.hosted.workspace, &reader.control).ok()?;
    let fold = CredentialFold::new(records);
    let passkeys = fold.passkeys(principal).ok()?;
    Some(passkeys.into_iter().cloned().collect())
}

/// The instant `secs` seconds after `at`, to the nanosecond; `None` past the last instant the
/// journal can record.
pub(crate) fn later(at: UtcNanos, secs: i64) -> Option<UtcNanos> {
    UtcNanos::from_parts(at.secs().checked_add(secs)?, at.nanos()).ok()
}

/// Whether `body` is the empty object the request's schema admits, and nothing else.
fn empty_object(body: &[u8]) -> bool {
    matches!(mandate_canon::parse(body), Ok(Value::Object(members)) if members.is_empty())
}

/// `invalid` with its one violation at `/challenge_id`: the registration challenge is not held,
/// has expired, or names another principal or workspace (DEC-2108 item 5).
fn challenge_refused() -> Response {
    invalid(vec![Violation::Schema {
        path: "/challenge_id".to_owned(),
        code: "challenge".to_owned(),
        message: "Ask for a new registration challenge".to_owned(),
    }])
}

/// `step_up_stale` with its one violation at `/session` (DEC-2118 item 4).
fn stale_session() -> Response {
    let body = Problem::of(ProblemCode::StepUpStale, Effect::None, None, None)
        .ok()
        .map(|refusal| Problem {
            violations: vec![Violation::Schema {
                path: "/session".to_owned(),
                code: "step_up_stale".to_owned(),
                message: "Sign in again to add a first passkey".to_owned(),
            }],
            ..refusal
        })
        .and_then(|refusal| encode(&refusal).ok())
        .map(axum::body::Bytes::from)
        .unwrap_or_default();
    problem(StatusCode::UNAUTHORIZED, body)
}

/// The `201` (DEC-2108 item 4): the challenge, the relying party, the user handle, the member's
/// passkeys to exclude and whether step-up is needed, its instants, and the control stream's
/// watermark.
fn challenged(
    reader: &OwnReader,
    registration: &Registration,
    rp_id: &str,
    passkeys: &[Enrolment],
) -> Response {
    let exclude = passkeys
        .iter()
        .map(|enrolment| string(&enrolment.credential))
        .collect();
    let members = vec![
        ("challenge_id", string(&registration.id)),
        (
            "webauthn_challenge",
            string(&URL_SAFE_NO_PAD.encode(registration.challenge)),
        ),
        ("rp_id", string(rp_id)),
        (
            "user_handle",
            string(&URL_SAFE_NO_PAD.encode(registration.principal.0.to_be_bytes())),
        ),
        ("exclude", Value::Array(exclude)),
        ("step_up_needed", Value::Bool(!passkeys.is_empty())),
        ("issued_at", string(&registration.issued_at.to_string())),
        ("expires_at", string(&registration.expires_at.to_string())),
        ("as_of", as_of(&reader.control)),
    ];
    no_store(enveloped(StatusCode::CREATED, reader.now, members))
}

/// One passkey of the list: its id, algorithm, the `recorded_at` of the row that enrolled it, and
/// its cool-off's end.
fn listed(enrolment: &Enrolment, control: &[StoredEvent]) -> Value {
    let enrolled_at = control
        .iter()
        .find(|row| row.seq == enrolment.seq)
        .map_or(Value::Null, |row| string(&row.recorded_at));
    let alg = match enrolment.alg {
        PasskeyAlg::Es256 => "es256",
        PasskeyAlg::EdDsa => "eddsa",
    };
    object(vec![
        ("passkey_id", string(&enrolment.credential)),
        ("alg", string(alg)),
        ("enrolled_at", enrolled_at),
        (
            "cool_off_ends",
            enrolment
                .cool_off_ends
                .map_or(Value::Null, |ends| string(&ends.to_string())),
        ),
    ])
}

fn as_of(control: &[StoredEvent]) -> Value {
    Value::Array(watermark(control).into_iter().collect())
}

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
