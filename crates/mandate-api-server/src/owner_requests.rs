//! `POST /v1/workspaces/{ws}/agents/{agent}/requests` (§3.11 item 13, §4.6,
//! [DEC-2151](../../../docs/project/decisions/DEC-2151.md), E10-30): an owner's request for an
//! order, recorded as owner input for the runtime's order builder, gate and autonomy rules.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::Response;

use crate::http::App;

/// The route, a stub until E10-30 lands; its tests come first (DEC-77). It is mounted in debug
/// builds only (`http::api`): a release build aborts on a panic, so a release server never routes
/// to this stub, and answers the path the one 401 until E10-30 mounts the route itself.
#[expect(
    clippy::todo,
    reason = "E10-30's stub: its tests land first (DEC-77, DEC-137)"
)]
pub async fn owner_request(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
    body: Body,
) -> Response {
    let _ = (app, headers, path, body);
    todo!()
}
