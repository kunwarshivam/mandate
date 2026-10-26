//! The records journal spec v0.50 §9.23 closes for an agentic research run (agent harness spec
//! §6.9, DEC-2203): the agent stream's `RunStarted`, `ToolCallRecorded`, `ToolResultRecorded`,
//! `RunSummaryRecorded`, and `RunEnded`. Rules 171 and 172 are the draft's ([`rules`]); rules 173 to
//! 183, and rule 271 on an event run's start (journal spec v0.87, DEC-2392), are stream rules
//! ([`stream_rule`]), which [`crate::check_stream`] runs with the stream's earlier records, rule 183
//! on the agent stream's `ModelInvocationRecorded`. Every rule only refuses a draft.

use mandate_canon::Value;

use crate::schema::Ty;
use crate::{Draft, Invalid, InvalidReason, StreamType};

const STARTED: &str = "RunStarted";
const CALLED: &str = "ToolCallRecorded";
const RESULT: &str = "ToolResultRecorded";
const SUMMARY: &str = "RunSummaryRecorded";
const ENDED: &str = "RunEnded";
const INVOCATION: &str = "ModelInvocationRecorded";
const OBSERVED: &str = "ObservationRecorded";

/// The event types §9.23 closes, all on the agent stream.
pub(crate) const RUN_RECORDS: [&str; 5] = [STARTED, CALLED, RESULT, SUMMARY, ENDED];

/// The codes only a refused result carries (rule 179).
const REFUSALS: [&str; 6] = [
    "out_of_universe",
    "unknown_item",
    "call_budget",
    "max_theses",
    "max_repeat_denials",
    "run_ended",
];

/// The codes that are no action's verdict (rule 179); `out_of_universe` may be a mediated
/// thesis's, from mandate spec §8.5 check 8.
const NOT_VERDICTS: [&str; 6] = [
    "unknown_item",
    "call_budget",
    "max_theses",
    "max_repeat_denials",
    "run_ended",
    "unavailable",
];

/// The most entries a summary's `watch` holds (rule 181).
const MAX_WATCH: usize = 10;

/// Whether §9.23 governs `event_type`: the catalogue admits its records on the agent stream alone,
/// so the stream is already the agent's here.
pub(crate) fn governs(event_type: &str) -> bool {
    RUN_RECORDS.contains(&event_type)
}

/// The §9.23 schema of `event_type` at `schema_version` on `stream`, or `None`.
pub(crate) fn schema(
    event_type: &str,
    schema_version: u64,
    stream: StreamType,
) -> Option<&'static Ty> {
    match (event_type, schema_version, stream) {
        (STARTED, 1, StreamType::Agent) => Some(&RUN_STARTED),
        (CALLED, 1, StreamType::Agent) => Some(&TOOL_CALL_RECORDED),
        (RESULT, 1, StreamType::Agent) => Some(&TOOL_RESULT_RECORDED),
        (SUMMARY, 1, StreamType::Agent) => Some(&RUN_SUMMARY_RECORDED),
        (ENDED, 1, StreamType::Agent) => Some(&RUN_ENDED),
        _ => None,
    }
}

/// Rules 171 and 172 on a payload its schema has normalized, in rule order: the first that fails
/// is reported. `actor` and `correlation_id` are the envelope's.
pub(crate) fn rules(
    event_type: &str,
    payload: &Value,
    actor: Option<&Value>,
    correlation_id: Option<&Value>,
) -> Result<(), Invalid> {
    if !governs(event_type) {
        return Ok(());
    }
    let writer = actor
        .and_then(|a| a.get("kind"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    ensure(writer == "agent", "actor.kind")?;
    ensure(
        correlation_id.is_some_and(|run| *run != Value::Null),
        "correlation_id",
    )?;
    if event_type != STARTED {
        return Ok(());
    }
    let p = View(payload);
    ascending(&p.texts("menu"), "payload.menu")?;
    ascending(&p.texts("memory"), "payload.memory")?;
    let trigger = p.member("trigger");
    let cause = trigger.is_null("cause");
    ensure(
        cause == (trigger.text("kind") == "schedule"),
        "payload.trigger.cause",
    )?;
    let fits = match p.text("kind") {
        "ideate" => trigger.text("kind") == "schedule",
        "monitor" => matches!(trigger.text("kind"), "schedule" | "event"),
        "owner" => trigger.text("kind") == "owner_question",
        _ => trigger.text("kind") == "approval",
    };
    ensure(fits, "payload.trigger.kind")
}

/// Rules 173 to 183 on one draft, against the stream's records `before` it, in order, in rule
/// order: the first that fails is reported.
pub(crate) fn stream_rule(before: &[&Draft], draft: &Draft) -> Result<(), Invalid> {
    let event_type = draft.event_type();
    if event_type == INVOCATION {
        return late_turn(before, draft);
    }
    if !governs(event_type) {
        return Ok(());
    }
    let run = correlation(draft);
    let of_run = |d: &&&Draft| correlation(d) == run;
    let mine = payload_of(draft);
    let m = View(&mine);
    if event_type == STARTED {
        ensure(
            !before
                .iter()
                .filter(of_run)
                .any(|d| d.event_type() == STARTED),
            "correlation_id",
        )?;
        let kind = m.text("kind");
        if matches!(kind, "explain" | "owner") {
            let cause = m.member("trigger").text("cause");
            let answered = before
                .iter()
                .filter(|d| d.event_type() == STARTED)
                .any(|d| {
                    let p = payload_of(d);
                    let p = View(&p);
                    p.text("kind") == kind && p.member("trigger").text("cause") == cause
                });
            ensure(!answered, "payload.trigger.cause")?;
        }
        if m.member("trigger").text("kind") == "event" {
            return event_cause(before, draft);
        }
        return Ok(());
    }
    let started = before
        .iter()
        .filter(of_run)
        .find(|d| d.event_type() == STARTED)
        .map(|d| payload_of(d));
    let Some(started) = started else {
        return Err(Invalid::new(InvalidReason::Schema, "correlation_id"));
    };
    let of_type = |t: &str| before.iter().filter(of_run).any(|d| d.event_type() == t);
    ensure(!of_type(ENDED), "correlation_id")?;
    match event_type {
        SUMMARY => {
            ensure(!of_type(SUMMARY), "correlation_id")?;
            ascending(&m.texts("instruments_read"), "payload.instruments_read")?;
            let watch = m.list("watch");
            let mut watched: Vec<&str> = watch
                .iter()
                .map(|w| View(w).text("instrument_id"))
                .collect();
            watched.sort_unstable();
            watched.dedup();
            ensure(
                watch.len() <= MAX_WATCH && watched.len() == watch.len(),
                "payload.watch",
            )
        }
        ENDED => {
            ensure(of_type(SUMMARY), "correlation_id")?;
            ended(m)
        }
        CALLED => tool_call(before, draft, m, View(&started)),
        _ => tool_result(before, draft, m),
    }
}

/// Rules 175 to 177 on a `ToolCallRecorded`, whose run started with `started`.
fn tool_call(
    before: &[&Draft],
    draft: &Draft,
    m: View<'_>,
    started: View<'_>,
) -> Result<(), Invalid> {
    let call = m.text("call_id");
    let turn = before.iter().find(|d| {
        let p = payload_of(d);
        let p = View(&p);
        d.event_type() == INVOCATION
            && p.text("call_id") == call
            && p.text("purpose") == "research"
            && p.text("outcome") == "ok"
    });
    let Some(turn) = turn else {
        return Err(Invalid::new(InvalidReason::Schema, "payload.call_id"));
    };
    ensure(correlation(turn) == correlation(draft), "correlation_id")?;
    ensure(entry(turn) == entry(draft), "config_refs.model_version")?;
    ensure(
        started.texts("menu").contains(&m.text("tool")),
        "payload.tool",
    )?;
    let earlier = before
        .iter()
        .filter(|d| d.event_type() == CALLED)
        .filter(|d| View(&payload_of(d)).text("call_id") == call)
        .count();
    ensure(
        u64::try_from(earlier).is_ok_and(|n| m.int("index") == n),
        "payload.index",
    )?;
    let provenance = m.texts("provenance");
    ascending(&provenance, "payload.provenance")?;
    ensure(
        provenance.is_empty() || m.text("class") == "action",
        "payload.provenance",
    )
}

/// Rules 178 to 180 on a `ToolResultRecorded`.
fn tool_result(before: &[&Draft], draft: &Draft, m: View<'_>) -> Result<(), Invalid> {
    let named = m.text("tool_call");
    let call = before
        .iter()
        .find(|d| {
            d.event_type() == CALLED
                && d.event_id() == named
                && correlation(d) == correlation(draft)
        })
        .map(|d| payload_of(d));
    let Some(call) = call else {
        return Err(Invalid::new(InvalidReason::Schema, "payload.tool_call"));
    };
    let results: Vec<Value> = before
        .iter()
        .filter(|d| d.event_type() == RESULT)
        .map(|d| payload_of(d))
        .collect();
    ensure(
        !results.iter().any(|r| View(r).text("tool_call") == named),
        "payload.tool_call",
    )?;
    let c = View(&call);
    let class = c.text("class");
    let result = m.text("result");
    let code = m.text("code");
    let ran = result == "ok" && matches!(class, "read" | "think");
    ensure(m.is_null("code") == ran, "payload.code")?;
    let fits = match result {
        "failed" => class == "read" && code == "unavailable",
        "refused" => REFUSALS.contains(&code),
        _ => class != "action" || !NOT_VERDICTS.contains(&code),
    };
    ensure(fits, "payload.result")?;
    ascending(&m.texts("observations"), "payload.observations")?;
    let read = result == "ok" && class == "read";
    ensure(
        read || m.list("observations").is_empty(),
        "payload.observations",
    )?;
    ensure(read || !m.flag("truncated"), "payload.truncated")?;
    budget(before, c.text("call_id"), code == "call_budget", &results)
}

/// Rule 180: the earlier results of the reply `call` agree with this one on `call_budget`.
fn budget(before: &[&Draft], call: &str, over: bool, results: &[Value]) -> Result<(), Invalid> {
    let calls: Vec<&str> = before
        .iter()
        .filter(|d| d.event_type() == CALLED)
        .filter(|d| View(&payload_of(d)).text("call_id") == call)
        .map(|d| d.event_id())
        .collect();
    let disagrees = results
        .iter()
        .map(View)
        .filter(|r| calls.contains(&r.text("tool_call")))
        .any(|r| (r.text("code") == "call_budget") != over);
    ensure(!disagrees, "payload.code")
}

/// Rule 182 on a `RunEnded`.
fn ended(m: View<'_>) -> Result<(), Invalid> {
    let reason = m.text("reason");
    let bound = m.text("bound");
    let fits = match reason {
        "bound" => !m.is_null("bound") && bound != "max_turns",
        "done" => m.is_null("bound") || matches!(bound, "max_turns" | "max_tool_calls"),
        _ => m.is_null("bound"),
    };
    ensure(fits, "payload.bound")?;
    let discarded = m.texts("discarded");
    ascending(&discarded, "payload.discarded")?;
    ensure(
        reason != "done" || discarded.is_empty(),
        "payload.discarded",
    )
}

/// Rule 183: a research turn record of a run that has ended follows only a `deadline` or `mode`
/// end, with no verdicts.
fn late_turn(before: &[&Draft], draft: &Draft) -> Result<(), Invalid> {
    let mine = payload_of(draft);
    let m = View(&mine);
    if m.text("purpose") != "research" {
        return Ok(());
    }
    let run = correlation(draft);
    let of_run = |d: &&&Draft| correlation(d) == run;
    if !before
        .iter()
        .filter(of_run)
        .any(|d| d.event_type() == STARTED)
    {
        return Ok(());
    }
    let end = before
        .iter()
        .filter(of_run)
        .find(|d| d.event_type() == ENDED)
        .map(|d| payload_of(d));
    let Some(end) = end else {
        return Ok(());
    };
    ensure(
        matches!(View(&end).text("reason"), "deadline" | "mode") && m.list("verdicts").is_empty(),
        "correlation_id",
    )
}

/// Rule 271 (DEC-2392) on an event-triggered `RunStarted`, after rule 173: its `trigger.cause` is
/// an earlier `ObservationRecorded` of its own run naming an instrument, and no earlier event run's
/// cause observation holds the same item (`data_ref`), so one item starts at most one run. A
/// cause that names no such record, or one whose item started a run before, is refused at
/// `payload.trigger.cause`.
fn event_cause(before: &[&Draft], draft: &Draft) -> Result<(), Invalid> {
    let path = "payload.trigger.cause";
    let observation = |id: &str| {
        before
            .iter()
            .find(|d| d.event_type() == OBSERVED && d.event_id() == id)
            .copied()
    };
    let started = payload_of(draft);
    let cause = View(&started).member("trigger").text("cause").to_owned();
    let Some(seen) = observation(&cause) else {
        return Err(Invalid::new(InvalidReason::Schema, path));
    };
    ensure(correlation(seen) == correlation(draft), path)?;
    let item = payload_of(seen);
    ensure(!View(&item).is_null("instrument_id"), path)?;
    let data = View(&item).text("data_ref").to_owned();
    let repeated = before
        .iter()
        .filter(|d| d.event_type() == STARTED)
        .filter_map(|d| {
            let p = payload_of(d);
            let trigger = View(&p).member("trigger");
            (trigger.text("kind") == "event").then(|| trigger.text("cause").to_owned())
        })
        .filter_map(|earlier| observation(&earlier))
        .any(|earlier| View(&payload_of(earlier)).text("data_ref") == data);
    ensure(!repeated, path)
}

fn correlation(draft: &Draft) -> Option<&str> {
    draft.fields().get("correlation_id").and_then(Value::as_str)
}

fn entry(draft: &Draft) -> Option<&str> {
    draft
        .fields()
        .get("config_refs")
        .and_then(|refs| refs.get("model_version"))
        .and_then(Value::as_str)
}

fn payload_of(draft: &Draft) -> Value {
    draft
        .fields()
        .get("payload")
        .cloned()
        .unwrap_or(Value::Null)
}

fn ensure(holds: bool, path: &str) -> Result<(), Invalid> {
    if holds {
        Ok(())
    } else {
        Err(Invalid::new(InvalidReason::Schema, path))
    }
}

/// Strictly ascending by bytes, so a repeat is refused as well as a swap (`non_canonical`).
fn ascending(items: &[&str], path: &str) -> Result<(), Invalid> {
    if items.windows(2).all(|w| matches!(w, [a, b] if a < b)) {
        Ok(())
    } else {
        Err(Invalid::new(InvalidReason::NonCanonical, path))
    }
}

/// A normalized payload, read member by member; a `null` or absent member reads as empty text.
#[derive(Clone, Copy)]
struct View<'a>(&'a Value);

impl<'a> View<'a> {
    fn text(self, member: &str) -> &'a str {
        self.0
            .get(member)
            .and_then(Value::as_str)
            .unwrap_or_default()
    }

    fn member(self, member: &str) -> View<'a> {
        View(self.0.get(member).unwrap_or(&Value::Null))
    }

    fn is_null(self, member: &str) -> bool {
        matches!(self.0.get(member), None | Some(Value::Null))
    }

    fn flag(self, member: &str) -> bool {
        self.0.get(member) == Some(&Value::Bool(true))
    }

    fn int(self, member: &str) -> u64 {
        self.0
            .get(member)
            .and_then(Value::as_int)
            .unwrap_or(u64::MAX)
    }

    fn list(self, member: &str) -> &'a [Value] {
        self.0
            .get(member)
            .and_then(Value::as_array)
            .unwrap_or_default()
    }

    fn texts(self, member: &str) -> Vec<&'a str> {
        self.list(member).iter().filter_map(Value::as_str).collect()
    }
}

static RUN_STARTED: Ty = Ty::Record(&[
    (
        "kind",
        Ty::OneOf(&["ideate", "monitor", "owner", "explain"]),
    ),
    (
        "trigger",
        Ty::Record(&[
            (
                "kind",
                Ty::OneOf(&["schedule", "event", "owner_question", "approval"]),
            ),
            ("cause", Ty::Nullable(&Ty::Ulid)),
        ]),
    ),
    ("output", Ty::OneOf(&["final_output", "action_tools"])),
    ("menu", Ty::List(&Ty::Str)),
    ("cut_off", Ty::Timestamp),
    ("allowlist_version", Ty::Int),
    ("data_universe_ref", Ty::DigestRef),
    ("cost_ceiling_usd", Ty::Decimal),
    ("deadline", Ty::Timestamp),
    ("memory", Ty::List(&Ty::Ulid)),
]);

static TOOL_CALL_RECORDED: Ty = Ty::Record(&[
    ("call_id", Ty::Ulid),
    ("index", Ty::Int),
    ("tool", Ty::Str),
    ("class", Ty::OneOf(&["read", "think", "action"])),
    ("arguments_ref", Ty::DigestRef),
    ("provenance", Ty::List(&Ty::Ulid)),
]);

static TOOL_RESULT_RECORDED: Ty = Ty::Record(&[
    ("tool_call", Ty::Ulid),
    ("result", Ty::OneOf(&["ok", "refused", "failed"])),
    (
        "code",
        Ty::Nullable(&Ty::OneOf(&[
            "admissible",
            "removed",
            "exit_requested",
            "not_held",
            "unknown_instrument",
            "unknown_evidence",
            "duplicate_instrument",
            "forbidden_language",
            "revisions_not_enabled",
            "unknown_thesis",
            "direction_not_allowed",
            "asset_class_not_allowed",
            "not_eligible",
            "source_not_allowed",
            "not_corroborated",
            "universe_full",
            "not_admitted",
            "out_of_universe",
            "unknown_item",
            "unavailable",
            "call_budget",
            "max_theses",
            "max_repeat_denials",
            "run_ended",
        ])),
    ),
    ("untrusted", Ty::Bool),
    ("content_ref", Ty::DigestRef),
    ("bytes", Ty::Int),
    ("truncated", Ty::Bool),
    ("observations", Ty::List(&Ty::Ulid)),
]);

static RUN_SUMMARY_RECORDED: Ty = Ty::Record(&[
    ("instruments_read", Ty::List(&Ty::AssetId)),
    (
        "watch",
        Ty::List(&Ty::Record(&[
            ("instrument_id", Ty::AssetId),
            (
                "reason",
                Ty::OneOf(&[
                    "earnings",
                    "filing_pending",
                    "price_band",
                    "thesis_expiry",
                    "news_flow",
                ]),
            ),
        ])),
    ),
    ("proposed", Ty::Int),
    ("admitted", Ty::Int),
    ("invalidated", Ty::Int),
]);

static RUN_ENDED: Ty = Ty::Record(&[
    (
        "reason",
        Ty::OneOf(&[
            "done", "bound", "deadline", "cost", "mode", "error", "answered",
        ]),
    ),
    (
        "bound",
        Ty::Nullable(&Ty::OneOf(&[
            "max_turns",
            "max_tool_calls",
            "max_repeat_denials",
            "max_theses",
        ])),
    ),
    ("turns", Ty::Int),
    ("tool_calls", Ty::Int),
    ("result_bytes", Ty::Int),
    ("discarded", Ty::List(&Ty::Ulid)),
]);
