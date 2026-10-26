//! The read routes (§4.3, §4.7, DEC-979): `GET /v1/workspaces/{ws}/agents`, `…/agents/{id}`,
//! `…/approvals`, and `…/approvals/{id}`.
//!
//! Each request is authenticated by its session (the one 401, or `reduction_only` for a live
//! session that reaches only pause and the kill switch), then authorized by `mandate-identity`'s
//! `authorize` over the hosted workspace's control-stream memberships for the matrix's "View agents,
//! positions, decisions" row (API-2). The streams are read only after that, under the tenant
//! context it returns (identity spec ID-8). An absent id, another workspace, and a workspace the
//! principal holds no membership of answer the same 404 (API-9); an id is found only by equality
//! with one the journal holds, so a malformed id is simply absent. Every value comes from journal
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
    Authorized, Permission, Principal, PrincipalId, Refusal, Scope, Session, TenantContext,
    WorkspaceId, authorize,
};
use mandate_identity_seal::Seal;
use mandate_journal::{ArtifactRef, StoredEvent};
use mandate_time::UtcNanos;

use crate::alerts::{AlertView, Followed, Stream, alerts, followed, recipient};
use crate::backtests::{
    Found, Outcome, backtest, backtest_report, evaluation, evaluation_report, paper,
};
use crate::connections::{ConnectionView, connections};
use crate::documents::{Envelope, checked, envelope};
use crate::http::{App, problem, problem_body, session_cookie, unauthenticated, with_type};
use crate::members::ControlLookup;
use crate::reads::{
    AgentView, ApprovalView, Deployment, Filter, account_positions, account_ref, agent,
    agent_orders, agent_positions, approvals, deployed, inbox,
};
use crate::session::Authenticated;

/// The build digest each response names (§3.2): the release build's own, given at compile time
/// as `MANDATE_BUILD_DIGEST`, or 64 zeros for a build that names none.
pub const BUILD: &str = match option_env!("MANDATE_BUILD_DIGEST") {
    Some(digest) => digest,
    None => "sha256:0000000000000000000000000000000000000000000000000000000000000000",
};

/// The workspace a request may read, once authorized, and the control stream's rows.
pub(crate) struct Reader {
    pub(crate) workspace: String,
    /// The member reading, whose own notice deliveries the alerts read serves (DEC-2141 item 4).
    pub(crate) principal: PrincipalId,
    /// The context `authorize` returned for the request (identity spec ID-8).
    pub(crate) tenant: TenantContext,
    pub(crate) control: Vec<StoredEvent>,
    pub(crate) now: UtcNanos,
}

/// Why a request refused, each answered by one problem document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refused {
    Unauthenticated,
    ReductionOnly,
    Forbidden,
    NotFound,
    Invalid,
    MembershipUnavailable,
    JournalUnavailable,
}

impl Refused {
    pub(crate) fn response(self) -> Response {
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
    let (admitted, now) = session(app, headers, Request::Other)?;
    authorized(
        app,
        &admitted,
        now,
        workspace,
        Permission::ViewAgents,
        Unreadable::Read,
    )
    .await
}

/// The session `headers` carry, admitted for `request` at the clock's instant: the one 401, or
/// `reduction_only` for a live session that reaches only pause and the kill switch.
pub(crate) fn session(
    app: &App,
    headers: &HeaderMap,
    request: Request,
) -> Result<(Authenticated, UtcNanos), Refused> {
    let cookie = session_cookie(headers).ok_or(Refused::Unauthenticated)?;
    let now = app.clock.now().ok_or(Refused::Unauthenticated)?;
    match app.sessions.authenticate(&cookie, request, now) {
        Ok(admitted) => Ok((admitted, now)),
        Err(_) if app.sessions.reduction_only(&cookie, now) => Err(Refused::ReductionOnly),
        Err(_) => Err(Refused::Unauthenticated),
    }
}

/// What an unreadable control stream means to the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unreadable {
    /// A read: its memberships cannot be read (`membership_unavailable`).
    Read,
    /// A command: nothing can be appended either (`journal_unavailable`), and a failed membership
    /// read refuses it too, until sessions keep a roles snapshot (DEC-1007 item 8).
    Command,
}

/// Authorizes `admitted` for `permission` on `workspace` and reads its control stream.
pub(crate) async fn authorized(
    app: &App,
    admitted: &Authenticated,
    now: UtcNanos,
    workspace: &str,
    permission: Permission,
    unreadable: Unreadable,
) -> Result<Reader, Refused> {
    let (authorized, control) =
        authorize_row(app, admitted, now, workspace, permission, unreadable).await?;
    match authorized {
        Ok(Authorized::Workspace { tenant, .. }) => Ok(Reader {
            workspace: workspace_text(tenant.workspace())?,
            principal: admitted.principal,
            tenant,
            control,
            now,
        }),
        other => Err(refused_by(other)),
    }
}

/// What an `own` row's route reads once `authorize` grants it: the workspace the membership is
/// used through, its control stream, and the instant (DEC-832 item 7).
pub(crate) struct OwnReader {
    pub(crate) workspace: WorkspaceId,
    pub(crate) workspace_text: String,
    pub(crate) control: Vec<StoredEvent>,
    pub(crate) now: UtcNanos,
}

/// Authorizes `admitted` for an `own` row's `permission` through its membership of `workspace`
/// and reads that workspace's control stream; `authorize` answers such a row for the principal,
/// not with a tenant context.
pub(crate) async fn own_authorized(
    app: &App,
    admitted: &Authenticated,
    now: UtcNanos,
    workspace: &str,
    permission: Permission,
    unreadable: Unreadable,
) -> Result<OwnReader, Refused> {
    let (authorized, control) =
        authorize_row(app, admitted, now, workspace, permission, unreadable).await?;
    match authorized {
        Ok(Authorized::Principal { .. }) => {
            let workspace =
                WorkspaceId::from_ulid_text(workspace).map_err(|_| Refused::NotFound)?;
            Ok(OwnReader {
                workspace_text: workspace_text(workspace)?,
                workspace,
                control,
                now,
            })
        }
        other => Err(refused_by(other)),
    }
}

/// The refusal for an answer a route does not serve.
fn refused_by(authorized: Result<Authorized, Refusal>) -> Refused {
    match authorized {
        Ok(_) | Err(Refusal::NoMembership) => Refused::NotFound,
        Err(Refusal::MembershipUnavailable) => Refused::MembershipUnavailable,
        Err(Refusal::ReductionOnly) => Refused::ReductionOnly,
        Err(_) => Refused::Forbidden,
    }
}

/// `authorize`'s answer for `admitted` and `permission` at `workspace`'s scope, and the control
/// stream it read.
async fn authorize_row(
    app: &App,
    admitted: &Authenticated,
    now: UtcNanos,
    workspace: &str,
    permission: Permission,
    unreadable: Unreadable,
) -> Result<(Result<Authorized, Refusal>, Vec<StoredEvent>), Refused> {
    let workspace = WorkspaceId::from_ulid_text(workspace).map_err(|_| Refused::NotFound)?;
    let hosted = app.hosted;
    let (records, control) = if workspace == hosted.workspace {
        let stream = format!("ctl:{}", workspace_text(workspace)?);
        match (app.journal.rows(&stream).await, unreadable) {
            (Ok(rows), _) => ((app.members)(&rows), rows),
            (Err(_), Unreadable::Read) => (Err(mandate_identity::LookupFailed), Vec::new()),
            (Err(_), Unreadable::Command) => return Err(Refused::JournalUnavailable),
        }
    } else {
        (Ok(Vec::new()), Vec::new())
    };
    if unreadable == Unreadable::Command && records.is_err() {
        return Err(Refused::MembershipUnavailable);
    }
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
        permission,
        now,
    );
    Ok((authorized, control))
}

pub(crate) fn workspace_text(workspace: WorkspaceId) -> Result<String, Refused> {
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

pub(crate) fn object(members: Vec<(&str, Value)>) -> Value {
    let mut object = Object::new();
    for (key, value) in members {
        if let Ok(key) = Key::new(key) {
            object.insert(key, value);
        }
    }
    Value::Object(object)
}

pub(crate) fn string(text: &str) -> Value {
    Value::Str(text.to_owned())
}

pub(crate) fn int(n: u64) -> Value {
    Int::new(n).map_or(Value::Null, Value::Int)
}

/// The watermark of a stream's last row (§6.1), or none for a stream with no row.
pub(crate) fn watermark(rows: &[StoredEvent]) -> Option<Value> {
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
    members.push(("as_of", Value::Array(as_of)));
    enveloped(StatusCode::OK, reader.now, members)
}

/// `members` with the envelope's (§3.2), as `status`; `members` carries `as_of`.
pub(crate) fn enveloped(
    status: StatusCode,
    now: UtcNanos,
    mut members: Vec<(&str, Value)>,
) -> Response {
    members.extend([
        ("api_version", string("v1")),
        ("build", string(BUILD)),
        ("served_at", string(&now.to_string())),
    ]);
    with_type(
        status,
        "application/json",
        to_canonical(&object(members)).into(),
    )
}

/// An agent's members, with the label and envelope of the mandate document its version names,
/// `null` when the store does not hold it (DEC-1020 item 2).
fn agent_members(view: &AgentView, document: Option<&Envelope>) -> Vec<(&'static str, Value)> {
    vec![
        ("agent_id", string(&view.agent_id)),
        ("environment", string(&view.environment)),
        ("mandate_version", string(&view.mandate_version)),
        ("deployed_at", string(&view.deployed_at)),
        ("mode", view.mode.as_deref().map_or(Value::Null, string)),
        ("pending_approvals", int(view.pending_approvals)),
        ("label", document.map_or(Value::Null, |d| string(&d.label))),
        (
            "mandate",
            document.map_or(Value::Null, |d| d.mandate.clone()),
        ),
    ]
}

/// The mandate document `deployment`'s version names, from the deployment's store.
fn document(app: &App, deployment: &Deployment) -> Option<Envelope> {
    let store = app.artifacts.as_ref()?;
    envelope(store.as_ref(), &deployment.2)
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
        ("order", view.order.clone()),
    ];
    if with_content {
        members.push(("content", view.content.clone()));
        members.push(("schema_version", int(view.schema_version)));
    }
    object(members)
}

/// `GET /v1/workspaces/{ws}/agents`.
pub async fn list_agents(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
) -> Response {
    let Ok(Path(workspace)) = path else {
        return path_refused(&app, &headers, Request::Other).await;
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
        .map(|(deployment, rows)| {
            let found = document(&app, deployment);
            object(agent_members(&agent(deployment, rows), found.as_ref()))
        })
        .collect();
    let streams: Vec<&[StoredEvent]> = read.iter().map(|(_, rows)| rows.as_slice()).collect();
    served(&reader, &streams, vec![("agents", Value::Array(agents))])
}

/// `GET /v1/workspaces/{ws}/connections` (§4.5, DEC-2620): `read-models/connections`, for the
/// workspace admin only (DEC-2631 item 1).
pub async fn list_connections(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
) -> Response {
    let Ok(Path(workspace)) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let reader = match connections_reader(&app, &headers, &workspace).await {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    let views = connections(&reader.control)
        .iter()
        .map(connection_value)
        .collect();
    served(&reader, &[], vec![("connections", Value::Array(views))])
}

/// `GET /v1/workspaces/{ws}/connections/{id}` (§4.5, DEC-2620): `read-models/connection`, the
/// role checked before the id (DEC-2631 item 2).
pub async fn one_connection(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
) -> Response {
    let Ok(Path((workspace, id))) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let reader = match connections_reader(&app, &headers, &workspace).await {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    let Some(view) = connections(&reader.control)
        .into_iter()
        .find(|view| view.connection_id == id)
    else {
        return Refused::NotFound.response();
    };
    served(&reader, &[], vec![("connection", connection_value(&view))])
}

/// The session, then the broker-connection row (DEC-2631 item 1), before anything is looked up.
async fn connections_reader(
    app: &App,
    headers: &HeaderMap,
    workspace: &str,
) -> Result<Reader, Refused> {
    let (admitted, now) = session(app, headers, Request::Other)?;
    authorized(
        app,
        &admitted,
        now,
        workspace,
        Permission::BrokerConnection,
        Unreadable::Read,
    )
    .await
}

/// `read-models/connection`'s `Connection` (DEC-2620 item 2).
fn connection_value(view: &ConnectionView) -> Value {
    let texts = |items: &[String]| Value::Array(items.iter().map(|item| string(item)).collect());
    object(vec![
        ("connection_id", string(&view.connection_id)),
        ("broker", string(&view.broker)),
        ("environment", string(&view.environment)),
        ("state", string(view.state.code())),
        ("scopes", view.scopes.as_deref().map_or(Value::Null, texts)),
        ("since", string(&view.since)),
        (
            "refusal",
            view.refusal.as_ref().map_or(Value::Null, |refusal| {
                object(vec![
                    (
                        "check",
                        refusal.check.as_deref().map_or(Value::Null, string),
                    ),
                    ("reason", string(&refusal.reason)),
                ])
            }),
        ),
        (
            "revoked_reason",
            view.revoked_reason.as_deref().map_or(Value::Null, string),
        ),
        ("agents", texts(&view.agents)),
    ])
}

/// `GET /v1/workspaces/{ws}/agents/{agent_id}`.
pub async fn one_agent(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
) -> Response {
    let Ok(Path((workspace, id))) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let reader = match reader(&app, &headers, &workspace).await {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    let Some(deployment) = deployed(&reader.control)
        .into_iter()
        .find(|deployment| deployment.0 == id)
    else {
        return Refused::NotFound.response();
    };
    let read = match agent_rows(&app, &reader, std::slice::from_ref(&deployment)).await {
        Ok(read) => read,
        Err(refusal) => return refusal.response(),
    };
    let Some((deployment, rows)) = read.first() else {
        return Refused::NotFound.response();
    };
    let found = document(&app, deployment);
    let account = match account_of(&app, &reader, found.as_ref()).await {
        Ok(account) => account.map(|(_, rows)| rows),
        Err(refusal) => return refusal.response(),
    };
    let mut members = agent_members(&agent(deployment, rows), found.as_ref());
    members.push((
        "account_positions",
        account
            .as_deref()
            .filter(|rows| !rows.is_empty())
            .map_or(Value::Null, |rows| Value::Array(account_positions(rows))),
    ));
    let mut streams: Vec<&[StoredEvent]> = vec![rows.as_slice()];
    streams.extend(account.as_deref());
    served(&reader, &streams, vec![("agent", object(members))])
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

/// The `account_ref` the control stream binds to the connection `document` names, and that
/// account stream's rows (DEC-1020 item 3); `None` with no document or no bound reference.
async fn account_of(
    app: &App,
    reader: &Reader,
    document: Option<&Envelope>,
) -> Result<Option<(String, Vec<StoredEvent>)>, Refused> {
    let Some(reference) = document.and_then(|d| account_ref(&reader.control, &d.connection_id))
    else {
        return Ok(None);
    };
    let stream = format!("acct:{}:{reference}", reader.workspace);
    match app.journal.rows(&stream).await {
        Ok(rows) => Ok(Some((reference, rows))),
        Err(_) => Err(Refused::JournalUnavailable),
    }
}

/// `GET /v1/workspaces/{ws}/agents/{agent_id}/orders` (§4.7 D3, DEC-2111): `read-models/orders`,
/// from the account stream of the connection the agent's deployed document names.
pub async fn agent_orders_read(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
) -> Response {
    let Ok(Path((workspace, id))) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let reader = match reader(&app, &headers, &workspace).await {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    let Some(deployment) = deployed(&reader.control)
        .into_iter()
        .find(|deployment| deployment.0 == id)
    else {
        return Refused::NotFound.response();
    };
    let account = match account_of(&app, &reader, document(&app, &deployment).as_ref()).await {
        Ok(account) => account,
        Err(refusal) => return refusal.response(),
    };
    let (reference, rows) = account.map_or((Value::Null, Vec::new()), |(reference, rows)| {
        (string(&reference), rows)
    });
    let orders = Value::Array(agent_orders(&rows, &deployment.0));
    let members = vec![
        ("agent_id", string(&deployment.0)),
        ("account_ref", reference),
        ("orders", orders),
    ];
    served(&reader, &[rows.as_slice()], members)
}

/// `GET /v1/workspaces/{ws}/agents/{agent_id}/positions` (§4.7 D3, DEC-2111):
/// `read-models/positions`, from the account stream of the connection the agent's deployed
/// document names. A fill of its own that cannot be read is `journal_unavailable`: no position is
/// served from part of its fills.
pub async fn agent_positions_read(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
) -> Response {
    let Ok(Path((workspace, id))) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let reader = match reader(&app, &headers, &workspace).await {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    let Some(deployment) = deployed(&reader.control)
        .into_iter()
        .find(|deployment| deployment.0 == id)
    else {
        return Refused::NotFound.response();
    };
    let account = match account_of(&app, &reader, document(&app, &deployment).as_ref()).await {
        Ok(account) => account,
        Err(refusal) => return refusal.response(),
    };
    let (reference, rows, holdings) = match account {
        Some((reference, rows)) => {
            let holdings = Value::Array(account_positions(&rows));
            (string(&reference), rows, holdings)
        }
        None => (Value::Null, Vec::new(), Value::Null),
    };
    let Some(positions) = agent_positions(&rows, &deployment.0) else {
        return Refused::JournalUnavailable.response();
    };
    let members = vec![
        ("agent_id", string(&deployment.0)),
        ("account_ref", reference),
        ("positions", Value::Array(positions)),
        ("account_positions", holdings),
    ];
    served(&reader, &[rows.as_slice()], members)
}

/// `GET /v1/workspaces/{ws}/approvals?state=pending|resolved`.
pub async fn list_approvals(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
    RawQuery(query): RawQuery,
) -> Response {
    let Ok(Path(workspace)) = path else {
        return path_refused(&app, &headers, Request::Other).await;
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

/// How many alerts `GET /alerts` serves without a `limit` (§4.7, DEC-2141 item 1).
const ALERTS_LISTED: usize = 50;

/// The most `limit` may ask for.
const ALERTS_LIMIT: usize = 200;

/// The alerts read's `limit`: [`ALERTS_LISTED`] without a query, else `limit=` and a decimal from
/// 1 to [`ALERTS_LIMIT`] with no leading zero; `None` for anything else.
fn alerts_limit(query: Option<&str>) -> Option<usize> {
    let Some(query) = query else {
        return Some(ALERTS_LISTED);
    };
    let digits = query.strip_prefix("limit=")?;
    if digits.is_empty() || digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits
        .parse::<usize>()
        .ok()
        .filter(|n| (1..=ALERTS_LIMIT).contains(n))
}

/// One alert as `read-models/alerts` states it.
fn alert_value(view: &AlertView) -> Value {
    let optional = |text: Option<&str>| text.map_or(Value::Null, string);
    let (stream_type, agent, account) = match &view.followed {
        Followed::Control => ("control", None, None),
        Followed::Agent(agent) => ("agent", Some(agent.as_str()), None),
        Followed::Account(account) => ("account", None, Some(account.as_str())),
    };
    let notice = view.notice.as_ref().map_or(Value::Null, |notice| {
        object(vec![
            ("issued_at", string(&notice.issued_at)),
            (
                "delivery",
                Value::Array(
                    notice
                        .delivery
                        .iter()
                        .map(|d| {
                            object(vec![
                                ("channel", string(&d.channel)),
                                ("status", string(&d.status)),
                                ("attempt", int(d.attempt)),
                                ("recorded_at", string(&d.recorded_at)),
                            ])
                        })
                        .collect(),
                ),
            ),
        ])
    });
    object(vec![
        ("alert_id", string(&view.alert_id)),
        ("kind", string(&view.kind)),
        ("class", string(view.class)),
        ("raised_at", string(&view.raised_at)),
        (
            "stream",
            object(vec![
                ("type", string(stream_type)),
                ("agent_id", optional(agent)),
                ("account_ref", optional(account)),
            ]),
        ),
        (
            "subject",
            object(vec![
                ("event_id", string(&view.subject_id)),
                ("event_type", string(&view.subject_type)),
            ]),
        ),
        ("owner_command", optional(view.owner_command.as_deref())),
        ("notice", notice),
    ])
}

/// `GET /v1/workspaces/{ws}/alerts` (§4.7, DEC-2141): `read-models/alerts`, the owner alerts on
/// every stream the control stream names, newest first, each with its notice and only the reading
/// member's own deliveries. Any stream that cannot be read, and any alert that cannot be served
/// whole, is `journal_unavailable`, never a shorter list.
pub async fn list_alerts(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
    RawQuery(query): RawQuery,
) -> Response {
    let Ok(Path(workspace)) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let reader = match reader(&app, &headers, &workspace).await {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    let Some(limit) = alerts_limit(query.as_deref()) else {
        return Refused::Invalid.response();
    };
    let Ok(member) = reader.principal.to_ulid_text() else {
        return Refused::NotFound.response();
    };
    let (agents, accounts) = followed(&reader.control);
    let mut read: Vec<(Followed, Vec<StoredEvent>)> = Vec::new();
    let named = agents
        .into_iter()
        .map(|agent| {
            (
                format!("agent:{}:{agent}", reader.workspace),
                Followed::Agent(agent),
            )
        })
        .chain(accounts.into_iter().map(|account| {
            (
                format!("acct:{}:{account}", reader.workspace),
                Followed::Account(account),
            )
        }));
    for (stream, followed) in named {
        match app.journal.rows(&stream).await {
            Ok(rows) => read.push((followed, rows)),
            Err(_) => return Refused::JournalUnavailable.response(),
        }
    }
    let Ok(notices) = app.journal.rows(&format!("ntf:{}", reader.workspace)).await else {
        return Refused::JournalUnavailable.response();
    };
    let streams: Vec<Stream<'_>> = std::iter::once(Stream {
        followed: Followed::Control,
        rows: &reader.control,
    })
    .chain(read.iter().map(|(followed, rows)| Stream {
        followed: followed.clone(),
        rows,
    }))
    .collect();
    let Ok(mut views) = alerts(&streams, &notices, &recipient(&member)) else {
        return Refused::JournalUnavailable.response();
    };
    let truncated = views.len() > limit;
    views.truncate(limit);
    let listed = views.iter().map(alert_value).collect();
    let watermarked: Vec<&[StoredEvent]> = read
        .iter()
        .map(|(_, rows)| rows.as_slice())
        .chain(std::iter::once(notices.as_slice()))
        .collect();
    served(
        &reader,
        &watermarked,
        vec![
            ("alerts", Value::Array(listed)),
            ("truncated", Value::Bool(truncated)),
        ],
    )
}

/// `GET /v1/workspaces/{ws}/approvals/{approval_id}`.
pub async fn one_approval(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
) -> Response {
    let Ok(Path((workspace, id))) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let reader = match reader(&app, &headers, &workspace).await {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
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

/// A path that does not decode: the one 401 without a session, `reduction_only` for a session that
/// reaches only pause and the kill switch, else the 404 an absent id gets.
pub(crate) async fn path_refused(app: &App, headers: &HeaderMap, request: Request) -> Response {
    let Some((cookie, now)) = session_cookie(headers).zip(app.clock.now()) else {
        return unauthenticated();
    };
    if app.sessions.authenticate(&cookie, request, now).is_ok() {
        Refused::NotFound.response()
    } else if app.sessions.reduction_only(&cookie, now) {
        Refused::ReductionOnly.response()
    } else {
        unauthenticated()
    }
}

/// The bytes the deployment's store holds under `reference`, digest-checked; `None` without a store.
fn stored(app: &App, reference: &ArtifactRef, name: &str) -> Option<Vec<u8>> {
    let store = app.artifacts.as_ref()?;
    checked(store.as_ref(), reference, name)
}

/// `GET /v1/workspaces/{ws}/backtests/{backtest_id}` (§4.2, DEC-1001 item 5): the workspace's own
/// backtest, from its `BacktestRunRecorded`. A completed run's report is read from the store and
/// checked against its digest; one the store lacks, or that fails its check, answers
/// `journal_unavailable`, since `read-models/backtest` serves no completed run without its report
/// (DEC-1143 item 4). `running` is never served: nothing journals a run in flight yet.
pub async fn one_backtest(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
) -> Response {
    let Ok(Path((workspace, id))) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let reader = match reader(&app, &headers, &workspace).await {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    let view = match backtest(&reader.control, &id) {
        Found::One(view) => view,
        Found::Absent => return Refused::NotFound.response(),
        Found::Unreadable => return Refused::JournalUnavailable.response(),
    };
    let (refusal, report_ref, report) = match &view.outcome {
        Outcome::Completed { report_ref } => {
            let Some(report) = stored(&app, report_ref, "backtest report")
                .and_then(|bytes| backtest_report(&bytes, &view).ok())
            else {
                return Refused::JournalUnavailable.response();
            };
            (Value::Null, string(&report_ref.to_string()), report)
        }
        Outcome::Refused { refusal } => (string(refusal), Value::Null, Value::Null),
    };
    let members = vec![
        ("backtest_id", string(&view.backtest_id)),
        ("mandate_version", string(&view.mandate_version)),
        ("state", string(view.state())),
        ("refusal", refusal),
        ("report_ref", report_ref),
        ("report", report),
    ];
    served(&reader, &[], vec![("backtest", object(members))])
}

/// `GET /v1/workspaces/{ws}/evaluations/{evaluation_id}` (§4.7, DEC-1002 items 6 and 7): the
/// founder's forward-paper evaluation, from its `EvaluationRunRecorded`, only where the deployment
/// is marked an internal paper workspace and its control stream is paper throughout. Elsewhere
/// every id answers the 404 an absent one does, even one the stream records (DEC-1143).
pub async fn one_evaluation(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
) -> Response {
    let Ok(Path((workspace, id))) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let reader = match reader(&app, &headers, &workspace).await {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    if !(app.hosted.internal_paper && paper(&reader.control)) {
        return Refused::NotFound.response();
    }
    let view = match evaluation(&reader.control, &id) {
        Found::One(view) => view,
        Found::Absent => return Refused::NotFound.response(),
        Found::Unreadable => return Refused::JournalUnavailable.response(),
    };
    let Some(report) = stored(&app, &view.report_ref, "evaluation report")
        .and_then(|b| evaluation_report(&b).ok())
    else {
        return Refused::JournalUnavailable.response();
    };
    let members = vec![
        ("evaluation_id", string(&view.evaluation_id)),
        ("report_ref", string(&view.report_ref.to_string())),
        ("report", report),
    ];
    served(&reader, &[], vec![("evaluation", object(members))])
}
