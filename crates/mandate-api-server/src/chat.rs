//! The agent's chat over the workspace API ([DEC-2293](../../../docs/project/decisions/DEC-2293.md),
//! workspace API §4.6.2):
//! - `POST /v1/workspaces/{ws}/agents/{agent}/messages` journals the owner's message and starts a
//!   run;
//! - `GET` the same path reads the owner's own thread back from the journal;
//! - `POST …/runs/{run}/stop` is the owner's Stop;
//! - `GET …/goals` and `GET`/`PUT …/settings` serve the agent's goals and chat settings;
//! - `POST /v1/workspaces/{ws}/setup/messages` and `GET …/setup/{setup}/messages` are the
//!   conversation that sets up a new agent;
//! - `GET /v1/workspaces/{ws}/models` lists the chat models;
//! - `GET /v1/workspaces/{ws}/home` serves Home.
//!
//! Each run is [`crate::chat_run`]'s, off every trading path: it acts only through the commands the
//! web app's own routes commit.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use axum::body::Body;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use mandate_api::problem::Violation;
use mandate_api::wire::Refused as Undecoded;
use mandate_authn::Request;
use mandate_canon::{Value, encode_ulid, parse};
use mandate_chat::ChatEntry;
use mandate_identity::Permission;
use mandate_journal::StoredEvent;
use mandate_time::UtcNanos;

use crate::chat_run::{self, Start, Thread};
use crate::commands::{Event, Settled, Writer, envelope_by, settle_batch_at};
use crate::http::{App, invalid};
use crate::reads::{Deployment, deployed, envelope, mode};
use crate::routes::{
    Reader, Refused, Unreadable, authorized, enveloped, int, object, path_refused, served, session,
    string,
};
use crate::writes::{Gate, derived_for, principal_text, strictly};

/// The longest message an owner may send, in characters.
pub const MAX_MESSAGE_CHARS: usize = 4_000;

/// The daily model spending cap of an agent with no settings of its own (DEC-2293 item 6), in USD.
pub const DEFAULT_DAILY_CAP_USD: &str = "5";

/// The actor id of the chat's own records, which the system writes.
pub const CHAT_ACTOR: &str = "chat";

/// What the deployment's chat is configured with, and the runs it holds.
#[derive(Default)]
pub struct Chat {
    /// The model gateway service's `host:port`, or `None` when the deployment names none; every
    /// run then ends at once with a message that the chat is unavailable.
    pub gateway: Option<String>,
    /// The chat entries the gateway offers, from the registry file both read; the first is the
    /// default.
    pub models: Vec<ChatModel>,
    /// The runs the owner asked to stop.
    stops: Mutex<BTreeSet<String>>,
    /// The run in flight per thread, so a thread runs one at a time.
    running: Mutex<BTreeMap<String, String>>,
    ids: AtomicU64,
}

/// One chat model the owner may choose, with its price per million tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatModel {
    pub entry: ChatEntry,
    pub label: String,
    pub input_per_million: String,
    pub output_per_million: String,
}

impl Chat {
    pub fn new(gateway: Option<String>, models: Vec<ChatModel>) -> Self {
        Self {
            gateway,
            models,
            ..Self::default()
        }
    }

    /// A fresh ULID at `now`: its millisecond, then this process's counter.
    pub fn ulid(&self, now: UtcNanos) -> String {
        let millis = u128::try_from(now.secs())
            .unwrap_or_default()
            .saturating_mul(1_000)
            .saturating_add(u128::from(now.nanos() / 1_000_000));
        let count = u128::from(self.ids.fetch_add(1, Ordering::Relaxed));
        encode_ulid((millis << 80).saturating_add(count))
    }

    pub fn stop(&self, run: &str) {
        self.stops
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(run.to_owned());
    }

    pub fn stopped(&self, run: &str) -> bool {
        self.stops
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(run)
    }

    /// Claims `thread` for `run`; `false` when another run holds it.
    pub fn claim(&self, thread: &str, run: &str) -> bool {
        let mut running = self.running.lock().unwrap_or_else(PoisonError::into_inner);
        if running.contains_key(thread) {
            return false;
        }
        running.insert(thread.to_owned(), run.to_owned());
        true
    }

    pub fn release(&self, thread: &str) {
        self.running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(thread);
    }

    /// The run `thread` holds, if any.
    pub fn running(&self, thread: &str) -> Option<String> {
        self.running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(thread)
            .cloned()
    }

    /// The model `id` names, or the default.
    pub fn model(&self, id: Option<&str>) -> Option<&ChatModel> {
        id.and_then(|id| self.models.iter().find(|m| m.entry.model_id == id))
            .or_else(|| self.models.first())
    }
}

/// The chat's settings from the process's: the gateway service's `host:port`, when set and not
/// empty, and the models of the registry file's bytes, none when there are none or they do not read.
pub fn chat_from(gateway: Option<String>, registry: Option<&[u8]>) -> Chat {
    let gateway = gateway.filter(|address| !address.is_empty());
    let models = registry
        .and_then(|bytes| parse(bytes).ok())
        .map(|file| models_from_registry(&file))
        .unwrap_or_default();
    Chat::new(gateway, models)
}

/// The models a registry file's `chat` items name, in its order, each labelled by its model name
/// and priced by its endpoint; an item that does not read is left out.
pub fn models_from_registry(file: &Value) -> Vec<ChatModel> {
    file.get("chat")
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(|item| {
            let entry = ChatEntry::from_registry(item.get("entry")?)?;
            let price = |name: &str| {
                item.get("endpoint")
                    .and_then(|e| e.get(name))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            };
            let label = entry
                .identity
                .model_name
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_owned();
            Some(ChatModel {
                label,
                input_per_million: price("input_per_million")?,
                output_per_million: price("output_per_million")?,
                entry,
            })
        })
        .collect()
}

/// The owner's message: `{"text": …}`, at most [`MAX_MESSAGE_CHARS`] characters, not blank.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub text: String,
}

fn decode_message(body: &[u8]) -> Result<Message, Undecoded> {
    let value = parse(body).map_err(|_| refused("", "malformed"))?;
    let members = value.as_object().ok_or_else(|| refused("", "object"))?;
    if members.keys().any(|k| k.as_str() != "text") {
        return Err(refused("", "unknown_member"));
    }
    let text = value
        .get("text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| refused("/text", "required"))?;
    if text.chars().count() > MAX_MESSAGE_CHARS {
        return Err(refused("/text", "too_long"));
    }
    Ok(Message {
        text: text.to_owned(),
    })
}

/// The settings body: `{"chat_model": …, "daily_cap_usd": …}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub chat_model: String,
    pub daily_cap_usd: String,
}

fn decode_settings(body: &[u8]) -> Result<Settings, Undecoded> {
    let value = parse(body).map_err(|_| refused("", "malformed"))?;
    let members = value.as_object().ok_or_else(|| refused("", "object"))?;
    if members
        .keys()
        .any(|k| !matches!(k.as_str(), "chat_model" | "daily_cap_usd"))
    {
        return Err(refused("", "unknown_member"));
    }
    let text = |name: &str| {
        value
            .get(name)
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
    };
    let chat_model = text("chat_model").ok_or_else(|| refused("/chat_model", "required"))?;
    let daily_cap_usd = text("daily_cap_usd")
        .filter(|cap| positive_decimal(cap))
        .ok_or_else(|| refused("/daily_cap_usd", "positive"))?;
    Ok(Settings {
        chat_model,
        daily_cap_usd,
    })
}

/// Whether `text` is a plain positive decimal: digits, at most one point, not zero.
fn positive_decimal(text: &str) -> bool {
    let mut parts = text.split('.');
    let whole = parts.next().unwrap_or_default();
    let fraction = parts.next();
    parts.next().is_none()
        && !whole.is_empty()
        && whole.bytes().all(|b| b.is_ascii_digit())
        && fraction.is_none_or(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()))
        && text.bytes().any(|b| (b'1'..=b'9').contains(&b))
}

fn refused(path: &str, code: &str) -> Undecoded {
    Undecoded::Invalid {
        violations: vec![Violation::Schema {
            path: path.to_owned(),
            code: code.to_owned(),
            message: "The request does not meet this rule.".to_owned(),
        }],
    }
}

/// `POST /v1/workspaces/{ws}/agents/{agent}/messages`.
pub async fn post_message(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
    body: Body,
) -> Response {
    let Ok(Path((workspace, agent))) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let (gate, message) = match strictly(&app, &headers, body, decode_message).await {
        Ok(admitted) => admitted,
        Err(refusal) => return *refusal,
    };
    let reader = match chat_reader(&app, &gate, &workspace).await {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    let Some(deployment) = agent_of(&reader.control, &agent) else {
        return Refused::NotFound.response();
    };
    let thread = Thread::agent(&deployment.0, &member(&gate));
    posted(app, gate, reader, thread, message.text, Some(deployment)).await
}

/// `POST /v1/workspaces/{ws}/setup/messages`, with `setup_id` in the query for a conversation
/// already started: `?setup_id=…`.
pub async fn post_setup(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
    RawQuery(query): RawQuery,
    body: Body,
) -> Response {
    let Ok(Path(workspace)) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let (gate, message) = match strictly(&app, &headers, body, decode_message).await {
        Ok(admitted) => admitted,
        Err(refusal) => return *refusal,
    };
    let reader = match chat_reader(&app, &gate, &workspace).await {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    let setup = match query.as_deref().and_then(|q| q.strip_prefix("setup_id=")) {
        Some(id) if ident(id) => id.to_owned(),
        Some(_) => return Refused::Invalid.response(),
        None => format!("setup_{}", app.chat.ulid(gate.now).to_ascii_lowercase()),
    };
    let thread = Thread::setup(&setup, &member(&gate));
    posted(app, gate, reader, thread, message.text, None).await
}

/// Journals the owner's message, then starts its run in the background, and answers with both
/// ids; a thread whose run is still in flight is refused, with nothing journaled.
async fn posted(
    app: Arc<App>,
    gate: Gate,
    reader: Reader,
    thread: Thread,
    text: String,
    deployment: Option<Deployment>,
) -> Response {
    let run = format!("run_{}", app.chat.ulid(gate.now).to_ascii_lowercase());
    if !app.chat.claim(&thread.key(), &run) {
        return problem_conflict();
    }
    let payload = thread.message_payload("owner", Some(&text), None, &[], None, false);
    let stored = match append_owner(&app, &gate, &reader, "ChatMessagePosted", payload).await {
        Ok(stored) => stored,
        Err(refusal) => {
            app.chat.release(&thread.key());
            return refusal.response();
        }
    };
    chat_run::spawn(
        Arc::clone(&app),
        Start {
            workspace: reader.workspace.clone(),
            thread: thread.clone(),
            run: run.clone(),
            now: gate.now,
            gate: Some(gate),
            deployment,
            cause: stored.event_id.clone(),
            message: text,
            proactive: false,
        },
    );
    let mut members = thread.id_members();
    members.extend([
        ("message_id", string(&stored.event_id)),
        ("run_id", string(&run)),
    ]);
    enveloped(StatusCode::ACCEPTED, reader.now, members)
}

/// `GET /v1/workspaces/{ws}/agents/{agent}/messages?after={seq}`.
pub async fn get_messages(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
    RawQuery(query): RawQuery,
) -> Response {
    let Ok(Path((workspace, agent))) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let reader = match read_chat(&app, &headers, &workspace).await {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    if agent_of(&reader.control, &agent).is_none() {
        return Refused::NotFound.response();
    }
    let thread = Thread::agent(&agent, &principal(&reader));
    thread_response(&app, &reader, &thread, query.as_deref())
}

/// `GET /v1/workspaces/{ws}/setup/{setup}/messages?after={seq}`.
pub async fn get_setup(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
    RawQuery(query): RawQuery,
) -> Response {
    let Ok(Path((workspace, setup))) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let reader = match read_chat(&app, &headers, &workspace).await {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    let thread = Thread::setup(&setup, &principal(&reader));
    thread_response(&app, &reader, &thread, query.as_deref())
}

fn thread_response(app: &App, reader: &Reader, thread: &Thread, query: Option<&str>) -> Response {
    let after = match query {
        None => 0,
        Some(q) => match q.strip_prefix("after=").and_then(|n| n.parse::<u64>().ok()) {
            Some(n) => n,
            None => return Refused::Invalid.response(),
        },
    };
    let messages: Vec<Value> = reader
        .control
        .iter()
        .filter(|row| row.seq > after && row.event_type == "ChatMessagePosted")
        .filter_map(|row| Some((row, envelope(row)?)))
        .filter(|(_, e)| thread.holds(e.get("payload")) && !marker(e))
        .map(|(row, e)| message_value(row, &e))
        .collect();
    let cursor = reader
        .control
        .last()
        .map_or(after, |row| row.seq.max(after));
    let running = app.chat.running(&thread.key());
    let mut members = thread.id_members();
    members.extend([
        ("messages", Value::Array(messages)),
        ("running", Value::Bool(running.is_some())),
        ("run_id", running.as_deref().map_or(Value::Null, string)),
        ("cursor", int(cursor)),
    ]);
    served(reader, &[], members)
}

/// Whether a message is the daily note's marker, which says nothing (`chat_watch`).
fn marker(envelope: &Value) -> bool {
    let payload = envelope.get("payload");
    let kind = payload
        .and_then(|p| p.get("card"))
        .and_then(|c| c.get("kind"))
        .and_then(Value::as_str);
    kind == Some("brief") && payload.and_then(|p| p.get("text")) == Some(&Value::Null)
}

fn message_value(row: &StoredEvent, envelope: &Value) -> Value {
    let p = |name: &str| {
        envelope
            .get("payload")
            .and_then(|p| p.get(name))
            .cloned()
            .unwrap_or(Value::Null)
    };
    let from = match p("author").as_str() {
        Some("owner") => "owner",
        _ => "agent",
    };
    object(vec![
        ("id", string(&row.event_id)),
        ("seq", int(row.seq)),
        ("at", string(&row.recorded_at)),
        ("from", string(from)),
        ("text", p("text")),
        ("card", p("card")),
        ("sources", p("sources")),
        ("proactive", p("proactive")),
    ])
}

/// `POST /v1/workspaces/{ws}/agents/{agent}/runs/{run}/stop`.
pub async fn stop_run(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String, String)>, PathRejection>,
    body: Body,
) -> Response {
    let Ok(Path((workspace, agent, run))) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let (gate, ()) = match strictly(&app, &headers, body, |_| Ok(())).await {
        Ok(admitted) => admitted,
        Err(refusal) => return *refusal,
    };
    let reader = match chat_reader(&app, &gate, &workspace).await {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    let thread = Thread::agent(&agent, &member(&gate));
    if app.chat.running(&thread.key()).as_deref() != Some(run.as_str()) {
        return Refused::NotFound.response();
    }
    app.chat.stop(&run);
    let mut payload = thread.id_payload();
    payload.push(("run_id", string(&run)));
    match append_owner(&app, &gate, &reader, "ChatRunStopRequested", payload).await {
        Ok(_) => enveloped(
            StatusCode::ACCEPTED,
            reader.now,
            vec![("run_id", string(&run))],
        ),
        Err(refusal) => refusal.response(),
    }
}

/// `GET /v1/workspaces/{ws}/agents/{agent}/goals`.
pub async fn goals(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
) -> Response {
    let Ok(Path((workspace, agent))) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let reader = match read_chat(&app, &headers, &workspace).await {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    if agent_of(&reader.control, &agent).is_none() {
        return Refused::NotFound.response();
    }
    let latest = latest_goals(&reader.control, &agent);
    let member = |name: &str, empty: Value| {
        latest
            .as_ref()
            .and_then(|p| p.get(name).cloned())
            .unwrap_or(empty)
    };
    served(
        &reader,
        &[],
        vec![
            ("agent_id", string(&agent)),
            ("goals", member("goals", Value::Array(Vec::new()))),
            ("plan", member("plan", Value::Null)),
            ("watching", member("watching", Value::Array(Vec::new()))),
        ],
    )
}

/// The payload of the latest `AgentGoalsUpdated` for `agent`.
pub fn latest_goals(control: &[StoredEvent], agent: &str) -> Option<Value> {
    control
        .iter()
        .rev()
        .filter(|row| row.event_type == "AgentGoalsUpdated")
        .filter_map(envelope)
        .filter_map(|e| e.get("payload").cloned())
        .find(|p| p.get("agent_id").and_then(Value::as_str) == Some(agent))
}

/// `GET /v1/workspaces/{ws}/models`.
pub async fn models(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
) -> Response {
    let Ok(Path(workspace)) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let reader = match read_chat(&app, &headers, &workspace).await {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    let listed = app
        .chat
        .models
        .iter()
        .map(|m| {
            object(vec![
                ("model_id", string(&m.entry.model_id)),
                ("label", string(&m.label)),
                ("input_per_million_usd", string(&m.input_per_million)),
                ("output_per_million_usd", string(&m.output_per_million)),
            ])
        })
        .collect();
    served(&reader, &[], vec![("models", Value::Array(listed))])
}

/// `GET /v1/workspaces/{ws}/agents/{agent}/settings`.
pub async fn get_settings(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
) -> Response {
    let Ok(Path((workspace, agent))) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let reader = match read_chat(&app, &headers, &workspace).await {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    if agent_of(&reader.control, &agent).is_none() {
        return Refused::NotFound.response();
    }
    let (model, cap) = settings_of(&app, &reader.control, &agent);
    served(
        &reader,
        &[],
        vec![
            ("agent_id", string(&agent)),
            ("chat_model", model.as_deref().map_or(Value::Null, string)),
            ("daily_cap_usd", string(&cap)),
        ],
    )
}

/// The agent's chat model (its own setting, else the default) and its daily cap.
pub fn settings_of(app: &App, control: &[StoredEvent], agent: &str) -> (Option<String>, String) {
    let own = control
        .iter()
        .rev()
        .filter(|row| row.event_type == "ChatSettingsChanged")
        .filter_map(envelope)
        .filter_map(|e| e.get("payload").cloned())
        .find(|p| p.get("agent_id").and_then(Value::as_str) == Some(agent));
    let text = |name: &str| {
        own.as_ref()
            .and_then(|p| p.get(name))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let model = text("chat_model")
        .filter(|id| app.chat.models.iter().any(|m| m.entry.model_id == *id))
        .or_else(|| app.chat.model(None).map(|m| m.entry.model_id.clone()));
    let cap = text("daily_cap_usd").unwrap_or_else(|| DEFAULT_DAILY_CAP_USD.to_owned());
    (model, cap)
}

/// `PUT /v1/workspaces/{ws}/agents/{agent}/settings`.
pub async fn put_settings(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
    body: Body,
) -> Response {
    let Ok(Path((workspace, agent))) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let (gate, settings) = match strictly(&app, &headers, body, decode_settings).await {
        Ok(admitted) => admitted,
        Err(refusal) => return *refusal,
    };
    let reader = match chat_reader(&app, &gate, &workspace).await {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    if agent_of(&reader.control, &agent).is_none() {
        return Refused::NotFound.response();
    }
    if !app
        .chat
        .models
        .iter()
        .any(|m| m.entry.model_id == settings.chat_model)
    {
        return invalid(vec![Violation::Schema {
            path: "/chat_model".to_owned(),
            code: "not_offered".to_owned(),
            message: "The request does not meet this rule.".to_owned(),
        }]);
    }
    let payload = vec![
        ("agent_id", string(&agent)),
        ("chat_model", string(&settings.chat_model)),
        ("daily_cap_usd", string(&settings.daily_cap_usd)),
    ];
    match append_owner(&app, &gate, &reader, "ChatSettingsChanged", payload).await {
        Ok(_) => enveloped(
            StatusCode::OK,
            reader.now,
            vec![
                ("agent_id", string(&agent)),
                ("chat_model", string(&settings.chat_model)),
                ("daily_cap_usd", string(&settings.daily_cap_usd)),
            ],
        ),
        Err(refusal) => refusal.response(),
    }
}

/// `GET /v1/workspaces/{ws}/home`: every agent the member may see, each with its mode, whether it
/// waits on the member, its last message in the member's thread, and its unread count (agent
/// messages after the member's last).
pub async fn home(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
) -> Response {
    let Ok(Path(workspace)) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let reader = match read_chat(&app, &headers, &workspace).await {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    let me = principal(&reader);
    let mut agents = Vec::new();
    let mut streams: Vec<Vec<StoredEvent>> = Vec::new();
    for deployment in deployed(&reader.control) {
        let stream = format!("agent:{}:{}", reader.workspace, deployment.0);
        let rows = app.journal.rows(&stream).await.unwrap_or_default();
        let thread = Thread::agent(&deployment.0, &me);
        let own: Vec<Value> = reader
            .control
            .iter()
            .filter(|row| row.event_type == "ChatMessagePosted")
            .filter_map(envelope)
            .filter(|e| thread.holds(e.get("payload")))
            .collect();
        let last = own.last().and_then(|e| e.get("payload")).map(|p| {
            object(vec![
                ("text", p.get("text").cloned().unwrap_or(Value::Null)),
                (
                    "from",
                    string(match p.get("author").and_then(Value::as_str) {
                        Some("owner") => "owner",
                        _ => "agent",
                    }),
                ),
            ])
        });
        let unread = own
            .iter()
            .rev()
            .take_while(|e| {
                e.get("payload")
                    .and_then(|p| p.get("author"))
                    .and_then(Value::as_str)
                    != Some("owner")
            })
            .count();
        let state = if deployment.4 {
            "stopped".to_owned()
        } else {
            mode(&rows).unwrap_or_else(|| "active".to_owned())
        };
        let waiting = crate::reads::approvals(&deployment.0, &rows)
            .iter()
            .any(|a| a.state.name() == "pending");
        let name = crate::routes::document(&app, &deployment)
            .map(|d| d.label)
            .unwrap_or_else(|| deployment.0.clone());
        agents.push(object(vec![
            ("agent_id", string(&deployment.0)),
            ("name", string(&name)),
            ("mode", string(&state)),
            ("needs_you", Value::Bool(waiting)),
            ("unread", int(u64::try_from(unread).unwrap_or(u64::MAX))),
            ("last_message", last.unwrap_or(Value::Null)),
        ]));
        streams.push(rows);
    }
    let slices: Vec<&[StoredEvent]> = streams.iter().map(Vec::as_slice).collect();
    served(&reader, &slices, vec![("agents", Value::Array(agents))])
}

/// The deployed, unstopped-or-stopped agent `agent` names.
pub fn agent_of(control: &[StoredEvent], agent: &str) -> Option<Deployment> {
    deployed(control).into_iter().find(|d| d.0 == agent)
}

/// Authorizes a write of the chat for the gate's session.
async fn chat_reader(app: &App, gate: &Gate, workspace: &str) -> Result<Reader, Refused> {
    authorized(
        app,
        &gate.admitted,
        gate.now,
        workspace,
        Permission::ChatThread,
        Unreadable::Command,
    )
    .await
}

/// Authorizes a read of the chat.
async fn read_chat(app: &App, headers: &HeaderMap, workspace: &str) -> Result<Reader, Refused> {
    let (admitted, now) = session(app, headers, Request::Other)?;
    authorized(
        app,
        &admitted,
        now,
        workspace,
        Permission::ChatThread,
        Unreadable::Read,
    )
    .await
}

fn member(gate: &Gate) -> String {
    principal_text(gate).unwrap_or_default()
}

fn principal(reader: &Reader) -> String {
    reader.principal.to_ulid_text().unwrap_or_default()
}

/// Whether `text` is an identifier: `[A-Za-z0-9_-]+`.
fn ident(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn problem_conflict() -> Response {
    crate::http::problem(
        StatusCode::TOO_MANY_REQUESTS,
        crate::http::problem_body(mandate_api::problem::ProblemCode::RateLimited),
    )
}

/// Appends one of the owner's own records (`user` actor), under §3.4's id for the gate's key.
async fn append_owner(
    app: &App,
    gate: &Gate,
    reader: &Reader,
    event_type: &'static str,
    payload: Vec<(&str, Value)>,
) -> Result<StoredEvent, Refused> {
    let operation = match event_type {
        "ChatRunStopRequested" => "chat_stop",
        "ChatSettingsChanged" => "chat_settings",
        _ => "chat_message",
    };
    let (_, id) =
        derived_for(gate, &reader.workspace, operation).ok_or(Refused::JournalUnavailable)?;
    let environment = reader
        .control
        .first()
        .map(|row| row.environment.clone())
        .ok_or(Refused::NotFound)?;
    let principal = member(gate);
    let writer = Writer {
        workspace: &reader.workspace,
        principal: &principal,
        event_id: &id,
        environment: &environment,
        now: gate.now,
    };
    let event = Event {
        event_type,
        schema_version: 1,
        payload: object(payload),
        artifact_refs: Vec::new(),
        config_refs: Vec::new(),
    };
    let draft = envelope_by("user", &writer, &event, None).ok_or(Refused::JournalUnavailable)?;
    let stream = format!("ctl:{}", reader.workspace);
    match settle_batch_at(
        app.journal.as_ref(),
        &stream,
        &id,
        &[draft.as_slice()],
        gate.now,
        None,
    )
    .await
    {
        Settled::Stored(row) => Ok(*row),
        _ => Err(Refused::JournalUnavailable),
    }
}

#[cfg(test)]
mod pure {
    //! The chat's small readers, each against the inputs on both sides of its every condition.

    use mandate_canon::parse;
    use mandate_chat::ChatEntry;
    use mandate_gateway::registry::{Identity, Snapshot};

    use super::{
        Chat, ChatModel, MAX_MESSAGE_CHARS, chat_from, decode_message, ident, marker,
        positive_decimal,
    };

    const REGISTRY: &str = include_str!("../../../deploy/cell/model-gateway/registry.json");

    fn model(id: &str) -> ChatModel {
        ChatModel {
            entry: ChatEntry {
                model_id: id.to_owned(),
                model_version: "1.0.0".to_owned(),
                identity: Identity {
                    provider: "anthropic".to_owned(),
                    model_name: format!("anthropic/{id}"),
                    snapshot: Snapshot::Dated(id.to_owned()),
                    quantization: None,
                },
                deadline_ms: 1_000,
                max_output_tokens: 100,
            },
            label: id.to_owned(),
            input_per_million: "1".to_owned(),
            output_per_million: "2".to_owned(),
        }
    }

    #[test]
    fn an_identifier_is_letters_digits_underscores_and_hyphens_and_never_empty() {
        for good in ["a", "setup_1", "setup-1", "A9", "_", "-"] {
            assert!(ident(good), "{good}");
        }
        for bad in ["", "a b", "a.b", "a/b", "é", "a+b"] {
            assert!(!ident(bad), "{bad}");
        }
    }

    #[test]
    fn a_cap_is_a_plain_positive_decimal() {
        for good in ["5", "0.5", "2.50", "10", "0.000001", "01"] {
            assert!(positive_decimal(good), "{good}");
        }
        for bad in [
            "", "0", "0.0", "00", ".5", "5.", "1.2.3", "-1", "1e3", "a", "1.a", "a.1", " 1",
        ] {
            assert!(!positive_decimal(bad), "{bad}");
        }
    }

    #[test]
    fn only_a_brief_card_with_no_text_is_the_daily_note_s_marker() {
        let reads = |text: &str| parse(text.as_bytes()).is_ok_and(|v| marker(&v));
        assert!(reads(
            r#"{"payload":{"card":{"kind":"brief"},"text":null}}"#
        ));
        assert!(!reads(
            r#"{"payload":{"card":{"kind":"brief"},"text":"Today went well."}}"#
        ));
        assert!(!reads(
            r#"{"payload":{"card":{"kind":"fill"},"text":null}}"#
        ));
        assert!(!reads(r#"{"payload":{"card":null,"text":null}}"#));
        assert!(!reads(r#"{"payload":{"card":{"kind":"brief"}}}"#));
    }

    #[test]
    fn a_message_is_at_most_its_cap_in_characters() {
        let body = |n: usize| format!(r#"{{"text":"{}"}}"#, "é".repeat(n));
        assert!(decode_message(body(MAX_MESSAGE_CHARS).as_bytes()).is_ok());
        assert!(decode_message(body(MAX_MESSAGE_CHARS - 1).as_bytes()).is_ok());
        assert!(decode_message(body(MAX_MESSAGE_CHARS + 1).as_bytes()).is_err());
        assert!(decode_message(br#"{"text":"  hi  "}"#).is_ok_and(|m| m.text == "hi"));
        assert!(decode_message(br#"{"text":"   "}"#).is_err());
        assert!(decode_message(br#"{"text":"hi","x":1}"#).is_err());
    }

    #[test]
    fn a_named_model_is_chosen_and_an_unknown_or_absent_one_falls_back_to_the_first() {
        let chat = Chat::new(None, vec![model("llm.a"), model("llm.b")]);
        let id = |m: Option<&ChatModel>| m.map(|m| m.entry.model_id.clone());
        assert_eq!(id(chat.model(Some("llm.b"))).as_deref(), Some("llm.b"));
        assert_eq!(id(chat.model(Some("llm.a"))).as_deref(), Some("llm.a"));
        assert_eq!(id(chat.model(Some("llm.z"))).as_deref(), Some("llm.a"));
        assert_eq!(id(chat.model(None)).as_deref(), Some("llm.a"));
        assert!(Chat::new(None, Vec::new()).model(None).is_none());
    }

    #[test]
    fn a_thread_is_held_by_one_run_until_released() {
        let chat = Chat::new(None, Vec::new());
        assert_eq!(chat.running("t"), None);
        assert!(chat.claim("t", "run_1"));
        assert!(!chat.claim("t", "run_2"));
        assert_eq!(chat.running("t").as_deref(), Some("run_1"));
        assert_eq!(chat.running("u"), None);
        chat.release("t");
        assert_eq!(chat.running("t"), None);
        assert!(!chat.stopped("run_1"));
        chat.stop("run_1");
        assert!(chat.stopped("run_1"));
        assert!(!chat.stopped("run_2"));
    }

    #[test]
    fn each_id_is_fresh_and_sorts_after_the_last_at_one_instant() {
        let chat = Chat::new(None, Vec::new());
        let Ok(now) = mandate_time::UtcNanos::from_parts(1_791_000_000, 0) else {
            return;
        };
        let first = chat.ulid(now);
        let second = chat.ulid(now);
        assert_eq!(first.len(), 26);
        assert!(second > first, "{first} {second}");
        let Ok(later) = mandate_time::UtcNanos::from_parts(1_791_000_000, 123_456_789) else {
            return;
        };
        let millis: u128 = 1_791_000_000_123;
        assert_eq!(
            chat.ulid(later),
            mandate_canon::encode_ulid((millis << 80).saturating_add(2)),
            "the millisecond leads and the counter follows"
        );
    }

    #[test]
    fn the_chat_takes_a_set_gateway_and_the_registry_s_chat_models() {
        let chat = chat_from(
            Some("model-gateway:8080".to_owned()),
            Some(REGISTRY.as_bytes()),
        );
        assert_eq!(chat.gateway.as_deref(), Some("model-gateway:8080"));
        let ids: Vec<&str> = chat
            .models
            .iter()
            .map(|m| m.entry.model_id.as_str())
            .collect();
        assert_eq!(ids, ["llm.chat_sonnet", "llm.chat_haiku"]);
        assert_eq!(
            chat.models.first().map(|m| m.label.as_str()),
            Some("claude-sonnet-5.5")
        );
        assert_eq!(
            chat.models.first().map(|m| m.input_per_million.as_str()),
            Some("2")
        );
        assert_eq!(
            chat.models.first().map(|m| m.output_per_million.as_str()),
            Some("10")
        );
        let unset = chat_from(Some(String::new()), Some(b"not json"));
        assert_eq!(unset.gateway, None, "an empty address is none");
        assert!(unset.models.is_empty());
        let none = chat_from(None, None);
        assert_eq!(none.gateway, None);
        assert!(none.models.is_empty());
    }
}
