//! The journal's audit reads over the workspace API (workspace API spec §4.8, §4.8.1; E12-6's route
//! slices, lane MB, DEC-760): `GET /v1/workspaces/{ws}/journal/streams/{stream_id}/events`, one
//! page of a stream in `seq` order, each event's stored canonical body bytes with its `hash` and
//! `prev_hash`; and `GET /v1/workspaces/{ws}/journal/events/{event_id}`, one event in the same
//! shape with its `stream_id` and its `artifact_refs` and `config_refs` as stored.
//!
//! The matrix row "Read records, verification; export" is checked before any id is resolved, so a
//! viewer, an operator or an approver gets 403 `forbidden` whatever the id. Query members are
//! checked next, 422 `invalid` before any id is resolved (DEC-760 item 5). A stream id is resolved
//! only among the path's workspace's streams: one that is absent, malformed, empty, or another
//! workspace's is the one `not_found` (API-9, AU-1).

use std::sync::Arc;

use axum::extract::rejection::PathRejection;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use mandate_api::problem::Violation;
use mandate_authn::Request;
use mandate_canon::Value;
use mandate_identity::Permission;
use mandate_journal::{StoredEvent, StreamId};

use crate::http::{App, invalid};
use crate::routes::{
    Refused, Unreadable, authorized, enveloped, int, object, path_refused, session, string,
    watermark,
};

/// The page size when the request names none (DEC-760 item 1).
const DEFAULT_LIMIT: u64 = 100;

/// The largest page (DEC-760 item 1).
const MAX_LIMIT: u64 = 1_000;

/// The largest `after_seq`: 2^53 − 1, the largest canonical integer (journal spec §4 rule 4).
const MAX_AFTER_SEQ: u64 = 9_007_199_254_740_991;

/// The base64 alphabet of RFC 4648 §4.
const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `GET /v1/workspaces/{ws}/journal/streams/{stream_id}/events?after_seq=&limit=` (§4.8.1, API-15).
pub async fn events(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
    RawQuery(query): RawQuery,
) -> Response {
    let Ok(Path((workspace, stream))) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let (admitted, now) = match session(&app, &headers, Request::Other) {
        Ok(admitted) => admitted,
        Err(refusal) => return refusal.response(),
    };
    let reader = match authorized(
        &app,
        &admitted,
        now,
        &workspace,
        Permission::ReadRecords,
        Unreadable::Read,
    )
    .await
    {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    let (after_seq, limit) = match cursor(query.as_deref()) {
        Ok(cursor) => cursor,
        Err(path) => {
            return invalid(vec![Violation::Schema {
                path: path.to_owned(),
                code: "out_of_range".to_owned(),
                message: "The query member is not an integer in its bounds.".to_owned(),
            }]);
        }
    };
    let Some(stream) = StreamId::parse(&stream).filter(|id| owned(&reader.workspace, id)) else {
        return Refused::NotFound.response();
    };
    let rows = match app.journal.rows(stream.as_str()).await {
        Ok(rows) => rows,
        Err(_) => return Refused::JournalUnavailable.response(),
    };
    let Some(head) = rows.last() else {
        return Refused::NotFound.response();
    };
    let served: Vec<&StoredEvent> = rows
        .iter()
        .filter(|row| row.seq > after_seq)
        .take(usize::try_from(limit).unwrap_or(usize::MAX))
        .collect();
    let next_after_seq = served.last().map_or(after_seq, |row| row.seq);
    let members = vec![
        ("stream_id", string(&head.stream_id)),
        (
            "head",
            object(vec![
                ("seq", int(head.seq)),
                ("hash", string(&head.hash.to_string())),
                ("recorded_at", string(&head.recorded_at)),
            ]),
        ),
        (
            "events",
            Value::Array(served.into_iter().map(event).collect()),
        ),
        ("next_after_seq", int(next_after_seq)),
        ("at_head", Value::Bool(next_after_seq >= head.seq)),
        (
            "as_of",
            Value::Array(watermark(&rows).into_iter().collect()),
        ),
    ];
    enveloped(StatusCode::OK, now, members)
}

/// `GET /v1/workspaces/{ws}/journal/events/{event_id}` (§4.8.1): the event when its stream is one
/// of the workspace's, else the one `not_found`.
pub async fn one_event(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
) -> Response {
    let Ok(Path((workspace, event_id))) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let (admitted, now) = match session(&app, &headers, Request::Other) {
        Ok(admitted) => admitted,
        Err(refusal) => return refusal.response(),
    };
    let reader = match authorized(
        &app,
        &admitted,
        now,
        &workspace,
        Permission::ReadRecords,
        Unreadable::Read,
    )
    .await
    {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    let row = match app.journal.event(&event_id).await {
        Ok(row) => row,
        Err(_) => return Refused::JournalUnavailable.response(),
    };
    let Some((row, stream)) = row.and_then(|row| {
        StreamId::parse(&row.stream_id)
            .filter(|id| owned(&reader.workspace, id))
            .map(|stream| (row, stream))
    }) else {
        return Refused::NotFound.response();
    };
    let rows = match app.journal.rows(stream.as_str()).await {
        Ok(rows) => rows,
        Err(_) => return Refused::JournalUnavailable.response(),
    };
    let envelope = mandate_canon::parse(&row.body).ok();
    let stored = |name: &str| {
        envelope
            .as_ref()
            .and_then(|envelope| envelope.get(name))
            .cloned()
            .unwrap_or(Value::Null)
    };
    let mut members = vec![
        ("stream_id", string(&row.stream_id)),
        ("artifact_refs", stored("artifact_refs")),
        ("config_refs", stored("config_refs")),
    ];
    members.extend(fields(&row));
    let served = vec![
        ("event", object(members)),
        (
            "as_of",
            Value::Array(watermark(&rows).into_iter().collect()),
        ),
    ];
    enveloped(StatusCode::OK, now, served)
}

/// Whether `stream` is one of the workspace's: [`mandate_journal::workspace_owns`], whose segment
/// comparison never matches by prefix (DEC-770 item 1) and whose deployment meter stream is no
/// workspace's (journal spec v0.53 §2).
fn owned(workspace: &str, stream: &StreamId) -> bool {
    mandate_journal::workspace_owns(workspace, stream)
}

/// The request's `after_seq` and `limit`, each defaulted when absent, or the path of the first that
/// is not an integer in its bounds; a value is never clamped (DEC-760 item 1).
fn cursor(query: Option<&str>) -> Result<(u64, u64), &'static str> {
    let value = |name: &str| {
        query
            .unwrap_or_default()
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value)
    };
    let after_seq = bounded(value("after_seq"), 0, 0, MAX_AFTER_SEQ, "/after_seq")?;
    let limit = bounded(value("limit"), DEFAULT_LIMIT, 1, MAX_LIMIT, "/limit")?;
    Ok((after_seq, limit))
}

/// `value` as an integer in `low..=high`, `default` when absent, or `path`.
fn bounded(
    value: Option<&str>,
    default: u64,
    low: u64,
    high: u64,
    path: &'static str,
) -> Result<u64, &'static str> {
    let Some(value) = value else {
        return Ok(default);
    };
    value
        .bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| value.parse::<u64>().ok())
        .flatten()
        .filter(|n| (low..=high).contains(n))
        .ok_or(path)
}

/// One served event (§4.8.1): its `seq`, ids, `recorded_at`, bare hashes, and body as base64.
fn event(row: &StoredEvent) -> Value {
    object(fields(row))
}

/// The members every served event carries.
fn fields(row: &StoredEvent) -> Vec<(&'static str, Value)> {
    vec![
        ("seq", int(row.seq)),
        ("event_id", string(&row.event_id)),
        ("event_type", string(&row.event_type)),
        ("recorded_at", string(&row.recorded_at)),
        ("prev_hash", string(&row.prev_hash.to_string())),
        ("hash", string(&row.hash.to_string())),
        ("body", string(&base64(&row.body))),
    ]
}

/// `bytes` as RFC 4648 §4 standard base64, with padding.
fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3).saturating_mul(4));
    for chunk in bytes.chunks(3) {
        let [a, b, c] = [0, 1, 2].map(|i| chunk.get(i).copied().unwrap_or(0));
        let n = u32::from_be_bytes([0, a, b, c]);
        for i in 0..4_usize {
            if i <= chunk.len() {
                let shift =
                    18_u32.saturating_sub(6_u32.saturating_mul(u32::try_from(i).unwrap_or(0)));
                let index = usize::try_from((n >> shift) & 63).unwrap_or(0);
                out.push(char::from(BASE64.get(index).copied().unwrap_or(b'=')));
            } else {
                out.push('=');
            }
        }
    }
    out
}
