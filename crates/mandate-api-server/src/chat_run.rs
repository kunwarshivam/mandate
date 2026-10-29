//! One chat run ([DEC-2293](../../../docs/project/decisions/DEC-2293.md)): `mandate-chat`'s turn
//! loop on a blocking thread, over the model gateway service, with this crate's tools.
//! - The read tools read only this agent's records, or this workspace's catalogue for a setup.
//! - The action tools commit the same commands the web app's routes commit, under the owner's
//!   session and a fresh authorization, so the order builder, the gate, the autonomy rules and
//!   the runtime decide every action (`AGENTS.md` rules 1, 4 and 12).
//! - Every model call and every result is journaled before anything acts on it, and every
//!   message the owner sees is journaled before it is served.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use mandate_api::idempotency::IdempotencyKey;
use mandate_canon::{Digest, Value, parse};
use mandate_chat::engine::{arg_text, parse_document};
use mandate_chat::entry::{AGENT, PROACTIVE, SETUP, name};
use mandate_chat::{End, Halt, Host, Model, RunSpec, Toolbox, run};
use mandate_gateway::frame;
use mandate_gateway::kinds::KeyOwner;
use mandate_gateway::tools::ToolCall;
use mandate_gateway::wire::Reply;
use mandate_gateway::{CallRecord, CallRequest, RequestError};
use mandate_identity::Permission;
use mandate_journal::StoredEvent;
use mandate_time::UtcNanos;
use tokio::runtime::Handle;

use crate::backtests::paper;
use crate::chat::{CHAT_ACTOR, latest_goals, settings_of};
use crate::commands::{Command, Event, KillScope, Settled, Writer, envelope_by, settle_batch_at};
use crate::http::App;
use crate::reads::{Deployment, account_ref, agent_orders, envelope, mode};
use crate::routes::{Unreadable, authorized, document, object, string};
use crate::writes::{Gate, committed};

/// How many messages of the thread the prompt holds, newest last.
const THREAD_MESSAGES: usize = 30;

/// The longest a tool result the model reads may be, in bytes; a longer one is cut.
const RESULT_BYTES: usize = 48 * 1024;

/// How long a connection to the gateway service may take.
const CONNECT: Duration = Duration::from_secs(5);

/// One thread: an agent's or a setup's, and the member whose private thread it is (DEC-2293
/// item 11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thread {
    pub agent: Option<String>,
    pub setup: Option<String>,
    pub member: String,
}

impl Thread {
    pub fn agent(agent: &str, member: &str) -> Self {
        Self {
            agent: Some(agent.to_owned()),
            setup: None,
            member: member.to_owned(),
        }
    }

    pub fn setup(setup: &str, member: &str) -> Self {
        Self {
            agent: None,
            setup: Some(setup.to_owned()),
            member: member.to_owned(),
        }
    }

    /// The key a thread's one run in flight is held under.
    pub fn key(&self) -> String {
        format!(
            "{}:{}:{}",
            self.agent.as_deref().unwrap_or_default(),
            self.setup.as_deref().unwrap_or_default(),
            self.member
        )
    }

    /// Whether a chat record's payload is of this thread.
    pub fn holds(&self, payload: Option<&Value>) -> bool {
        let Some(p) = payload else {
            return false;
        };
        let text = |name: &str| p.get(name).and_then(Value::as_str);
        text("agent_id") == self.agent.as_deref()
            && text("setup_id") == self.setup.as_deref()
            && text("member") == Some(self.member.as_str())
    }

    pub fn id_members(&self) -> Vec<(&'static str, Value)> {
        vec![
            (
                "agent_id",
                self.agent.as_deref().map_or(Value::Null, string),
            ),
            (
                "setup_id",
                self.setup.as_deref().map_or(Value::Null, string),
            ),
        ]
    }

    pub fn id_payload(&self) -> Vec<(&'static str, Value)> {
        let mut members = self.id_members();
        members.push(("member", string(&self.member)));
        members
    }

    pub fn message_payload(
        &self,
        author: &str,
        text: Option<&str>,
        card: Option<Value>,
        sources: &[(String, String)],
        run: Option<&str>,
        proactive: bool,
    ) -> Vec<(&'static str, Value)> {
        let mut members = self.id_payload();
        members.extend([
            ("author", string(author)),
            ("text", text.map_or(Value::Null, string)),
            ("card", card.unwrap_or(Value::Null)),
            (
                "sources",
                Value::Array(
                    sources
                        .iter()
                        .map(|(label, reference)| {
                            object(vec![("label", string(label)), ("ref", string(reference))])
                        })
                        .collect(),
                ),
            ),
            ("run_id", run.map_or(Value::Null, string)),
            ("proactive", Value::Bool(proactive)),
        ]);
        members
    }
}

/// What a run starts from.
pub(crate) struct Start {
    pub workspace: String,
    pub thread: Thread,
    pub run: String,
    /// The session of the owner whose message started it, under which its actions commit; `None`
    /// for a proactive run, which no one's message started and which commits nothing.
    pub gate: Option<Gate>,
    /// When it started.
    pub now: UtcNanos,
    pub deployment: Option<Deployment>,
    /// The record that woke it: the owner's message, or the event a proactive run follows.
    pub cause: String,
    pub message: String,
    pub proactive: bool,
}

/// Starts `start`'s run on a blocking thread; the thread is released when it ends.
pub(crate) fn spawn(app: Arc<App>, start: Start) {
    start_blocking(app, &Handle::current(), start);
}

/// Starts `start`'s run on a blocking thread of `handle`'s runtime, from any thread.
pub(crate) fn start_blocking(app: Arc<App>, handle: &Handle, start: Start) {
    let inner = handle.clone();
    handle.spawn_blocking(move || {
        let key = start.thread.key();
        execute(&app, &inner, start);
        app.chat.release(&key);
    });
}

fn execute(app: &App, handle: &Handle, start: Start) {
    let control = handle
        .block_on(app.journal.rows(&format!("ctl:{}", start.workspace)))
        .unwrap_or_default();
    let environment = control
        .first()
        .map(|row| row.environment.clone())
        .unwrap_or_else(|| "paper".to_owned());
    let kind = if start.thread.setup.is_some() {
        SETUP
    } else if start.proactive {
        PROACTIVE
    } else {
        AGENT
    };
    let agent = start.thread.agent.clone();
    let chosen = agent
        .as_deref()
        .and_then(|a| settings_of(app, &control, a).0);
    let mut journal = Journaled {
        app,
        handle,
        start: &start,
        environment,
        posted: false,
        cap: agent
            .as_deref()
            .map(|a| settings_of(app, &control, a).1)
            .unwrap_or_else(|| crate::chat::DEFAULT_DAILY_CAP_USD.to_owned()),
    };
    let (Some(address), Some(model)) =
        (app.chat.gateway.clone(), app.chat.model(chosen.as_deref()))
    else {
        journal
            .say("I can't think right now: the model service isn't connected on this deployment.");
        journal.ended("refused");
        return;
    };
    let entry = model.entry.clone();
    let spec = RunSpec {
        workspace_id: start.workspace.clone(),
        agent_id: agent.clone(),
        kind: kind.to_owned(),
        model: entry.content().model_ref(),
        inputs: BTreeMap::from([
            ("context".to_owned(), context(app, &control, &start)),
            ("message".to_owned(), start.message.clone()),
            ("thread".to_owned(), transcript(&control, &start.thread)),
        ]),
        key_owner: KeyOwner::Platform,
        call_deadline_ms: entry.deadline_ms,
    };
    let mut tools = Tools {
        app,
        handle,
        start: &start,
        turn: 0,
        index: 0,
        posted: false,
    };
    let mut remote = Remote { address };
    let end = run(
        &spec,
        mandate_chat::entry::CEILINGS.max_turns,
        &mut remote,
        &mut tools,
        &mut journal,
    );
    journal.posted |= tools.posted;
    if !journal.posted && kind != PROACTIVE {
        let said = match &end {
            End::Answered { .. } => None,
            End::Stopped => Some("Stopped."),
            End::Budget => Some(
                "I've reached today's AI spending cap for this agent. You can raise it in Settings.",
            ),
            End::Model(_) | End::Refused(_) => Some(
                "I couldn't finish that: the model service didn't answer. Try again in a moment.",
            ),
            End::Halted => Some("I couldn't record my work, so I stopped before doing anything."),
            End::Ceiling => {
                Some("That took more steps than I can take in one go. Ask me to continue.")
            }
        };
        if let Some(text) = said {
            journal.say(text);
        }
    }
    journal.ended(end.code());
}

/// The run's journal: its records, its clock, and its Stop and cap.
struct Journaled<'a> {
    app: &'a App,
    handle: &'a Handle,
    start: &'a Start,
    environment: String,
    posted: bool,
    cap: String,
}

impl Journaled<'_> {
    fn append(&self, event_type: &'static str, payload: Vec<(&str, Value)>) -> Option<StoredEvent> {
        append_system(
            self.app,
            self.handle,
            (&self.start.workspace, &self.environment),
            (event_type, Some(&self.start.cause)),
            object(payload),
        )
    }

    fn say(&mut self, text: &str) {
        let payload = self.start.thread.message_payload(
            "agent",
            Some(text),
            None,
            &[],
            Some(&self.start.run),
            self.start.proactive,
        );
        if self.append("ChatMessagePosted", payload).is_some() {
            self.posted = true;
        }
    }

    fn ended(&self, reason: &str) {
        let mut payload = self.start.thread.id_payload();
        payload.extend([
            ("run_id", string(&self.start.run)),
            ("reason", string(reason)),
        ]);
        let _ = self.append("ChatRunEnded", payload);
    }

    /// The cost of the agent's chat today, summed from its turns' records.
    fn spent_today(&self) -> u128 {
        let Some(agent) = self.start.thread.agent.as_deref() else {
            return 0;
        };
        let today = self
            .app
            .clock
            .now()
            .map(|now| now.date().to_string())
            .unwrap_or_default();
        let control = self
            .handle
            .block_on(
                self.app
                    .journal
                    .rows(&format!("ctl:{}", self.start.workspace)),
            )
            .unwrap_or_default();
        control
            .iter()
            .filter(|row| {
                row.event_type == "ChatTurnRecorded" && row.recorded_at.starts_with(&today)
            })
            .filter_map(envelope)
            .filter_map(|e| e.get("payload").cloned())
            .filter(|p| p.get("agent_id").and_then(Value::as_str) == Some(agent))
            .filter_map(|p| p.get("cost_usd").and_then(Value::as_str).and_then(micros))
            .sum()
    }
}

impl Host for Journaled<'_> {
    fn invoked(&mut self, turn: u32, record: &CallRecord) -> Result<(), Halt> {
        let store = |bytes: Option<&Vec<u8>>| {
            let bytes = bytes?;
            let objects = self.app.objects.as_ref()?;
            objects.put(bytes).ok()?;
            Some(format!("sha256:{}", Digest::of(bytes).to_hex()))
        };
        let prompt = store(record.prompt.as_ref());
        let response = store(record.response.as_ref());
        let mut payload = self.start.thread.id_payload();
        payload.extend([
            ("run_id", string(&self.start.run)),
            ("turn", crate::routes::int(u64::from(turn))),
            ("kind", string(run_kind(self.start))),
            ("call_id", string(&record.call_id)),
            (
                "model",
                object(vec![
                    ("model_id", string(&record.model.model_id)),
                    ("model_version", string(&record.model.model_version)),
                    (
                        "content_hash",
                        string(&format!("sha256:{}", record.model.content_hash.to_hex())),
                    ),
                ]),
            ),
            ("outcome", string(record.outcome.code())),
            ("cost_usd", string(&record.cost.to_string())),
            ("prompt_ref", prompt.as_deref().map_or(Value::Null, string)),
            (
                "response_ref",
                response.as_deref().map_or(Value::Null, string),
            ),
            (
                "calls",
                Value::Array(
                    record
                        .tool_calls
                        .iter()
                        .map(|call| {
                            object(vec![
                                ("name", string(&call.name)),
                                ("arguments", call.arguments.clone()),
                            ])
                        })
                        .collect(),
                ),
            ),
        ]);
        self.append("ChatTurnRecorded", payload)
            .map(|_| ())
            .ok_or(Halt)
    }

    fn results(&mut self, turn: u32, calls: &[ToolCall], results: &[String]) -> Result<(), Halt> {
        let mut payload = self.start.thread.id_payload();
        payload.extend([
            ("run_id", string(&self.start.run)),
            ("turn", crate::routes::int(u64::from(turn))),
            (
                "results",
                Value::Array(
                    calls
                        .iter()
                        .zip(results)
                        .map(|(call, result)| {
                            object(vec![
                                ("name", string(&call.name)),
                                ("result", string(result)),
                            ])
                        })
                        .collect(),
                ),
            ),
        ]);
        self.append("ChatToolResultsRecorded", payload)
            .map(|_| ())
            .ok_or(Halt)
    }

    fn stopped(&mut self) -> bool {
        self.app.chat.stopped(&self.start.run)
    }

    fn over_budget(&mut self) -> bool {
        micros(&self.cap).is_some_and(|cap| self.spent_today() >= cap)
    }

    fn now(&mut self) -> UtcNanos {
        self.app.clock.now().unwrap_or(self.start.now)
    }

    fn call_id(&mut self) -> String {
        let now = self.now();
        self.app.chat.ulid(now)
    }
}

fn run_kind(start: &Start) -> &'static str {
    if start.thread.setup.is_some() {
        SETUP
    } else if start.proactive {
        PROACTIVE
    } else {
        AGENT
    }
}

/// A decimal USD text in millionths; `None` when it does not read.
fn micros(text: &str) -> Option<u128> {
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    if fraction.len() > 6 || !whole.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let whole: u128 = whole.parse().ok()?;
    let padded = format!("{fraction:0<6}");
    let fraction: u128 = padded.parse().ok()?;
    whole.checked_mul(1_000_000)?.checked_add(fraction)
}

/// Appends one of the chat's own records, written by the system, under a fresh ULID.
pub fn append_system(
    app: &App,
    handle: &Handle,
    (workspace, environment): (&str, &str),
    (event_type, causation): (&'static str, Option<&str>),
    payload: Value,
) -> Option<StoredEvent> {
    let now = app.clock.now()?;
    let id = app.chat.ulid(now);
    let writer = Writer {
        workspace,
        principal: CHAT_ACTOR,
        event_id: &id,
        environment,
        now,
    };
    let event = Event {
        event_type,
        schema_version: 1,
        artifact_refs: references(&payload),
        payload,
        config_refs: Vec::new(),
    };
    let draft = envelope_by("system", &writer, &event, causation)?;
    let stream = format!("ctl:{workspace}");
    match handle.block_on(settle_batch_at(
        app.journal.as_ref(),
        &stream,
        &id,
        &[draft.as_slice()],
        now,
        None,
    )) {
        Settled::Stored(row) => Some(*row),
        _ => None,
    }
}

/// The sorted set of `sha256:` references a payload holds (journal spec §3's `artifact_refs`).
fn references(payload: &Value) -> Vec<Value> {
    fn walk(value: &Value, out: &mut BTreeSet<String>) {
        match value {
            Value::Str(text) if text.starts_with("sha256:") && text.len() == 71 => {
                out.insert(text.clone());
            }
            Value::Array(items) => items.iter().for_each(|v| walk(v, out)),
            Value::Object(members) => members.values().for_each(|v| walk(v, out)),
            _ => {}
        }
    }
    let mut out = BTreeSet::new();
    walk(payload, &mut out);
    out.into_iter().map(Value::Str).collect()
}

/// The gateway service, one framed call per connection.
struct Remote {
    address: String,
}

impl Model for Remote {
    fn call(&mut self, request: &CallRequest) -> Reply {
        let addresses: Vec<SocketAddr> = self
            .address
            .to_socket_addrs()
            .map(Iterator::collect)
            .unwrap_or_default();
        let Some(mut stream) = addresses
            .iter()
            .find_map(|a| TcpStream::connect_timeout(a, CONNECT).ok())
        else {
            return Err(RequestError::GatewayUnreachable);
        };
        let timeout = Duration::from_secs(180);
        let _ = stream.set_read_timeout(Some(timeout));
        let _ = stream.set_write_timeout(Some(timeout));
        frame::call(&mut Bounded(&mut stream), request)
            .unwrap_or(Err(RequestError::GatewayUnreachable))
    }
}

/// A stream read and written whole.
struct Bounded<'a>(&'a mut TcpStream);

impl Read for Bounded<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}

impl Write for Bounded<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

/// The context the prompt holds: who the agent is, its mode, its mandate's summary, and today.
fn context(app: &App, control: &[StoredEvent], start: &Start) -> String {
    let today = app
        .clock
        .now()
        .map(|now| now.to_string())
        .unwrap_or_default();
    let Some(deployment) = start.deployment.as_ref() else {
        let connections: Vec<String> = crate::connections::connections(control)
            .iter()
            .map(|c| {
                format!(
                    "{} ({} {}, {})",
                    c.connection_id,
                    c.broker,
                    c.environment,
                    c.state.code()
                )
            })
            .collect();
        return format!(
            "No agent yet: the owner is setting one up. Environment: {}. Now: {today}.\n\
             Broker connections (a mandate's connection_id must be an established one): {}.",
            if paper(control) { "paper" } else { "live" },
            if connections.is_empty() {
                "none yet; the owner connects one under Settings".to_owned()
            } else {
                connections.join("; ")
            }
        );
    };
    let doc = document(app, deployment);
    let name = doc
        .as_ref()
        .map_or(deployment.0.clone(), |d| d.label.clone());
    let summary = doc
        .as_ref()
        .map(|d| String::from_utf8_lossy(&mandate_canon::to_canonical(&d.mandate)).into_owned())
        .unwrap_or_default();
    format!(
        "Agent: {name} ({id}). Environment: {env}. Stopped: {stopped}. Now: {today}.\nMandate summary: {summary}",
        id = deployment.0,
        env = deployment.1,
        stopped = deployment.4,
    )
}

/// The thread's last messages, oldest first, as `Owner:` and `Agent:` lines.
fn transcript(control: &[StoredEvent], thread: &Thread) -> String {
    let lines: Vec<String> = control
        .iter()
        .filter(|row| row.event_type == "ChatMessagePosted")
        .filter_map(envelope)
        .filter_map(|e| e.get("payload").cloned())
        .filter(|p| thread.holds(Some(p)))
        .map(|p| {
            let who = match p.get("author").and_then(Value::as_str) {
                Some("owner") => "Owner",
                _ => "Agent",
            };
            let text = p.get("text").and_then(Value::as_str).unwrap_or("(card)");
            format!("{who}: {text}")
        })
        .collect();
    let skip = lines.len().saturating_sub(THREAD_MESSAGES);
    let kept: Vec<String> = lines.into_iter().skip(skip).collect();
    if kept.is_empty() {
        "(no earlier messages)".to_owned()
    } else {
        kept.join("\n")
    }
}

/// The run's tools, each scoped to the run's own agent or setup by construction (DEC-2293 item 4).
struct Tools<'a> {
    app: &'a App,
    handle: &'a Handle,
    start: &'a Start,
    turn: u32,
    index: u32,
    posted: bool,
}

impl Toolbox for Tools<'_> {
    fn run(&mut self, call: &ToolCall) -> String {
        self.index = self.index.saturating_add(1);
        if call.index == 0 {
            self.turn = self.turn.saturating_add(1);
        }
        let result = match call.name.as_str() {
            name::REPLY => self.reply(call),
            name::NOTE => self.note(call),
            name::SUMMARY | name::POSITIONS => self.positions(call.name == name::SUMMARY),
            name::ORDERS => self.orders(),
            name::PRICE => self.price(arg_text(call, "symbol")),
            name::MANDATE => self.mandate(),
            name::GOALS => self.goals(),
            name::THESES => self.theses(),
            name::NEWS => self.news(arg_text(call, "symbol")),
            name::SEARCH => self.search(arg_text(call, "query")),
            name::MODELS => self.signal_models(),
            name::BUY => self.buy(arg_text(call, "symbol"), arg_decimal(call, "quantity")),
            name::SELL => self.sell(arg_text(call, "symbol")),
            name::PAUSE => {
                self.command(|agent| Command::Pause(agent.to_owned()), Permission::Pause)
            }
            name::RESUME => self.command(
                |agent| Command::Resume {
                    agent: agent.to_owned(),
                    step_up: Value::Null,
                },
                Permission::ResumeOrStop,
            ),
            name::STOP => self.command(
                |agent| Command::Stop {
                    agent: agent.to_owned(),
                    release: false,
                    step_up: Value::Null,
                },
                Permission::ResumeOrStop,
            ),
            name::KILL => self.command(
                |agent| Command::KillSwitch(KillScope::Agent(agent.to_owned())),
                Permission::KillSwitchAgent,
            ),
            name::PROPOSE => self.propose(call),
            name::DRAFT => self.draft(arg_text(call, "document")),
            _ => refusal("unknown_tool"),
        };
        cut(result)
    }
}

impl Tools<'_> {
    fn control(&self) -> Vec<StoredEvent> {
        self.handle
            .block_on(
                self.app
                    .journal
                    .rows(&format!("ctl:{}", self.start.workspace)),
            )
            .unwrap_or_default()
    }

    fn deployment(&self) -> Option<&Deployment> {
        self.start.deployment.as_ref()
    }

    fn agent_rows(&self) -> Vec<StoredEvent> {
        let Some(agent) = self.start.thread.agent.as_deref() else {
            return Vec::new();
        };
        self.handle
            .block_on(
                self.app
                    .journal
                    .rows(&format!("agent:{}:{agent}", self.start.workspace)),
            )
            .unwrap_or_default()
    }

    fn account_rows(&self) -> Option<Vec<StoredEvent>> {
        let deployment = self.deployment()?;
        let doc = document(self.app, deployment)?;
        let control = self.control();
        let reference = account_ref(&control, &doc.connection_id)?;
        self.handle
            .block_on(
                self.app
                    .journal
                    .rows(&format!("acct:{}:{reference}", self.start.workspace)),
            )
            .ok()
    }

    fn reply(&mut self, call: &ToolCall) -> String {
        let text = arg_text(call, "text");
        let sources: Vec<(String, String)> = call
            .arguments
            .get("sources")
            .and_then(Value::as_array)
            .unwrap_or_default()
            .iter()
            .filter_map(|s| {
                Some((
                    s.get("label")?.as_str()?.to_owned(),
                    s.get("ref")?.as_str()?.to_owned(),
                ))
            })
            .collect();
        if text.trim().is_empty() {
            return refusal("empty_text");
        }
        self.post(Some(text), None, &sources)
    }

    fn post(
        &mut self,
        text: Option<&str>,
        card: Option<Value>,
        sources: &[(String, String)],
    ) -> String {
        let payload = self.start.thread.message_payload(
            "agent",
            text,
            card,
            sources,
            Some(&self.start.run),
            self.start.proactive,
        );
        match self.append("ChatMessagePosted", payload) {
            Some(row) => {
                self.posted = true;
                ok(vec![("message_id", string(&row.event_id))])
            }
            None => refusal("journal_unavailable"),
        }
    }

    fn append(&self, event_type: &'static str, payload: Vec<(&str, Value)>) -> Option<StoredEvent> {
        let control = self.control();
        let environment = control
            .first()
            .map(|row| row.environment.clone())
            .unwrap_or_else(|| "paper".to_owned());
        append_system(
            self.app,
            self.handle,
            (&self.start.workspace, &environment),
            (event_type, Some(&self.start.cause)),
            object(payload),
        )
    }

    fn note(&mut self, call: &ToolCall) -> String {
        let Some(agent) = self.start.thread.agent.clone() else {
            return refusal("no_agent");
        };
        let goals = call
            .arguments
            .get("goals")
            .cloned()
            .unwrap_or(Value::Array(Vec::new()));
        let plan = arg_text(call, "plan");
        let watching = call
            .arguments
            .get("watching")
            .cloned()
            .unwrap_or(Value::Array(Vec::new()));
        let payload = vec![
            ("agent_id", string(&agent)),
            ("run_id", string(&self.start.run)),
            ("goals", goals),
            (
                "plan",
                if plan.is_empty() {
                    Value::Null
                } else {
                    string(plan)
                },
            ),
            ("watching", watching),
        ];
        match self.append("AgentGoalsUpdated", payload) {
            Some(_) => ok(vec![]),
            None => refusal("journal_unavailable"),
        }
    }

    fn positions(&self, summary: bool) -> String {
        let Some(agent) = self.start.thread.agent.as_deref() else {
            return refusal("no_agent");
        };
        let rows = self.account_rows().unwrap_or_default();
        let positions = self.app.folds.positions(&rows, agent).unwrap_or_default();
        if !summary {
            return ok(vec![("positions", Value::Array(positions))]);
        }
        let agent_rows = self.agent_rows();
        let pending = crate::reads::approvals(agent, &agent_rows)
            .iter()
            .filter(|a| a.state.name() == "pending")
            .count();
        let stopped = self.deployment().is_some_and(|d| d.4);
        ok(vec![
            (
                "mode",
                string(&if stopped {
                    "stopped".to_owned()
                } else {
                    mode(&agent_rows).unwrap_or_else(|| "active".to_owned())
                }),
            ),
            ("positions", Value::Array(positions)),
            (
                "approvals_waiting",
                crate::routes::int(u64::try_from(pending).unwrap_or(u64::MAX)),
            ),
        ])
    }

    fn orders(&self) -> String {
        let Some(agent) = self.start.thread.agent.as_deref() else {
            return refusal("no_agent");
        };
        let rows = self.account_rows().unwrap_or_default();
        let mut orders = agent_orders(&rows, agent);
        orders.reverse();
        orders.truncate(25);
        ok(vec![("orders", Value::Array(orders))])
    }

    fn snapshots(&self) -> Vec<crate::versions::Snapshot> {
        let control = self.control();
        let Some(store) = self.app.artifacts.as_deref() else {
            return Vec::new();
        };
        crate::catalogue::pinnable_snapshots(&control, store).unwrap_or_default()
    }

    fn price(&self, symbol: &str) -> String {
        let wanted = symbol.trim().to_ascii_uppercase();
        let Some(snapshot) = self
            .snapshots()
            .into_iter()
            .find(|s| s.symbol.eq_ignore_ascii_case(&wanted))
        else {
            return refusal("unknown_symbol");
        };
        let now = self.app.clock.now().unwrap_or(self.start.now);
        match crate::prices::last_close(
            self.app.bars.as_deref(),
            &snapshot.symbol,
            snapshot.asset_class.as_deref(),
            now,
        ) {
            Ok(close) => ok(vec![
                ("symbol", string(&snapshot.symbol)),
                ("close", string(&close.close)),
                ("bar_start", string(&close.bar_start.to_string())),
                ("kind", string("last trusted daily close")),
            ]),
            Err(why) => refusal(why.as_str()),
        }
    }

    fn mandate(&self) -> String {
        let Some(doc) = self.deployment().and_then(|d| document(self.app, d)) else {
            return refusal("no_mandate");
        };
        ok(vec![
            ("name", string(&doc.label)),
            ("summary", doc.mandate.clone()),
            ("pinned_universe", Value::Bool(doc.pinned)),
        ])
    }

    fn goals(&self) -> String {
        let Some(agent) = self.start.thread.agent.as_deref() else {
            return refusal("no_agent");
        };
        let control = self.control();
        ok(vec![(
            "goals",
            latest_goals(&control, agent).unwrap_or(Value::Null),
        )])
    }

    fn theses(&self) -> String {
        let theses: Vec<Value> = self
            .agent_rows()
            .iter()
            .filter(|row| {
                matches!(
                    row.event_type.as_str(),
                    "ThesisProposed" | "ThesisRevised" | "ThesisInvalidated"
                )
            })
            .filter_map(envelope)
            .filter_map(|e| {
                Some(object(vec![
                    ("type", e.get("event_type")?.clone()),
                    ("record", string(e.get("event_id")?.as_str()?)),
                    ("payload", e.get("payload")?.clone()),
                ]))
            })
            .rev()
            .take(20)
            .collect();
        ok(vec![
            ("theses", Value::Array(theses)),
            ("untrusted", Value::Bool(true)),
        ])
    }

    fn news(&self, symbol: &str) -> String {
        let rows = self
            .handle
            .block_on(
                self.app
                    .journal
                    .rows(&format!("news:{}", self.start.workspace)),
            )
            .unwrap_or_default();
        let wanted = symbol.trim().to_ascii_uppercase();
        let items: Vec<Value> = rows
            .iter()
            .filter(|row| row.event_type == "NewsItemStored")
            .filter_map(envelope)
            .filter_map(|e| e.get("payload").cloned())
            .filter(|p| {
                wanted.is_empty()
                    || p.get("symbols")
                        .and_then(Value::as_array)
                        .unwrap_or_default()
                        .iter()
                        .any(|s| s.as_str().is_some_and(|s| s.eq_ignore_ascii_case(&wanted)))
            })
            .map(|p| {
                let headline = (p.get("licensed") != Some(&Value::Bool(true)))
                    .then(|| self.headline(p.get("item_ref").and_then(Value::as_str)))
                    .flatten();
                object(vec![
                    ("symbols", p.get("symbols").cloned().unwrap_or(Value::Null)),
                    (
                        "published_at",
                        p.get("published_at").cloned().unwrap_or(Value::Null),
                    ),
                    ("headline", headline.map_or(Value::Null, |h| string(&h))),
                    ("ref", p.get("item_id").cloned().unwrap_or(Value::Null)),
                ])
            })
            .rev()
            .take(15)
            .collect();
        ok(vec![
            ("news", Value::Array(items)),
            ("untrusted", Value::Bool(true)),
        ])
    }

    fn headline(&self, reference: Option<&str>) -> Option<String> {
        let reference = mandate_journal::ArtifactRef::parse(reference?)?;
        let store = self.app.artifacts.as_deref()?;
        let bytes = mandate_journal::get_artifact(store, &reference).ok()?;
        let item = parse(&bytes).ok()?;
        item.get("headline")
            .and_then(Value::as_str)
            .map(str::to_owned)
    }

    fn search(&self, query: &str) -> String {
        let wanted = query.trim().to_ascii_lowercase();
        let found: Vec<Value> = self
            .snapshots()
            .iter()
            .filter(|s| wanted.is_empty() || s.symbol.to_ascii_lowercase().contains(&wanted))
            .take(25)
            .map(|s| {
                object(vec![
                    ("symbol", string(&s.symbol)),
                    ("instrument_id", string(&s.instrument_id)),
                    (
                        "asset_class",
                        s.asset_class.as_deref().map_or(Value::Null, string),
                    ),
                ])
            })
            .collect();
        ok(vec![("instruments", Value::Array(found))])
    }

    fn signal_models(&self) -> String {
        let control = self.control();
        let Some(store) = self.app.artifacts.as_deref() else {
            return refusal("no_store");
        };
        ok(vec![(
            "models",
            Value::Array(crate::catalogue::listed_models(&control, store).unwrap_or_default()),
        )])
    }

    /// The owner's buy (DEC-2159): the instrument the symbol names, then the owner request route's
    /// own checks and command, under a fresh authorization of the owner's session.
    fn buy(&mut self, symbol: &str, quantity: Option<String>) -> String {
        let Some(deployment) = self.deployment().cloned() else {
            return refusal("no_agent");
        };
        if deployment.4 {
            return refusal("agent_stopped");
        }
        let Some(quantity) = quantity.filter(|q| !q.starts_with('-') && q != "0") else {
            return refusal("quantity_positive");
        };
        let wanted = symbol.trim().to_ascii_uppercase();
        let Some(snapshot) = self
            .snapshots()
            .into_iter()
            .find(|s| s.symbol.eq_ignore_ascii_case(&wanted))
        else {
            return refusal("unknown_symbol");
        };
        let control = self.control();
        if !paper(&control) {
            return refusal("live_unavailable");
        }
        if !self.pins(&deployment, &snapshot.instrument_id) {
            return refusal("not_in_mandate");
        }
        let command = Command::OwnerRequest {
            agent: deployment.0.clone(),
            instrument: snapshot.instrument_id.clone(),
            quantity,
        };
        self.commit(&command, Permission::OwnerRequest)
    }

    fn pins(&self, deployment: &Deployment, instrument: &str) -> bool {
        let Some(store) = self.app.artifacts.as_deref() else {
            return false;
        };
        let Some((document, _)) = crate::versions::stored(store, &deployment.2) else {
            return false;
        };
        document
            .get("universe")
            .and_then(|u| u.get("pinned_instruments"))
            .and_then(Value::as_array)
            .unwrap_or_default()
            .iter()
            .any(|pin| pin.get("asset_id").and_then(Value::as_str) == Some(instrument))
    }

    /// The owner's sale is the owner exit of the whole instrument (DEC-2151 item 7).
    fn sell(&mut self, symbol: &str) -> String {
        let Some(deployment) = self.deployment().cloned() else {
            return refusal("no_agent");
        };
        let wanted = symbol.trim().to_ascii_uppercase();
        let Some(snapshot) = self
            .snapshots()
            .into_iter()
            .find(|s| s.symbol.eq_ignore_ascii_case(&wanted))
        else {
            return refusal("unknown_symbol");
        };
        let command = Command::OwnerExit {
            agent: deployment.0.clone(),
            instrument: snapshot.instrument_id.clone(),
            bid: None,
        };
        let done = self.commit(&command, Permission::OwnerExit);
        format!(
            "{done}\nNote: a sale exits the whole position in {}.",
            snapshot.symbol
        )
    }

    fn command(&mut self, make: impl Fn(&str) -> Command, permission: Permission) -> String {
        let Some(deployment) = self.deployment().cloned() else {
            return refusal("no_agent");
        };
        self.commit(&make(&deployment.0), permission)
    }

    /// Commits `command` as the owner's own route would: a fresh authorization of the owner's
    /// session for `permission`, and an idempotency key of this run's own, so a retried run
    /// replays rather than repeats.
    fn commit(&mut self, command: &Command, permission: Permission) -> String {
        let Some(session) = self.start.gate.as_ref() else {
            return refusal("no_session");
        };
        let now = self.app.clock.now().unwrap_or(self.start.now);
        let Ok(key) = IdempotencyKey::parse(&format!(
            "chat-{}-{}-{}",
            self.start.run, self.turn, self.index
        )) else {
            return refusal("idempotency");
        };
        let gate = Gate {
            admitted: session.admitted,
            now,
            key,
        };
        let reader = match self.handle.block_on(authorized(
            self.app,
            &gate.admitted,
            now,
            &self.start.workspace,
            permission,
            Unreadable::Command,
        )) {
            Ok(reader) => reader,
            Err(_) => return refusal("not_permitted"),
        };
        let response =
            self.handle
                .block_on(committed(self.app, &gate, &reader, command, &[], None));
        let status = response.status();
        let body = self
            .handle
            .block_on(axum::body::to_bytes(response.into_body(), 64 * 1024))
            .unwrap_or_default();
        let answer = parse(&body).ok();
        if status.is_success() {
            ok(vec![
                ("accepted", Value::Bool(true)),
                (
                    "record",
                    answer
                        .as_ref()
                        .and_then(|a| a.get("event_id").cloned())
                        .unwrap_or(Value::Null),
                ),
                (
                    "note",
                    string(
                        "Recorded. The platform sizes, checks and places it; watch the order card for the result.",
                    ),
                ),
            ])
        } else {
            let code = answer
                .as_ref()
                .and_then(|a| a.get("code").and_then(Value::as_str).map(str::to_owned))
                .unwrap_or_else(|| status_code(status));
            refusal(&code)
        }
    }

    /// A proactive run's proposal: a card the owner approves with one tap, which then sends the
    /// owner's own request; nothing is placed until they do.
    fn propose(&mut self, call: &ToolCall) -> String {
        let card = object(vec![
            ("kind", string("proposal")),
            ("side", string(arg_text(call, "side"))),
            (
                "symbol",
                string(&arg_text(call, "symbol").to_ascii_uppercase()),
            ),
            (
                "qty",
                arg_decimal(call, "quantity").map_or(Value::Null, |q| string(&q)),
            ),
            ("reason", string(arg_text(call, "reason"))),
        ]);
        self.post(None, Some(card), &[])
    }

    /// The setup's mandate draft: parsed, created as a version through the version route's own
    /// checks, and offered as a confirmation card; its problems otherwise.
    fn draft(&mut self, text: &str) -> String {
        let Some(session) = self.start.gate.as_ref() else {
            return refusal("no_session");
        };
        let Some(document) = parse_document(text) else {
            return refusal("document_not_json");
        };
        match self.handle.block_on(crate::version_routes::create_for_chat(
            self.app,
            session,
            &self.start.workspace,
            document,
        )) {
            Ok(version) => {
                let card = object(vec![
                    ("kind", string("confirm_mandate")),
                    ("version", string(&version.hash)),
                    ("summary", version.summary.clone()),
                ]);
                let posted = self.post(None, Some(card), &[]);
                format!("{posted}\nThe draft is valid; the owner now sees a confirmation card.")
            }
            Err(problems) => refusal_with(problems),
        }
    }
}

fn arg_decimal(call: &ToolCall, name: &str) -> Option<String> {
    call.arguments
        .get(name)
        .and_then(Value::as_str)
        .filter(|q| !q.is_empty())
        .map(str::to_owned)
}

fn status_code(status: StatusCode) -> String {
    format!("http_{}", status.as_u16())
}

fn ok(members: Vec<(&str, Value)>) -> String {
    let mut all = vec![("ok", Value::Bool(true))];
    all.extend(members);
    String::from_utf8_lossy(&mandate_canon::to_canonical(&object(all))).into_owned()
}

fn refusal(code: &str) -> String {
    String::from_utf8_lossy(&mandate_canon::to_canonical(&object(vec![
        ("ok", Value::Bool(false)),
        ("refused", string(code)),
    ])))
    .into_owned()
}

fn refusal_with(problems: Value) -> String {
    String::from_utf8_lossy(&mandate_canon::to_canonical(&object(vec![
        ("ok", Value::Bool(false)),
        ("refused", string("invalid_mandate")),
        ("problems", problems),
    ])))
    .into_owned()
}

/// A result cut to [`RESULT_BYTES`] on a character boundary.
fn cut(mut text: String) -> String {
    if text.len() > RESULT_BYTES {
        let mut end = RESULT_BYTES;
        while !text.is_char_boundary(end) {
            end = end.saturating_sub(1);
        }
        text.truncate(end);
        text.push_str("…(cut)");
    }
    text
}
