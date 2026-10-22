//! The passkey routes (workspace API spec §3.6, [DEC-2108] items 4 and 5, [DEC-2115],
//! [DEC-2118], [DEC-2124]): `POST /v1/workspaces/{ws}/passkeys/registration-challenges` and
//! `GET /v1/workspaces/{ws}/passkeys`. The enrolment, `POST /v1/workspaces/{ws}/passkeys`, lands
//! with its own slice.
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
use axum::http::HeaderMap;
use axum::response::Response;

use crate::http::App;

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
/// DEC-2118 item 1, DEC-2124).
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub async fn registration_challenge(
    State(_app): State<Arc<App>>,
    _headers: HeaderMap,
    _path: Result<Path<String>, PathRejection>,
    _body: Body,
) -> Response {
    todo!()
}

/// `GET /v1/workspaces/{ws}/passkeys` (DEC-2115 item 3, DEC-2124 items 1 to 3).
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub async fn list(
    State(_app): State<Arc<App>>,
    _headers: HeaderMap,
    _path: Result<Path<String>, PathRejection>,
) -> Response {
    todo!()
}
