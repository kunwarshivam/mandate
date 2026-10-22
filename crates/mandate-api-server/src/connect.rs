//! The connect start (§4.5, [DEC-2623](../../../docs/project/decisions/DEC-2623.md)):
//! `POST /v1/workspaces/{ws}/connections/oauth/start` commits the pending connection's
//! `ConnectionRequested` version 2 with the step-up it spent, then issues the single-use `state`
//! the callback redeems. It reaches no broker (§1.4): the browser goes to the authorization page.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use mandate_api::envelope::StepUpEvidence;
use mandate_api::problem::{ProblemCode, Violation};
use mandate_api::requests::{Environment as Wire, OauthStartRequest};
use mandate_api::wire::decode;
use mandate_authn::Request;
use mandate_canon::{Digest, Value, parse};
use mandate_connections::manager::{
    AccountRefFold, IdFold, StartFacts, StartPlan, StartRefusal, StartRequest, StepUpCheck,
    oauth_connect_digest, plan_start,
};
use mandate_connections::record::{AccountRef, Broker, ConnectionId, Environment};
use mandate_connections::start::{
    Binding, Environment as Bound, OAuthClient, PendingStates, PkceVerifier, STATE_LIFETIME_SECS,
};
use mandate_identity::{Permission, StepUpActionKind};
use mandate_journal::StoredEvent;
use mandate_time::UtcNanos;
use secrecy::SecretString;

use crate::backtests::paper;
use crate::commands::{Event, Settled, Writer, envelope};
use crate::http::{App, invalid, problem, problem_body};
use crate::routes::{Reader, Refused, Unreadable, authorized, enveloped, path_refused, string};
use crate::session::Random;
use crate::stepup::{Action, Judged, journaled};
use crate::versions::object;
use crate::writes::{
    Gate, appended_at, derived_for, id_text, principal_text, repeated, strictly, unknown,
};

/// The token-breadth disclosure's document (DEC-2621 item 5).
pub const DISCLOSURE_DOCUMENT: &str = "alpaca_oauth_token_breadth";

/// The disclosure's current version; a start naming another is `invalid` at `/disclosure`
/// (DEC-2623 item 6).
pub const DISCLOSURE_VERSION: &str = "1";

/// The §3.4 operation the start's `ConnectionRequested` event id is derived under.
pub const OPERATION: &str = "connect";

/// The operation the pending `connection_id` is derived under: `conn_` and that ULID (DEC-2623
/// item 5).
pub const CONNECTION_OPERATION: &str = "connect_connection";

/// The operation the pending connection's `account_ref` ULID is derived under.
pub const ACCOUNT_OPERATION: &str = "connect_account";

/// How many bytes the `state` and the PKCE verifier each draw (DEC-2623 item 7).
pub const RANDOM_BYTES: usize = 32;

/// What a deployment that offers a connect holds: its registered OAuth client, the `state`
/// values issued and not yet redeemed, and the random source they are drawn from (DEC-2623 items
/// 2, 4 and 7).
pub struct Connect {
    pub client: OAuthClient,
    pub states: Mutex<PendingStates>,
    pub random: Box<dyn Random>,
    /// Each start's `201`, by its event id, for a retry while its `state` is held (item 8).
    answered: Mutex<BTreeMap<String, Answered>>,
}

/// A start's answer: the URL, the `state` in it, when it expires, and the connection.
#[derive(Clone)]
struct Answered {
    url: String,
    state: String,
    expires_at: UtcNanos,
    connection_id: String,
}

impl Connect {
    pub fn new(client: OAuthClient, random: Box<dyn Random>) -> Self {
        Self {
            client,
            states: Mutex::new(PendingStates::default()),
            random,
            answered: Mutex::default(),
        }
    }
}

/// `POST /v1/workspaces/{ws}/connections/oauth/start` (DEC-2623 items 1 and 5 to 9).
pub async fn start(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
    body: Body,
) -> Response {
    let Ok(Path(workspace)) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    started(&app, &headers, &workspace, body)
        .await
        .unwrap_or_else(|refusal| *refusal)
}

async fn started(
    app: &App,
    headers: &HeaderMap,
    workspace: &str,
    body: Body,
) -> Result<Response, Box<Response>> {
    let (gate, request) = strictly(app, headers, body, decode::<OauthStartRequest>).await?;
    let connect = app
        .connect
        .clone()
        .ok_or_else(|| Box::new(Refused::NotFound.response()))?;
    let reader = authorized(
        app,
        &gate.admitted,
        gate.now,
        workspace,
        Permission::BrokerConnection,
        Unreadable::Command,
    )
    .await
    .map_err(|refused| Box::new(refused.response()))?;
    if !paper(&reader.control) {
        return Err(coded(StatusCode::CONFLICT, ProblemCode::LiveUnavailable));
    }
    let disclosure = &request.disclosure;
    if id_text(&disclosure.document).as_deref() != Some(DISCLOSURE_DOCUMENT)
        || disclosure.version != DISCLOSURE_VERSION
    {
        return Err(Box::new(invalid(vec![Violation::Schema {
            path: "/disclosure".to_owned(),
            code: "disclosure_version".to_owned(),
            message: "The request does not meet this rule.".to_owned(),
        }])));
    }
    let evidence = request
        .step_up
        .as_ref()
        .ok_or_else(|| coded(StatusCode::UNAUTHORIZED, ProblemCode::StepUpRequired))?;
    let ids = Ids::of(&gate, &reader.workspace).ok_or_else(unavailable)?;
    let draft = drafted(&gate, &reader, &ids, &request, evidence).ok_or_else(unavailable)?;
    if let Some(settled) = repeated(&reader.control, &ids.event, &draft) {
        return replayed(&connect, &ids, settled, gate.now);
    }
    let action = Action {
        kind: StepUpActionKind::Connection,
        digest: oauth_connect_digest(&reader.workspace, DISCLOSURE_DOCUMENT, DISCLOSURE_VERSION)
            .map_err(|_| unavailable())?,
    };
    let judged = Judged {
        tenant: &reader.tenant,
        control: &reader.control,
        now: gate.now,
    };
    app.step_up
        .verify(evidence, &action, &judged)
        .map_err(|code| coded(StatusCode::UNAUTHORIZED, code))?;
    planned(&reader, &ids, &request, evidence)?;
    let judged_head = reader.control.last().map(|row| row.seq);
    match appended_at(app, &reader, (&ids.event, &draft), gate.now, judged_head).await {
        Settled::Stored(_) => {}
        Settled::Conflict => {
            return Err(coded(
                StatusCode::CONFLICT,
                ProblemCode::IdempotencyConflict,
            ));
        }
        Settled::Unknown => return Err(Box::new(unknown(ids.event_id))),
        Settled::Unavailable => return Err(unavailable()),
    }
    let answered = issued(&connect, &reader, &ids, &gate).ok_or_else(unavailable)?;
    Ok(created(&answered, gate.now))
}

/// The ids derived from the key: the event's, the connection's, and the account's.
struct Ids {
    event_id: mandate_api::wire::EventId,
    event: String,
    connection: String,
    account: String,
    principal: String,
}

impl Ids {
    fn of(gate: &Gate, workspace: &str) -> Option<Self> {
        let (event_id, event) = derived_for(gate, workspace, OPERATION)?;
        let (_, connection) = derived_for(gate, workspace, CONNECTION_OPERATION)?;
        let (_, account) = derived_for(gate, workspace, ACCOUNT_OPERATION)?;
        Some(Self {
            event_id,
            event,
            connection: format!("conn_{connection}"),
            account,
            principal: principal_text(gate)?,
        })
    }
}

fn coded(status: StatusCode, code: ProblemCode) -> Box<Response> {
    Box::new(problem(status, problem_body(code)))
}

fn unavailable() -> Box<Response> {
    Box::new(Refused::JournalUnavailable.response())
}

/// The `ConnectionRequested` version 2 this start appends (DEC-2621 item 1).
fn drafted(
    gate: &Gate,
    reader: &Reader,
    ids: &Ids,
    request: &OauthStartRequest,
    evidence: &StepUpEvidence,
) -> Option<Vec<u8>> {
    let environment = match request.environment {
        Wire::Paper => "paper",
        Wire::Live => "live",
    };
    let breadth = object(vec![
        ("other_environment", string("may_reach")),
        (
            "disclosure",
            object(vec![
                ("document", string(DISCLOSURE_DOCUMENT)),
                ("version", string(DISCLOSURE_VERSION)),
            ])?,
        ),
    ])?;
    let payload = object(vec![
        ("connection_id", string(&ids.connection)),
        ("account_ref", string(&ids.account)),
        ("broker", string("alpaca")),
        ("environment", string(environment)),
        ("user", string(&ids.principal)),
        ("step_up", journaled(Some(evidence))?),
        ("credential", string("oauth")),
        ("breadth", breadth),
    ])?;
    let event = Event {
        event_type: "ConnectionRequested",
        schema_version: 2,
        payload,
        artifact_refs: Vec::new(),
        config_refs: Vec::new(),
    };
    let writer = Writer {
        workspace: &reader.workspace,
        principal: &ids.principal,
        event_id: &ids.event,
        environment: "paper",
        now: gate.now,
    };
    envelope(&writer, &event)
}

/// A retry under the key: the same `201` while the `state` is held, else the key is spent.
fn replayed(
    connect: &Connect,
    ids: &Ids,
    settled: Settled,
    now: UtcNanos,
) -> Result<Response, Box<Response>> {
    let spent = || coded(StatusCode::CONFLICT, ProblemCode::IdempotencyConflict);
    let Settled::Stored(_) = settled else {
        return Err(spent());
    };
    let held = connect
        .answered
        .lock()
        .ok()
        .and_then(|answered| answered.get(&ids.event).cloned())
        .ok_or_else(spent)?;
    let still = connect
        .states
        .lock()
        .ok()
        .is_some_and(|states| states.holds(&held.state, now));
    if still {
        Ok(created(&held, now))
    } else {
        Err(spent())
    }
}

/// `plan_start`'s refusals, after step-up (DEC-883, DEC-2623 item 6).
fn planned(
    reader: &Reader,
    ids: &Ids,
    request: &OauthStartRequest,
    evidence: &StepUpEvidence,
) -> Result<(), Box<Response>> {
    let spent = || coded(StatusCode::CONFLICT, ProblemCode::IdempotencyConflict);
    let connection_id = ConnectionId::new(&ids.connection).map_err(|_| unavailable())?;
    let account_ref = AccountRef::new(&ids.account).map_err(|_| unavailable())?;
    let presented = journaled(Some(evidence)).ok_or_else(unavailable)?;
    let member = |name: &str| {
        presented
            .get(name)
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let (assertion_id, method) = member("assertion_id")
        .zip(member("method"))
        .ok_or_else(unavailable)?;
    let authenticated_at = member("authenticated_at")
        .and_then(|at| UtcNanos::parse_rfc3339(&at).ok())
        .ok_or_else(unavailable)?;
    let start = StartRequest {
        connection_id,
        account_ref,
        broker: Broker::Alpaca,
        environment: match request.environment {
            Wire::Paper => Environment::Paper,
            Wire::Live => Environment::Live,
        },
        user: ids.principal.clone(),
        step_up: StepUpCheck::Verified(mandate_connections::manager::StepUpEvidence {
            assertion_id,
            authenticated_at,
            method,
        }),
    };
    let facts = facts(&reader.control, &ids.connection, &ids.account);
    match plan_start(&start, &facts).map_err(|_| unavailable())? {
        StartPlan::Commit(_) => Ok(()),
        StartPlan::Refused(StartRefusal::EnvironmentRefused) => {
            Err(coded(StatusCode::CONFLICT, ProblemCode::LiveUnavailable))
        }
        StartPlan::Refused(StartRefusal::FoldCannotAnswer) => Err(unavailable()),
        StartPlan::Refused(StartRefusal::BrokerRefused | StartRefusal::StepUpNotVerified) => {
            Err(unavailable())
        }
        StartPlan::Refused(
            StartRefusal::RequestOpen
            | StartRefusal::AlreadyEstablished
            | StartRefusal::AccountRefUsed,
        ) => Err(spent()),
    }
}

/// What the control stream holds of the derived connection and account (rule 131, opening).
/// A connection row without the members journal spec §9 gives it (every one a `connection_id`,
/// a request its `account_ref`) leaves both unanswerable: the start is refused, never folded past.
fn facts(control: &[StoredEvent], connection: &str, account: &str) -> StartFacts {
    const UNANSWERABLE: StartFacts = StartFacts {
        id: IdFold::CannotAnswer,
        account_ref: AccountRefFold::CannotAnswer,
    };
    let mut id = IdFold::Free;
    let mut account_ref = AccountRefFold::Unused;
    for row in control {
        let kind = row.event_type.as_str();
        if !matches!(
            kind,
            "ConnectionRequested"
                | "ConnectionEstablished"
                | "ConnectionRefused"
                | "ConnectionRevoked"
        ) {
            continue;
        }
        let event = parse(&row.body).ok();
        let member = |name: &str| {
            event
                .as_ref()
                .and_then(|e| e.get("payload"))
                .and_then(|p| p.get(name))
                .and_then(Value::as_str)
        };
        let Some(named) = member("connection_id") else {
            return UNANSWERABLE;
        };
        let requested_account = member("account_ref");
        if kind == "ConnectionRequested" && requested_account.is_none() {
            return UNANSWERABLE;
        }
        if matches!(kind, "ConnectionRequested" | "ConnectionEstablished")
            && requested_account == Some(account)
        {
            account_ref = AccountRefFold::Used;
        }
        if named != connection {
            continue;
        }
        id = match (kind, id) {
            ("ConnectionRequested", IdFold::Free) => IdFold::RequestOpen,
            ("ConnectionEstablished", _) => IdFold::Established,
            ("ConnectionRefused" | "ConnectionRevoked", IdFold::RequestOpen) => IdFold::Free,
            (_, unchanged) => unchanged,
        };
    }
    StartFacts { id, account_ref }
}

/// Draws the `state` and verifier, issues the `state`, and keeps the answer (items 4 and 7). The
/// answers are held locked from the check to the insert, so a request whose record another start
/// under its key committed first gets that start's answer, never a second `state` (item 8).
fn issued(connect: &Connect, reader: &Reader, ids: &Ids, gate: &Gate) -> Option<Answered> {
    let mut answered = connect.answered.lock().ok()?;
    if let Some(earlier) = answered.get(&ids.event) {
        return Some(earlier.clone());
    }
    let mut drawn = [[0_u8; RANDOM_BYTES]; 2];
    for draw in &mut drawn {
        connect.random.fill(draw).ok()?;
    }
    let [state, verifier] = drawn.map(|draw| URL_SAFE_NO_PAD.encode(draw));
    let challenge = URL_SAFE_NO_PAD.encode(Digest::of(verifier.as_bytes()).as_bytes());
    let binding = Binding {
        workspace_id: reader.workspace.clone(),
        user_id: ids.principal.clone(),
        connection_id: ConnectionId::new(&ids.connection).ok()?,
        environment: Bound::Paper,
    };
    let verifier = PkceVerifier(SecretString::from(verifier));
    let now = gate.now;
    let url = {
        let mut states = connect.states.lock().ok()?;
        states.prune(now);
        answered.retain(|_, kept| states.holds(&kept.state, now));
        states
            .begin(&connect.client, binding, &state, verifier, &challenge, now)
            .ok()?
    };
    let expires_at =
        UtcNanos::from_parts(now.secs().checked_add(STATE_LIFETIME_SECS)?, now.nanos()).ok()?;
    let issued = Answered {
        url: url.0,
        state,
        expires_at,
        connection_id: ids.connection.clone(),
    };
    answered.insert(ids.event.clone(), issued.clone());
    Some(issued)
}

/// The `201` (DEC-2623 item 1); it reads no stream into its answer, so `as_of` is empty.
fn created(answered: &Answered, now: UtcNanos) -> Response {
    enveloped(
        StatusCode::CREATED,
        now,
        vec![
            ("authorization_url", string(&answered.url)),
            ("expires_at", string(&answered.expires_at.to_string())),
            ("connection_id", string(&answered.connection_id)),
            ("as_of", Value::Array(Vec::new())),
        ],
    )
}
