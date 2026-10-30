//! The version and deployment routes (§3.11 item 11, §4.1, §4.2, §5.1,
//! [DEC-2101](../../../docs/project/decisions/DEC-2101.md)): `POST
//! /v1/workspaces/{ws}/mandate-versions`, `GET …/mandate-versions/{v}/confirmation`, `POST
//! …/mandate-versions/{v}/confirm`, `POST …/agents`, and `POST …/agents/{id}/go-live`.
//!
//! Each command is checked in §3.3 item 1's order (the session, the CSRF mark, the key, then the
//! body, decoded strictly), authorized for a user by its matrix row, compared with the event its
//! key already holds before anything is judged again (§3.4 step 1), then judged as the CLI judges
//! it, step-up last, through the port that verifies nothing until E9-4 (DEC-2101 item 2). Its
//! record is in the store and its event on the control stream before the answer (rule 5). A
//! deployment names its connection at version 2 exactly when journal spec §9.24 rule 201 holds on
//! the control stream, appended at the head it was decided at; rule 200 holds by construction,
//! since the `connection_id` it names is the one parsed from the stored document it deploys.

use std::collections::BTreeSet;
use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use mandate_api::envelope::{Record, StepUpAssertion, StepUpEvidence};
use mandate_api::problem::{CurrentBase, Effect, Problem, ProblemCode, Violation};
use mandate_api::requests::{ConfirmRequest, assertion_checked};
use mandate_api::responses::Classification;
use mandate_api::wire::{EventId, Id, Ref, decode, encode};
use mandate_authn::Request;
use mandate_canon::{Key, Value, parse, to_canonical};
use mandate_identity::{Permission, StepUpActionKind};
use mandate_journal::{StoredEvent, StreamId};
use mandate_spec::Mandate;

use crate::commands::{Event, Settled, Writer, envelope};
use crate::http::{App, browser, invalid, problem, problem_body};
use crate::objects::ObjectStore;
use crate::reads::account_ref;
use crate::real_money::{Action as Gated, gated};
use crate::routes::{
    Reader, Refused, Unreadable, authorized, enveloped, path_refused, session, string, watermark,
};
use crate::stepup::{Action, Asserted, Judged, Unverified, journaled, waiver_id};
use crate::versions::{
    CONFIRMED, CREATED, DEPLOYED, FIRST_VERSION, Pending, Refusal, active, another_on_account,
    check, confirm_digest, deploy_digest, envelope_paths, established_v2, instruments,
    last_confirmed, latest, object, reference, screen, stopped, stored, text,
};
use crate::writes::{
    Gate, appended_batch_at, derived_for, id_text, one_key, principal_text, ref_text, strict,
    unknown,
};

/// The message every violation these routes report carries: the code and the pointer say what and
/// where, and nothing from the body is echoed (rule 6).
const MESSAGE: &str = "The request does not meet this rule.";
/// The operation §3.4 derives a confirmation's `StepUpAsserted` id under, distinct from the
/// confirmation's own `confirm_version` (DEC-2128 item 3).
pub const CONFIRM_ASSERTED: &str = "confirm_version_step_up_asserted";
/// The operation §3.4 derives a deployment's `StepUpAsserted` id under, distinct from the
/// deployment's own `deploy` (DEC-2128 item 3).
pub const DEPLOY_ASSERTED: &str = "deploy_step_up_asserted";

/// A refused request, answered by one problem document.
enum Answer {
    Refused(Refused),
    Code(StatusCode, ProblemCode),
    Invalid(Vec<Violation>),
    Stale(String),
    Unknown(EventId),
}

impl From<Refused> for Answer {
    fn from(refused: Refused) -> Self {
        Self::Refused(refused)
    }
}

impl Answer {
    fn response(self) -> Response {
        match self {
            Self::Refused(refused) => refused.response(),
            Self::Code(status, code) => problem(status, problem_body(code)),
            Self::Invalid(violations) => invalid(violations),
            Self::Stale(current) => {
                let body = Ref::try_from(current).ok().and_then(|current| {
                    let base = Some(CurrentBase::Ref(current));
                    Problem::of(ProblemCode::StaleBase, Effect::None, None, base).ok()
                });
                problem(StatusCode::CONFLICT, encoded(body.as_ref()))
            }
            Self::Unknown(id) => unknown(id),
        }
    }

    fn schema(path: &str, code: &str) -> Self {
        Self::Invalid(vec![Violation::Schema {
            path: path.to_owned(),
            code: code.to_owned(),
            message: MESSAGE.to_owned(),
        }])
    }

    fn rules(path: &str, codes: &[&str]) -> Self {
        let violations = codes
            .iter()
            .map(|code| Violation::Rule {
                path: path.to_owned(),
                code: (*code).to_owned(),
                message: MESSAGE.to_owned(),
            })
            .collect();
        Self::Invalid(violations)
    }

    fn of(refusal: Refusal) -> Self {
        match refusal {
            Refusal::Rules(codes) => Self::rules("", &codes),
            Refusal::Unregistered => {
                Self::schema("/universe/pinned_instruments", "instrument_unregistered")
            }
            Refusal::Unreadable => Self::Refused(Refused::JournalUnavailable),
        }
    }

    fn step_up(code: ProblemCode) -> Self {
        Self::Code(StatusCode::UNAUTHORIZED, code)
    }
}

/// A problem's bytes; an empty body only for one `mandate-api` cannot encode.
fn encoded(problem: Option<&Problem>) -> Bytes {
    problem
        .and_then(|problem| encode(problem).ok())
        .map(Bytes::from)
        .unwrap_or_default()
}

/// One of these routes' answers, as a response.
fn answered(answer: Result<Response, Answer>) -> Response {
    answer.unwrap_or_else(Answer::response)
}

/// What every command here has once admitted: its gate, body, authorized reader, the store, the
/// control stream's environment, and the principal's id.
struct Admitted {
    gate: Gate,
    body: Bytes,
    reader: Reader,
    objects: Arc<dyn ObjectStore>,
    environment: String,
    principal: String,
}

/// §3.3's checks, the strict body's bytes, and `permission`'s row for a user; then the store and
/// the control stream's environment, read from its first row (a workspace with none is the one
/// 404).
async fn admitted(
    app: &App,
    headers: &HeaderMap,
    workspace: &str,
    permission: Permission,
    body: Body,
) -> Result<Admitted, Answer> {
    let (gate, body) = strict(app, headers, body).await?;
    let reader = authorized(
        app,
        &gate.admitted,
        gate.now,
        workspace,
        permission,
        Unreadable::Command,
    )
    .await?;
    let objects = app.objects.clone().ok_or(Refused::JournalUnavailable)?;
    let environment = reader
        .control
        .first()
        .map(|row| row.environment.clone())
        .ok_or(Refused::NotFound)?;
    let principal = principal_text(&gate).ok_or(Refused::JournalUnavailable)?;
    Ok(Admitted {
        gate,
        body,
        reader,
        objects,
        environment,
        principal,
    })
}

/// A check's pending confirmation of `paths`.
fn confirming(paths: &[String]) -> Pending<'_> {
    Pending {
        created: None,
        confirmed: Some(paths),
    }
}

/// A record member as the journal's canonical value.
fn record_value(record: &Record) -> Option<Value> {
    parse(&encode(record).ok()?).ok()
}

/// `record`'s canonical bytes, and the event naming it and `version` with `members`.
fn recorded(
    record: &Value,
    version: &str,
    event_type: &'static str,
    mut members: Vec<(&'static str, Value)>,
    config_refs: Vec<(&'static str, Value)>,
) -> Option<(Vec<u8>, Event)> {
    let bytes = to_canonical(record);
    let record_ref = reference(&bytes);
    members.push(("record_ref", text(&record_ref)));
    let mut refs = [version.to_owned(), record_ref];
    refs.sort();
    let event = Event {
        event_type,
        schema_version: 1,
        payload: object(members)?,
        artifact_refs: refs.iter().map(|r| text(r)).collect(),
        config_refs,
    };
    Some((bytes, event))
}

/// A command ready to append: its derived id, its record's bytes, its draft, and the head it must
/// be appended at, when what it decided from the rows must still hold at the append.
struct Drafted {
    id: EventId,
    text: String,
    record: Vec<u8>,
    draft: Vec<u8>,
    pinned: Option<u64>,
}

/// `event` under §3.4's id for `operation`, with its record's bytes.
fn drafted(
    admitted: &Admitted,
    operation: &str,
    (record, event): (Vec<u8>, Event),
) -> Result<Drafted, Answer> {
    let (id, text) = derived_for(&admitted.gate, &admitted.reader.workspace, operation)
        .ok_or(Refused::JournalUnavailable)?;
    let writer = Writer {
        workspace: &admitted.reader.workspace,
        principal: &admitted.principal,
        event_id: &text,
        environment: &admitted.environment,
        now: admitted.gate.now,
    };
    let draft = envelope(&writer, &event).ok_or(Refused::JournalUnavailable)?;
    Ok(Drafted {
        id,
        text,
        record,
        draft,
        pinned: None,
    })
}

/// The event the key already holds (§3.4 step 1): `Some` of the stored row, or of the conflict.
fn repeat(admitted: &Admitted, drafted: &Drafted) -> Option<Result<StoredEvent, Answer>> {
    let settled = crate::writes::repeated(&admitted.reader.control, &drafted.text, &drafted.draft)?;
    Some(outcome(settled, &drafted.id))
}

/// Stores the record, then appends the draft (rule 5), with the `StepUpAsserted` that authorized
/// it, when there is one, in the same batch after it (journal spec §9.32, [DEC-2128] item 3).
///
/// [DEC-2128]: ../../../docs/project/decisions/DEC-2128.md
async fn commit(
    app: &App,
    admitted: &Admitted,
    drafted: Drafted,
    assertion: &[Vec<u8>],
) -> Result<StoredEvent, Answer> {
    admitted
        .objects
        .put(&drafted.record)
        .map_err(|_| Refused::JournalUnavailable)?;
    let batch: Vec<&[u8]> = std::iter::once(drafted.draft.as_slice())
        .chain(assertion.iter().map(Vec::as_slice))
        .collect();
    let settled = appended_batch_at(
        app,
        &admitted.reader,
        (&drafted.text, &batch),
        admitted.gate.now,
        drafted.pinned,
    )
    .await;
    outcome(settled, &drafted.id)
}

/// The `StepUpAsserted` a step-up `asserted` leaves to append after the event `drafted` names,
/// under §3.4's id for `operation` (DEC-2128 item 3), then its `StepUpCoolOffWaived` when a test
/// cell waived the cool-off (DEC-2146 item 4).
fn assertion_drafted(
    admitted: &Admitted,
    drafted: &Drafted,
    operation: &str,
    asserted: &Asserted,
) -> Result<Vec<Vec<u8>>, Answer> {
    let derive = |operation: &str| {
        derived_for(&admitted.gate, &admitted.reader.workspace, operation).map(|(_, id)| id)
    };
    let event_id = derive(operation).ok_or(Refused::JournalUnavailable)?;
    let waiver = waiver_id(asserted, operation, derive).ok_or(Refused::JournalUnavailable)?;
    let writer = Writer {
        workspace: &admitted.reader.workspace,
        principal: &admitted.principal,
        event_id: &event_id,
        environment: &admitted.environment,
        now: admitted.gate.now,
    };
    asserted
        .drafted(
            admitted.objects.as_ref(),
            &writer,
            &drafted.text,
            waiver.as_deref(),
        )
        .ok_or(Refused::JournalUnavailable.into())
}

/// An append's outcome: the stored row, or its problem.
fn outcome(settled: Settled, id: &EventId) -> Result<StoredEvent, Answer> {
    match settled {
        Settled::Stored(row) => Ok(*row),
        Settled::Conflict => Err(Answer::Code(
            StatusCode::CONFLICT,
            ProblemCode::IdempotencyConflict,
        )),
        Settled::Unknown => Err(Answer::Unknown(id.clone())),
        Settled::Unavailable => Err(Refused::JournalUnavailable.into()),
    }
}

/// Judges `evidence`, with the `assertion` beside it, for `action` through the port's `asserted`
/// ([DEC-2128] item 2): `step_up_required` without any (§3.5), and `journal_unavailable` when the
/// port cannot read what it judges by.
///
/// [DEC-2128]: ../../../docs/project/decisions/DEC-2128.md
fn stepped_up(
    app: &App,
    admitted: &Admitted,
    evidence: Option<&StepUpEvidence>,
    assertion: Option<&StepUpAssertion>,
    action: &Action,
) -> Result<Option<Asserted>, Answer> {
    let evidence = evidence.ok_or(Answer::step_up(ProblemCode::StepUpRequired))?;
    let judged = Judged {
        tenant: &admitted.reader.tenant,
        control: &admitted.reader.control,
        now: admitted.gate.now,
    };
    app.step_up
        .asserted(evidence, assertion, action, &judged)
        .map_err(|unverified| match unverified {
            Unverified::Code(ProblemCode::JournalUnavailable) => {
                Answer::from(Refused::JournalUnavailable)
            }
            Unverified::Code(code) => Answer::step_up(code),
        })
}

/// `members` with the event's watermark, enveloped, as `status`.
fn served(
    status: StatusCode,
    admitted: &Admitted,
    row: &StoredEvent,
    mut members: Vec<(&str, Value)>,
) -> Response {
    members.push((
        "as_of",
        Value::Array(watermark(std::slice::from_ref(row)).into_iter().collect()),
    ));
    enveloped(status, admitted.gate.now, members)
}

/// The version's document, recorded on the control stream and in the store, or the one 404.
fn document(admitted: &Admitted, version: &str) -> Result<(Value, Mandate), Answer> {
    latest(&admitted.reader.control, CREATED, version).ok_or(Refused::NotFound)?;
    Ok(stored(admitted.objects.as_ref(), version).ok_or(Refused::NotFound)?)
}

/// `POST /v1/workspaces/{ws}/mandate-versions` (§3.11 item 11, DEC-2101 item 3).
pub async fn create_version(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
    body: Body,
) -> Response {
    let Ok(Path(workspace)) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    answered(created(&app, &headers, &workspace, body).await)
}

async fn created(
    app: &App,
    headers: &HeaderMap,
    workspace: &str,
    body: Body,
) -> Result<Response, Answer> {
    let admitted = admitted(app, headers, workspace, Permission::DraftVersion, body).await?;
    created_by(app, &admitted)
        .await
        .map(|(response, _)| response)
}

/// [`created`] once admitted, with the version it created or found.
async fn created_by(app: &App, admitted: &Admitted) -> Result<(Response, String), Answer> {
    let value = parse(&admitted.body).map_err(|_| Answer::schema("", "malformed"))?;
    let mandate = Mandate::parse(&value).map_err(|_| Answer::schema("", "mandate"))?;
    match value.get("environment").and_then(Value::as_str) {
        Some("live") => {
            gated(app, &admitted.reader, &admitted.gate, Gated::CreateVersion).await?;
            return Err(Answer::Code(
                StatusCode::CONFLICT,
                ProblemCode::LiveUnavailable,
            ));
        }
        Some(environment) if environment == admitted.environment => {}
        _ => return Err(Answer::schema("/environment", "environment")),
    }
    let canonical = mandate
        .canonical_bytes()
        .map_err(|_| Answer::schema("", "mandate"))?;
    let version = reference(&canonical);
    let paths = envelope_paths(&value);
    let provenance = paths
        .iter()
        .map(|path| object(vec![("path", text(path)), ("source", text("user_entered"))]))
        .collect::<Option<Vec<_>>>()
        .ok_or(Refused::JournalUnavailable)?;
    let record = object(vec![
        ("kind", text("mandate_version_record")),
        ("mandate_version", text(&version)),
        ("user", text(&admitted.principal)),
    ])
    .ok_or(Refused::JournalUnavailable)?;
    let event = recorded(
        &record,
        &version,
        CREATED,
        vec![
            ("mandate_version", text(&version)),
            ("provenance", Value::Array(provenance.clone())),
        ],
        Vec::new(),
    )
    .ok_or(Refused::JournalUnavailable)?;
    let drafted = drafted(admitted, "create_version", event)?;
    let created = object(vec![
        ("mandate_version", text(&version)),
        ("provenance", Value::Array(provenance)),
    ])
    .ok_or(Refused::JournalUnavailable)?;
    let pending = Pending {
        created: Some(&created),
        confirmed: Some(&paths),
    };
    let date = admitted.gate.now.date();
    let control = &admitted.reader.control;
    let objects = admitted.objects.as_ref();
    let earlier = repeat(admitted, &drafted)
        .map(|repeated| repeated.map(|row| (StatusCode::ACCEPTED, row)))
        .or_else(|| {
            latest(control, CREATED, &version).map(|row| Ok((StatusCode::OK, row.clone())))
        });
    if let Some(earlier) = earlier {
        let (status, row) = earlier?;
        let warnings =
            check((control, &[]), objects, (&mandate, ""), pending, date).unwrap_or_default();
        let response = version_created(status, admitted, &row, &version, &warnings);
        return Ok((response, version));
    }
    let warnings =
        check((control, &[]), objects, (&mandate, ""), pending, date).map_err(Answer::of)?;
    instruments(control, objects, &value).map_err(Answer::of)?;
    objects
        .put(&canonical)
        .map_err(|_| Refused::JournalUnavailable)?;
    let row = commit(app, admitted, drafted, &[]).await?;
    let response = version_created(StatusCode::ACCEPTED, admitted, &row, &version, &warnings);
    Ok((response, version))
}

/// The version a chat's setup created (DEC-2293): its reference and the summary its confirmation
/// card shows.
pub struct ChatVersion {
    pub hash: String,
    pub summary: Value,
}

/// Creates a mandate version from a setup chat's `document` exactly as `POST /mandate-versions`
/// does, under the owner's session and a fresh authorization for *Draft a mandate version*, with
/// an idempotency key of the document's own; its problems, as `{path, code}` items, otherwise.
///
/// # Errors
/// The violations, or the one refusal's code, as a JSON array.
pub(crate) async fn create_for_chat(
    app: &App,
    gate: &Gate,
    workspace: &str,
    document: Value,
) -> Result<ChatVersion, Value> {
    let failure = |code: &str| {
        Value::Array(vec![
            object(vec![("code", text(code))]).unwrap_or(Value::Null),
        ])
    };
    let body = mandate_canon::to_canonical(&document);
    let digest = mandate_canon::Digest::of(&body).to_hex();
    let key = mandate_api::idempotency::IdempotencyKey::parse(&format!(
        "chat-draft-{}",
        digest.get(..40).unwrap_or_default()
    ))
    .map_err(|_| failure("idempotency"))?;
    let now = app.clock.now().unwrap_or(gate.now);
    let gate = Gate {
        admitted: gate.admitted,
        now,
        key,
    };
    let reader = authorized(
        app,
        &gate.admitted,
        now,
        workspace,
        Permission::DraftVersion,
        Unreadable::Command,
    )
    .await
    .map_err(|_| failure("not_permitted"))?;
    let objects = app.objects.clone().ok_or_else(|| failure("no_store"))?;
    let environment = reader
        .control
        .first()
        .map(|row| row.environment.clone())
        .ok_or_else(|| failure("not_found"))?;
    let principal = principal_text(&gate).ok_or_else(|| failure("journal_unavailable"))?;
    let admitted = Admitted {
        gate,
        body: Bytes::from(body),
        reader,
        objects,
        environment,
        principal,
    };
    match created_by(app, &admitted).await {
        Ok((_, version)) => {
            let summary = app
                .artifacts
                .as_ref()
                .and_then(|store| crate::documents::envelope(store.as_ref(), &version))
                .map(|envelope| {
                    object(vec![
                        ("name", text(&envelope.label)),
                        ("mandate", envelope.mandate),
                    ])
                    .unwrap_or(Value::Null)
                })
                .unwrap_or(Value::Null);
            Ok(ChatVersion {
                hash: version,
                summary,
            })
        }
        Err(Answer::Invalid(violations)) => Err(Value::Array(
            violations
                .iter()
                .map(|v| {
                    let (path, code) = match v {
                        Violation::Schema { path, code, .. }
                        | Violation::Rule { path, code, .. } => (path.as_str(), code.as_str()),
                        _ => ("", "invalid"),
                    };
                    object(vec![("path", text(path)), ("code", text(code))]).unwrap_or(Value::Null)
                })
                .collect(),
        )),
        Err(Answer::Code(_, code)) => Err(failure(&format!("{code:?}"))),
        Err(_) => Err(failure("refused")),
    }
}

/// `commands/version-created`.
fn version_created(
    status: StatusCode,
    admitted: &Admitted,
    row: &StoredEvent,
    version: &str,
    warnings: &[&str],
) -> Response {
    served(
        status,
        admitted,
        row,
        vec![
            ("command_id", string(&row.event_id)),
            ("phase", string("recorded")),
            ("mandate_version", string(version)),
            (
                "warnings",
                Value::Array(warnings.iter().map(|w| string(w)).collect()),
            ),
        ],
    )
}

/// `GET /v1/workspaces/{ws}/mandate-versions/{v}/confirmation` (§3.11 item 11, DEC-2101 item 4).
pub async fn confirmation(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
) -> Response {
    let Ok(Path((workspace, version))) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    answered(screened(&app, &headers, &workspace, &version).await)
}

async fn screened(
    app: &App,
    headers: &HeaderMap,
    workspace: &str,
    version: &str,
) -> Result<Response, Answer> {
    let (admitted, now) = session(app, headers, Request::Other)?;
    let reader = authorized(
        app,
        &admitted,
        now,
        workspace,
        Permission::DraftVersion,
        Unreadable::Read,
    )
    .await?;
    let objects = app.objects.clone().ok_or(Refused::JournalUnavailable)?;
    latest(&reader.control, CREATED, version).ok_or(Refused::NotFound)?;
    let (document, mandate) = stored(objects.as_ref(), version).ok_or(Refused::NotFound)?;
    let required = envelope_paths(&document);
    let warnings = check(
        (&reader.control, &[]),
        objects.as_ref(),
        (&mandate, ""),
        confirming(&required),
        now.date(),
    )
    .map_err(Answer::of)?;
    let shown = screen(version, required, warnings);
    let members = vec![
        ("mandate_version", string(version)),
        ("document", document),
        ("agent_id", Value::Null),
        ("base_version", Value::Null),
        ("classification", string(FIRST_VERSION)),
        (
            "required_paths",
            Value::Array(shown.required_paths.iter().map(|p| string(p)).collect()),
        ),
        (
            "warnings",
            Value::Array(shown.warnings.iter().map(|w| string(w)).collect()),
        ),
        ("step_up_required", Value::Bool(true)),
        ("screen_digest", string(&shown.digest)),
        (
            "as_of",
            Value::Array(watermark(&reader.control).into_iter().collect()),
        ),
    ];
    Ok(enveloped(StatusCode::OK, now, members))
}

/// `POST /v1/workspaces/{ws}/mandate-versions/{v}/confirm` (§5.1, DEC-2101 item 5).
pub async fn confirm(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
    body: Body,
) -> Response {
    let Ok(Path((workspace, version))) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    answered(confirmed(&app, &headers, &workspace, &version, body).await)
}

async fn confirmed(
    app: &App,
    headers: &HeaderMap,
    workspace: &str,
    version: &str,
    body: Body,
) -> Result<Response, Answer> {
    let admitted = admitted(
        app,
        headers,
        workspace,
        Permission::ConfirmRiskIncreasing,
        body,
    )
    .await?;
    let request = decode::<ConfirmRequest>(&admitted.body).map_err(|refused| {
        let mandate_api::wire::Refused::Invalid { violations } = refused;
        Answer::Invalid(violations)
    })?;
    if request.agent_id.is_some() {
        return Err(Answer::schema("/agent_id", "agent_link_unavailable"));
    }
    if request.base_version.is_some() {
        return Err(Answer::schema("/base_version", "no_base"));
    }
    let mut paths = request.confirmed_paths.clone();
    paths.sort();
    let acknowledged: Vec<String> = request
        .warnings_acknowledged
        .iter()
        .filter_map(id_text)
        .collect();
    let mut sorted_acknowledged: Vec<&str> = acknowledged.iter().map(String::as_str).collect();
    sorted_acknowledged.sort_unstable();
    let shown_digest = ref_text(&request.screen_digest).ok_or(Refused::JournalUnavailable)?;
    let record = object(vec![
        ("kind", text("mandate_confirmation_record")),
        ("mandate_version", text(version)),
        (
            "record",
            record_value(&request.record).ok_or(Refused::JournalUnavailable)?,
        ),
        ("screen_digest", text(&shown_digest)),
        (
            "step_up",
            journaled(request.step_up.as_ref()).ok_or(Refused::JournalUnavailable)?,
        ),
        ("user", text(&admitted.principal)),
        (
            "warnings_acknowledged",
            Value::Array(sorted_acknowledged.iter().map(|w| text(w)).collect()),
        ),
    ])
    .ok_or(Refused::JournalUnavailable)?;
    let event = recorded(
        &record,
        version,
        CONFIRMED,
        vec![
            ("mandate_version", text(version)),
            (
                "confirmed_paths",
                Value::Array(paths.iter().map(|p| text(p)).collect()),
            ),
        ],
        Vec::new(),
    )
    .ok_or(Refused::JournalUnavailable)?;
    let passkey = request.step_up_assertion.is_some();
    let event = match (passkey, request.step_up.as_ref()) {
        (true, Some(evidence)) => {
            let (record, event) = event;
            let judged = journaled(Some(evidence)).ok_or(Refused::JournalUnavailable)?;
            (
                record,
                passkey_confirmation(event, judged).ok_or(Refused::JournalUnavailable)?,
            )
        }
        _ => event,
    };
    let drafted = drafted(&admitted, "confirm_version", event)?;
    if let Some(repeated) = repeat(&admitted, &drafted) {
        return Ok(confirm_accepted(
            StatusCode::ACCEPTED,
            &admitted,
            &repeated?,
        ));
    }
    let (document, mandate) = document(&admitted, version)?;
    if request.classification_shown != Classification::RiskIncreasing {
        return Err(Answer::Code(
            StatusCode::CONFLICT,
            ProblemCode::ClassificationChanged,
        ));
    }
    let required = envelope_paths(&document);
    if paths != required {
        return Err(Answer::rules("/confirmed_paths", &["V-020"]));
    }
    let control = &admitted.reader.control;
    let objects = admitted.objects.as_ref();
    let date = admitted.gate.now.date();
    let warnings = check(
        (control, &[]),
        objects,
        (&mandate, ""),
        confirming(&required),
        date,
    )
    .map_err(Answer::of)?;
    instruments(control, objects, &document).map_err(Answer::of)?;
    let shown: BTreeSet<&str> = warnings.iter().copied().collect();
    let owned: BTreeSet<&str> = acknowledged.iter().map(String::as_str).collect();
    if shown != owned || owned.len() != acknowledged.len() {
        return Err(Answer::schema("/warnings_acknowledged", "unacknowledged"));
    }
    if screen(version, required, warnings).digest != shown_digest {
        return Err(Answer::Code(
            StatusCode::CONFLICT,
            ProblemCode::ClassificationChanged,
        ));
    }
    if let Some((row, last)) = last_confirmed(control)
        && last == version
    {
        return Ok(confirm_accepted(StatusCode::OK, &admitted, row));
    }
    let digest = confirm_digest(version).ok_or(Refused::JournalUnavailable)?;
    let action = Action {
        kind: StepUpActionKind::ConfirmVersion,
        digest,
    };
    let asserted = stepped_up(
        app,
        &admitted,
        request.step_up.as_ref(),
        request.step_up_assertion.as_ref(),
        &action,
    )?;
    if asserted.is_some() != passkey {
        return Err(Refused::JournalUnavailable.into());
    }
    let assertion = asserted
        .map(|asserted| assertion_drafted(&admitted, &drafted, CONFIRM_ASSERTED, &asserted))
        .transpose()?;
    let row = commit(app, &admitted, drafted, &assertion.unwrap_or_default()).await?;
    Ok(confirm_accepted(StatusCode::ACCEPTED, &admitted, &row))
}

/// The confirmation committed under passkey step-up: `MandateConfirmed` version 3 ([DEC-2131]
/// item 3, journal spec v0.84 §9.9), version 2's members (a new mandate's, so no agent link and no
/// base), then `step_up`, the evidence the port judged, which the record also holds and §9.32's
/// rule 253 reads. `None` when the payload is not an object.
///
/// [DEC-2131]: ../../../docs/project/decisions/DEC-2131.md
fn passkey_confirmation(mut event: Event, step_up: Value) -> Option<Event> {
    let Value::Object(members) = &mut event.payload else {
        return None;
    };
    members.insert(Key::new("agent_id").ok()?, Value::Null);
    members.insert(Key::new("base_version").ok()?, Value::Null);
    members.insert(Key::new("step_up").ok()?, step_up);
    event.schema_version = 3;
    Some(event)
}

/// `commands/confirm-accepted` for a first version.
fn confirm_accepted(status: StatusCode, admitted: &Admitted, row: &StoredEvent) -> Response {
    served(
        status,
        admitted,
        row,
        vec![
            ("command_id", string(&row.event_id)),
            ("phase", string("recorded")),
            ("classification", string(FIRST_VERSION)),
            ("applies", string("next_safe_point")),
        ],
    )
}

/// The deploy body (DEC-2101 item 6), decoded strictly by the server until its schema and its
/// `mandate-api` type land together.
struct DeployBody {
    agent_id: String,
    mandate_version: String,
    backtest_id: Value,
    record: Value,
    step_up: Option<StepUpEvidence>,
    step_up_assertion: Option<StepUpAssertion>,
}

/// The members a deploy body names, in its order; the last, `step_up_assertion`, is optional
/// (DEC-2128 item 1).
const DEPLOY_MEMBERS: [&str; 6] = [
    "agent_id",
    "mandate_version",
    "backtest_id",
    "record",
    "step_up",
    "step_up_assertion",
];

/// `body` as a deploy: an object of [`DEPLOY_MEMBERS`], every one but `step_up_assertion`
/// present, each in its type and canonical form, else `invalid` at the first member that fails.
fn deploy_body(body: &[u8]) -> Result<DeployBody, Answer> {
    let Ok(Value::Object(members)) = parse(body) else {
        return Err(Answer::schema("", "type"));
    };
    if let Some(unknown) = members
        .keys()
        .find(|k| !DEPLOY_MEMBERS.contains(&k.as_str()))
    {
        return Err(Answer::schema(
            &format!("/{}", unknown.as_str()),
            "unknown_member",
        ));
    }
    let member = |name: &str| {
        members
            .get(name)
            .ok_or_else(|| Answer::schema(&format!("/{name}"), "missing"))
    };
    let non_canonical = |name: &str| Answer::schema(&format!("/{name}"), "non_canonical");
    let agent = member("agent_id")?.as_str().map(str::to_owned);
    let agent_id = agent
        .and_then(|a| Id::try_from(a).ok())
        .and_then(|id| id_text(&id))
        .ok_or_else(|| non_canonical("agent_id"))?;
    let version = member("mandate_version")?.as_str().map(str::to_owned);
    let mandate_version = version
        .and_then(|v| Ref::try_from(v).ok())
        .and_then(|r| ref_text(&r))
        .ok_or_else(|| non_canonical("mandate_version"))?;
    let backtest_id = match member("backtest_id")? {
        Value::Null => Value::Null,
        Value::Str(id) => Id::try_from(id.clone())
            .ok()
            .and_then(|id| id_text(&id))
            .map(Value::Str)
            .ok_or_else(|| non_canonical("backtest_id"))?,
        _ => return Err(non_canonical("backtest_id")),
    };
    let record = decode::<Record>(&to_canonical(member("record")?))
        .ok()
        .and_then(|record| record_value(&record))
        .ok_or_else(|| non_canonical("record"))?;
    let step_up = match member("step_up")? {
        Value::Null => None,
        other => Some(
            decode::<StepUpEvidence>(&to_canonical(other)).map_err(|_| non_canonical("step_up"))?,
        ),
    };
    let step_up_assertion = match members.get("step_up_assertion") {
        None | Some(Value::Null) => None,
        Some(other) => Some(
            decode::<StepUpAssertion>(&to_canonical(other))
                .map_err(|_| non_canonical("step_up_assertion"))?,
        ),
    };
    assertion_checked(step_up.as_ref(), step_up_assertion.as_ref()).map_err(|refused| {
        let mandate_api::wire::Refused::Invalid { violations } = refused;
        Answer::Invalid(violations)
    })?;
    Ok(DeployBody {
        agent_id,
        mandate_version,
        backtest_id,
        record,
        step_up,
        step_up_assertion,
    })
}

/// `POST /v1/workspaces/{ws}/agents` (§4.2, DEC-2101 item 6).
pub async fn deploy(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
    body: Body,
) -> Response {
    let Ok(Path(workspace)) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    answered(deployed(&app, &headers, &workspace, body).await)
}

async fn deployed(
    app: &App,
    headers: &HeaderMap,
    workspace: &str,
    body: Body,
) -> Result<Response, Answer> {
    let admitted = admitted(app, headers, workspace, Permission::Deploy, body).await?;
    let request = deploy_body(&admitted.body)?;
    let (agent, version) = (&request.agent_id, &request.mandate_version);
    let stream = format!("agent:{}:{agent}", admitted.reader.workspace);
    if StreamId::parse(&stream).is_none() {
        return Err(Answer::schema("/agent_id", "agent_invalid"));
    }
    let record = object(vec![
        ("agent_id", text(agent)),
        ("backtest_id", request.backtest_id.clone()),
        ("kind", text("deployment_record")),
        ("mandate_version", text(version)),
        ("record", request.record.clone()),
        (
            "step_up",
            journaled(request.step_up.as_ref()).ok_or(Refused::JournalUnavailable)?,
        ),
        ("user", text(&admitted.principal)),
    ])
    .ok_or(Refused::JournalUnavailable)?;
    let (document, mandate) = document(&admitted, version)?;
    let connection = mandate.connection_id.as_str();
    let named = established_v2(&admitted.reader.control, connection);
    let mut members = vec![
        ("agent_id", text(agent)),
        ("mandate_version", text(version)),
    ];
    if named {
        members.push(("connection_id", text(connection)));
    }
    let (record, mut event) = recorded(
        &record,
        version,
        DEPLOYED,
        members,
        vec![("mandate_version", text(version))],
    )
    .ok_or(Refused::JournalUnavailable)?;
    event.schema_version = if named { 2 } else { 1 };
    let passkey = request.step_up_assertion.is_some();
    if let (true, true, Some(evidence)) = (passkey, named, request.step_up.as_ref()) {
        let judged = journaled(Some(evidence)).ok_or(Refused::JournalUnavailable)?;
        event = passkey_deployment(event, judged).ok_or(Refused::JournalUnavailable)?;
    }
    let mut drafted = drafted(&admitted, "deploy", (record, event))?;
    drafted.pinned = named
        .then(|| admitted.reader.control.last().map(|row| row.seq))
        .flatten();
    if let Some(repeated) = repeat(&admitted, &drafted) {
        return Ok(command_accepted(
            StatusCode::ACCEPTED,
            &admitted,
            &repeated?,
        ));
    }
    match document.get("environment").and_then(Value::as_str) {
        Some("live") => {
            gated(app, &admitted.reader, &admitted.gate, Gated::Deploy).await?;
            return Err(Answer::Code(
                StatusCode::CONFLICT,
                ProblemCode::LiveUnavailable,
            ));
        }
        Some(environment) if environment == admitted.environment => {}
        _ => return Err(Answer::schema("/mandate_version", "environment")),
    }
    let control = &admitted.reader.control;
    let confirmed = latest(control, CONFIRMED, version)
        .ok_or_else(|| Answer::schema("/mandate_version", "version_unconfirmed"))?;
    if let Some((last, current)) = last_confirmed(control)
        && last.seq != confirmed.seq
    {
        return Err(Answer::Stale(current));
    }
    if let Some((row, deployed)) = active(control, agent) {
        if deployed == *version {
            return Ok(command_accepted(StatusCode::OK, &admitted, row));
        }
        return Err(Answer::schema("/agent_id", "agent_active"));
    }
    if stopped(control, agent) {
        return Err(Answer::schema("/agent_id", "agent_stopped"));
    }
    if mandate.universe.max_instruments > 1 {
        return Err(Answer::schema("/mandate_version", "second_instrument"));
    }
    if another_on_account(control, mandate.connection_id.as_str(), agent) {
        return Err(Answer::schema("/agent_id", "second_agent_on_account"));
    }
    let objects = admitted.objects.as_ref();
    let date = admitted.gate.now.date();
    let account = match account_ref(control, mandate.connection_id.as_str()) {
        Some(reference) => {
            let stream = format!("acct:{}:{reference}", admitted.reader.workspace);
            app.journal
                .rows(&stream)
                .await
                .map_err(|_| Refused::JournalUnavailable)?
        }
        None => Vec::new(),
    };
    check(
        (control, &account),
        objects,
        (&mandate, agent),
        Pending::default(),
        date,
    )
    .map_err(Answer::of)?;
    instruments(control, objects, &document).map_err(Answer::of)?;
    let digest = deploy_digest(agent, version).ok_or(Refused::JournalUnavailable)?;
    let action = Action {
        kind: StepUpActionKind::Deploy,
        digest,
    };
    if passkey && !named {
        return Err(Answer::schema("/mandate_version", "connection_unrecorded"));
    }
    let asserted = stepped_up(
        app,
        &admitted,
        request.step_up.as_ref(),
        request.step_up_assertion.as_ref(),
        &action,
    )?;
    if asserted.is_some() != passkey {
        return Err(Refused::JournalUnavailable.into());
    }
    let assertion = asserted
        .map(|asserted| assertion_drafted(&admitted, &drafted, DEPLOY_ASSERTED, &asserted))
        .transpose()?;
    let row = commit(app, &admitted, drafted, &assertion.unwrap_or_default()).await?;
    Ok(command_accepted(StatusCode::ACCEPTED, &admitted, &row))
}

/// The deployment committed under passkey step-up on a connection §9.24's rule 201 admits:
/// `AgentDeployed` version 3 ([DEC-2131] item 3, journal spec v0.84 §9.24), version 2's members
/// then `step_up`, the evidence the port judged, which the record also holds and §9.32's rule 253
/// reads. `None` when the payload is not an object. A passkey deployment rule 201 refuses has no
/// version to carry its evidence and is refused before step-up is judged ([DEC-2133]).
///
/// [DEC-2131]: ../../../docs/project/decisions/DEC-2131.md
/// [DEC-2133]: ../../../docs/project/decisions/DEC-2133.md
fn passkey_deployment(mut event: Event, step_up: Value) -> Option<Event> {
    let Value::Object(members) = &mut event.payload else {
        return None;
    };
    members.insert(Key::new("step_up").ok()?, step_up);
    event.schema_version = 3;
    Some(event)
}

/// `commands/command-accepted`.
fn command_accepted(status: StatusCode, admitted: &Admitted, row: &StoredEvent) -> Response {
    served(
        status,
        admitted,
        row,
        vec![
            ("command_id", string(&row.event_id)),
            ("phase", string("recorded")),
        ],
    )
}

/// `POST /v1/workspaces/{ws}/agents/{id}/go-live` (§4.2, DEC-2101 item 7): `live_unavailable`
/// once §3.3's checks and the deploy row pass, before the body is read.
pub async fn go_live(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
) -> Response {
    let Ok(Path((workspace, _agent))) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    answered(refused_live(&app, &headers, &workspace).await)
}

async fn refused_live(app: &App, headers: &HeaderMap, workspace: &str) -> Result<Response, Answer> {
    let (admitted, now) = session(app, headers, Request::Other)?;
    if !browser(headers, &app.settings.app_origin) {
        return Err(Refused::Forbidden.into());
    }
    let key = one_key(headers).ok_or(Refused::Invalid)?;
    let reader = authorized(
        app,
        &admitted,
        now,
        workspace,
        Permission::Deploy,
        Unreadable::Command,
    )
    .await?;
    let gate = Gate { admitted, now, key };
    gated(app, &reader, &gate, Gated::GoLive).await?;
    Err(Answer::Code(
        StatusCode::CONFLICT,
        ProblemCode::LiveUnavailable,
    ))
}
