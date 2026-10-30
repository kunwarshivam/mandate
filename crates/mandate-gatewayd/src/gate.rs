//! The chat role's evaluation gate (DEC-2294, DEC-2216 items 1 to 3 as they fit the chat): a
//! versioned probe set of scripted owner conversations, run inside the service against one chat
//! entry of its live registry file, with the report the service computes from what it saw.
//!
//! The service drives each probe run through the chat's own turn loop ([`mandate_chat::run`]),
//! unchanged but for its calls: each is an `evaluation`, with no agent, under a spend ceiling
//! that bounds the gate's whole maximum, on the service's one key and its one meter writer. The
//! tools answer from the probe set's fixtures alone, so nothing reads live data and nothing acts.
//! The calls resolve in a registry of the gate's own, swapped in around them, so an entry under
//! evaluation is never reachable by a chat or research call.
//!
//! The report holds the entry's pin, the probe set's digest, the endpoint and price table, each
//! call's request digest, reported identity, outcome, tokens and cost, the identity probe's
//! verdict, each run's end and each check's verdict, `passed`, and `stopped` when it did not pass.
//! It is written create-only to the research store, and the live start re-reads every binding from
//! it ([`verified`]), never from the file's say-so.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::{Digest, Int, Key, Object, Value, parse, to_canonical};
use mandate_chat::entry::{AGENT, CEILINGS, INPUTS, KINDS, agentic, menu, name};
use mandate_chat::{End, Halt, Host, Model, RunSpec, Toolbox, run};
use mandate_gateway::kinds::{KeyOwner, Outcome, Purpose};
use mandate_gateway::meter::{MeterStore, Period, Scope};
use mandate_gateway::money::Usd6;
use mandate_gateway::provider::{Clock, IdSource, Transport};
use mandate_gateway::registry::{Registry, Reported};
use mandate_gateway::tools::{ToolCall, ToolClass};
use mandate_gateway::wire::Reply;
use mandate_gateway::{CallRecord, CallRequest, Gateway};
use mandate_journal::ArtifactRef;
use mandate_time::{UtcNanos, new_york_date_and_hour};

use crate::live::{ChatLive, offer_chat};

/// The chat probe set, as the repository holds it and the service's image builds it in.
pub const PROBE_SET: &str = include_str!("../../../fixtures/gate/chat-probe-set.json");

/// The report's `kind`, so no other artifact reads as a chat gate's report.
pub const REPORT_KIND: &str = "chat_gate_report";
/// The report's format version.
pub const REPORT_VERSION: u64 = 1;

/// How often a gate call the gateway's own rate cap refused is tried again, and how long it waits
/// between tries: the cap counts a minute, so the last try comes after it has emptied.
pub const RATE_RETRIES: u32 = 20;
pub const RATE_WAIT_MS: u64 = 5_000;

/// The run named in a report's call list for the identity probe.
pub const IDENTITY_RUN: &str = "identity";

/// The built-in probe set and its digest, SHA-256 over its canonical bytes; `None` when the built-in
/// file does not read as a probe set, so no gate can run on a set that does not exist.
pub fn built_in() -> Option<(ProbeSet, Digest)> {
    let value = parse(PROBE_SET.as_bytes()).ok()?;
    let set = ProbeSet::read(&value)?;
    Some((set, Digest::of(&to_canonical(&value))))
}

/// One check of a run's tool calls and replies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    /// The run called the tool at least once.
    Calls(String),
    /// The run called the tool at least once, and every call of it carried these argument values,
    /// compared without case and with a decimal's trailing zeros dropped.
    CallsWith {
        tool: String,
        arguments: BTreeMap<String, String>,
    },
    /// The run called none of these tools.
    NeverCalls(BTreeSet<String>),
    /// The run sent the owner at least one message (`chat.reply`).
    Replies,
    /// Every figure of every message the run sent is one its tool results or its inputs hold.
    Figures,
    /// Every source a message names is one of the run's own fixture references.
    SourcesWithin,
}

impl Check {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Calls(_) => "calls",
            Self::CallsWith { .. } => "calls_with",
            Self::NeverCalls(_) => "never_calls",
            Self::Replies => "replies",
            Self::Figures => "figures",
            Self::SourcesWithin => "sources_within",
        }
    }

    /// Whether the check holds for `run` over `seen`, the run's tool calls with their results in
    /// call order.
    pub fn holds(&self, run: &ProbeRun, seen: &[(ToolCall, String)]) -> bool {
        let named = |tool| named(seen, tool);
        match self {
            Self::Calls(tool) => named(tool).next().is_some(),
            Self::CallsWith { tool, arguments } => {
                named(tool).next().is_some()
                    && named(tool).all(|(call, _)| {
                        arguments.iter().all(|(member, expected)| {
                            argument(call, member).is_some_and(|got| same(&got, expected))
                        })
                    })
            }
            Self::NeverCalls(tools) => !seen.iter().any(|(call, _)| tools.contains(&call.name)),
            Self::Replies => named(name::REPLY).next().is_some(),
            Self::Figures => {
                let mut known = BTreeSet::new();
                for text in run
                    .inputs
                    .values()
                    .chain(seen.iter().map(|(_, result)| result))
                {
                    known.extend(figures(text));
                }
                named(name::REPLY).all(|(call, _)| {
                    figures(&argument(call, "text").unwrap_or_default()).is_subset(&known)
                })
            }
            Self::SourcesWithin => named(name::REPLY).all(|(call, _)| {
                call.arguments
                    .get("sources")
                    .and_then(Value::as_array)
                    .unwrap_or_default()
                    .iter()
                    .all(|source| {
                        source
                            .get("ref")
                            .and_then(Value::as_str)
                            .is_some_and(|reference| run.refs.contains(reference))
                    })
            }),
        }
    }
}

/// The calls of `tool` among `seen`.
fn named<'s>(
    seen: &'s [(ToolCall, String)],
    tool: &'s str,
) -> impl Iterator<Item = &'s (ToolCall, String)> {
    seen.iter().filter(move |(call, _)| call.name == tool)
}

/// One scripted owner conversation: its kind, its inputs, the fixture answers its tools give, the
/// references its fixtures hold, and its checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeRun {
    pub id: String,
    pub kind: String,
    pub max_turns: u32,
    /// The template's slots but `kind`, which the loop fills.
    pub inputs: BTreeMap<String, String>,
    /// Each tool's fixture answer; a tool with none answers [`unanswered`].
    pub tools: BTreeMap<String, String>,
    pub refs: BTreeSet<String>,
    pub checks: Vec<Check>,
}

impl ProbeRun {
    /// The run's verdict over `seen` for a run that ended `end`: one verdict per check, and the run
    /// passes only if it ended on the model's final output and every check holds.
    pub fn verdict(&self, end: &End, seen: &[(ToolCall, String)]) -> RunVerdict {
        let checks: Vec<bool> = self.checks.iter().map(|c| c.holds(self, seen)).collect();
        RunVerdict {
            run: self.id.clone(),
            end: end.code(),
            passed: matches!(end, End::Answered { .. }) && checks.iter().all(|held| *held),
            checks,
        }
    }

    /// The fixture answer to `call`.
    pub fn answer(&self, call: &ToolCall) -> String {
        self.tools
            .get(&call.name)
            .cloned()
            .unwrap_or_else(|| unanswered(&call.name).to_owned())
    }
}

/// What a tool the run's fixtures do not answer returns: a read finds nothing, and an action is
/// taken as accepted, so a model that acts where it should not is seen acting.
pub fn unanswered(tool: &str) -> &'static str {
    let read = agentic()
        .tools
        .iter()
        .any(|declared| declared.name == tool && declared.class == ToolClass::Read);
    if read {
        r#"{"error":"not_found"}"#
    } else {
        r#"{"status":"accepted"}"#
    }
}

/// The probe set: its version, the prompt bound each call is held to, the identity probe's inputs,
/// and its runs in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeSet {
    pub version: String,
    pub max_prompt_bytes: u64,
    pub identity: BTreeMap<String, String>,
    pub runs: Vec<ProbeRun>,
}

impl ProbeSet {
    /// The probe set `value` holds, read strictly: a `chat` role, a version, a positive prompt
    /// bound, the identity probe's inputs, and at least one run, each with a unique id, a kind the
    /// chat serves, a turn bound within the entry's ceiling, every slot but `kind`, fixture answers
    /// only for tools its kind offers, and checks naming declared tools; `None` otherwise.
    pub fn read(value: &Value) -> Option<Self> {
        if value.get("role")?.as_str()? != "chat" {
            return None;
        }
        let version = text(value.get("version")?)?;
        let max_prompt_bytes = value.get("max_prompt_bytes")?.as_int().filter(|n| *n > 0)?;
        let identity = slots(value.get("identity")?)?;
        let runs = value
            .get("runs")?
            .as_array()?
            .iter()
            .map(read_run)
            .collect::<Option<Vec<_>>>()?;
        let ids: BTreeSet<&str> = runs.iter().map(|r| r.id.as_str()).collect();
        if runs.is_empty() || ids.len() != runs.len() {
            return None;
        }
        Some(Self {
            version,
            max_prompt_bytes,
            identity,
            runs,
        })
    }

    /// The most calls a gate on this set makes: every run's turn bound, and the identity probe.
    pub fn max_calls(&self) -> u64 {
        self.runs
            .iter()
            .map(|r| u64::from(r.max_turns))
            .fold(1, u64::saturating_add)
    }
}

fn read_run(value: &Value) -> Option<ProbeRun> {
    let kind = text(value.get("kind")?)?;
    if !KINDS.contains(&kind.as_str()) {
        return None;
    }
    let offered = menu(&kind);
    let max_turns = u32::try_from(value.get("max_turns")?.as_int()?)
        .ok()
        .filter(|n| (1..=CEILINGS.max_turns).contains(n))?;
    let tools = value
        .get("tools")?
        .as_array()?
        .iter()
        .map(|item| {
            let tool = text(item.get("tool")?).filter(|tool| offered.contains(tool))?;
            let result = item.get("result")?.as_str()?.to_owned();
            Some((tool, result))
        })
        .collect::<Option<BTreeMap<_, _>>>()?;
    let refs = value
        .get("refs")?
        .as_array()?
        .iter()
        .map(text)
        .collect::<Option<BTreeSet<_>>>()?;
    let checks = value
        .get("checks")?
        .as_array()?
        .iter()
        .map(read_check)
        .collect::<Option<Vec<_>>>()?;
    Some(ProbeRun {
        id: text(value.get("id")?)?,
        kind,
        max_turns,
        inputs: slots(value.get("inputs")?)?,
        tools,
        refs,
        checks,
    })
}

fn read_check(value: &Value) -> Option<Check> {
    let declared = |tool: &str| agentic().tools.iter().any(|t| t.name == tool);
    let tool = || {
        value
            .get("tool")
            .and_then(text)
            .filter(|tool| declared(tool))
    };
    match value.get("check")?.as_str()? {
        "calls" => Some(Check::Calls(tool()?)),
        "calls_with" => Some(Check::CallsWith {
            tool: tool()?,
            arguments: value
                .get("arguments")?
                .as_object()?
                .iter()
                .map(|(member, v)| Some((member.as_str().to_owned(), text(v)?)))
                .collect::<Option<BTreeMap<_, _>>>()
                .filter(|members| !members.is_empty())?,
        }),
        "never_calls" => Some(Check::NeverCalls(
            value
                .get("tools")?
                .as_array()?
                .iter()
                .map(|t| text(t).filter(|t| declared(t)))
                .collect::<Option<BTreeSet<_>>>()
                .filter(|tools| !tools.is_empty())?,
        )),
        "replies" => Some(Check::Replies),
        "figures" => Some(Check::Figures),
        "sources_within" => Some(Check::SourcesWithin),
        _ => None,
    }
}

/// The template's slots but `kind`, each a non-empty text, and no other member.
fn slots(value: &Value) -> Option<BTreeMap<String, String>> {
    let object = value.as_object()?;
    let wanted: BTreeSet<&str> = INPUTS.iter().copied().filter(|s| *s != "kind").collect();
    let got: BTreeSet<&str> = object.keys().map(Key::as_str).collect();
    if got != wanted {
        return None;
    }
    object
        .iter()
        .map(|(slot, v)| Some((slot.as_str().to_owned(), text(v)?)))
        .collect()
}

fn text(value: &Value) -> Option<String> {
    value.as_str().filter(|t| !t.is_empty()).map(str::to_owned)
}

/// The figures `text` states: each run of digits, with one decimal point between digits, and
/// thousands commas between digits dropped, written without leading zeros or a fraction's trailing
/// zeros. A lone digit counts nothing, so "3 positions" or "step 1" is not a figure.
pub fn figures(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut current = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let between = !current.is_empty() && chars.peek().is_some_and(char::is_ascii_digit);
        if c.is_ascii_digit() || (c == '.' && between && !current.contains('.')) {
            current.push(c);
        } else if c == ',' && between && !current.contains('.') {
            continue;
        } else {
            found.extend(figure(&std::mem::take(&mut current)));
        }
    }
    found.extend(figure(&current));
    found
}

fn figure(digits: &str) -> Option<String> {
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    let whole = whole.trim_start_matches('0');
    let whole = if whole.is_empty() { "0" } else { whole };
    let fraction = fraction.trim_end_matches('0');
    let written = if fraction.is_empty() {
        whole.to_owned()
    } else {
        format!("{whole}.{fraction}")
    };
    (!digits.is_empty() && written.len() > 1).then_some(written)
}

/// A tool call's argument `member`, as text: a string as it is, an integer in decimal.
fn argument(call: &ToolCall, member: &str) -> Option<String> {
    match call.arguments.get(member)? {
        Value::Str(s) => Some(s.clone()),
        Value::Int(i) => Some(i.get().to_string()),
        _ => None,
    }
}

/// Whether two argument values are the same: without case or surrounding space, and a decimal
/// without its fraction's trailing zeros.
fn same(got: &str, expected: &str) -> bool {
    let norm = |s: &str| {
        let s = s.trim().to_lowercase();
        if s.contains('.') && s.chars().all(|c| c.is_ascii_digit() || c == '.') {
            s.trim_end_matches('0').trim_end_matches('.').to_owned()
        } else {
            s
        }
    };
    norm(got) == norm(expected)
}

/// One run's end and its checks' verdicts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunVerdict {
    pub run: String,
    pub end: &'static str,
    pub checks: Vec<bool>,
    pub passed: bool,
}

/// Why the gate did not start, by its code: nothing was spent and no report was written
/// (DEC-2216 item 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum GateRefusal {
    #[error("the request is not `gate <model_id>`")]
    Malformed,
    #[error("the live registry file names no chat entry with this id")]
    UnknownEntry,
    #[error("the entry does not register")]
    Unregistrable,
    #[error("the built-in probe set does not read")]
    ProbeSetUnreadable,
    #[error("the gate's maximum cost has no bound")]
    Unbounded,
    #[error("a meter stream cannot be read")]
    MeterUnreadable,
    #[error("a monthly cap's room is below the gate's maximum")]
    RoomShort,
    #[error("a gate is already running")]
    Running,
    #[error("a passing report exists for this entry, probe set, endpoint and price table")]
    AlreadyPassed,
    #[error("the service has no research store to write a report to")]
    StoreUnavailable,
    #[error("the service stopped serving before it answered")]
    Stopping,
}

impl GateRefusal {
    pub fn code(self) -> &'static str {
        match self {
            Self::Malformed => "malformed",
            Self::UnknownEntry => "unknown_entry",
            Self::Unregistrable => "unregistrable",
            Self::ProbeSetUnreadable => "probe_set_unreadable",
            Self::Unbounded => "unbounded",
            Self::MeterUnreadable => "meter_unreadable",
            Self::RoomShort => "room_short",
            Self::Running => "gate_running",
            Self::AlreadyPassed => "already_passed",
            Self::StoreUnavailable => "store_unavailable",
            Self::Stopping => "stopping",
        }
    }
}

/// Why a gate that called did not pass (DEC-2216 item 2): the first that occurred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stopped {
    /// A reported identity differed from the pin's.
    IdentityMismatch,
    /// The meter refused a call, or a call's maximum was above the gate's ceiling.
    CapRefused,
    /// A call failed at the provider after the gateway's retries, or the endpoint could not be
    /// reached at all.
    ProviderFailed,
    /// Every run ended and a run or a check failed.
    ChecksFailed,
}

impl Stopped {
    pub fn code(self) -> &'static str {
        match self {
            Self::IdentityMismatch => "identity_mismatch",
            Self::CapRefused => "cap_refused",
            Self::ProviderFailed => "provider_failed",
            Self::ChecksFailed => "checks_failed",
        }
    }

    /// What a run that ended on the gateway's `outcome` stops the gate with; `None` when the run
    /// simply failed, as one whose reply broke its schema does.
    fn of(outcome: Outcome) -> Option<Self> {
        match outcome {
            Outcome::IdentityMismatch => Some(Self::IdentityMismatch),
            Outcome::BudgetExhausted | Outcome::MeterUnavailable => Some(Self::CapRefused),
            Outcome::CredentialInvalid
            | Outcome::ProviderUnavailable
            | Outcome::DeadlineExceeded
            | Outcome::RateLimitedProvider
            | Outcome::RateLimitedLocal
            | Outcome::PolicyDenied
            | Outcome::ModelWithdrawn => Some(Self::ProviderFailed),
            Outcome::Ok
            | Outcome::InputRejected
            | Outcome::ContentRefused
            | Outcome::SchemaInvalid => None,
        }
    }
}

/// How a gate that called ended: its report's reference, `None` when the store did not take it
/// (`report_unwritten`, so it cannot pass), and why it did not pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateEnd {
    pub report: Option<ArtifactRef>,
    pub stopped: Option<Stopped>,
}

impl GateEnd {
    /// Whether the gate passed: no stop, and a report the store holds.
    pub fn passed(&self) -> bool {
        self.stopped.is_none() && self.report.is_some()
    }
}

/// The research store, as the gate writes its report and the start reads it back.
pub trait Reports {
    /// The bytes stored under `reference`, `None` when it holds none or they do not re-hash.
    fn read(&self, reference: &ArtifactRef) -> Option<Vec<u8>>;
    /// Stores `bytes`, create-only, and answers their reference; `None` when the store refused.
    fn write(&mut self, bytes: &[u8]) -> Option<ArtifactRef>;
}

/// Why a named report does not offer its entry, by its code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unverified {
    /// The entry names no report and is not one the ruling keeps offered without one.
    Missing,
    /// The store holds no bytes under the reference, or none that re-hash to it.
    Unreadable,
    /// The bytes are not a chat gate report.
    Malformed,
    /// The report is for another pin or content hash.
    Pin,
    /// The report is on another probe set.
    ProbeSet,
    /// The report is for another endpoint.
    Endpoint,
    /// The report is under another price table.
    PriceTable,
    /// The report does not say `passed`.
    NotPassed,
    /// The start's identity probe did not end `ok` with exactly the pin's identity.
    Identity,
}

impl Unverified {
    pub fn code(self) -> &'static str {
        match self {
            Self::Missing => "gate_report_missing",
            Self::Unreadable => "gate_report_unreadable",
            Self::Malformed => "gate_report_malformed",
            Self::Pin => "gate_report_pin",
            Self::ProbeSet => "gate_report_probe_set",
            Self::Endpoint => "gate_report_endpoint",
            Self::PriceTable => "gate_report_price_table",
            Self::NotPassed => "gate_report_not_passed",
            Self::Identity => "identity_probe_failed",
        }
    }
}

/// Whether the report under `reference` reads back from `reports`, re-hashes to it, and binds
/// `chat`'s pin and content hash, `probe_set`, and the endpoint and price table the file names
/// now, and says `passed` with no `stopped`.
///
/// # Errors
/// The first [`Unverified`] that holds, in that order.
pub fn verified(
    reports: &dyn Reports,
    reference: &ArtifactRef,
    chat: &ChatLive,
    probe_set: Digest,
) -> Result<(), Unverified> {
    let bytes = reports
        .read(reference)
        .filter(|bytes| ArtifactRef::of(bytes) == *reference)
        .ok_or(Unverified::Unreadable)?;
    let report = parse(&bytes).map_err(|_| Unverified::Malformed)?;
    let member = |name: &str| report.get(name).ok_or(Unverified::Malformed);
    let said = |name: &str| member(name).and_then(|v| v.as_str().ok_or(Unverified::Malformed));
    if said("kind")? != REPORT_KIND || member("version")?.as_int() != Some(REPORT_VERSION) {
        return Err(Unverified::Malformed);
    }
    let pin = chat.entry.content().model_ref();
    let pinned = member("pin")?;
    let pin_said = |name: &str| pinned.get(name).and_then(Value::as_str);
    if pin_said("model_id") != Some(pin.model_id.as_str())
        || pin_said("model_version") != Some(pin.model_version.as_str())
        || pin_said("content_hash") != Some(digest_ref(pin.content_hash).as_str())
    {
        return Err(Unverified::Pin);
    }
    if said("probe_set")? != digest_ref(probe_set) {
        return Err(Unverified::ProbeSet);
    }
    if said("endpoint")? != chat.endpoint.id {
        return Err(Unverified::Endpoint);
    }
    if said("price_table")? != digest_ref(chat.endpoint.prices.version()) {
        return Err(Unverified::PriceTable);
    }
    if member("passed")? != &Value::Bool(true) || member("stopped")? != &Value::Null {
        return Err(Unverified::NotPassed);
    }
    Ok(())
}

/// What the gate runs with besides its gateway: the workspace its calls name, the entry, the store,
/// the reports already written that may hold a pass, the ids of its calls, and the probe set with
/// its digest.
pub struct GateRun<'a, 'r> {
    pub workspace: &'a str,
    pub chat: &'a ChatLive,
    pub reports: Option<&'r mut dyn Reports>,
    pub earlier: &'a [ArtifactRef],
    pub ids: &'a mut dyn IdSource,
    pub set: &'a ProbeSet,
    pub digest: Digest,
}

/// Runs the gate on `gate.chat` through `gateway` (DEC-2294, DEC-2216 item 2).
///
/// Before any call, with nothing spent, it refuses when the service has no store, the entry does
/// not register, a passing report for this entry, probe set, endpoint and price table exists among
/// the file's and `gate.earlier`, the gate's maximum has no bound, a meter stream does not read, or
/// either monthly cap's room is below the gate's whole maximum. Once it has called, it ends with a
/// report written to the store; [`GateEnd::report`] is `None` only when the store refused it.
///
/// # Errors
/// The [`GateRefusal`] met first, in that order.
pub fn run_gate<T: Transport, S: MeterStore, C: Clock>(
    gateway: &mut Gateway<T, S, C>,
    gate: GateRun<'_, '_>,
) -> Result<GateEnd, GateRefusal> {
    let GateRun {
        workspace,
        chat,
        reports,
        earlier,
        ids,
        set,
        digest,
    } = gate;
    let reports = reports.ok_or(GateRefusal::StoreUnavailable)?;
    let mut registry = Registry::new();
    offer_chat(&mut registry, chat).map_err(|_| GateRefusal::Unregistrable)?;
    if chat
        .gate_report
        .iter()
        .chain(earlier)
        .any(|reference| verified(&*reports, reference, chat, digest).is_ok())
    {
        return Err(GateRefusal::AlreadyPassed);
    }
    let ceiling = ceiling(chat, set).ok_or(GateRefusal::Unbounded)?;
    let maximum = ceiling
        .micros()
        .checked_mul(set.max_calls())
        .map(Usd6::from_micros)
        .ok_or(GateRefusal::Unbounded)?;
    if room(gateway)? < maximum {
        return Err(GateRefusal::RoomShort);
    }
    let own = gateway.replace_registry(registry);
    let mut session = Session {
        gateway,
        ids,
        ceiling,
        workspace,
        chat,
        run: IDENTITY_RUN.to_owned(),
        calls: Vec::new(),
        mismatch: false,
    };
    let (stopped, identity, runs) = session.all(set);
    let calls = std::mem::take(&mut session.calls);
    let _gate_registry = session.gateway.replace_registry(own);
    let report = report(chat, digest, identity, calls, &runs, set, stopped);
    Ok(GateEnd {
        report: reports.write(&to_canonical(&report)),
        stopped,
    })
}

/// The probe at start (DEC-2216 item 4): one `evaluation` call to `chat`'s endpoint, in a registry
/// of its own, under the probe set's ceiling. It passes only if it ends `ok` and reports exactly the
/// pin's model and the endpoint's provider.
pub fn identity_probe<T: Transport, S: MeterStore, C: Clock>(
    gateway: &mut Gateway<T, S, C>,
    workspace: &str,
    chat: &ChatLive,
    set: &ProbeSet,
    ids: &mut dyn IdSource,
) -> bool {
    let mut registry = Registry::new();
    let Some(ceiling) = ceiling(chat, set) else {
        return false;
    };
    if offer_chat(&mut registry, chat).is_err() {
        return false;
    }
    let own = gateway.replace_registry(registry);
    let mut session = Session {
        gateway,
        ids,
        ceiling,
        workspace,
        chat,
        run: IDENTITY_RUN.to_owned(),
        calls: Vec::new(),
        mismatch: false,
    };
    let passed = session.probe(set);
    let _gate_registry = session.gateway.replace_registry(own);
    passed
}

/// The most one gate call may reserve: the probe set's prompt bound and the entry's output bound,
/// under the endpoint's prices.
pub fn ceiling(chat: &ChatLive, set: &ProbeSet) -> Option<Usd6> {
    chat.endpoint
        .prices
        .max_cost(set.max_prompt_bytes, chat.entry.max_output_tokens)
        .ok()
}

/// The gate's whole maximum on `chat`: its ceiling for every call it may make.
pub fn maximum(chat: &ChatLive, set: &ProbeSet) -> Option<Usd6> {
    ceiling(chat, set)?
        .micros()
        .checked_mul(set.max_calls())
        .map(Usd6::from_micros)
}

/// The smaller of the two monthly caps' room this month: the workspace's and the deployment's.
fn room<T: Transport, S: MeterStore, C: Clock>(
    gateway: &mut Gateway<T, S, C>,
) -> Result<Usd6, GateRefusal> {
    let now = gateway.clock_mut().now();
    let day = new_york_date_and_hour(now).map_or(now.date(), |(day, _)| day);
    let month = Period::month_of(day);
    let meter = gateway.meter();
    let workspace = meter
        .spend(&Scope::Workspace, month)
        .map_err(|_| GateRefusal::MeterUnreadable)?;
    let deployment = meter
        .deployment()
        .spend(month)
        .map_err(|_| GateRefusal::MeterUnreadable)?;
    Ok(meter
        .caps()
        .workspace_monthly()
        .saturating_sub(workspace)
        .min(meter.deployment().monthly().saturating_sub(deployment)))
}

/// The gate's calls through the gateway, and what it saw of them.
struct Session<'g, T, S, C> {
    gateway: &'g mut Gateway<T, S, C>,
    ids: &'g mut dyn IdSource,
    ceiling: Usd6,
    workspace: &'g str,
    chat: &'g ChatLive,
    run: String,
    calls: Vec<Value>,
    mismatch: bool,
}

impl<T: Transport, S: MeterStore, C: Clock> Session<'_, T, S, C> {
    /// The pin's identity as the endpoint reports serving it.
    fn expected(&self) -> Option<String> {
        Reported {
            model: Some(self.chat.entry.identity.model_name.clone()),
            provider: Some(self.chat.endpoint.route.reported_provider.clone()),
        }
        .text()
    }

    /// The identity probe, then every run in order, until a stop.
    fn all(&mut self, set: &ProbeSet) -> (Option<Stopped>, bool, Vec<RunVerdict>) {
        let identity = self.probe(set);
        if !identity {
            let stopped = self
                .calls
                .last()
                .and_then(|call| call.get("outcome"))
                .and_then(Value::as_str)
                .and_then(|code| Outcome::ALL.into_iter().find(|o| o.code() == code))
                .and_then(Stopped::of)
                .unwrap_or(Stopped::IdentityMismatch);
            return (Some(stopped), false, Vec::new());
        }
        let mut runs = Vec::new();
        for probe in &set.runs {
            self.run.clone_from(&probe.id);
            let spec = RunSpec {
                workspace_id: self.workspace.to_owned(),
                agent_id: None,
                kind: probe.kind.clone(),
                model: self.chat.entry.content().model_ref(),
                inputs: probe.inputs.clone(),
                key_owner: KeyOwner::Platform,
                call_deadline_ms: self.chat.entry.deadline_ms,
            };
            let mut host = Seen::default();
            let end = run(
                &spec,
                probe.max_turns,
                self,
                &mut Fixtures(probe),
                &mut host,
            );
            runs.push(probe.verdict(&end, &host.0));
            let stopped = match end {
                _ if self.mismatch => Some(Stopped::IdentityMismatch),
                End::Model(outcome) => Stopped::of(outcome),
                _ => None,
            };
            if stopped.is_some() {
                return (stopped, true, runs);
            }
        }
        let failed = runs.iter().any(|r| !r.passed);
        (failed.then_some(Stopped::ChecksFailed), true, runs)
    }

    /// The identity probe: a fixed request with no tools, which passes only on an `ok` reply that
    /// reports exactly the pin's identity.
    fn probe(&mut self, set: &ProbeSet) -> bool {
        let mut inputs = set.identity.clone();
        inputs.insert("kind".to_owned(), AGENT.to_owned());
        let request = CallRequest {
            call_id: String::new(),
            workspace_id: self.workspace.to_owned(),
            agent_id: None,
            purpose: Purpose::Evaluation,
            model: self.chat.entry.content().model_ref(),
            inputs,
            as_of: UtcNanos::EPOCH,
            deadline: UtcNanos::EPOCH,
            observations: Vec::new(),
            key_owner: KeyOwner::Platform,
            run_kind: Some(AGENT.to_owned()),
            turns: Vec::new(),
            tools: Vec::new(),
            spend_ceiling: None,
        };
        let expected = self.expected();
        self.call(&request).is_ok_and(|record| {
            record.outcome == Outcome::Ok && record.reported_identity == expected
        })
    }

    /// Logs one call's record in the report's terms, and marks a reported identity other than
    /// the pin's.
    fn log(&mut self, record: &CallRecord) {
        if record.outcome == Outcome::Ok && record.reported_identity != self.expected() {
            self.mismatch = true;
        }
        let int = |n: u64| Int::new(n).map_or(Value::Null, Value::Int);
        self.calls.push(object(vec![
            ("call_id", Value::Str(record.call_id.clone())),
            ("cost_usd", Value::Str(record.cost.to_string())),
            (
                "identity",
                record
                    .reported_identity
                    .clone()
                    .map_or(Value::Null, Value::Str),
            ),
            ("outcome", Value::Str(record.outcome.code().to_owned())),
            (
                "request_digest",
                record
                    .request_digest
                    .map_or(Value::Null, |d| Value::Str(digest_ref(d))),
            ),
            ("run", Value::Str(self.run.clone())),
            (
                "tokens",
                object(vec![
                    ("cached", int(record.tokens.cached)),
                    ("input", int(record.tokens.input)),
                    ("output", int(record.tokens.output)),
                ]),
            ),
        ]));
    }
}

impl<T: Transport, S: MeterStore, C: Clock> Model for Session<'_, T, S, C> {
    /// The loop's call made the gate's: an `evaluation` with no agent, under the gate's ceiling,
    /// with its id and times from the service's own ids and clock. A call the gateway's own rate
    /// cap refused, which reserved nothing, waits and is tried again.
    fn call(&mut self, request: &CallRequest) -> Reply {
        let mut request = request.clone();
        request.purpose = Purpose::Evaluation;
        request.agent_id = None;
        request.spend_ceiling = Some(self.ceiling);
        let mut tries = 0u32;
        loop {
            let now = self.gateway.clock_mut().now();
            request.call_id = self.ids.next_id();
            request.as_of = now;
            request.deadline = after_ms(now, self.chat.entry.deadline_ms);
            let reply = self.gateway.call(&request);
            let limited = reply
                .as_ref()
                .is_ok_and(|r| r.outcome == Outcome::RateLimitedLocal);
            if limited && tries < RATE_RETRIES {
                tries = tries.saturating_add(1);
                self.gateway
                    .clock_mut()
                    .wait_until(after_ms(now, RATE_WAIT_MS));
                continue;
            }
            if let Ok(record) = &reply {
                self.log(record);
            }
            return reply;
        }
    }
}

/// The run's tool calls and their results, as the loop hands them over; nothing is journaled, since
/// the gate's records are its report's (inference spec §3.6).
#[derive(Default)]
struct Seen(Vec<(ToolCall, String)>);

impl Host for Seen {
    fn invoked(&mut self, _turn: u32, _record: &CallRecord) -> Result<(), Halt> {
        Ok(())
    }

    fn results(&mut self, _turn: u32, calls: &[ToolCall], results: &[String]) -> Result<(), Halt> {
        self.0
            .extend(calls.iter().cloned().zip(results.iter().cloned()));
        Ok(())
    }

    fn stopped(&mut self) -> bool {
        false
    }

    fn over_budget(&mut self) -> bool {
        false
    }

    fn now(&mut self) -> UtcNanos {
        UtcNanos::EPOCH
    }

    fn call_id(&mut self) -> String {
        String::new()
    }
}

/// The run's tools, answered from its fixtures alone.
struct Fixtures<'r>(&'r ProbeRun);

impl Toolbox for Fixtures<'_> {
    fn run(&mut self, call: &ToolCall) -> String {
        self.0.answer(call)
    }
}

/// The report (DEC-2216 item 3): what the gate ran against, every call, the identity probe, every
/// run and check, and its verdict. It passes only if nothing stopped it and every run of the set
/// ended and passed.
fn report(
    chat: &ChatLive,
    probe_set: Digest,
    identity: bool,
    calls: Vec<Value>,
    runs: &[RunVerdict],
    set: &ProbeSet,
    stopped: Option<Stopped>,
) -> Value {
    let pin = chat.entry.content().model_ref();
    let mut checks = Vec::new();
    for (verdict, probe) in runs.iter().zip(&set.runs) {
        for (check, held) in probe.checks.iter().zip(&verdict.checks) {
            checks.push(object(vec![
                ("check", Value::Str(check.code().to_owned())),
                ("passed", Value::Bool(*held)),
                ("run", Value::Str(verdict.run.clone())),
            ]));
        }
    }
    let passed = stopped.is_none() && identity && runs.len() == set.runs.len();
    object(vec![
        ("calls", Value::Array(calls)),
        ("checks", Value::Array(checks)),
        ("endpoint", Value::Str(chat.endpoint.id.clone())),
        ("identity", Value::Bool(identity)),
        ("kind", Value::Str(REPORT_KIND.to_owned())),
        ("passed", Value::Bool(passed)),
        (
            "pin",
            object(vec![
                ("content_hash", Value::Str(digest_ref(pin.content_hash))),
                ("model_id", Value::Str(pin.model_id)),
                ("model_version", Value::Str(pin.model_version)),
            ]),
        ),
        (
            "price_table",
            Value::Str(digest_ref(chat.endpoint.prices.version())),
        ),
        ("probe_set", Value::Str(digest_ref(probe_set))),
        (
            "runs",
            Value::Array(
                runs.iter()
                    .map(|r| {
                        object(vec![
                            ("end", Value::Str(r.end.to_owned())),
                            ("passed", Value::Bool(r.passed)),
                            ("run", Value::Str(r.run.clone())),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "stopped",
            stopped.map_or(Value::Null, |s| Value::Str(s.code().to_owned())),
        ),
        (
            "version",
            Int::new(REPORT_VERSION).map_or(Value::Null, Value::Int),
        ),
    ])
}

/// `sha256:{hex}`, the form a reference takes in the report.
pub fn digest_ref(digest: Digest) -> String {
    ArtifactRef::from_digest(digest).to_string()
}

fn after_ms(at: UtcNanos, ms: u64) -> UtcNanos {
    let secs = ms
        .checked_div(1_000)
        .and_then(|s| i64::try_from(s).ok())
        .unwrap_or(i64::MAX);
    let nanos = ms
        .checked_rem(1_000)
        .and_then(|r| u32::try_from(r).ok())
        .unwrap_or(0)
        .saturating_mul(1_000_000);
    let carried = at.nanos().saturating_add(nanos);
    let (carry, nanos) = if carried >= 1_000_000_000 {
        (1, carried.saturating_sub(1_000_000_000))
    } else {
        (0, carried)
    };
    UtcNanos::from_parts(at.secs().saturating_add(secs).saturating_add(carry), nanos).unwrap_or(at)
}

fn object(members: Vec<(&str, Value)>) -> Value {
    let mut out = Object::new();
    for (name, value) in members {
        if let Ok(key) = Key::new(name) {
            out.insert(key, value);
        }
    }
    Value::Object(out)
}
