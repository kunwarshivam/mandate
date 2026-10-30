//! The chat's turn loop (DEC-2293). Each turn asks the model through the gateway, hands the reply's
//! record to the caller to journal before anything acts on it (item 7), runs the reply's tool calls
//! through the caller's [`Toolbox`], hands their results to the caller to journal, and asks again
//! with them, until the model answers without a tool call, the owner stops the run, or the gateway
//! refuses. Nothing here reads the journal, a store, or a clock other than the caller's.

use std::collections::BTreeMap;

use mandate_canon::{Value, parse};
use mandate_gateway::kinds::{KeyOwner, Outcome, Purpose};
use mandate_gateway::registry::ModelRef;
use mandate_gateway::tools::{ToolCall, Turn};
use mandate_gateway::wire::Reply;
use mandate_gateway::{CallRecord, CallRequest};
use mandate_time::UtcNanos;

use crate::entry::menu;

/// What one run asks of the gateway, fixed at its start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSpec {
    pub workspace_id: String,
    /// The agent the run is about; `None` exactly for a `setup` run.
    pub agent_id: Option<String>,
    /// [`crate::entry::AGENT`], [`crate::entry::PROACTIVE`], or [`crate::entry::SETUP`].
    pub kind: String,
    pub model: ModelRef,
    /// The template's slots, [`crate::entry::INPUTS`], except `kind`, which the run fills.
    pub inputs: BTreeMap<String, String>,
    pub key_owner: KeyOwner,
    /// How long one model call may take, from its start.
    pub call_deadline_ms: u64,
}

impl RunSpec {
    /// A `setup` run has no agent, so it is the compiler's purpose (rule 156); every other run is
    /// the agent's chat.
    pub fn purpose(&self) -> Purpose {
        if self.agent_id.is_some() {
            Purpose::Chat
        } else {
            Purpose::Compiler
        }
    }
}

/// The gateway, as the run sees it.
pub trait Model {
    fn call(&mut self, request: &CallRequest) -> Reply;
}

/// The caller's tools: each call's result, as the JSON text the model reads next turn. A call the
/// caller refuses is answered with a result that says why, never with an error the run stops on.
pub trait Toolbox {
    fn run(&mut self, call: &ToolCall) -> String;
}

/// Why the caller halted a run: it could not journal a record, so nothing after it may act.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Halt;

/// Where the run's records go, and what the run reads of the outside: time, ids, and the owner's
/// Stop.
pub trait Host {
    /// Journals one turn's model call before any of its tool calls runs.
    fn invoked(&mut self, turn: u32, record: &CallRecord) -> Result<(), Halt>;
    /// Journals the results of a turn's tool calls before the next turn reads them.
    fn results(&mut self, turn: u32, calls: &[ToolCall], results: &[String]) -> Result<(), Halt>;
    /// Whether the owner stopped the run.
    fn stopped(&mut self) -> bool;
    /// Whether the run's spending cap is reached; a run stops before a call it cannot pay for.
    fn over_budget(&mut self) -> bool;
    fn now(&mut self) -> UtcNanos;
    /// A fresh ULID for a call.
    fn call_id(&mut self) -> String;
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum End {
    /// The model answered without a tool call; `watch` is its summary's list.
    Answered { watch: Vec<String> },
    /// The owner stopped it.
    Stopped,
    /// The spending cap was reached.
    Budget,
    /// The gateway answered the call with an outcome other than `ok`.
    Model(Outcome),
    /// The gateway refused the request itself, by its code.
    Refused(&'static str),
    /// The caller could not journal a record.
    Halted,
    /// The run reached the entry's turn ceiling.
    Ceiling,
}

impl End {
    /// The code the run's end record carries.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Answered { .. } => "answered",
            Self::Stopped => "stopped",
            Self::Budget => "budget",
            Self::Model(_) => "model_error",
            Self::Refused(_) => "refused",
            Self::Halted => "halted",
            Self::Ceiling => "ceiling",
        }
    }
}

/// Runs one chat run to its end.
pub fn run(
    spec: &RunSpec,
    max_turns: u32,
    model: &mut dyn Model,
    tools: &mut dyn Toolbox,
    host: &mut dyn Host,
) -> End {
    let offered = menu(&spec.kind);
    let mut inputs = spec.inputs.clone();
    inputs.insert("kind".to_owned(), spec.kind.clone());
    let mut turns: Vec<Turn> = Vec::new();
    for turn in 0..max_turns {
        if host.stopped() {
            return End::Stopped;
        }
        if host.over_budget() {
            return End::Budget;
        }
        let now = host.now();
        let deadline = UtcNanos::from_parts(
            now.secs()
                .saturating_add(i64::try_from(spec.call_deadline_ms / 1_000).unwrap_or(i64::MAX)),
            now.nanos(),
        )
        .unwrap_or(now);
        let request = CallRequest {
            call_id: host.call_id(),
            workspace_id: spec.workspace_id.clone(),
            agent_id: spec.agent_id.clone(),
            purpose: spec.purpose(),
            model: spec.model.clone(),
            inputs: inputs.clone(),
            as_of: now,
            deadline,
            observations: Vec::new(),
            key_owner: spec.key_owner,
            run_kind: Some(spec.kind.clone()),
            turns: turns.clone(),
            tools: offered.clone(),
            spend_ceiling: None,
        };
        let record = match model.call(&request) {
            Ok(record) => record,
            Err(refused) => return End::Refused(refused.code()),
        };
        if host.invoked(turn, &record).is_err() {
            return End::Halted;
        }
        if record.outcome != Outcome::Ok {
            return End::Model(record.outcome);
        }
        if record.tool_calls.is_empty() {
            return End::Answered {
                watch: watch(record.output.as_ref()),
            };
        }
        let results: Vec<String> = record
            .tool_calls
            .iter()
            .map(|call| tools.run(call))
            .collect();
        if host.results(turn, &record.tool_calls, &results).is_err() {
            return End::Halted;
        }
        let Some(response) = record.response.clone() else {
            return End::Model(Outcome::SchemaInvalid);
        };
        turns.push(Turn { response, results });
    }
    End::Ceiling
}

/// The symbols of a summary's `watch` list; anything else reads as none.
fn watch(output: Option<&Value>) -> Vec<String> {
    output
        .and_then(|o| o.get("watch"))
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

/// A tool call's arguments, read as the text member `name`, empty when absent.
pub fn arg_text<'a>(call: &'a ToolCall, name: &str) -> &'a str {
    call.arguments
        .get(name)
        .and_then(Value::as_str)
        .unwrap_or_default()
}

/// A JSON document a tool argument carries as text, parsed strictly; `None` when it does not parse.
pub fn parse_document(text: &str) -> Option<Value> {
    parse(text.as_bytes()).ok()
}
