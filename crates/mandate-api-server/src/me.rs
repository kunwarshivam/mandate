//! The routes about the caller's own principal and session, outside every workspace path (§4.10;
//! identity spec §4.5; DEC-2113).
//!
//! `GET /v1/me/session` is on the authentication plane (identity spec §4.2, DEC-816 item 7): it
//! reads only the caller's own session record and yields no context. It serves the caller's
//! principal id, the session's kind, and when it ends untouched; never a session reference, the
//! cookie, a token, a digest, the subject, or the issuer.

use std::sync::Arc;

use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::Response;

use crate::http::App;

/// `GET /v1/me/session` (DEC-2113 items 1 to 3): `{principal_id, kind, expires_at}` with an
/// empty `as_of`, for a full session; the one 401 without one, and `reduction_only` for a session
/// an outage narrowed.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub async fn session(State(_app): State<Arc<App>>, _headers: HeaderMap) -> Response {
    todo!()
}
