//! `POST /v1/workspaces/{ws}/agents/{agent}/requests` (§3.11 item 13, §4.6,
//! [DEC-2151](../../../docs/project/decisions/DEC-2151.md), E10-30): an owner's request for an
//! order, recorded as owner input for the runtime's order builder, gate and autonomy rules.
//!
//! It places nothing and decides nothing: it is refused, with nothing committed, at the first of
//! item 13's checks that fails, and otherwise commits one `OwnerRequestSubmitted` (journal spec
//! §9.9) under §3.4's id, `requested_by` `owner` from the session (API-6). The request is a buy of
//! the agent's own pinned instrument with a positive size (DEC-2151 items 1, 3 and 7), so it never
//! widens the envelope, the builder only clips it, and an owner's sale stays the owner exit's.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use mandate_api::problem::{ProblemCode, Violation};
use mandate_api::requests::{OwnerRequest, Side};
use mandate_api::wire::decode;
use mandate_authn::Request;
use mandate_canon::Value;
use mandate_identity::Permission;

use crate::backtests::paper;
use crate::commands::Command;
use crate::http::{App, invalid, problem, problem_body};
use crate::reads::{Deployment, deployed};
use crate::routes::{Reader, Refused, Unreadable, authorized, path_refused};
use crate::versions::stored;
use crate::writes::{Gate, asset_text, committed, committed_caused, decimal_text, strictly};

/// The route: §3.3's checks and the strict body, the "Make an owner request" row for a user, then
/// the agent, the environment, the side, the size, and the instrument, in item 13's order.
pub async fn owner_request(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
    body: Body,
) -> Response {
    let Ok(Path((workspace, agent))) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let (gate, request) = match strictly(&app, &headers, body, decode::<OwnerRequest>).await {
        Ok(admitted) => admitted,
        Err(refusal) => return *refusal,
    };
    let reader = match authorized(
        &app,
        &gate.admitted,
        gate.now,
        &workspace,
        Permission::OwnerRequest,
        Unreadable::Command,
    )
    .await
    {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    let Some(deployment) = live_deployment(&reader.control, &agent) else {
        return Refused::NotFound.response();
    };
    if !paper(&reader.control) {
        return problem(
            StatusCode::CONFLICT,
            problem_body(ProblemCode::LiveUnavailable),
        );
    }
    if request.side != Side::Buy {
        return invalid(vec![violation("/side", "buy_only")]);
    }
    let Some(quantity) = request
        .size
        .as_ref()
        .and_then(decimal_text)
        .filter(|size| positive(size))
    else {
        return invalid(vec![violation("/size", "positive")]);
    };
    let Some(instrument) = asset_text(&request.instrument) else {
        return invalid(vec![violation("/instrument", "asset")]);
    };
    submit(
        &app,
        &gate,
        &reader,
        (deployment, instrument, quantity),
        None,
    )
    .await
}

/// Item 13's last check and the commit, which the chat's *Confirm an order* shares (workspace API
/// §4.6.2, [DEC-2859] item 8): the instrument must be one the deployed mandate pins, and the
/// `OwnerRequestSubmitted` names `cause` as its `causation_id` when there is one.
///
/// [DEC-2859]: ../../../docs/project/decisions/DEC-2859.md
pub(crate) async fn submit(
    app: &App,
    gate: &Gate,
    reader: &Reader,
    (deployment, instrument, quantity): (Deployment, String, String),
    cause: Option<&str>,
) -> Response {
    match pins(app, &deployment, &instrument) {
        Some(true) => {}
        Some(false) => return invalid(vec![violation("/instrument", "not_pinned")]),
        None => return Refused::JournalUnavailable.response(),
    }
    let command = Command::OwnerRequest {
        agent: deployment.0,
        instrument,
        quantity,
    };
    match cause {
        Some(cause) => committed_caused(app, gate, reader, &command, cause).await,
        None => committed(app, gate, reader, &command, &[], None).await,
    }
}

/// The agent's deployment, when an `AgentDeployed` names it and no `AgentStopped` follows.
pub(crate) fn live_deployment(
    control: &[mandate_journal::StoredEvent],
    agent: &str,
) -> Option<Deployment> {
    deployed(control)
        .into_iter()
        .find(|deployment| deployment.0 == agent && !deployment.4)
}

/// Whether a canonical decimal is greater than zero: canonical text writes zero as `0` and a
/// negative value with a leading `-`.
pub(crate) fn positive(canonical: &str) -> bool {
    !canonical.starts_with('-') && canonical != "0"
}

/// Whether the deployed mandate document pins `instrument`; `None` when the store cannot give the
/// document. An unpinned universe lists no pinned instrument (mandate spec §3), so it pins nothing.
fn pins(app: &App, deployment: &Deployment, instrument: &str) -> Option<bool> {
    let store = app.artifacts.as_deref()?;
    let (document, _) = stored(store, &deployment.2)?;
    let listed = document
        .get("universe")
        .and_then(|u| u.get("pinned_instruments"))
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .any(|pin| pin.get("asset_id").and_then(Value::as_str) == Some(instrument));
    Some(listed)
}

fn violation(path: &str, code: &str) -> Violation {
    Violation::Schema {
        path: path.to_owned(),
        code: code.to_owned(),
        message: "The request does not meet this rule.".to_owned(),
    }
}
