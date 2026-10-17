//! Pinned tools and the calls a model makes to them (inference spec v0.4 INF-5, §3.1, §3.2;
//! DEC-2203, DEC-2204). An `agentic` research entry pins its tool declarations inside its content
//! hash; a turn offers some of them; a reply's tool calls are validated against the offered
//! declarations and returned as data. The gateway runs no tool: it names no data-plane, journal, or
//! source port, so it cannot.

use std::collections::BTreeMap;

use mandate_canon::{Key, Value, parse, to_canonical};
use serde_json::{Map, Value as Json, json};

use crate::registry::{int, object};
use crate::schema;

/// The longest name a provider takes for a function (OpenAI-compatible APIs: `[a-zA-Z0-9_-]{1,64}`).
pub const MAX_WIRE_NAME: usize = 64;

/// What a tool does (agent harness spec §6.10): reads, thinks, or proposes. The gateway treats
/// every class alike; the harness runs or mediates each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ToolClass {
    Read,
    Think,
    Action,
}

impl ToolClass {
    pub fn code(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Think => "think",
            Self::Action => "action",
        }
    }
}

/// One pinned tool declaration (inference spec §4.1): a name from the harness's set, its class, a
/// description the model reads, its argument schema in the gateway's schema language (an object),
/// its result cap, and the run kinds whose menus may offer it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDeclaration {
    pub name: String,
    pub class: ToolClass,
    pub description: String,
    pub arguments: Value,
    pub result_cap_bytes: u64,
    pub menu: Vec<String>,
}

impl ToolDeclaration {
    /// The canonical object the content hash covers.
    pub fn canonical(&self) -> Value {
        object([
            ("arguments", self.arguments.clone()),
            ("class", Value::Str(self.class.code().to_owned())),
            ("description", Value::Str(self.description.clone())),
            (
                "menu",
                Value::Array(self.menu.iter().cloned().map(Value::Str).collect()),
            ),
            ("name", Value::Str(self.name.clone())),
            ("result_cap_bytes", int(self.result_cap_bytes)),
        ])
    }

    /// Whether the declaration is one a registry may pin: a well-formed name, a non-empty
    /// description, an object argument schema in the gateway's language, a non-zero result cap,
    /// and a strictly sorted, non-empty menu.
    pub fn well_formed(&self) -> bool {
        let object_schema = self.arguments.get("type").and_then(Value::as_str) == Some("object")
            && schema::well_formed(&self.arguments);
        let menu_sorted = self.menu.windows(2).all(|w| matches!(w, [a, b] if a < b));
        wire_name(&self.name).is_some()
            && !self.description.trim().is_empty()
            && object_schema
            && self.result_cap_bytes > 0
            && !self.menu.is_empty()
            && self.menu.iter().all(|kind| !kind.is_empty())
            && menu_sorted
    }
}

/// The name a declared tool goes by on the wire, or `None` when `name` is not a declared name: one
/// or two segments joined by `.`, each a lowercase letter followed by lowercase letters and
/// digits, in words joined by single underscores (so no segment holds `__`). The wire name joins
/// the segments with `__` (DEC-2204), which no declared name can contain, so the mapping is
/// one-to-one, and fits [`MAX_WIRE_NAME`].
pub fn wire_name(name: &str) -> Option<String> {
    let segments: Vec<&str> = name.split('.').collect();
    if segments.len() > 2 || !segments.iter().all(|s| segment_ok(s)) {
        return None;
    }
    let wire = segments.join("__");
    (wire.len() <= MAX_WIRE_NAME).then_some(wire)
}

/// The declared name of a wire name, or `None` when no declared name maps to it.
pub fn declared_name(wire: &str) -> Option<String> {
    let name = wire.replace("__", ".");
    (wire_name(&name).as_deref() == Some(wire)).then_some(name)
}

fn segment_ok(segment: &str) -> bool {
    let mut words = segment.split('_');
    let first_ok = words.next().is_some_and(|w| {
        w.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
            && w.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    });
    first_ok
        && words.all(|w| {
            !w.is_empty()
                && w.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

/// The bounds of an agentic run, pinned in its entry (agent harness spec §6.11). The gateway
/// enforces `max_turns` on the transcript and `max_tool_calls_per_turn` on each reply; the
/// harness enforces the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopBounds {
    pub max_turns: u32,
    pub max_tool_calls: u32,
    pub max_tool_calls_per_turn: u32,
    pub max_theses: u32,
    pub max_result_bytes: u64,
    pub max_run_result_bytes: u64,
    pub max_repeat_denials: u32,
    pub max_run_s: u64,
    pub tool_timeout_s: u64,
}

impl LoopBounds {
    pub fn canonical(&self) -> Value {
        object([
            (
                "max_repeat_denials",
                int(u64::from(self.max_repeat_denials)),
            ),
            ("max_result_bytes", int(self.max_result_bytes)),
            ("max_run_result_bytes", int(self.max_run_result_bytes)),
            ("max_run_s", int(self.max_run_s)),
            ("max_theses", int(u64::from(self.max_theses))),
            ("max_tool_calls", int(u64::from(self.max_tool_calls))),
            (
                "max_tool_calls_per_turn",
                int(u64::from(self.max_tool_calls_per_turn)),
            ),
            ("max_turns", int(u64::from(self.max_turns))),
            ("tool_timeout_s", int(self.tool_timeout_s)),
        ])
    }

    /// Every bound non-zero, and one turn's calls at most the run's (so the run's are non-zero
    /// too), and one read's bytes at most the run's.
    pub fn well_formed(&self) -> bool {
        self.max_turns > 0
            && self.max_tool_calls_per_turn > 0
            && self.max_tool_calls_per_turn <= self.max_tool_calls
            && self.max_theses > 0
            && self.max_result_bytes > 0
            && self.max_result_bytes <= self.max_run_result_bytes
            && self.max_repeat_denials > 0
            && self.max_run_s > 0
            && self.tool_timeout_s > 0
    }
}

/// How an agentic run gives its opinion (agent harness spec §6.9): its last reply is the structured
/// output, judged once, or the model proposes through action tools and its last reply is a summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OutputKind {
    FinalOutput,
    ActionTools,
}

impl OutputKind {
    pub fn code(self) -> &'static str {
        match self {
            Self::FinalOutput => "final_output",
            Self::ActionTools => "action_tools",
        }
    }
}

/// The members a structured output schema holds and a summary-only schema never does (agent harness
/// spec §6.5's `{theses, invalidations}`).
pub const STRUCTURED_MEMBERS: [&str; 2] = ["theses", "invalidations"];

/// The only members a summary-only schema may have: agent harness spec §6.12's `watch` list, the
/// part of a run's summary the model writes.
pub const SUMMARY_MEMBERS: [&str; 1] = ["watch"];

/// Whether `schema` is a structured output's: an object with a `theses` or `invalidations` member.
pub fn structured(schema: &Value) -> bool {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .is_some_and(|members| {
            members
                .keys()
                .any(|k| STRUCTURED_MEMBERS.contains(&k.as_str()))
        })
}

/// Whether `schema` is summary-only: an object whose members are all in [`SUMMARY_MEMBERS`], with
/// no structured member anywhere inside it. Anything else is refused for an `action_tools` entry,
/// the reading that only refuses more (DEC-176).
pub fn summary_only(schema: &Value) -> bool {
    let members = schema.get("properties").and_then(Value::as_object);
    schema.get("type").and_then(Value::as_str) == Some("object")
        && members.is_some_and(|m| {
            !m.is_empty() && m.keys().all(|k| SUMMARY_MEMBERS.contains(&k.as_str()))
        })
        && !mentions_structured(schema)
}

/// Whether a structured member's name appears as a property anywhere in `schema`.
fn mentions_structured(schema: &Value) -> bool {
    let here = schema
        .get("properties")
        .and_then(Value::as_object)
        .is_some_and(|m| {
            m.iter().any(|(k, inner)| {
                STRUCTURED_MEMBERS.contains(&k.as_str()) || mentions_structured(inner)
            })
        });
    here || schema.get("items").is_some_and(mentions_structured)
}

/// An agentic entry's pinned loop (inference spec §4.1): how it gives its opinion, its tools,
/// strictly sorted by name, its bounds, and the final reply's schema for each run kind it serves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agentic {
    pub output: OutputKind,
    pub tools: Vec<ToolDeclaration>,
    pub loop_bounds: LoopBounds,
    pub output_schemas: BTreeMap<String, Value>,
}

impl Agentic {
    pub fn tool(&self, name: &str) -> Option<&ToolDeclaration> {
        self.tools.iter().find(|t| t.name == name)
    }

    /// Whether the loop is one a registry may pin: at least one run kind, each kind's schema in the
    /// gateway's language, every declaration well formed with a menu of kinds the entry serves,
    /// names strictly sorted, the bounds well formed, and the output kind one of agent harness spec
    /// §6.9's two combinations: `final_output` with no action tool and every schema structured, or
    /// `action_tools` with an action tool and every schema summary-only, so no action call can
    /// reach a run with no path to mediate it.
    pub fn well_formed(&self) -> bool {
        let actions = self.tools.iter().any(|t| t.class == ToolClass::Action);
        let combination = match self.output {
            OutputKind::FinalOutput => !actions && self.output_schemas.values().all(structured),
            OutputKind::ActionTools => actions && self.output_schemas.values().all(summary_only),
        };
        let sorted = self
            .tools
            .windows(2)
            .all(|w| matches!(w, [a, b] if a.name < b.name));
        !self.output_schemas.is_empty()
            && self
                .output_schemas
                .iter()
                .all(|(kind, s)| Key::new(kind).is_ok() && schema::well_formed(s))
            && sorted
            && self.tools.iter().all(|t| {
                t.well_formed() && t.menu.iter().all(|k| self.output_schemas.contains_key(k))
            })
            && self.loop_bounds.well_formed()
            && combination
    }
}

/// One validated tool call (inference spec §3.2): its position in the reply, a declared and offered
/// tool's name, and arguments that fit that tool's pinned schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub index: u32,
    pub name: String,
    pub arguments: Value,
}

impl ToolCall {
    pub fn canonical(&self) -> Value {
        object([
            ("arguments", self.arguments.clone()),
            ("index", int(u64::from(self.index))),
            ("name", Value::Str(self.name.clone())),
        ])
    }
}

/// A tool call as a provider adapter read it from a reply, before validation: either part missing
/// when the reply's call had no such member of the right type.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WireCall {
    pub name: Option<String>,
    pub arguments: Option<String>,
}

/// Why a reply's tool calls were refused; each is `schema_invalid` (inference spec §3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallsRefused {
    NoneOffered,
    TooMany,
    NotOffered,
    BadArguments,
}

/// Validates a reply's calls against the tools offered in the turn, in reply order: at most
/// `per_turn` of them, each naming an offered tool with arguments that parse and fit its schema.
pub fn validate(
    calls: &[WireCall],
    offered: &[&ToolDeclaration],
    per_turn: u32,
) -> Result<Vec<ToolCall>, CallsRefused> {
    if offered.is_empty() {
        return Err(CallsRefused::NoneOffered);
    }
    if u32::try_from(calls.len()).map_or(true, |n| n > per_turn) {
        return Err(CallsRefused::TooMany);
    }
    calls
        .iter()
        .zip(0u32..)
        .map(|(call, index)| {
            let declared = call
                .name
                .as_deref()
                .and_then(declared_name)
                .ok_or(CallsRefused::NotOffered)?;
            let tool = offered
                .iter()
                .find(|t| t.name == declared)
                .ok_or(CallsRefused::NotOffered)?;
            let arguments = call
                .arguments
                .as_deref()
                .and_then(|text| parse(text.as_bytes()).ok())
                .filter(|value| schema::fits(&tool.arguments, value))
                .ok_or(CallsRefused::BadArguments)?;
            Ok(ToolCall {
                index,
                name: declared,
                arguments,
            })
        })
        .collect()
}

/// One earlier turn of a run, as the caller hands it back (inference spec §3.1 `turns`): the
/// reply's bytes, by its response artifact, and the caller's result for each of that reply's tool
/// calls, in call order, as the canonical JSON text the model is shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    pub response: Vec<u8>,
    pub results: Vec<String>,
}

/// One earlier turn as the prompt holds it: the reply's validated calls, and each result inside
/// its data section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptTurn {
    pub calls: Vec<ToolCall>,
    pub results: Vec<String>,
}

impl TranscriptTurn {
    pub fn canonical(&self) -> Value {
        object([
            (
                "calls",
                Value::Array(self.calls.iter().map(ToolCall::canonical).collect()),
            ),
            (
                "results",
                Value::Array(self.results.iter().cloned().map(Value::Str).collect()),
            ),
        ])
    }
}

/// The id a transcript gives the call at `index` of turn `turn` (from 1): `t{turn}c{index}`.
/// Synthesized, never the provider's, so a reply cannot steer it and a re-run renders the same
/// request (DEC-2204, HI-3).
pub fn call_id(turn: usize, index: u32) -> String {
    format!("t{turn}c{index}")
}

/// A canonical value as JSON text, for a wire request.
pub fn canonical_text(value: &Value) -> String {
    String::from_utf8(to_canonical(value)).unwrap_or_default()
}

/// The JSON Schema a provider is given for an argument schema in the gateway's language. It only
/// describes; the gateway's own validator decides (INF-5).
pub fn wire_schema(schema: &Value) -> Json {
    let member = |name: &str| schema.get(name).and_then(Value::as_int);
    match schema.get("type").and_then(Value::as_str) {
        Some("object") => {
            let properties = schema.get("properties").and_then(Value::as_object);
            let mut props = Map::new();
            let mut required = Vec::new();
            for (name, inner) in properties.into_iter().flatten() {
                props.insert(name.as_str().to_owned(), wire_schema(inner));
                required.push(json!(name.as_str()));
            }
            json!({"type": "object", "properties": props, "required": required, "additionalProperties": false})
        }
        Some("array") => json!({
            "type": "array",
            "items": schema.get("items").map_or(Json::Null, wire_schema),
            "maxItems": member("max_items"),
        }),
        Some("string") => match schema.get("enum").and_then(Value::as_array) {
            Some(options) => json!({
                "type": "string",
                "enum": options.iter().filter_map(Value::as_str).collect::<Vec<_>>(),
            }),
            None => json!({"type": "string", "maxLength": member("max_length")}),
        },
        Some("integer") => json!({
            "type": "integer",
            "minimum": member("minimum"),
            "maximum": member("maximum"),
        }),
        Some("decimal") => json!({
            "type": "string",
            "description": "a decimal number written as text, such as \"0.6\"",
        }),
        Some("boolean") => json!({"type": "boolean"}),
        _ => Json::Null,
    }
}
