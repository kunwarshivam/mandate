//! The read routes (§4.3, §4.7, DEC-979): `GET /v1/workspaces/{ws}/agents`, `…/agents/{id}`,
//! `…/approvals`, and `…/approvals/{id}`.
//!
//! Each request is authenticated by its session (the one 401, or `reduction_only` for a live
//! session that reaches only pause and the kill switch), then authorized by `mandate-identity`'s
//! `authorize` over the hosted workspace's control-stream memberships for the matrix's "View agents,
//! positions, decisions" row (API-2). The streams are read only after that, under the tenant
//! context it returns (identity spec ID-8). An absent id, another workspace, and a workspace the
//! principal holds no membership of answer the same 404 (API-9). Every value comes from journal
//! rows, and `as_of` names the last row of each stream read (API-14).

use std::sync::Arc;

use axum::extract::rejection::PathRejection;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use mandate_api::problem::ProblemCode;
use mandate_authn::Request;
use mandate_canon::{Int, Key, Object, Value, to_canonical};
use mandate_identity::{
    Authorized, Permission, Principal, Refusal, Scope, Session, WorkspaceId, authorize,
};
use mandate_identity_seal::Seal;
use mandate_journal::StoredEvent;
use mandate_time::UtcNanos;

use crate::http::{App, problem, problem_body, session_cookie, unauthenticated, with_type};
use crate::members::ControlLookup;
use crate::reads::{
    AgentView, ApprovalView, Deployment, Filter, agent, approvals, deployed, inbox,
};

/// The build digest each response names (§3.2): the release build's own, given at compile time
/// as `MANDATE_BUILD_DIGEST`, or 64 zeros for a build that names none.
pub const BUILD: &str = match option_env!("MANDATE_BUILD_DIGEST") {
    Some(digest) => digest,
    None => "sha256:0000000000000000000000000000000000000000000000000000000000000000",
};

/// The workspace a request may read, once authorized, and the control stream's rows.
struct Reader {
    workspace: String,
    control: Vec<StoredEvent>,
    now: UtcNanos,
}

/// Why a read refused, each answered by one problem document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refused {
    Unauthenticated,
    ReductionOnly,
    Forbidden,
    NotFound,
    Invalid,
    MembershipUnavailable,
    JournalUnavailable,
}

impl Refused {
    fn response(self) -> Response {
        let (code, status) = match self {
            Self::Unauthenticated => return unauthenticated(),
            Self::ReductionOnly => (ProblemCode::ReductionOnly, StatusCode::FORBIDDEN),
            Self::Forbidden => (ProblemCode::Forbidden, StatusCode::FORBIDDEN),
            Self::NotFound => (ProblemCode::NotFound, StatusCode::NOT_FOUND),
            Self::Invalid => (ProblemCode::Invalid, StatusCode::UNPROCESSABLE_ENTITY),
            Self::MembershipUnavailable => (
                ProblemCode::MembershipUnavailable,
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            Self::JournalUnavailable => (
                ProblemCode::JournalUnavailable,
                StatusCode::SERVICE_UNAVAILABLE,
            ),
        };
        problem(status, problem_body(code))
    }
}

/// Authenticates, then authorizes the read of `workspace`, then reads its control stream.
async fn reader(app: &App, headers: &HeaderMap, workspace: &str) -> Result<Reader, Refused> {
    let cookie = session_cookie(headers).ok_or(Refused::Unauthenticated)?;
    let now = app.clock.now().ok_or(Refused::Unauthenticated)?;
    let admitted = match app.sessions.authenticate(&cookie, Request::Other, now) {
        Ok(admitted) => admitted,
        Err(_) if app.sessions.reduction_only(&cookie, now) => {
            return Err(Refused::ReductionOnly);
        }
        Err(_) => return Err(Refused::Unauthenticated),
    };
    let workspace = WorkspaceId::from_ulid_text(workspace).map_err(|_| Refused::NotFound)?;
    let hosted = app.hosted;
    let (records, control) = if workspace == hosted.workspace {
        let stream = format!("ctl:{}", workspace_text(workspace)?);
        match app.journal.rows(&stream).await {
            Ok(rows) => ((app.members)(&rows), rows),
            Err(_) => (Err(mandate_identity::LookupFailed), Vec::new()),
        }
    } else {
        (Ok(Vec::new()), Vec::new())
    };
    let lookup = ControlLookup::new(hosted, records, now);
    let session = Session::new(
        Seal::grant(),
        admitted.reference,
        admitted.kind,
        lookup.snapshot(admitted.principal),
    );
    let authorized = authorize(
        &lookup,
        &Principal::User {
            id: admitted.principal,
        },
        &session,
        Scope::Workspace {
            org: hosted.org,
            workspace,
        },
        Permission::ViewAgents,
        now,
    );
    match authorized {
        Ok(Authorized::Workspace { tenant, .. }) => Ok(Reader {
            workspace: workspace_text(tenant.workspace())?,
            control,
            now,
        }),
        Ok(_) | Err(Refusal::NoMembership) => Err(Refused::NotFound),
        Err(Refusal::MembershipUnavailable) => Err(Refused::MembershipUnavailable),
        Err(Refusal::ReductionOnly) => Err(Refused::ReductionOnly),
        Err(_) => Err(Refused::Forbidden),
    }
}

fn workspace_text(workspace: WorkspaceId) -> Result<String, Refused> {
    workspace.to_ulid_text().map_err(|_| Refused::NotFound)
}

/// The rows of each deployed agent's stream, after the control stream's.
async fn agent_rows(
    app: &App,
    reader: &Reader,
    deployments: &[Deployment],
) -> Result<Vec<(Deployment, Vec<StoredEvent>)>, Refused> {
    let mut read = Vec::with_capacity(deployments.len());
    for deployment in deployments {
        let stream = format!("agent:{}:{}", reader.workspace, deployment.0);
        let rows = app
            .journal
            .rows(&stream)
            .await
            .map_err(|_| Refused::JournalUnavailable)?;
        read.push((deployment.clone(), rows));
    }
    Ok(read)
}

fn object(members: Vec<(&str, Value)>) -> Value {
    let mut object = Object::new();
    for (key, value) in members {
        if let Ok(key) = Key::new(key) {
            object.insert(key, value);
        }
    }
    Value::Object(object)
}

fn string(text: &str) -> Value {
    Value::Str(text.to_owned())
}

fn int(n: u64) -> Value {
    Int::new(n).map_or(Value::Null, Value::Int)
}

/// The watermark of a stream's last row (§6.1), or none for a stream with no row.
fn watermark(rows: &[StoredEvent]) -> Option<Value> {
    rows.last().map(|row| {
        object(vec![
            ("stream_id", string(&row.stream_id)),
            ("seq", int(row.seq)),
            ("hash", string(&format!("sha256:{}", row.hash))),
            ("recorded_at", string(&row.recorded_at)),
        ])
    })
}

/// A 200 with the envelope (§3.2, §6.1) and `members`.
fn served(
    reader: &Reader,
    streams: &[&[StoredEvent]],
    mut members: Vec<(&str, Value)>,
) -> Response {
    let as_of = std::iter::once(reader.control.as_slice())
        .chain(streams.iter().copied())
        .filter_map(watermark)
        .collect();
    members.extend([
        ("api_version", string("v1")),
        ("build", string(BUILD)),
        ("served_at", string(&reader.now.to_string())),
        ("as_of", Value::Array(as_of)),
    ]);
    with_type(
        StatusCode::OK,
        "application/json",
        to_canonical(&object(members)).into(),
    )
}

fn agent_value(view: &AgentView) -> Value {
    object(vec![
        ("agent_id", string(&view.agent_id)),
        ("environment", string(&view.environment)),
        ("mandate_version", string(&view.mandate_version)),
        ("deployed_at", string(&view.deployed_at)),
        ("mode", view.mode.as_deref().map_or(Value::Null, string)),
        ("pending_approvals", int(view.pending_approvals)),
    ])
}

fn approval_value(view: &ApprovalView, with_content: bool) -> Value {
    let mut members = vec![
        ("approval_id", string(&view.approval_id)),
        ("agent_id", string(&view.agent_id)),
        ("state", string(view.state.name())),
        ("requested_at", string(&view.requested_at)),
        (
            "deadline",
            int(u64::try_from(view.deadline).unwrap_or(u64::MAX)),
        ),
        ("content_hash", string(&view.content_hash)),
    ];
    if with_content {
        members.push(("content", view.content.clone()));
    }
    object(members)
}

/// Whether `id` is journal spec §2's identifier, at most 64 characters (common `Id`).
fn is_agent_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Whether `id` is an event id: 26 Crockford base-32 digits, the first at most `7`.
fn is_event_id(id: &str) -> bool {
    mandate_canon::is_ulid(id)
}

/// `GET /v1/workspaces/{ws}/agents`.
pub async fn list_agents(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
) -> Response {
    let Ok(Path(workspace)) = path else {
        return path_refused(&app, &headers).await;
    };
    let reader = match reader(&app, &headers, &workspace).await {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    let deployments = deployed(&reader.control);
    let read = match agent_rows(&app, &reader, &deployments).await {
        Ok(read) => read,
        Err(refusal) => return refusal.response(),
    };
    let agents = read
        .iter()
        .map(|(deployment, rows)| agent_value(&agent(deployment, rows)))
        .collect();
    let streams: Vec<&[StoredEvent]> = read.iter().map(|(_, rows)| rows.as_slice()).collect();
    served(&reader, &streams, vec![("agents", Value::Array(agents))])
}

/// `GET /v1/workspaces/{ws}/agents/{agent_id}`.
pub async fn one_agent(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
) -> Response {
    let Ok(Path((workspace, id))) = path else {
        return path_refused(&app, &headers).await;
    };
    let reader = match reader(&app, &headers, &workspace).await {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    let Some(deployment) = deployed(&reader.control)
        .into_iter()
        .find(|deployment| is_agent_id(&id) && deployment.0 == id)
    else {
        return Refused::NotFound.response();
    };
    let read = match agent_rows(&app, &reader, std::slice::from_ref(&deployment)).await {
        Ok(read) => read,
        Err(refusal) => return refusal.response(),
    };
    let streams: Vec<&[StoredEvent]> = read.iter().map(|(_, rows)| rows.as_slice()).collect();
    let views: Vec<Value> = read
        .iter()
        .map(|(deployment, rows)| agent_value(&agent(deployment, rows)))
        .collect();
    match views.into_iter().next() {
        Some(view) => served(&reader, &streams, vec![("agent", view)]),
        None => Refused::NotFound.response(),
    }
}

/// Every approval of every deployed agent, with the streams read.
async fn all_approvals(
    app: &App,
    reader: &Reader,
) -> Result<(Vec<ApprovalView>, Vec<Vec<StoredEvent>>), Refused> {
    let read = agent_rows(app, reader, &deployed(&reader.control)).await?;
    let views = read
        .iter()
        .flat_map(|(deployment, rows)| approvals(&deployment.0, rows))
        .collect();
    Ok((views, read.into_iter().map(|(_, rows)| rows).collect()))
}

/// `GET /v1/workspaces/{ws}/approvals?state=pending|resolved`.
pub async fn list_approvals(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
    RawQuery(query): RawQuery,
) -> Response {
    let Ok(Path(workspace)) = path else {
        return path_refused(&app, &headers).await;
    };
    let reader = match reader(&app, &headers, &workspace).await {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    let filter = match query.as_deref() {
        None => Filter::All,
        Some("state=pending") => Filter::Pending,
        Some("state=resolved") => Filter::Resolved,
        Some(_) => return Refused::Invalid.response(),
    };
    let (views, rows) = match all_approvals(&app, &reader).await {
        Ok(read) => read,
        Err(refusal) => return refusal.response(),
    };
    let listed = inbox(views, filter)
        .iter()
        .map(|view| approval_value(view, false))
        .collect();
    let streams: Vec<&[StoredEvent]> = rows.iter().map(Vec::as_slice).collect();
    served(&reader, &streams, vec![("approvals", Value::Array(listed))])
}

/// `GET /v1/workspaces/{ws}/approvals/{approval_id}`.
pub async fn one_approval(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
) -> Response {
    let Ok(Path((workspace, id))) = path else {
        return path_refused(&app, &headers).await;
    };
    let reader = match reader(&app, &headers, &workspace).await {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    if !is_event_id(&id) {
        return Refused::NotFound.response();
    }
    let (views, rows) = match all_approvals(&app, &reader).await {
        Ok(read) => read,
        Err(refusal) => return refusal.response(),
    };
    let streams: Vec<&[StoredEvent]> = rows.iter().map(Vec::as_slice).collect();
    match views.iter().find(|view| view.approval_id == id) {
        Some(view) => served(
            &reader,
            &streams,
            vec![("approval", approval_value(view, true))],
        ),
        None => Refused::NotFound.response(),
    }
}

/// A path that does not decode: the one 401 without a session, else the 404 an absent id gets.
async fn path_refused(app: &App, headers: &HeaderMap) -> Response {
    let live = session_cookie(headers)
        .zip(app.clock.now())
        .is_some_and(|(cookie, now)| {
            app.sessions
                .authenticate(&cookie, Request::Other, now)
                .is_ok()
        });
    if live {
        Refused::NotFound.response()
    } else {
        unauthenticated()
    }
}
