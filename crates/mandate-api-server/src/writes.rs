//! The owner-command routes (§4.2, §5.4, §3.11 item 10, DEC-1007): `POST
//! /v1/workspaces/{ws}/agents/{id}/pause`, `…/agents/{id}/hold`, and `…/kill-switch`.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::Response;

use crate::http::{App, unauthenticated};

/// `POST /v1/workspaces/{ws}/agents/{id}/pause`.
pub async fn pause(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
    body: Bytes,
) -> Response {
    let _ = (app, headers, path, body);
    unauthenticated()
}

/// `POST /v1/workspaces/{ws}/agents/{id}/hold`.
pub async fn hold(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
    body: Bytes,
) -> Response {
    let _ = (app, headers, path, body);
    unauthenticated()
}

/// `POST /v1/workspaces/{ws}/kill-switch`.
pub async fn kill_switch(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
    body: Bytes,
) -> Response {
    let _ = (app, headers, path, body);
    unauthenticated()
}
