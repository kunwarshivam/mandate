//! The owner-command routes (§4.2, §5.4, §3.11 item 10, DEC-1007): `POST
//! /v1/workspaces/{ws}/agents/{id}/pause` and `…/kill-switch`. The hold is not mounted until the
//! runtime folds it (DEC-1007 item 1).
//!
//! Each is checked in §3.3 item 1's order: the session (the one 401, or `reduction_only`), the
//! CSRF mark (403), then the `Idempotency-Key` and the body, read only then, by §5's lenient
//! decoder (422), then the
//! matrix row and the scope (403 or the one 404). Its `OwnerCommandIssued` is on the control stream
//! before the `202` (rule 5), and a repeat under the key replays it (API-4). Neither readiness nor
//! a drain gates them (API-7).

use std::sync::Arc;

use axum::body::{Body, Bytes, to_bytes};
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use mandate_api::idempotency::{Derivation, IdempotencyKey, event_id};
use mandate_api::lenient::{Api7, Kept, decode_lenient};
use mandate_api::problem::{Effect, Problem, ProblemCode};
use mandate_api::requests::{KillSwitchRequest, PauseRequest, Scope};
use mandate_api::wire::{EventId, Id, encode};
use mandate_authn::Request;
use mandate_canon::{Value, parse};
use mandate_journal::StoredEvent;
use mandate_time::UtcNanos;

use crate::commands::{Command, Issued, KillScope, Settled, draft, in_workspace, same, settle};
use crate::http::{App, browser, problem, problem_body};
use crate::routes::{
    Refused, Unreadable, authorized, enveloped, path_refused, session, string, watermark,
};
use crate::session::Authenticated;

/// The `Idempotency-Key` request header (§3.4).
const IDEMPOTENCY_KEY: &str = "idempotency-key";

/// `POST /v1/workspaces/{ws}/agents/{id}/pause`.
pub async fn pause(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
    body: Body,
) -> Response {
    let Ok(Path((workspace, agent))) = path else {
        return path_refused(&app, &headers, Request::Pause).await;
    };
    let accepted = admitted::<PauseRequest>(&app, &headers, Request::Pause, body).await;
    match accepted {
        Ok((gate, kept)) => {
            let command = Command::Pause(agent);
            recorded(&app, &gate, &workspace, &command, &kept.dropped, None).await
        }
        Err(refusal) => refusal.response(),
    }
}

/// `POST /v1/workspaces/{ws}/kill-switch`. `step_up_status` is `missing` or `unbound`: the server
/// forwards no step-up until it verifies one (DEC-1007 item 3).
pub async fn kill_switch(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
    body: Body,
) -> Response {
    let Ok(Path(workspace)) = path else {
        return path_refused(&app, &headers, Request::KillSwitch).await;
    };
    let (gate, kept) =
        match admitted::<KillSwitchRequest>(&app, &headers, Request::KillSwitch, body).await {
            Ok(accepted) => accepted,
            Err(refusal) => return refusal.response(),
        };
    let id = |id: &Id| id_text(id).unwrap_or_default();
    let scope = match &kept.value.scope {
        Scope::Agent { id: agent } => KillScope::Agent(id(agent)),
        Scope::Connection { id: connection } => KillScope::Connection(id(connection)),
        Scope::Workspace { .. } => KillScope::Workspace,
    };
    let status = match kept.value.step_up {
        Some(_) => "unbound",
        None => "missing",
    };
    let command = Command::KillSwitch(scope);
    recorded(
        &app,
        &gate,
        &workspace,
        &command,
        &kept.dropped,
        Some(status),
    )
    .await
}

/// A request past §3.3's checks: its session, the instant, and its key.
struct Gate {
    admitted: Authenticated,
    now: UtcNanos,
    key: IdempotencyKey,
}

/// The largest body read, in bytes: a command's members and a record's two references fit many
/// times over.
pub const MAX_BODY: usize = 65_536;

/// The session, the CSRF mark, the key, and the body, in that order: the body is read only once
/// the call is authenticated, so an unauthenticated one gets the one 401 whatever it sends.
async fn admitted<T: Api7>(
    app: &App,
    headers: &HeaderMap,
    request: Request,
    body: Body,
) -> Result<(Gate, Kept<T>), Refused> {
    let (admitted, now) = session(app, headers, request)?;
    if !browser(headers, &app.settings.app_origin) {
        return Err(Refused::Forbidden);
    }
    let key = one_key(headers).ok_or(Refused::Invalid)?;
    let body = to_bytes(body, MAX_BODY)
        .await
        .map_err(|_| Refused::Invalid)?;
    let kept = decode_lenient::<T>(&body).map_err(|_| Refused::Invalid)?;
    Ok((Gate { admitted, now, key }, kept))
}

/// The one `Idempotency-Key`, when it is sent once and is well formed.
fn one_key(headers: &HeaderMap) -> Option<IdempotencyKey> {
    let mut sent = headers.get_all(IDEMPOTENCY_KEY).iter();
    match (sent.next(), sent.next()) {
        (Some(key), None) => IdempotencyKey::parse(key.to_str().ok()?).ok(),
        _ => None,
    }
}

/// The text of an id, as it serializes.
fn id_text(id: &Id) -> Option<String> {
    json_string(&encode(id).ok()?)
}

/// The text of an event id, as it serializes.
fn event_text(id: &EventId) -> Option<String> {
    json_string(&encode(id).ok()?)
}

/// `bytes` as a JSON string's text.
fn json_string(bytes: &[u8]) -> Option<String> {
    parse(bytes).ok()?.as_str().map(str::to_owned)
}

/// Authorizes `command`, finds its scope, and records it, or replays the event its key already
/// recorded.
async fn recorded(
    app: &App,
    gate: &Gate,
    workspace: &str,
    command: &Command,
    dropped: &[String],
    status: Option<&str>,
) -> Response {
    let authorized = authorized(
        app,
        &gate.admitted,
        gate.now,
        workspace,
        command.permission(),
        Unreadable::Command,
    )
    .await;
    let reader = match authorized {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    let Some(environment) = reader.control.first().map(|row| row.environment.as_str()) else {
        return Refused::NotFound.response();
    };
    if !in_workspace(&reader.control, command) {
        return Refused::NotFound.response();
    }
    let Some((id, text)) = derived(gate, &reader.workspace, command) else {
        return Refused::JournalUnavailable.response();
    };
    let issued = Issued {
        workspace: &reader.workspace,
        principal: &principal_text(gate).unwrap_or_default(),
        event_id: &text,
        environment,
        now: gate.now,
        command,
    };
    let Some(bytes) = draft(&issued) else {
        return Refused::JournalUnavailable.response();
    };
    let stream = format!("ctl:{}", reader.workspace);
    let settled = match repeated(&reader.control, &text, &bytes) {
        Some(settled) => settled,
        None => match settle(app.journal.as_ref(), &stream, &text, &bytes, gate.now).await {
            Settled::Conflict => app
                .journal
                .rows(&stream)
                .await
                .ok()
                .and_then(|rows| repeated(&rows, &text, &bytes))
                .unwrap_or(Settled::Unknown),
            settled => settled,
        },
    };
    match settled {
        Settled::Stored(row) => accepted(&row, gate.now, dropped, status),
        Settled::Conflict => problem(
            StatusCode::CONFLICT,
            problem_body(ProblemCode::IdempotencyConflict),
        ),
        Settled::Unknown => unknown(id),
        Settled::Unavailable => Refused::JournalUnavailable.response(),
    }
}

/// What `rows` already hold under `id` (§3.4 step 1): the same command, replayed, or other content,
/// a conflict; `None` when nothing is stored under it.
fn repeated(rows: &[StoredEvent], id: &str, draft: &[u8]) -> Option<Settled> {
    let row = rows.iter().find(|row| row.event_id == id)?;
    Some(match same(row, draft) {
        true => Settled::Stored(Box::new(row.clone())),
        false => Settled::Conflict,
    })
}

fn principal_text(gate: &Gate) -> Option<String> {
    gate.admitted.principal.to_ulid_text().ok()
}

/// §3.4's event id for `command` under the gate's key, and its text.
fn derived(gate: &Gate, workspace: &str, command: &Command) -> Option<(EventId, String)> {
    let workspace = Id::try_from(workspace.to_owned()).ok()?;
    let principal = Id::try_from(principal_text(gate)?).ok()?;
    let id = event_id(&Derivation {
        workspace: &workspace,
        principal: &principal,
        operation: command.operation(),
        key: &gate.key,
        position: None,
    })
    .ok()?;
    let text = event_text(&id)?;
    Some((id, text))
}

/// The `202` for the stored `row` (§5.4): its id, `recorded`, and what this call dropped.
fn accepted(
    row: &StoredEvent,
    now: UtcNanos,
    dropped: &[String],
    status: Option<&str>,
) -> Response {
    let mut members = vec![
        ("command_id", string(&row.event_id)),
        ("phase", string("recorded")),
        (
            "as_of",
            Value::Array(watermark(std::slice::from_ref(row)).into_iter().collect()),
        ),
    ];
    if let Some(status) = status {
        members.push(("step_up_status", string(status)));
    }
    if !dropped.is_empty() {
        members.push((
            "dropped",
            Value::Array(dropped.iter().map(|pointer| string(pointer)).collect()),
        ));
    }
    enveloped(StatusCode::ACCEPTED, now, members)
}

/// 503 `outcome_unknown`, naming the event id the client polls (§3.5, §5.5).
fn unknown(id: EventId) -> Response {
    let body = Problem::of(ProblemCode::OutcomeUnknown, Effect::Unknown, Some(id), None)
        .ok()
        .and_then(|problem| encode(&problem).ok())
        .map(Bytes::from)
        .unwrap_or_default();
    problem(StatusCode::SERVICE_UNAVAILABLE, body)
}
