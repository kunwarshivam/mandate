//! The routes about the caller's own principal and session, outside every workspace path (§4.10;
//! identity spec §4.5; DEC-2113).
//!
//! `GET /v1/me/session` is on the authentication plane (identity spec §4.2, DEC-816 item 7): it
//! reads only the caller's own session record and yields no context. It serves the caller's
//! principal id, the session's kind, and when it ends untouched; never a session reference, the
//! cookie, a token, a digest, the subject, or the issuer.

use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use mandate_authn::Request;
use mandate_canon::Value;

use crate::http::{App, session_cookie};
use crate::routes::{self, Refused, enveloped, string};

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
