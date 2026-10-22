//! The routes about the caller's own principal and session, outside every workspace path (§4.10;
//! identity spec §4.5; DEC-2113).
//!
//! `GET /v1/me/session` is on the authentication plane (identity spec §4.2, DEC-816 item 7): it
//! reads only the caller's own session record and yields no context. It serves the caller's
//! principal id, the session's kind, and when it ends untouched; never a session reference, the
//! cookie, a token, a digest, the subject, or the issuer.
//!
//! `GET /v1/me/workspaces` is the `self` row "List one's own workspace memberships": a full session
//! of a user principal, reading the caller's own row of the membership index, which this hosted
//! deployment keeps as its control stream's membership fold (DEC-2113 items 4 to 6).

use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use mandate_authn::Request;
use mandate_canon::Value;
use mandate_identity::{
    Authorized, LookupFailed, Permission, Principal, Refusal, Scope, Session, authorize,
};
use mandate_identity_seal::Seal;
use mandate_time::UtcNanos;

use crate::http::{App, session_cookie};
use crate::members::ControlLookup;
use crate::reduction;
use crate::routes::{self, Refused, enveloped, object, string};
use crate::session::Authenticated;

/// `GET /v1/me/session` (DEC-2113 items 1 to 3): `{principal_id, kind, expires_at}` with an
/// empty `as_of`, for a full session; the one 401 without one, and `reduction_only` for a session
/// an outage narrowed.
pub async fn session(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if let Some(answer) = passkey_session(&app, &headers).await {
        return answer;
    }
    let (admitted, now) = match routes::session(&app, &headers, Request::Other) {
        Ok(admitted) => admitted,
        Err(refused) => return refused.response(),
    };
    let expires =
        session_cookie(&headers).and_then(|cookie| app.sessions.expires_at(&cookie, now).ok());
    match (expires, admitted.principal.to_ulid_text()) {
        (Some(expires), Ok(principal)) => enveloped(
            StatusCode::OK,
            now,
            vec![
                ("principal_id", string(&principal)),
                ("kind", string("full")),
                ("expires_at", string(&expires.to_string())),
                ("as_of", Value::Array(Vec::new())),
            ],
        ),
        _ => Refused::Unauthenticated.response(),
    }
}

/// `GET /v1/me/workspaces` (DEC-2113 items 4 to 6): the caller's active membership of the hosted
/// workspace with the roles in effect, or none; the one 401 without a session, and
/// `reduction_only` for a session an outage narrowed.
pub async fn workspaces(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    match routes::session(&app, &headers, Request::Other) {
        Ok((admitted, now)) => own_workspaces(&app, &admitted, now).await,
        Err(refused) => refused.response(),
    }
}

/// The admitted caller's memberships: authorized by the `self` row at the principal's scope, then
/// read from the control stream's fold; `membership_unavailable` when it cannot be read.
async fn own_workspaces(app: &App, admitted: &Authenticated, now: UtcNanos) -> Response {
    let Ok(workspace) = app.hosted.workspace.to_ulid_text() else {
        return Refused::MembershipUnavailable.response();
    };
    let records = match app.journal.rows(&format!("ctl:{workspace}")).await {
        Ok(rows) => (app.members)(&rows),
        Err(_) => Err(LookupFailed),
    };
    let lookup = ControlLookup::new(app.hosted, records, now);
    let session = Session::new(Seal::grant(), admitted.reference, admitted.kind, Vec::new());
    let authorized = authorize(
        &lookup,
        &Principal::User {
            id: admitted.principal,
        },
        &session,
        Scope::Principal,
        Permission::ListOwnMemberships,
        now,
    );
    let caller = match authorized {
        Ok(Authorized::Principal { context, .. }) => context.principal(),
        Err(Refusal::ReductionOnly) => return Refused::ReductionOnly.response(),
        Ok(_) | Err(_) => return Refused::Forbidden.response(),
    };
    let rows = match lookup.own_roles(caller) {
        Ok(Some(roles)) => vec![object(vec![
            ("workspace_id", string(&workspace)),
            ("label", Value::Null),
            (
                "roles",
                Value::Array(roles.iter().filter_map(|r| r.code()).map(string).collect()),
            ),
        ])],
        Ok(None) => Vec::new(),
        Err(LookupFailed) => return Refused::MembershipUnavailable.response(),
    };
    enveloped(
        StatusCode::OK,
        now,
        vec![
            ("workspaces", Value::Array(rows)),
            ("as_of", Value::Array(Vec::new())),
        ],
    )
}

/// The answer for a session route 2 opened (§4.10 route 2 item 6): `kind: "reduction_only"`, its
/// absolute `expires_at`, and the workspaces it covers with what it may act on there; `None` for
/// any other cookie, which [`session`] answers as before. A session an outage narrowed is not
/// route 2's and is still refused `reduction_only`.
async fn passkey_session(app: &App, headers: &HeaderMap) -> Option<Response> {
    let cookie = session_cookie(headers)?;
    let now = app.clock.now()?;
    if !app.sessions.opened_by_passkey(&cookie, now) {
        return None;
    }
    let admitted = app
        .sessions
        .authenticate(&cookie, Request::Pause, now)
        .ok()?;
    let expires = app.sessions.expires_at(&cookie, now).ok()?;
    let principal = admitted.principal.to_ulid_text().ok()?;
    let workspaces = reduction::workspaces(app, admitted.principal, now).await?;
    Some(enveloped(
        StatusCode::OK,
        now,
        vec![
            ("principal_id", string(&principal)),
            ("kind", string("reduction_only")),
            ("expires_at", string(&expires.to_string())),
            ("workspaces", workspaces),
            ("as_of", Value::Array(Vec::new())),
        ],
    ))
}
