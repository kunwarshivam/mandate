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

use crate::backtests::paper;
use crate::commands::Command;
use crate::http::{App, problem, problem_body};
use crate::reads::{Deployment, account_ref, deployed, envelope};
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

/// The prefix of a reconciliation hold's `restriction` (journal spec §9.28 version 2).
const RECONCILIATION: &str = "reconciliation:";

/// The reconciliation subjects that are not an instrument (trading spec §7.1 and §11).
const ACCOUNT_KINDS: [&str; 4] = ["cash", "fees", "external_activity", "incomplete"];

/// What a hold holds, by `subject`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// A latched limit, rung, or tripwire: its `limit` code.
    Limit(String),
    /// An account restriction: its `cause`.
    Cause(String),
    /// A reconciliation hold: its `restriction`, its kind, and the instrument it holds.
    Reconciliation {
        restriction: String,
        kind: String,
        instrument: Option<String>,
    },
}

impl Reason {
    /// §9.28 version 2's `subject` for it.
    pub fn subject(&self) -> &'static str {
        match self {
            Self::Limit(_) => "limit",
            Self::Cause(_) => "restriction",
            Self::Reconciliation { .. } => "reconciliation",
        }
    }
}

/// Whose a hold is: one agent's, or the account's (a restriction, or a reconciliation hold of
/// every agent, `*`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Holder {
    Agent(String),
    Account,
}

/// One hold standing on an account stream, by journal spec §9.28's checks 1 and 2 for its subject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Raised {
    /// The `event_id` of the record that raised it.
    pub event: String,
    pub holder: Holder,
    pub reason: Reason,
    /// The agent mode it holds, `None` for a limit.
    pub mode: Option<&'static str>,
    /// The raising record's `recorded_at`.
    pub raised_at: String,
}

/// The holds that stand at the end of one account stream's `rows`, in the order raised, or `None`
/// when a record that raises or ends one cannot be read, so the read fails rather than leave a hold
/// out or serve one already ended.
///
/// A `RiskLimitTriggered` stands until a later trigger or lift of the same agent's `limit`; an
/// `AccountRestrictionChanged` until a later change or lift of its `cause`; and an executor's
/// reconciliation `AgentModeApplied` to `exits_only` or `paused` until a later `AgentModeApplied`
/// of the same `agent` and `restriction`.
pub fn standing(rows: &[StoredEvent]) -> Option<Vec<Raised>> {
    let mut held: Vec<Raised> = Vec::new();
    for row in rows {
        let read = |member: &str| -> Option<String> {
            let event = envelope(row)?;
            event
                .get("payload")?
                .get(member)?
                .as_str()
                .map(str::to_owned)
        };
        match row.event_type.as_str() {
            "RiskLimitTriggered" | "RiskLimitLifted" => {
                let (agent, limit) = (read("agent_id")?, read("limit")?);
                held.retain(|hold| {
                    !(hold.holder == Holder::Agent(agent.clone())
                        && hold.reason == Reason::Limit(limit.clone()))
                });
                if row.event_type == "RiskLimitTriggered" {
                    held.push(Raised {
                        event: row.event_id.clone(),
                        holder: Holder::Agent(agent),
                        reason: Reason::Limit(limit),
                        mode: None,
                        raised_at: row.recorded_at.clone(),
                    });
                }
            }
            "AccountRestrictionChanged" | "AccountRestrictionLifted" => {
                let cause = read("cause")?;
                held.retain(|hold| hold.reason != Reason::Cause(cause.clone()));
                if row.event_type == "AccountRestrictionChanged" {
                    let mode = match read("restriction")?.as_str() {
                        "closing_only" => "exits_only",
                        "blocked" => "paused",
                        _ => return None,
                    };
                    held.push(Raised {
                        event: row.event_id.clone(),
                        holder: Holder::Account,
                        reason: Reason::Cause(cause),
                        mode: Some(mode),
                        raised_at: row.recorded_at.clone(),
                    });
                }
            }
            "AgentModeApplied" => {
                let restriction = read("restriction")?;
                let Some(subject) = restriction.strip_prefix(RECONCILIATION) else {
                    continue;
                };
                let agent = read("agent")?;
                let holder = match agent.as_str() {
                    "*" => Holder::Account,
                    _ => Holder::Agent(agent),
                };
                held.retain(|hold| {
                    !(hold.holder == holder
                        && matches!(&hold.reason, Reason::Reconciliation { restriction: r, .. } if *r == restriction))
                });
                let originated = match envelope(row)?.get("payload")?.get("originated")? {
                    Value::Bool(originated) => *originated,
                    _ => return None,
                };
                let mode = match read("to")?.as_str() {
                    "exits_only" => "exits_only",
                    "paused" => "paused",
                    _ => continue,
                };
                if !originated || subject.is_empty() {
                    continue;
                }
                let account_wide = ACCOUNT_KINDS.contains(&subject);
                let reason = Reason::Reconciliation {
                    kind: if account_wide { subject } else { "instrument" }.to_owned(),
                    instrument: (!account_wide).then(|| subject.to_owned()),
                    restriction,
                };
                held.push(Raised {
                    event: row.event_id.clone(),
                    holder,
                    reason,
                    mode: Some(mode),
                    raised_at: row.recorded_at.clone(),
                });
            }
            _ => {}
        }
    }
    Some(held)
}

/// An unspent `OwnerAcknowledgmentRequested` on the control stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requested {
    pub event_id: String,
    pub agent: String,
    pub event: String,
    pub subject: String,
    pub requested_by: String,
}

/// Every `OwnerAcknowledgmentRequested` on `control`, in stream order, with whether an
/// `OwnerAcknowledged` names it (rule 241). A version 1 request's subject reads as `limit`, as
/// §9.28 reads version 1, and a `subject` that is not text reads as one that matches nothing.
/// `None` when such a record cannot be read.
pub fn requests(control: &[StoredEvent]) -> Option<Vec<(Requested, bool)>> {
    let mut spent: Vec<String> = Vec::new();
    for row in control
        .iter()
        .filter(|r| r.event_type == "OwnerAcknowledged")
    {
        let payload = envelope(row)?.get("payload")?.clone();
        if let Some(request) = payload.get("request").and_then(Value::as_str) {
            spent.push(request.to_owned());
        }
    }
    let mut found = Vec::new();
    for row in control
        .iter()
        .filter(|r| r.event_type == "OwnerAcknowledgmentRequested")
    {
        let payload = envelope(row)?.get("payload")?.clone();
        let read = |member: &str| {
            payload
                .get(member)
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        let subject = match (row.schema_version, payload.get("subject")) {
            (1, None) => "limit".to_owned(),
            (_, Some(Value::Str(subject))) => subject.clone(),
            _ => String::new(),
        };
        let requested = Requested {
            event_id: row.event_id.clone(),
            agent: read("agent")?,
            event: read("event")?,
            subject,
            requested_by: read("requested_by")?,
        };
        let is_spent = spent.contains(&requested.event_id);
        found.push((requested, is_spent));
    }
    Some(found)
}

/// One hold as the list serves it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hold {
    pub raised: Raised,
    /// The agent to post an acknowledgment to.
    pub agent_id: String,
    /// The account stream's id.
    pub account: String,
    /// The latest unspent request for it.
    pub request: Option<Requested>,
}

impl Hold {
    /// Whether it blocks `agent`, whose account stream is `account`: its own limit or hold, or a
    /// hold of every agent of that account. A hold of another account never blocks it, so an
    /// acknowledgment posted to an agent of another account is refused here, not copied to a stream
    /// where the executor would find no trigger.
    pub fn blocks(&self, agent: &str, account: &str) -> bool {
        self.account == account
            && match &self.raised.holder {
                Holder::Agent(holder) => holder == agent,
                Holder::Account => true,
            }
    }
}

/// The holds standing on each account in `accounts` (its stream id, its rows, and its deployed
/// agents in id order), each with the agent to post to and its unspent request.
pub fn holds(
    accounts: &[(String, Vec<StoredEvent>, Vec<String>)],
    requested: &[(Requested, bool)],
) -> Option<Vec<Hold>> {
    let mut listed = Vec::new();
    for (account, rows, agents) in accounts {
        for raised in standing(rows)? {
            let request = requested
                .iter()
                .filter(|(r, spent)| {
                    !spent && r.event == raised.event && r.subject == raised.reason.subject()
                })
                .map(|(r, _)| r.clone())
                .next_back();
            let agent_id = match (&request, &raised.holder) {
                (Some(request), _) => request.agent.clone(),
                (None, Holder::Agent(agent)) => agent.clone(),
                (None, Holder::Account) => match agents.first() {
                    Some(agent) => agent.clone(),
                    None => continue,
                },
            };
            listed.push(Hold {
                raised,
                agent_id,
                account: account.clone(),
                request,
            });
        }
    }
    Some(listed)
}

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
    let requested = requests(&reader.control).ok_or(Refused::JournalUnavailable)?;
    let listed = holds(&accounts, &requested).ok_or(Refused::JournalUnavailable)?;
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
    let Some(requested) = requests(&reader.control) else {
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
