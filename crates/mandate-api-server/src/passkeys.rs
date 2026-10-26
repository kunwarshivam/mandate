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
use mandate_api::problem::{Effect, Problem, ProblemCode, Violation};
use mandate_api::wire::encode;
use mandate_authn::Request;
use mandate_canon::Value;
use mandate_identity::{AssertionId, Permission, PrincipalId};
use mandate_journal::StoredEvent;
use mandate_passkey::credentials::{CredentialFold, Enrolment, PasskeyAlg};
use mandate_time::UtcNanos;

use crate::backtests::paper;
use crate::challenges::{Registration, Withheld};
use crate::http::{App, problem, problem_body};
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
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub async fn enrol(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
    body: Body,
) -> Response {
    let _ = (app, headers, path, body);
    todo!()
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
