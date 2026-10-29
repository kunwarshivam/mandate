//! The owner's acknowledgment of a hold (§3.11 item 12,
//! [DEC-2144](../../../docs/project/decisions/DEC-2144.md)): `GET /v1/workspaces/{ws}/acknowledgments`
//! lists every hold that stands now on the account streams of the workspace's deployed agents, and
//! `POST /v1/workspaces/{ws}/agents/{id}/acknowledgments` and `…/acknowledgment-requests` journal
//! `OwnerAcknowledged` and `OwnerAcknowledgmentRequested` version 2 (journal spec §9.28) as the CLI
//! does, each with its passkey step-up's `StepUpAsserted` in one batch.
//!
//! An acknowledgment adds risk back, so a refusal commits nothing: the hold must be one the list
//! serves now, a named request must be unspent, match, and be another user's, and the step-up must
//! be a passkey's that the port verifies. The executor judges every acknowledgment again by
//! §9.28's four checks; these checks only keep out what it would refuse.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use mandate_api::envelope::{StepUpAssertion, StepUpEvidence, StepUpMethod};
use mandate_api::problem::ProblemCode;
use mandate_api::requests::{AcknowledgeRequest, AcknowledgeSubject, AcknowledgmentRequestRequest};
use mandate_api::wire::{EventId, decode, encode};
use mandate_authn::Request;
use mandate_canon::Value;
use mandate_identity::{Permission, StepUpActionKind};
use mandate_journal::StoredEvent;
use mandate_journal::holds::{Hold, Reason, holds, requests};

use crate::backtests::paper;
use crate::commands::Command;
use crate::http::{App, problem, problem_body};
use crate::reads::{Deployment, account_ref, deployed};
use crate::routes::{
    Reader, Refused, Unreadable, authorized, document, object, path_refused, served, session,
    string,
};
use crate::stepup::{Action, Judged, Unverified};
use crate::writes::{
    Bound, Gate, accepted, answered, committed_asserted, json_string, replayed, strictly,
};

/// The operation §3.4 derives an acknowledgment's `StepUpAsserted` id under.
pub const ACKNOWLEDGE_ASSERTED: &str = "acknowledge_step_up_asserted";

/// The operation §3.4 derives an acknowledgment request's `StepUpAsserted` id under.
pub const REQUEST_ASSERTED: &str = "acknowledgment_request_step_up_asserted";

/// Each deployed agent's account stream id, from the connection its deployed document names
/// (DEC-1020 item 3), or `None` when a document or its account cannot be found: the read then fails
/// rather than leave that agent's holds out.
fn agent_accounts(app: &App, reader: &Reader) -> Option<Vec<(Deployment, String)>> {
    deployed(&reader.control)
        .into_iter()
        .map(|deployment| {
            let connection = document(app, &deployment)?.connection_id;
            let reference = account_ref(&reader.control, &connection)?;
            Some((deployment, format!("acct:{}:{reference}", reader.workspace)))
        })
        .collect()
}

/// What [`read_holds`] read: every standing hold, the account streams' rows, and each deployed
/// agent's account stream.
struct Read {
    holds: Vec<Hold>,
    rows: Vec<Vec<StoredEvent>>,
    accounts: BTreeMap<String, String>,
}

impl Read {
    /// The holds that block `agent`, none when it has no account here.
    fn blocking<'a>(&'a self, agent: &'a str) -> impl Iterator<Item = &'a Hold> {
        let account = self.accounts.get(agent);
        self.holds
            .iter()
            .filter(move |hold| account.is_some_and(|account| hold.blocks(agent, account)))
    }
}

/// Every hold standing on the workspace's account streams, with the streams read, or
/// `journal_unavailable` when an account, a stream, or a record cannot be read.
async fn read_holds(app: &App, reader: &Reader) -> Result<Read, Refused> {
    let mapped = agent_accounts(app, reader).ok_or(Refused::JournalUnavailable)?;
    let mut by_account: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (deployment, account) in &mapped {
        by_account
            .entry(account.clone())
            .or_default()
            .push(deployment.0.clone());
    }
    let mut accounts = Vec::new();
    for (account, mut agents) in by_account {
        agents.sort();
        let rows = app
            .journal
            .rows(&account)
            .await
            .map_err(|_| Refused::JournalUnavailable)?;
        accounts.push((account, rows, agents));
    }
    let requested = requests(&reader.control).map_err(|_| Refused::JournalUnavailable)?;
    let listed = holds(&accounts, &requested).map_err(|_| Refused::JournalUnavailable)?;
    Ok(Read {
        holds: listed,
        rows: accounts.into_iter().map(|(_, rows, _)| rows).collect(),
        accounts: mapped
            .into_iter()
            .map(|(deployment, account)| (deployment.0, account))
            .collect(),
    })
}

/// The list's query: `state=open`, and an optional `agent`, in either order; `None` for anything
/// else.
fn listed_for(query: Option<&str>) -> Option<Option<String>> {
    let mut state = None;
    let mut agent = None;
    for pair in query?.split('&') {
        match pair.split_once('=')? {
            ("state", value) if state.is_none() => state = Some(value),
            ("agent", value) if agent.is_none() && !value.is_empty() => {
                agent = Some(value.to_owned());
            }
            _ => return None,
        }
    }
    (state == Some("open")).then_some(agent)
}

/// `GET /v1/workspaces/{ws}/acknowledgments?state=open[&agent={id}]` (`read-models/acknowledgments`).
pub async fn list(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
    RawQuery(query): RawQuery,
) -> Response {
    let Ok(Path(workspace)) = path else {
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
        Permission::ViewAgents,
        Unreadable::Read,
    )
    .await
    {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    let Some(agent) = listed_for(query.as_deref()) else {
        return Refused::Invalid.response();
    };
    let deployments = deployed(&reader.control);
    if let Some(agent) = &agent
        && !deployments.iter().any(|deployment| &deployment.0 == agent)
    {
        return Refused::Invalid.response();
    }
    let read = match read_holds(&app, &reader).await {
        Ok(read) => read,
        Err(refusal) => return refusal.response(),
    };
    let items = match &agent {
        Some(agent) => read.blocking(agent).map(hold_value).collect(),
        None => read.holds.iter().map(hold_value).collect(),
    };
    let streams: Vec<&[StoredEvent]> = read.rows.iter().map(Vec::as_slice).collect();
    served(
        &reader,
        &streams,
        vec![("acknowledgments", Value::Array(items))],
    )
}

/// One hold as `read-models/acknowledgments` states it.
fn hold_value(hold: &Hold) -> Value {
    let reason = match &hold.raised.reason {
        Reason::Limit(limit) => object(vec![("limit", string(limit))]),
        Reason::Cause(cause) => object(vec![("cause", string(cause))]),
        Reason::Reconciliation {
            restriction,
            kind,
            instrument,
        } => object(vec![
            ("restriction", string(restriction)),
            ("kind", string(kind)),
            (
                "instrument",
                instrument.as_deref().map_or(Value::Null, string),
            ),
        ]),
    };
    let request = hold.request.as_ref().map_or(Value::Null, |request| {
        object(vec![
            ("event_id", string(&request.event_id)),
            ("requested_by", string(&request.requested_by)),
        ])
    });
    object(vec![
        ("event", string(&hold.raised.event)),
        ("subject", string(hold.raised.reason.subject())),
        ("agent_id", string(&hold.agent_id)),
        ("account", string(&hold.account)),
        ("reason", reason),
        ("mode", hold.raised.mode.map_or(Value::Null, string)),
        ("raised_at", string(&hold.raised.raised_at)),
        ("independent_required", Value::Null),
        ("request", request),
    ])
}

/// §9.28 version 2's `subject` for the body's.
fn subject_text(subject: AcknowledgeSubject) -> &'static str {
    match subject {
        AcknowledgeSubject::Limit => "limit",
        AcknowledgeSubject::Restriction => "restriction",
        AcknowledgeSubject::Reconciliation => "reconciliation",
    }
}

/// What both routes carry: the agent, the hold, its subject, and the evidence.
struct Asked<'a> {
    agent: String,
    event: String,
    subject: &'static str,
    request: Option<String>,
    step_up: Option<&'a StepUpEvidence>,
    assertion: Option<&'a StepUpAssertion>,
}

/// `POST /v1/workspaces/{ws}/agents/{id}/acknowledgments`: `OwnerAcknowledged` version 2.
pub async fn acknowledge(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
    body: Body,
) -> Response {
    let Ok(Path((workspace, agent))) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let (gate, request) = match strictly(&app, &headers, body, decode::<AcknowledgeRequest>).await {
        Ok(admitted) => admitted,
        Err(refusal) => return *refusal,
    };
    let (Some(event), request_id) = (
        event_text(&request.acknowledged),
        request.request.as_ref().map(event_text),
    ) else {
        return Refused::Invalid.response();
    };
    let Some(request_id) = request_id.map_or(Some(None), |id| id.map(Some)) else {
        return Refused::Invalid.response();
    };
    let asked = Asked {
        agent,
        event,
        subject: subject_text(request.subject),
        request: request_id,
        step_up: request.step_up.as_ref(),
        assertion: request.step_up_assertion.as_ref(),
    };
    judged(&app, &gate, &workspace, &asked, true).await
}

/// `POST /v1/workspaces/{ws}/agents/{id}/acknowledgment-requests`: `OwnerAcknowledgmentRequested`
/// version 2.
pub async fn request_acknowledgment(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
    body: Body,
) -> Response {
    let Ok(Path((workspace, agent))) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let (gate, request) =
        match strictly(&app, &headers, body, decode::<AcknowledgmentRequestRequest>).await {
            Ok(admitted) => admitted,
            Err(refusal) => return *refusal,
        };
    let Some(event) = event_text(&request.acknowledged) else {
        return Refused::Invalid.response();
    };
    let asked = Asked {
        agent,
        event,
        subject: subject_text(request.subject),
        request: None,
        step_up: request.step_up.as_ref(),
        assertion: request.step_up_assertion.as_ref(),
    };
    judged(&app, &gate, &workspace, &asked, false).await
}

/// The text of an event id, as it serializes.
fn event_text(id: &EventId) -> Option<String> {
    json_string(&encode(id).ok()?)
}

fn refused(code: ProblemCode, status: StatusCode) -> Response {
    problem(status, problem_body(code))
}

/// §3.11 item 12's checks 2 to 7 for an acknowledgment (`answers` true) or a request, then the
/// record and its `StepUpAsserted` in one batch; a repeat under the key replays first.
async fn judged(
    app: &App,
    gate: &Gate,
    workspace: &str,
    asked: &Asked<'_>,
    answers: bool,
) -> Response {
    let reader = match authorized(
        app,
        &gate.admitted,
        gate.now,
        workspace,
        Permission::Acknowledge,
        Unreadable::Command,
    )
    .await
    {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    if !deployed(&reader.control)
        .iter()
        .any(|deployment| deployment.0 == asked.agent)
    {
        return Refused::NotFound.response();
    }
    if !paper(&reader.control) {
        return refused(ProblemCode::LiveUnavailable, StatusCode::CONFLICT);
    }
    let Ok(requested) = requests(&reader.control) else {
        return Refused::JournalUnavailable.response();
    };
    let named = asked.request.as_ref().map(|id| {
        requested
            .iter()
            .find(|(request, _)| &request.event_id == id)
    });
    let Some(principal) = crate::writes::principal_text(gate) else {
        return Refused::Unauthenticated.response();
    };
    let make = |step_up: Value| match answers {
        true => Command::Acknowledge {
            agent: asked.agent.clone(),
            event: asked.event.clone(),
            subject: asked.subject,
            request: asked.request.clone(),
            requested_by: match named {
                Some(Some((request, _))) => request.requested_by.clone(),
                _ => principal.clone(),
            },
            step_up,
        },
        false => Command::RequestAcknowledgment {
            agent: asked.agent.clone(),
            event: asked.event.clone(),
            subject: asked.subject,
            step_up,
        },
    };
    let presented = asked.step_up.and_then(answered);
    if let Some(presented) = &presented {
        let as_presented = make(presented.clone());
        if let Some(row) = replayed(gate, &reader, &as_presented) {
            return accepted(as_presented.id_member(), &row, gate.now, &[], None);
        }
    }
    let read = match read_holds(app, &reader).await {
        Ok(read) => read,
        Err(refusal) => return refusal.response(),
    };
    let stands = read.blocking(&asked.agent).any(|hold| {
        hold.raised.event == asked.event && hold.raised.reason.subject() == asked.subject
    });
    if !stands {
        return refused(ProblemCode::AcknowledgmentUnavailable, StatusCode::CONFLICT);
    }
    if let Some(named) = named {
        let answerable = named.is_some_and(|(request, spent)| {
            !spent
                && request.agent == asked.agent
                && request.event == asked.event
                && request.subject == asked.subject
                && request.requested_by != principal
        });
        if !answerable {
            return refused(
                ProblemCode::AcknowledgmentRequestInvalid,
                StatusCode::CONFLICT,
            );
        }
    }
    let Some(evidence) = asked.step_up else {
        return refused(ProblemCode::StepUpRequired, StatusCode::UNAUTHORIZED);
    };
    if evidence.method != StepUpMethod::Passkey {
        return refused(ProblemCode::StepUpMethod, StatusCode::UNAUTHORIZED);
    }
    if asked.assertion.is_none() {
        return refused(ProblemCode::StepUpMissing, StatusCode::UNAUTHORIZED);
    }
    let Some(presented) = presented else {
        return Refused::Invalid.response();
    };
    let head = reader.control.last().map_or(0, |row| row.seq);
    let bound = Bound::held(
        if answers {
            "acknowledge"
        } else {
            "request_ack"
        },
        &asked.agent,
        &asked.event,
        asked.subject,
        asked.request.as_deref(),
    );
    let Some(digest) = bound.digest(head) else {
        return Refused::Invalid.response();
    };
    let action = Action {
        kind: StepUpActionKind::Acknowledge,
        digest,
    };
    let judged = Judged {
        tenant: &reader.tenant,
        control: &reader.control,
        now: gate.now,
    };
    let operation = if answers {
        ACKNOWLEDGE_ASSERTED
    } else {
        REQUEST_ASSERTED
    };
    match app
        .step_up
        .asserted(evidence, asked.assertion, &action, &judged)
    {
        Ok(Some(asserted)) => {
            let command = make(presented);
            committed_asserted(app, gate, &reader, &command, Some((&asserted, operation))).await
        }
        Ok(None) => refused(ProblemCode::StepUpMissing, StatusCode::UNAUTHORIZED),
        Err(Unverified::Code(ProblemCode::JournalUnavailable)) => {
            Refused::JournalUnavailable.response()
        }
        Err(Unverified::Code(code)) => refused(code, StatusCode::UNAUTHORIZED),
    }
}

#[cfg(test)]
mod digest {
    //! [`Bound::held`] against §3.11 item 12's worked vectors, which lane WB and the spec's review
    //! recomputed apart from this code.

    use crate::writes::Bound;

    const AGENT: &str = "agt_01JB3K6F0C9R2V7N4M8Q1T5W3Z";
    const EVENT: &str = "01JBH3BTZ8K2M4N6P8R0S2T4V6";

    fn hex(confirms: &'static str, request: Option<&str>) -> Option<String> {
        let bound = Bound::held(confirms, AGENT, EVENT, "reconciliation", request);
        bound
            .digest(42)
            .map(|digest| format!("sha256:{}", digest.to_hex()))
    }

    #[test]
    fn the_held_object_reproduces_the_spec_s_three_vectors() {
        assert_eq!(
            hex("acknowledge", None).as_deref(),
            Some("sha256:5f5506a3ac32cae637a2cc222393f852035c699430656870ba024702431260a7")
        );
        assert_eq!(
            hex("acknowledge", Some("01JBH3BTZ8K2M4N6P8R0S2T4V7")).as_deref(),
            Some("sha256:b9367c62d1201059c5a9f2cf0bf35e637d27a5934e174b779276a2fcbc7af765")
        );
        assert_eq!(
            hex("request_ack", None).as_deref(),
            Some("sha256:b15c59e4e09394ddf0f5430c04569276c27c7a957ac68c683e53b15e6b393ebc")
        );
    }
}
