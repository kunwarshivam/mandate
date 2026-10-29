//! Command status (§5.5, §3.11 item 12, [DEC-2102](../../../docs/project/decisions/DEC-2102.md)
//! item 6): `GET /v1/workspaces/{ws}/commands/{event_id}`. A command's steps are the events whose
//! `causation_id` is it on each deployed agent's stream and each account stream the control stream
//! names, and its phase is the most final of theirs. Only the owning streams' events say what
//! happened (§6.3); a stream that cannot be read is `journal_unavailable`, never a shorter list.

use std::sync::Arc;

use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use mandate_authn::Request;
use mandate_canon::{Int, Key, Object, Value, parse};
use mandate_identity::Permission;
use mandate_journal::StoredEvent;

use crate::http::App;
use crate::reads::deployed;
use crate::routes::{Refused, Unreadable, authorized, enveloped, path_refused, session, watermark};

/// A command's phase (§5.5), in the order a later one overrides an earlier one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Phase {
    Recorded,
    Taken,
    Ended,
    Applied,
    Refused,
}

impl Phase {
    pub fn name(self) -> &'static str {
        match self {
            Self::Recorded => "recorded",
            Self::Taken => "taken",
            Self::Ended => "ended",
            Self::Applied => "applied",
            Self::Refused => "refused",
        }
    }
}

/// An acknowledgment's account-stream copy (journal spec §9.28): the executor's lift of its hold
/// names the copy as its cause, so a command's steps also hold what its copy caused (§5.5).
const COPY: &str = "OwnerAcknowledged";

/// What one owning-stream event says of the command that caused it.
pub fn phase_of(event_type: &str, payload: &Value) -> Phase {
    let result = payload.get("result").and_then(Value::as_str);
    match (event_type, result) {
        ("OwnerCommandRefused", _)
        | ("ApprovalResponded", Some("refused"))
        | ("MandateVersionApplied", Some("rejected")) => Phase::Refused,
        (
            "AgentModeChanged" | "AgentModeApplied" | "KillSwitchActivated" | "RiskLimitLifted",
            _,
        )
        | ("MandateVersionApplied", Some("applied"))
        | ("ApprovalRevalidated", Some("act")) => Phase::Applied,
        ("ApprovalRevalidated", Some("skip")) | ("ApprovalTimedOut" | "ApprovalCanceled", _) => {
            Phase::Ended
        }
        _ => Phase::Taken,
    }
}

/// The command's phase over `steps`, each an event type and payload: the most final, and
/// `recorded` with none.
pub fn phase<'a>(steps: impl IntoIterator<Item = (&'a str, &'a Value)>) -> Phase {
    steps
        .into_iter()
        .map(|(event_type, payload)| phase_of(event_type, payload))
        .max()
        .unwrap_or(Phase::Recorded)
}

/// A step's reason: its payload's `reason` when that is a lowercase code, else `None`.
pub fn reason(payload: &Value) -> Option<&str> {
    let reason = payload.get("reason").and_then(Value::as_str)?;
    let mut chars = reason.chars();
    let first = chars.next()?;
    let code = first.is_ascii_lowercase()
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    code.then_some(reason)
}

/// The account references the control stream names, each once, in the order first named.
pub fn accounts(control: &[StoredEvent]) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for row in control.iter().filter(|row| {
        matches!(
            row.event_type.as_str(),
            "ConnectionEstablished" | "ConnectionRequested"
        )
    }) {
        let reference = parse(&row.body)
            .ok()
            .and_then(|event| event.get("payload")?.get("account_ref").cloned())
            .and_then(|reference| reference.as_str().map(str::to_owned));
        if let Some(reference) = reference.filter(|r| !found.contains(r)) {
            found.push(reference);
        }
    }
    found
}

/// The rows of `rows` caused by `command`, with their payloads; `None` when a row does not parse,
/// so the read reports `journal_unavailable` rather than a shorter list.
fn caused<'a>(rows: &'a [StoredEvent], command: &str) -> Option<Vec<(&'a StoredEvent, Value)>> {
    let mut found = Vec::new();
    for row in rows {
        let event = parse(&row.body).ok()?;
        let cause = event.get("causation_id").and_then(Value::as_str);
        if cause == Some(command) {
            found.push((row, event.get("payload").cloned().unwrap_or(Value::Null)));
        }
    }
    Some(found)
}

fn object(members: Vec<(&str, Value)>) -> Value {
    let mut object = Object::new();
    for (name, value) in members {
        if let Ok(key) = Key::new(name) {
            object.insert(key, value);
        }
    }
    Value::Object(object)
}

fn string(text: &str) -> Value {
    Value::Str(text.to_owned())
}

/// `GET /v1/workspaces/{ws}/commands/{event_id}` (§5.5).
pub async fn status(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
) -> Response {
    let Ok(Path((workspace, command))) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    match reported(&app, &headers, &workspace, &command).await {
        Ok(response) => response,
        Err(refusal) => refusal.response(),
    }
}

async fn reported(
    app: &App,
    headers: &HeaderMap,
    workspace: &str,
    command: &str,
) -> Result<Response, Refused> {
    let (admitted, now) = session(app, headers, Request::Other)?;
    let reader = authorized(
        app,
        &admitted,
        now,
        workspace,
        Permission::ViewAgents,
        Unreadable::Read,
    )
    .await?;
    if !reader.control.iter().any(|row| row.event_id == command) {
        return Err(Refused::NotFound);
    }
    let agents = deployed(&reader.control)
        .into_iter()
        .map(|deployment| format!("agent:{}:{}", reader.workspace, deployment.0));
    let accounts = accounts(&reader.control)
        .into_iter()
        .map(|reference| format!("acct:{}:{reference}", reader.workspace));
    let mut read = Vec::new();
    for stream in agents.chain(accounts) {
        let rows = app
            .journal
            .rows(&stream)
            .await
            .map_err(|_| Refused::JournalUnavailable)?;
        read.push(rows);
    }
    let mut steps: Vec<(&StoredEvent, Value)> = Vec::new();
    for rows in &read {
        steps.extend(caused(rows, command).ok_or(Refused::JournalUnavailable)?);
    }
    let copies: Vec<String> = steps
        .iter()
        .filter(|(row, _)| row.event_type == COPY)
        .map(|(row, _)| row.event_id.clone())
        .collect();
    for copy in &copies {
        for rows in &read {
            steps.extend(caused(rows, copy).ok_or(Refused::JournalUnavailable)?);
        }
    }
    let streams: Vec<&str> = read
        .iter()
        .filter_map(|rows| rows.first().map(|row| row.stream_id.as_str()))
        .collect();
    let position = |row: &StoredEvent| {
        let stream = streams.iter().position(|s| *s == row.stream_id);
        (stream, row.seq)
    };
    steps.sort_by_key(|(row, _)| position(row));
    let phase = phase(
        steps
            .iter()
            .map(|(row, payload)| (row.event_type.as_str(), payload)),
    );
    let listed = steps
        .iter()
        .map(|(row, payload)| {
            object(vec![
                ("stream_id", string(&row.stream_id)),
                ("seq", Int::new(row.seq).map_or(Value::Null, Value::Int)),
                ("event_type", string(&row.event_type)),
                ("recorded_at", string(&row.recorded_at)),
                ("reason", reason(payload).map_or(Value::Null, string)),
            ])
        })
        .collect();
    let as_of = std::iter::once(reader.control.as_slice())
        .chain(read.iter().map(Vec::as_slice))
        .filter_map(watermark)
        .collect();
    Ok(enveloped(
        StatusCode::OK,
        now,
        vec![
            ("command_id", string(command)),
            ("phase", string(phase.name())),
            ("steps", Value::Array(listed)),
            ("as_of", Value::Array(as_of)),
        ],
    ))
}
