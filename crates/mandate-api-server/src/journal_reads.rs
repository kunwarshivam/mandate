//! The journal's audit reads over the workspace API (workspace API spec §4.8, §4.8.1; E12-6's route
//! slices, lane MB, DEC-760): `GET /v1/workspaces/{ws}/journal/streams/{stream_id}/events`, one
//! page of a stream in `seq` order, each event's stored canonical body bytes with its `hash` and
//! `prev_hash`.
//!
//! The matrix row "Read records, verification; export" is checked before any id is resolved, so a
//! viewer, an operator or an approver gets 403 `forbidden` whatever the id. Query members are
//! checked next, 422 `invalid` before any id is resolved (DEC-760 item 5). A stream id is resolved
//! only among the path's workspace's streams: one that is absent, malformed, empty, or another
//! workspace's is the one `not_found` (API-9, AU-1).

use std::sync::Arc;

use axum::extract::rejection::PathRejection;
use axum::extract::{Path, RawQuery, State};
use axum::http::HeaderMap;
use axum::response::Response;

use crate::http::App;

/// `GET /v1/workspaces/{ws}/journal/streams/{stream_id}/events?after_seq=&limit=` (§4.8.1, API-15).
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub async fn events(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
    RawQuery(query): RawQuery,
) -> Response {
    let _ = (app, headers, path, query);
    todo!()
}
