//! The owner-command routes (§4.2, §5.2, §5.4, §3.11 item 10, DEC-1007): `POST
//! /v1/workspaces/{ws}/agents/{id}/pause`, `…/kill-switch`, and `…/approvals/{id}/responses`.
//! The hold is not mounted until the runtime folds it (DEC-1007 item 1).
//!
//! Each is checked in §3.3 item 1's order: the session (the one 401, or `reduction_only`), the
//! CSRF mark (403), then the `Idempotency-Key` and the body, read only then, by §5's lenient
//! decoder (422), then the matrix row and the scope (403 or the one 404). Its event is on the
//! control stream before the `202` (rule 5), and a repeat under the key replays it (API-4).
//! Neither readiness nor a drain gates them (API-7).

use std::sync::Arc;

use axum::body::{Body, Bytes, to_bytes};
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use mandate_api::envelope::{StepUpAssertion, StepUpEvidence};
use mandate_api::idempotency::{Derivation, IdempotencyKey, event_id};
use mandate_api::lenient::{Api7, ApprovalAnswer, Kept, decode_lenient};
use mandate_api::problem::Violation;
use mandate_api::problem::{Effect, Problem, ProblemCode};
use mandate_api::requests::{
    ApprovalResponseRequest, KillSwitchRequest, OwnerExitRequest, PauseRequest, ResumeRequest,
    Scope, StopRequest,
};
use mandate_api::wire::{Asset, Decimal, EventId, Id, Ref, decode, encode};
use mandate_authn::Request;
use mandate_canon::{Digest, Int, Value, parse, to_canonical};
use mandate_identity::{Permission, PrincipalId, Roster, StepUpActionKind};
use mandate_journal::StoredEvent;
use mandate_time::UtcNanos;

use crate::backtests::paper;
use crate::commands::{
    Command, Event, Issued, KillScope, Settled, Writer, caused_envelope, draft, in_workspace,
    release_warning, same, settle_batch_at,
};
use crate::http::{App, browser, invalid, problem, problem_body};
use crate::reads::{deferrals_agree, deployed};
use crate::routes::{
    Reader, Refused, Unreadable, authorized, enveloped, path_refused, session, string, watermark,
};
use crate::session::Authenticated;
use crate::stepup::{Action, Asserted, Judged, Unverified, journaled};
use crate::versions::stored;

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
pub(crate) struct Gate {
    pub(crate) admitted: Authenticated,
    pub(crate) now: UtcNanos,
    pub(crate) key: IdempotencyKey,
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

/// The session (for any but pause and the kill switch), the CSRF mark, the key, and the body's
/// bytes, in that order, for a route whose body is decoded strictly (§3.11 item 11).
pub(crate) async fn strict(
    app: &App,
    headers: &HeaderMap,
    body: Body,
) -> Result<(Gate, Bytes), Refused> {
    let (admitted, now) = session(app, headers, Request::Other)?;
    if !browser(headers, &app.settings.app_origin) {
        return Err(Refused::Forbidden);
    }
    let key = one_key(headers).ok_or(Refused::Invalid)?;
    let body = to_bytes(body, MAX_BODY)
        .await
        .map_err(|_| Refused::Invalid)?;
    Ok((Gate { admitted, now, key }, body))
}

/// The one `Idempotency-Key`, when it is sent once and is well formed.
pub(crate) fn one_key(headers: &HeaderMap) -> Option<IdempotencyKey> {
    let mut sent = headers.get_all(IDEMPOTENCY_KEY).iter();
    match (sent.next(), sent.next()) {
        (Some(key), None) => IdempotencyKey::parse(key.to_str().ok()?).ok(),
        _ => None,
    }
}

/// The text of an id, as it serializes.
pub(crate) fn id_text(id: &Id) -> Option<String> {
    json_string(&encode(id).ok()?)
}

/// The text of an event id, as it serializes.
fn event_text(id: &EventId) -> Option<String> {
    json_string(&encode(id).ok()?)
}

/// `bytes` as a JSON string's text.
pub(crate) fn json_string(bytes: &[u8]) -> Option<String> {
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
    match authorized {
        Ok(reader) => committed(app, gate, &reader, command, dropped, status).await,
        Err(refusal) => refusal.response(),
    }
}

/// Records `command` on the control stream `reader` read, once its scope is found there, or
/// replays the event its key already recorded.
async fn committed(
    app: &App,
    gate: &Gate,
    reader: &Reader,
    command: &Command,
    dropped: &[String],
    status: Option<&str>,
) -> Response {
    commit_with(app, gate, reader, command, (dropped, status), None).await
}

/// [`committed`] for a command a passkey step-up authorized: its `StepUpAsserted`, under §3.4's id
/// for the operation named beside it, follows the command in its batch ([DEC-2128] item 3).
///
/// [DEC-2128]: ../../../docs/project/decisions/DEC-2128.md
async fn committed_asserted(
    app: &App,
    gate: &Gate,
    reader: &Reader,
    command: &Command,
    asserted: Option<(&Asserted, &'static str)>,
) -> Response {
    commit_with(app, gate, reader, command, (&[], None), asserted).await
}

/// The record [`committed`] and [`committed_asserted`] share.
async fn commit_with(
    app: &App,
    gate: &Gate,
    reader: &Reader,
    command: &Command,
    (dropped, status): (&[String], Option<&str>),
    asserted: Option<(&Asserted, &'static str)>,
) -> Response {
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
    let alert = match command {
        Command::KillSwitch(_) => match kill_switch_alert(gate, &issued) {
            Some(alert) => Some(alert),
            None => return Refused::JournalUnavailable.response(),
        },
        _ => None,
    };
    let assertion = match asserted {
        Some((asserted, operation)) => {
            match assertion_draft(app, gate, &issued, asserted, operation) {
                Some(assertion) => Some(assertion),
                None => return Refused::JournalUnavailable.response(),
            }
        }
        None => None,
    };
    let batch: Vec<&[u8]> = std::iter::once(bytes.as_slice())
        .chain(alert.as_deref())
        .chain(assertion.as_deref())
        .collect();
    match appended_batch_at(app, reader, (&text, &batch), gate.now, None).await {
        Settled::Stored(row) => accepted(command.id_member(), &row, gate.now, dropped, status),
        Settled::Conflict => problem(
            StatusCode::CONFLICT,
            problem_body(ProblemCode::IdempotencyConflict),
        ),
        Settled::Unknown => unknown(id),
        Settled::Unavailable => Refused::JournalUnavailable.response(),
    }
}

/// The `StepUpAsserted` recording the passkey step-up that authorized `issued`, its id derived under
/// `operation`, its raw assertion stored first. `None` when no id derives, no store is bound, or
/// the store refuses it.
fn assertion_draft(
    app: &App,
    gate: &Gate,
    issued: &Issued<'_>,
    asserted: &Asserted,
    operation: &str,
) -> Option<Vec<u8>> {
    let (_, id) = derived_for(gate, issued.workspace, operation)?;
    let writer = Writer {
        workspace: issued.workspace,
        principal: issued.principal,
        event_id: &id,
        environment: issued.environment,
        now: issued.now,
    };
    asserted.drafted(app.objects.as_deref()?, &writer, issued.event_id)
}

/// The operation §3.4 derives a resume's `StepUpAsserted` id under ([DEC-2128] item 3).
///
/// [DEC-2128]: ../../../docs/project/decisions/DEC-2128.md
pub const RESUME_ASSERTED: &str = "resume_step_up_asserted";
/// The operation §3.4 derives a Stop's `StepUpAsserted` id under.
pub const STOP_ASSERTED: &str = "stop_step_up_asserted";

/// The operation §3.4 derives a user kill switch's `OwnerAlertSent` id under, distinct from the
/// command's own `kill_switch` (E8-9k).
const KILL_SWITCH_ALERT: &str = "kill_switch_alert";

/// The `OwnerAlertSent` a user's kill switch commits in its own batch, right after its
/// `OwnerCommandIssued` (E8-9k; notifications spec §3.2 and §3.4, journal spec §9.15 rules 119,
/// 120 and 130): kind `kill_switch`, its `subject`, `causation_id` and `owner_command` the command,
/// on which the dispatcher gives the command exactly one notice. `None` when no id derives or the
/// canonical form cannot hold it.
fn kill_switch_alert(gate: &Gate, issued: &Issued<'_>) -> Option<Vec<u8>> {
    let (_, id) = derived_for(gate, issued.workspace, KILL_SWITCH_ALERT)?;
    let writer = Writer {
        workspace: issued.workspace,
        principal: issued.principal,
        event_id: &id,
        environment: issued.environment,
        now: issued.now,
    };
    let command = string(issued.event_id);
    let mut payload = mandate_canon::Object::new();
    for (name, value) in [
        ("kind", string("kill_switch")),
        ("owner_command", command.clone()),
        ("subject", command),
    ] {
        payload.insert(mandate_canon::Key::new(name).ok()?, value);
    }
    let event = Event {
        event_type: "OwnerAlertSent",
        schema_version: 1,
        payload: Value::Object(payload),
        artifact_refs: Vec::new(),
        config_refs: Vec::new(),
    };
    caused_envelope(&writer, &event, Some(issued.event_id))
}

/// `drafts`, the first of which has the id `id`, on the control stream `reader` read, appended in
/// one batch, all or none, at `pinned` when it names a head: the event already stored under the id,
/// replayed or a conflict (§3.4 step 1), or the append's outcome (DEC-1007 item 6). A retry finds
/// the first stored.
pub(crate) async fn appended_batch_at(
    app: &App,
    reader: &Reader,
    (id, drafts): (&str, &[&[u8]]),
    now: UtcNanos,
    pinned: Option<u64>,
) -> Settled {
    let stream = format!("ctl:{}", reader.workspace);
    let Some(first) = drafts.first() else {
        return Settled::Unavailable;
    };
    if let Some(settled) = repeated(&reader.control, id, first) {
        return settled;
    }
    match settle_batch_at(app.journal.as_ref(), &stream, id, drafts, now, pinned).await {
        Settled::Conflict => app
            .journal
            .rows(&stream)
            .await
            .ok()
            .and_then(|rows| repeated(&rows, id, first))
            .unwrap_or(Settled::Unknown),
        settled => settled,
    }
}

/// What `rows` already hold under `id` (§3.4 step 1): the same command, replayed, or other content,
/// a conflict; `None` when nothing is stored under it.
pub(crate) fn repeated(rows: &[StoredEvent], id: &str, draft: &[u8]) -> Option<Settled> {
    let row = rows.iter().find(|row| row.event_id == id)?;
    Some(match same(row, draft) {
        true => Settled::Stored(Box::new(row.clone())),
        false => Settled::Conflict,
    })
}

pub(crate) fn principal_text(gate: &Gate) -> Option<String> {
    gate.admitted.principal.to_ulid_text().ok()
}

/// §3.4's event id for `command` under the gate's key, and its text.
fn derived(gate: &Gate, workspace: &str, command: &Command) -> Option<(EventId, String)> {
    derived_for(gate, workspace, command.operation())
}

/// §3.4's event id for `operation` under the gate's key, and its text.
pub(crate) fn derived_for(
    gate: &Gate,
    workspace: &str,
    operation: &str,
) -> Option<(EventId, String)> {
    let workspace = Id::try_from(workspace.to_owned()).ok()?;
    let principal = Id::try_from(principal_text(gate)?).ok()?;
    let id = event_id(&Derivation {
        workspace: &workspace,
        principal: &principal,
        operation,
        key: &gate.key,
        position: None,
    })
    .ok()?;
    let text = event_text(&id)?;
    Some((id, text))
}

/// The `202` for the stored `row` (§5.4): its id, `recorded`, and what this call dropped.
fn accepted(
    id_member: &'static str,
    row: &StoredEvent,
    now: UtcNanos,
    dropped: &[String],
    status: Option<&str>,
) -> Response {
    let mut members = vec![
        (id_member, string(&row.event_id)),
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
pub(crate) fn unknown(id: EventId) -> Response {
    let body = Problem::of(ProblemCode::OutcomeUnknown, Effect::Unknown, Some(id), None)
        .ok()
        .and_then(|problem| encode(&problem).ok())
        .map(Bytes::from)
        .unwrap_or_default();
    problem(StatusCode::SERVICE_UNAVAILABLE, body)
}

/// `POST /v1/workspaces/{ws}/approvals/{id}/responses` (§5.2, DEC-1007 item 10): a Skip is
/// recorded; a grant, in paper only and once the approval is found and its deferred checks are
/// exactly its decision's (`deferral_mismatch` otherwise, DEC-2521), is recorded with its
/// `cli_confirm` evidence when the port verifies it as an `approve` bound to the request's content
/// hash (DEC-2104), and refused with the port's code otherwise.
pub async fn respond(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
    body: Body,
) -> Response {
    let Ok(Path((workspace, approval))) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let (gate, kept) = match admitted::<ApprovalAnswer>(&app, &headers, Request::Other, body).await
    {
        Ok(accepted) => accepted,
        Err(refusal) => return refusal.response(),
    };
    let permission = match kept.value {
        ApprovalAnswer::Approved(_) => Permission::Approve,
        ApprovalAnswer::Skipped(_) => Permission::Skip,
    };
    let authorized = authorized(
        &app,
        &gate.admitted,
        gate.now,
        &workspace,
        permission,
        Unreadable::Command,
    )
    .await;
    let reader = match authorized {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    let (agent, version, agrees) = match requester(&app, &reader, &approval).await {
        Ok(Some(found)) => found,
        Ok(None) => return Refused::NotFound.response(),
        Err(refusal) => return refusal.response(),
    };
    let skip = match &kept.value {
        ApprovalAnswer::Skipped(skip) => skip,
        ApprovalAnswer::Approved(grant) => {
            match approves(&app, &reader, version.as_deref(), gate.admitted.principal) {
                Ok(true) => {}
                Ok(false) => return Refused::Forbidden.response(),
                Err(refusal) => return refusal.response(),
            }
            if !agrees {
                return problem(
                    StatusCode::CONFLICT,
                    problem_body(ProblemCode::DeferralMismatch),
                );
            }
            let asked = Asked {
                agent,
                approval,
                grant,
            };
            return granted(&app, &gate, &reader, &asked).await;
        }
    };
    let Some(content_hash) = ref_text(&skip.content_hash) else {
        return Refused::JournalUnavailable.response();
    };
    let command = Command::Skip {
        agent,
        approval,
        content_hash,
    };
    committed(&app, &gate, &reader, &command, &kept.dropped, None).await
}

/// A grant of the approval `approval` an agent requested.
struct Asked<'a> {
    agent: String,
    approval: String,
    grant: &'a ApprovalResponseRequest,
}

/// The grant `asked` recorded with its evidence once the port verifies it as an `approve` bound to
/// the request's content hash (§3.6, identity spec §7.2 step 1, DEC-2104 item 6), or refused with
/// the port's code. A grant naming a delegation is §5.3's batch, not served here, and `invalid`.
/// A repeat whose key holds the same grant replays before the port is asked, since the port spent
/// the challenge with the first (§3.4 step 1).
async fn granted(app: &App, gate: &Gate, reader: &Reader, asked: &Asked<'_>) -> Response {
    if asked.grant.delegation.is_some() {
        return invalid(vec![schema_violation("/delegation", "delegation_unserved")]);
    }
    if !paper(&reader.control) {
        return problem(
            StatusCode::CONFLICT,
            problem_body(ProblemCode::LiveUnavailable),
        );
    }
    let Some(evidence) = &asked.grant.step_up else {
        return problem(
            StatusCode::UNAUTHORIZED,
            problem_body(ProblemCode::StepUpRequired),
        );
    };
    let Some(content_hash) = ref_text(&asked.grant.content_hash) else {
        return Refused::Invalid.response();
    };
    let Some(digest) = content_hash
        .strip_prefix("sha256:")
        .and_then(Digest::from_hex)
    else {
        return Refused::Invalid.response();
    };
    let Some(step_up) = answered(evidence) else {
        return problem(
            StatusCode::UNAUTHORIZED,
            problem_body(ProblemCode::StepUpStale),
        );
    };
    let command = Command::Grant {
        agent: asked.agent.clone(),
        approval: asked.approval.clone(),
        content_hash,
        step_up,
    };
    if let Some(row) = replayed(gate, reader, &command) {
        return accepted(command.id_member(), &row, gate.now, &[], None);
    }
    let action = Action {
        kind: StepUpActionKind::Approve,
        digest,
    };
    let judged = Judged {
        tenant: &reader.tenant,
        control: &reader.control,
        now: gate.now,
    };
    match app.step_up.asserted(evidence, None, &action, &judged) {
        Ok(None) => committed(app, gate, reader, &command, &[], None).await,
        Ok(Some(_)) | Err(Unverified::Code(ProblemCode::JournalUnavailable)) => {
            Refused::JournalUnavailable.response()
        }
        Err(Unverified::Code(code)) => problem(StatusCode::UNAUTHORIZED, problem_body(code)),
        Err(Unverified::Unimplemented { story }) => invalid(vec![Unverified::unimplemented(story)]),
    }
}

/// `evidence` as `ApprovalResponseSubmitted` records it (journal spec §9.7): its assertion id and
/// method, and `authenticated_at` as the integer second the CLI writes, the second it falls in, so
/// the recorded evidence is never fresher than the presented; `None` only for an instant the
/// canonical integer cannot hold.
fn answered(evidence: &StepUpEvidence) -> Option<Value> {
    let presented = journaled(Some(evidence))?;
    let at = UtcNanos::parse(presented.get("authenticated_at")?.as_str()?).ok()?;
    let second = Value::Int(Int::new(u64::try_from(at.secs()).ok()?)?);
    let mut object = mandate_canon::Object::new();
    let members = [
        ("assertion_id", presented.get("assertion_id")?.clone()),
        ("authenticated_at", second),
        ("method", presented.get("method")?.clone()),
    ];
    for (name, value) in members {
        object.insert(mandate_canon::Key::new(name).ok()?, value);
    }
    Some(Value::Object(object))
}

/// Whether the principal is one the mandate version an approval binds lists in
/// `autonomy.approval.approvers` ([DEC-2129] item 7, as [DEC-2130] item 1 reads it): the
/// references resolved by [`Roster::approvers`] over the control stream's membership records at
/// the request's instant (DEC-2130 item 3, DEC-2384 item 3), the set the runtime's admission check
/// 3 compares a responder with. A version that is not named, not stored, or not a mandate, or a fold
/// that cannot be read, is `journal_unavailable`, never a pass.
///
/// [DEC-2129]: ../../../docs/project/decisions/DEC-2129.md
/// [DEC-2130]: ../../../docs/project/decisions/DEC-2130.md
fn approves(
    app: &App,
    reader: &Reader,
    version: Option<&str>,
    principal: PrincipalId,
) -> Result<bool, Refused> {
    let store = app
        .artifacts
        .as_deref()
        .ok_or(Refused::JournalUnavailable)?;
    let (_, mandate) = version
        .and_then(|version| stored(store, version))
        .ok_or(Refused::JournalUnavailable)?;
    let records = (app.members)(&reader.control).map_err(|_| Refused::JournalUnavailable)?;
    let roster = Roster::read(records, reader.now).map_err(|_| Refused::JournalUnavailable)?;
    let references: Vec<&str> = mandate
        .autonomy
        .approval
        .approvers
        .iter()
        .map(|approver| approver.as_str())
        .collect();
    Ok(roster.approvers(&references).contains(&principal))
}

/// The deployed agent whose stream requested `approval`, found by equality with an
/// `ApprovalRequested` event id, with the mandate version its content binds
/// (`trigger.mandate_version`) when it names one, and whether its deferred checks are exactly its
/// decision's ([`deferrals_agree`], DEC-2521). A stream that cannot be read does not hide the
/// approval on another that can; only when none that was read holds it and one could not be read
/// is the answer `journal_unavailable` rather than `not_found`.
async fn requester(
    app: &App,
    reader: &Reader,
    approval: &str,
) -> Result<Option<(String, Option<String>, bool)>, Refused> {
    let mut unreadable = false;
    for deployment in deployed(&reader.control) {
        let stream = format!("agent:{}:{}", reader.workspace, deployment.0);
        let Ok(rows) = app.journal.rows(&stream).await else {
            unreadable = true;
            continue;
        };
        let requested = rows
            .iter()
            .find(|row| row.event_type == "ApprovalRequested" && row.event_id == approval);
        if let Some(row) = requested {
            let version = parse(&row.body).ok().and_then(|event| {
                ["payload", "content", "trigger", "mandate_version"]
                    .iter()
                    .try_fold(&event, |value, name| value.get(name))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            });
            let agrees = deferrals_agree(row, &rows);
            return Ok(Some((deployment.0, version, agrees)));
        }
    }
    match unreadable {
        true => Err(Refused::JournalUnavailable),
        false => Ok(None),
    }
}

/// The text of a content reference, as it serializes.
pub(crate) fn ref_text(reference: &Ref) -> Option<String> {
    json_string(&encode(reference).ok()?)
}

/// `POST /v1/workspaces/{ws}/agents/{id}/resume` (§3.11 item 12, DEC-2102): recorded with the
/// evidence the port verified, or `null`, which the runtime refuses (§5.4).
pub async fn resume(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
    body: Body,
) -> Response {
    let Ok(Path((workspace, agent))) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let (gate, request) = match strictly(&app, &headers, body, decode::<ResumeRequest>).await {
        Ok(admitted) => admitted,
        Err(refusal) => return *refusal,
    };
    let bound = Bound::command("resume", &agent);
    let make = |step_up| Command::Resume {
        agent: agent.clone(),
        step_up,
    };
    let forwarded = Forwarded {
        kind: StepUpActionKind::Resume,
        bound,
        presented: request.step_up.as_ref(),
        assertion: request.step_up_assertion.as_ref(),
        asserted_as: RESUME_ASSERTED,
    };
    stepped(&app, &gate, &workspace, &make, &forwarded).await
}

/// `POST /v1/workspaces/{ws}/agents/{id}/stop` (§3.11 item 12, DEC-2102 items 3 and 4): a Stop
/// with release names the platform's own release warning, else `invalid` at `/warning_shown`.
pub async fn stop(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
    body: Body,
) -> Response {
    let Ok(Path((workspace, agent))) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let (gate, request) = match strictly(&app, &headers, body, decode::<StopRequest>).await {
        Ok(admitted) => admitted,
        Err(refusal) => return *refusal,
    };
    let shown = request.warning_shown.as_ref().and_then(ref_text);
    if request.release && shown.as_deref() != Some(release_warning().as_str()) {
        return invalid(vec![schema_violation("/warning_shown", "warning_unknown")]);
    }
    let release = request.release;
    let confirms = if release { "stop_release" } else { "stop" };
    let make = |step_up| Command::Stop {
        agent: agent.clone(),
        release,
        step_up,
    };
    let forwarded = Forwarded {
        kind: StepUpActionKind::Stop,
        bound: Bound::command(confirms, &agent),
        presented: request.step_up.as_ref(),
        assertion: request.step_up_assertion.as_ref(),
        asserted_as: STOP_ASSERTED,
    };
    stepped(&app, &gate, &workspace, &make, &forwarded).await
}

/// `POST /v1/workspaces/{ws}/agents/{id}/exits` (§3.11 item 12, §5.4, DEC-2102): an API-7
/// operation, decoded leniently; recorded with the confirmed bid when all of it was sent, and no
/// step-up, so it loses only the owner-exit privilege (mandate spec §6.1).
pub async fn owner_exit(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
    body: Body,
) -> Response {
    let Ok(Path((workspace, agent))) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let (gate, kept) =
        match admitted::<OwnerExitRequest>(&app, &headers, Request::Other, body).await {
            Ok(accepted) => accepted,
            Err(refusal) => return refusal.response(),
        };
    let request = &kept.value;
    let Some(instrument) = asset_text(&request.instrument) else {
        return Refused::Invalid.response();
    };
    let bid = match (&request.bid, &request.bid_size, &request.floor) {
        (Some(bid), Some(size), Some(floor)) => {
            match (decimal_text(bid), decimal_text(size), decimal_text(floor)) {
                (Some(bid), Some(size), Some(floor)) => Some([bid, size, floor]),
                _ => None,
            }
        }
        _ => None,
    };
    let command = Command::OwnerExit {
        agent,
        instrument,
        bid,
    };
    recorded(&app, &gate, &workspace, &command, &kept.dropped, None).await
}

/// A strict command's gate and body, decoded as `T`, or its refusal: §3.3's checks, then
/// `invalid` with the decoder's violations.
pub(crate) async fn strictly<T>(
    app: &App,
    headers: &HeaderMap,
    body: Body,
    decoded: fn(&[u8]) -> Result<T, mandate_api::wire::Refused>,
) -> Result<(Gate, T), Box<Response>> {
    let (gate, body) = strict(app, headers, body)
        .await
        .map_err(|refused| Box::new(refused.response()))?;
    match decoded(&body) {
        Ok(request) => Ok((gate, request)),
        Err(mandate_api::wire::Refused::Invalid { violations }) => {
            Err(Box::new(invalid(violations)))
        }
    }
}

fn schema_violation(path: &str, code: &str) -> Violation {
    Violation::Schema {
        path: path.to_owned(),
        code: code.to_owned(),
        message: "The request does not meet this rule.".to_owned(),
    }
}

/// DEC-279 item 1's object for a command, which the step-up digest binds (§3.6): what it
/// confirms, the agent, and the control stream's head.
struct Bound {
    confirms: &'static str,
    agent: String,
}

impl Bound {
    fn command(confirms: &'static str, agent: &str) -> Self {
        Self {
            confirms,
            agent: agent.to_owned(),
        }
    }

    /// The SHA-256 of the canonical object at the control stream's head `head`.
    fn digest(&self, head: u64) -> Option<Digest> {
        let mut object = mandate_canon::Object::new();
        let members = [
            ("confirms", text_value(self.confirms)),
            ("agent", text_value(&self.agent)),
            ("control_head", text_value(&head.to_string())),
            ("instrument", Value::Null),
            ("bid", Value::Null),
            ("bid_size", Value::Null),
            ("floor", Value::Null),
            ("event", Value::Null),
        ];
        for (name, value) in members {
            object.insert(mandate_canon::Key::new(name).ok()?, value);
        }
        Some(Digest::of(&to_canonical(&Value::Object(object))))
    }
}

fn text_value(value: &str) -> Value {
    Value::Str(value.to_owned())
}

/// What a step-up command forwards: its kind, its bound object, the evidence presented, the passkey
/// assertion beside it, and the operation §3.4 derives that assertion's `StepUpAsserted` id under.
struct Forwarded<'a> {
    kind: StepUpActionKind,
    bound: Bound,
    presented: Option<&'a StepUpEvidence>,
    assertion: Option<&'a StepUpAssertion>,
    asserted_as: &'static str,
}

/// Authorizes the command `make` builds and records it with the presented evidence when the port
/// verifies it, else with `null` (DEC-2102 item 4). Evidence with a passkey assertion beside it is
/// judged by the port's `asserted`, and a verified one commits its `StepUpAsserted` after the
/// command in the same batch ([DEC-2128] items 2 and 3). A repeat whose key holds the command with
/// the evidence as presented replays before the port is asked again (§3.4 step 1).
///
/// [DEC-2128]: ../../../docs/project/decisions/DEC-2128.md
async fn stepped(
    app: &App,
    gate: &Gate,
    workspace: &str,
    make: &(dyn Fn(Value) -> Command + Sync),
    forwarded: &Forwarded<'_>,
) -> Response {
    let null = make(Value::Null);
    let reader = match authorized(
        app,
        &gate.admitted,
        gate.now,
        workspace,
        null.permission(),
        Unreadable::Command,
    )
    .await
    {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    if !in_workspace(&reader.control, &null) {
        return Refused::NotFound.response();
    }
    let Some(presented) = journaled(forwarded.presented) else {
        return Refused::Invalid.response();
    };
    let as_presented = make(presented.clone());
    if let Some(row) = replayed(gate, &reader, &as_presented) {
        return accepted(as_presented.id_member(), &row, gate.now, &[], None);
    }
    let head = reader.control.last().map_or(0, |row| row.seq);
    let verified: Option<Option<Asserted>> =
        match (forwarded.presented, forwarded.bound.digest(head)) {
            (Some(evidence), Some(digest)) => {
                let action = Action {
                    kind: forwarded.kind,
                    digest,
                };
                let judged = Judged {
                    tenant: &reader.tenant,
                    control: &reader.control,
                    now: gate.now,
                };
                match forwarded.assertion {
                    Some(assertion) => {
                        match app
                            .step_up
                            .asserted(evidence, Some(assertion), &action, &judged)
                        {
                            Ok(asserted) => asserted.map(Some),
                            Err(Unverified::Unimplemented { story }) => {
                                return invalid(vec![Unverified::unimplemented(story)]);
                            }
                            Err(Unverified::Code(_)) => None,
                        }
                    }
                    None => app
                        .step_up
                        .verify(evidence, &action, &judged)
                        .is_ok()
                        .then_some(None),
                }
            }
            _ => None,
        };
    match verified {
        Some(asserted) => {
            let asserted = asserted.as_ref().map(|a| (a, forwarded.asserted_as));
            committed_asserted(app, gate, &reader, &as_presented, asserted).await
        }
        None => committed(app, gate, &reader, &null, &[], None).await,
    }
}

/// The row the gate's key already holds for `command`, when it is that command.
fn replayed(gate: &Gate, reader: &Reader, command: &Command) -> Option<StoredEvent> {
    let (_, id) = derived(gate, &reader.workspace, command)?;
    let issued = Issued {
        workspace: &reader.workspace,
        principal: &principal_text(gate)?,
        event_id: &id,
        environment: &reader.control.first()?.environment,
        now: gate.now,
        command,
    };
    match repeated(&reader.control, &id, &draft(&issued)?)? {
        Settled::Stored(row) => Some(*row),
        _ => None,
    }
}

fn asset_text(asset: &Asset) -> Option<String> {
    json_string(&encode(asset).ok()?)
}

fn decimal_text(decimal: &Decimal) -> Option<String> {
    json_string(&encode(decimal).ok()?)
}
