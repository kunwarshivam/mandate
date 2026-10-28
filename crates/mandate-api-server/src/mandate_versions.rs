//! `GET /v1/workspaces/{ws}/agents/{agent}/mandate-versions` (§4.7,
//! [DEC-2147](../../../docs/project/decisions/DEC-2147.md), E10-27): the agent's mandate versions,
//! oldest first, each with its classification and journaled times, and the version in effect.
//! Read-only.

use std::sync::Arc;

use axum::extract::rejection::PathRejection;
use axum::extract::{Path, RawQuery, State};
use axum::http::HeaderMap;
use axum::response::Response;

use crate::http::App;

/// The route, a stub until E10-27 lands; its tests come first (DEC-77).
#[expect(
    clippy::todo,
    reason = "E10-27's stub: its tests land first (DEC-77, DEC-137)"
)]
pub async fn agent_mandate_versions(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
    query: RawQuery,
) -> Response {
    let _ = (app, headers, path, query);
    todo!()
}
