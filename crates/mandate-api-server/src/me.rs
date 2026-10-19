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
use mandate_time::UtcNanos;

use crate::http::{App, session_cookie};
use crate::routes::{self, Refused, enveloped, string};
use crate::session::Authenticated;

/// `GET /v1/me/session` (DEC-2113 items 1 to 3): `{principal_id, kind, expires_at}` with an
/// empty `as_of`, for a full session; the one 401 without one, and `reduction_only` for a session
/// an outage narrowed.
pub async fn session(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
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
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
async fn own_workspaces(_app: &App, _admitted: &Authenticated, _now: UtcNanos) -> Response {
    todo!()
}
