//! The chat's records (journal spec §9.39, [DEC-2293](../../../docs/project/decisions/DEC-2293.md)),
//! on the control stream, which the workspace API writes:
//! - `ChatMessagePosted`: a message the owner or the agent posts;
//! - `ChatTurnRecorded`: one model call and the tool calls it asked for, before any of them runs;
//! - `ChatToolResultsRecorded`: those calls' results, before the next call reads them;
//! - `ChatRunEnded`: how a run ended;
//! - `ChatRunStopRequested`: the owner's Stop;
//! - `ChatSettingsChanged`: an agent's chat model and daily cap;
//! - `AgentGoalsUpdated`: what the agent keeps for its owner.
//!
//! Every rule only refuses a draft.

use mandate_canon::Value;

use crate::schema::Ty;
use crate::{Invalid, InvalidReason, StreamType};

/// The control-stream event types §9.39 adds.
pub(crate) const CONTROL: [&str; 7] = [MESSAGE, TURN, RESULTS, ENDED, STOP, SETTINGS, GOALS];

const MESSAGE: &str = "ChatMessagePosted";
const TURN: &str = "ChatTurnRecorded";
const RESULTS: &str = "ChatToolResultsRecorded";
const ENDED: &str = "ChatRunEnded";
const STOP: &str = "ChatRunStopRequested";
const SETTINGS: &str = "ChatSettingsChanged";
const GOALS: &str = "AgentGoalsUpdated";

/// The run kinds of the chat's registry entry.
const KINDS: [&str; 3] = ["agent", "proactive", "setup"];

/// How a run ends.
const ENDS: [&str; 7] = [
    "answered",
    "stopped",
    "budget",
    "model_error",
    "refused",
    "halted",
    "ceiling",
];

/// The §9.39 schema of `event_type` at `schema_version` on `stream`, or `None`.
pub(crate) fn schema(
    event_type: &str,
    schema_version: u64,
    stream: StreamType,
) -> Option<&'static Ty> {
    match (event_type, schema_version, stream) {
        (MESSAGE, 1, StreamType::Control) => Some(&MESSAGE_POSTED),
        (TURN, 1, StreamType::Control) => Some(&TURN_RECORDED),
        (RESULTS, 1, StreamType::Control) => Some(&RESULTS_RECORDED),
        (ENDED, 1, StreamType::Control) => Some(&RUN_ENDED),
        (STOP, 1, StreamType::Control) => Some(&STOP_REQUESTED),
        (SETTINGS, 1, StreamType::Control) => Some(&SETTINGS_CHANGED),
        (GOALS, 1, StreamType::Control) => Some(&GOALS_UPDATED),
        _ => None,
    }
}

/// Rules 330 to 334 on a normalized payload:
/// - 330: a thread names exactly one of an agent and a setup;
/// - 331: the owner's message, settings and Stop are a user's; the agent's message, turns,
///   results, ends and goals are the system's;
/// - 332: an owner's message carries text and no card, and is never proactive;
/// - 333: an agent's message carries text, a card, or both;
/// - 334: a setup's turn is of the `setup` kind and an agent's is not.
pub(crate) fn rules(
    event_type: &str,
    payload: &Value,
    actor: Option<&Value>,
) -> Result<(), Invalid> {
    if !CONTROL.contains(&event_type) {
        return Ok(());
    }
    let kind = actor
        .and_then(|a| a.get("kind"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let null = |member: &str| matches!(payload.get(member), None | Some(Value::Null));
    if matches!(event_type, MESSAGE | TURN | RESULTS | ENDED | STOP) {
        ensure(null("agent_id") != null("setup_id"), "payload.setup_id")?;
    }
    let owners = match event_type {
        MESSAGE => payload.get("author").and_then(Value::as_str) == Some("owner"),
        STOP | SETTINGS => true,
        _ => false,
    };
    ensure((kind == "user") == owners, "actor.kind")?;
    ensure(owners || kind == "system", "actor.kind")?;
    if event_type == MESSAGE {
        let text = !null("text");
        let card = !null("card");
        let proactive = payload.get("proactive") == Some(&Value::Bool(true));
        if owners {
            ensure(text && !card && !proactive, "payload.card")?;
        } else {
            ensure(text || card, "payload.text")?;
        }
    }
    if event_type == TURN {
        let setup = payload.get("kind").and_then(Value::as_str) == Some("setup");
        ensure(setup == !null("setup_id"), "payload.kind")?;
    }
    Ok(())
}

fn ensure(holds: bool, at: &'static str) -> Result<(), Invalid> {
    if holds {
        Ok(())
    } else {
        Err(Invalid::new(InvalidReason::Schema, at))
    }
}

static MESSAGE_POSTED: Ty = Ty::Record(&[
    ("agent_id", Ty::Nullable(&Ty::Ident)),
    ("setup_id", Ty::Nullable(&Ty::Ident)),
    ("member", Ty::Str),
    ("author", Ty::OneOf(&["owner", "agent"])),
    ("text", Ty::Nullable(&Ty::Str)),
    ("card", Ty::Nullable(&Ty::OpenObject)),
    (
        "sources",
        Ty::List(&Ty::Record(&[("label", Ty::Str), ("ref", Ty::Str)])),
    ),
    ("run_id", Ty::Nullable(&Ty::Ident)),
    ("proactive", Ty::Bool),
]);

static TURN_RECORDED: Ty = Ty::Record(&[
    ("agent_id", Ty::Nullable(&Ty::Ident)),
    ("setup_id", Ty::Nullable(&Ty::Ident)),
    ("member", Ty::Str),
    ("run_id", Ty::Ident),
    ("turn", Ty::Int),
    ("kind", Ty::OneOf(&KINDS)),
    ("call_id", Ty::Ulid),
    (
        "model",
        Ty::Record(&[
            ("model_id", Ty::Str),
            ("model_version", Ty::Str),
            ("content_hash", Ty::DigestRef),
        ]),
    ),
    ("outcome", Ty::Str),
    ("cost_usd", Ty::Decimal),
    ("prompt_ref", Ty::Nullable(&Ty::DigestRef)),
    ("response_ref", Ty::Nullable(&Ty::DigestRef)),
    (
        "calls",
        Ty::List(&Ty::Record(&[
            ("name", Ty::Str),
            ("arguments", Ty::OpenObject),
        ])),
    ),
]);

static RESULTS_RECORDED: Ty = Ty::Record(&[
    ("agent_id", Ty::Nullable(&Ty::Ident)),
    ("setup_id", Ty::Nullable(&Ty::Ident)),
    ("member", Ty::Str),
    ("run_id", Ty::Ident),
    ("turn", Ty::Int),
    (
        "results",
        Ty::List(&Ty::Record(&[("name", Ty::Str), ("result", Ty::Str)])),
    ),
]);

static RUN_ENDED: Ty = Ty::Record(&[
    ("agent_id", Ty::Nullable(&Ty::Ident)),
    ("setup_id", Ty::Nullable(&Ty::Ident)),
    ("member", Ty::Str),
    ("run_id", Ty::Ident),
    ("reason", Ty::OneOf(&ENDS)),
]);

static STOP_REQUESTED: Ty = Ty::Record(&[
    ("agent_id", Ty::Nullable(&Ty::Ident)),
    ("setup_id", Ty::Nullable(&Ty::Ident)),
    ("member", Ty::Str),
    ("run_id", Ty::Ident),
]);

static SETTINGS_CHANGED: Ty = Ty::Record(&[
    ("agent_id", Ty::Ident),
    ("chat_model", Ty::Str),
    ("daily_cap_usd", Ty::Decimal),
]);

static GOALS_UPDATED: Ty = Ty::Record(&[
    ("agent_id", Ty::Ident),
    ("run_id", Ty::Ident),
    (
        "goals",
        Ty::List(&Ty::Record(&[
            ("text", Ty::Str),
            ("status", Ty::OneOf(&["active", "done"])),
        ])),
    ),
    ("plan", Ty::Nullable(&Ty::Str)),
    ("watching", Ty::List(&Ty::Str)),
]);
