//! An agentic research run in the core (agent harness spec v0.2 §6.9 to §6.11, journal spec §9.23's
//! run records, DEC-2206): the run's kind and menu, fixed at its start from the trigger and folded
//! state; `RunStarted` before the seed; each turn's record heading its tool calls and the results
//! refused in its batch; the reply's reads and thinks dispatched only after that batch; their
//! results journaled before the next turn; a final output judged once, as a single call's is; and
//! the run's summary and end.
//!
//! The loop's decisions are `mandate_research_run::agentic::RunState`'s (lane AH): which calls a
//! reply runs, which it refuses, and when the run ends. This module journals them and asks the
//! worker for the next step; it never decides a bound itself, so a replay of the same records
//! decides the same (HI-3).

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::{Digest, Value, to_canonical};
use mandate_domain::AssetId;
use mandate_gateway::{CallRecord, Outcome};
use mandate_research::WorkingUniverse;
use mandate_research_run::agentic::{
    Bound, Code, EndReason, OutputMode, ReplyInput, RunKind, RunStart, RunState, Tool, ToolCall,
    ToolCallId, ToolClass, ToolOutcome, ToolResult,
};
use mandate_research_run::{CallPlan, ItemId, ItemKind, Symbol};
use mandate_time::UtcNanos;

use super::{
    AgenticConfig, Called, DispatchRequest, DispatchedResult, ItemFacts, ResearchConfig,
    ResearchContext, ResearchRequest, Run, TurnRef, TurnRequest, judge,
};
use crate::error::RuntimeError;
use crate::payload;
use crate::state::RuntimeState;
use crate::step::{Batch, observed};
use crate::types::{EventId, Mode, RiskClock};

/// The ordinal a turn's call id is derived at: far above any draft's ordinal and far below the run
/// and thesis ids' (`super::polled`, `judge`), so no derived id meets another. Each turn's id is
/// derived at the head of the step that asks for it, and every such step journals, so no two
/// turns share a head.
const TURN_ORDINAL: u32 = 1 << 31;

/// An agentic run in flight: the loop's state and what the core needs to journal its records.
#[derive(Debug, Clone)]
pub(super) struct Agentic {
    pub(super) state: RunState,
    pub(super) deadline: RiskClock,
    /// Every observation of the run, in the order the prompt holds them.
    pub(super) observations: Vec<EventId>,
    pub(super) items: Vec<ItemFacts>,
    /// Each item the run has observed, by its id, and the observation that holds it.
    observed: BTreeMap<ItemId, EventId>,
    stage: Stage,
    /// Each tool call's `ToolCallRecorded`, by its place in the run.
    calls: BTreeMap<ToolCallId, EventId>,
    transcript: Vec<TurnRef>,
    counts: Counts,
    instruments_read: BTreeSet<String>,
}

impl Agentic {
    /// Whether a turn's call is out, so a reply to it may arrive.
    pub(super) fn turning(&self) -> bool {
        matches!(self.stage, Stage::Turning { .. })
    }
}

/// Where the run is between the worker's answers.
#[derive(Debug, Clone)]
enum Stage {
    Seeding,
    /// Turn `turn`'s call is out.
    Turning {
        turn: u32,
    },
    /// Turn `turn`'s reads and thinks are out; `results` holds each call's content so far, by
    /// index, and `response` the reply's artifact.
    Dispatching {
        turn: u32,
        response: Digest,
        results: BTreeMap<u32, Digest>,
        /// Each dispatched call's index and tool, in reply order.
        dispatched: Vec<(u32, Tool)>,
    },
}

/// What `RunEnded` counts (journal spec §9.23).
#[derive(Debug, Clone, Copy, Default)]
struct Counts {
    turns: u64,
    tool_calls: u64,
    result_bytes: u64,
    proposed: u64,
    admitted: u64,
    invalidated: u64,
}

/// The kind of run due at a scheduled poll, and its menu (§6.9's table): `ideate` in `normal` with
/// room in the working universe; `monitor` in `exits_only` or with a full universe while the agent
/// has an active thesis; else none.
pub(super) fn due_kind<'c>(
    mode: Mode,
    agentic: &'c AgenticConfig,
    context: &ResearchContext<'_>,
) -> Option<(RunKind, &'c BTreeSet<Tool>)> {
    let max = context.mandate.envelope().max_instruments;
    let full = match context.universe {
        WorkingUniverse::Known { instruments, .. } => {
            u32::try_from(instruments.len()).map_or(true, |held| held >= max)
        }
        WorkingUniverse::Unavailable => true,
    };
    let active = !context.active_theses.is_empty();
    match mode {
        Mode::Normal if !full => Some((RunKind::Ideate, &agentic.ideate_menu)),
        Mode::Normal | Mode::ExitsOnly if active => Some((RunKind::Monitor, &agentic.monitor_menu)),
        _ => None,
    }
}

/// The modes an agentic run may run in: `normal`, and `exits_only` for a `monitor` (§6.9).
pub(super) fn runs_in(mode: Mode) -> bool {
    matches!(mode, Mode::Normal | Mode::ExitsOnly)
}

/// A run's start (§6.9 step 1): its loop state, and `RunStarted` journaled with what it froze. The
/// data universe's canonical bytes are returned for the shell to store (DEC-2206 item 4). `event`
/// is the `ObservationRecorded` of the item that brought a `monitor` run forward, journaled just
/// before in the same batch, or `None` for a scheduled run (journal spec rule 271, DEC-2392).
pub(super) fn start(
    kind: RunKind,
    menu: &BTreeSet<Tool>,
    agentic: &AgenticConfig,
    cut_off: RiskClock,
    context: &ResearchContext<'_>,
    batch: &mut Batch<'_>,
    event: Option<&EventId>,
) -> Result<(Agentic, Vec<u8>), RuntimeError> {
    let deadline = RiskClock::from_secs(
        cut_off
            .secs()
            .checked_add(i64::from(agentic.bounds.max_run_s))
            .ok_or_else(|| payload::non_canonical("deadline"))?,
    );
    let universe: BTreeSet<String> = context
        .facts
        .data_universe
        .iter()
        .flatten()
        .map(|id| id.as_str().to_owned())
        .collect();
    let universe_bytes = to_canonical(&Value::Array(
        universe.iter().map(|id| payload::text(id)).collect(),
    ));
    let allowlist_version = context.facts.allowlist.version.0;
    let names: BTreeSet<&str> = menu.iter().map(|tool| tool.name()).collect();
    let body = payload::object(vec![
        ("kind", payload::text(kind_name(kind))),
        (
            "trigger",
            payload::object(vec![
                (
                    "kind",
                    payload::text(if event.is_some() { "event" } else { "schedule" }),
                ),
                (
                    "cause",
                    event.map_or(Value::Null, |cause| payload::text(&cause.0)),
                ),
            ])?,
        ),
        ("output", payload::text(output_name(agentic.output))),
        (
            "menu",
            Value::Array(names.iter().map(|name| payload::text(name)).collect()),
        ),
        ("cut_off", payload::stamp(cut_off, "cut_off")?),
        (
            "allowlist_version",
            payload::count(u64::from(allowlist_version), "allowlist_version")?,
        ),
        ("data_universe_ref", reference(&Digest::of(&universe_bytes))),
        (
            "cost_ceiling_usd",
            payload::text(&agentic.cost_ceiling.to_string()),
        ),
        ("deadline", payload::stamp(deadline, "deadline")?),
        ("memory", Value::Array(Vec::new())),
    ])?;
    batch.journal("RunStarted", None, body)?;
    let state = RunState::start(RunStart {
        kind,
        output: agentic.output,
        menu: menu.clone(),
        bounds: agentic.bounds,
        cost_ceiling: agentic.cost_ceiling,
        cut_off: UtcNanos::from_parts(cut_off.secs(), 0)?,
        deadline: UtcNanos::from_parts(deadline.secs(), 0)?,
        allowlist_version,
        data_universe: context.data_symbols.clone(),
        memory: Vec::new(),
    });
    Ok((
        Agentic {
            state,
            deadline,
            observations: Vec::new(),
            items: Vec::new(),
            observed: BTreeMap::new(),
            stage: Stage::Seeding,
            calls: BTreeMap::new(),
            transcript: Vec::new(),
            counts: Counts::default(),
            instruments_read: BTreeSet::new(),
        },
        universe_bytes,
    ))
}

/// What one agentic step produces besides its drafts: the worker's next request, the artifacts the
/// core made, and whether the run ended.
#[derive(Default)]
pub(super) struct Out {
    pub(super) requests: Vec<ResearchRequest>,
    pub(super) artifacts: Vec<Vec<u8>>,
    pub(super) ended: bool,
}

/// The seed's observations, then turn 1 (§6.9 steps 2 and 3, HI-2).
pub(super) fn seeded(
    run: &mut Run,
    agentic: &mut Agentic,
    items: &[super::RetrievedObservation],
    config: &ResearchConfig,
    state: &RuntimeState,
    batch: &mut Batch<'_>,
) -> Result<Out, RuntimeError> {
    if !matches!(agentic.stage, Stage::Seeding) {
        return Err(RuntimeError::NotInterpreted {
            what: "a second seed for an agentic run".to_owned(),
            story: "E19-13",
        });
    }
    observe(agentic, items, batch)?;
    let received: Vec<(ItemKind, ItemId)> = items
        .iter()
        .filter(|item| matches!(item.item.kind, ItemKind::News | ItemKind::Filing))
        .map(|item| (item.item.kind, item.item.item_id.clone()))
        .collect();
    agentic.state.on_seed(&received).map_err(loop_error)?;
    let now = state
        .risk_clock()
        .map_or(run.cut_off, |at| at.max(run.cut_off));
    let mut out = Out::default();
    next_turn(run, agentic, config, state, now, batch, &mut out)?;
    Ok(out)
}

/// Adds the asset IDs of the instruments a read's items are about to the run's `instruments_read`
/// (journal spec §9.23, DEC-2267): each item's symbols resolved through the seed's instrument facts,
/// a symbol the seed does not map left out. The item's `ObservationRecorded` keeps its own
/// symbol-derived `instrument_id` either way.
///
/// The facts are the run's own `Instrument` items: retrieval emits one per symbol in scope beside
/// the reads about it (`retrieve`'s `covered` set), so the map is what the seed resolved and nothing
/// is looked up afresh here. A symbol with no `Instrument` item — the vector's `IBM`, which the seed
/// has no fact for — resolves to nothing and stays out of the summary.
fn read_instruments(
    agentic: &mut Agentic,
    items: &[super::RetrievedObservation],
) -> Result<(), RuntimeError> {
    let by_symbol: BTreeMap<&Symbol, &AssetId> = agentic
        .items
        .iter()
        .filter_map(|facts| {
            let fact = facts.instrument.as_ref()?;
            Some((&fact.symbol, &fact.asset_id))
        })
        .collect();
    let mut read: BTreeSet<String> = BTreeSet::new();
    for item in items {
        for symbol in &item.item.symbols {
            if let Some(asset) = by_symbol.get(symbol) {
                read.insert(asset.to_string());
            }
        }
    }
    agentic.instruments_read.extend(read);
    Ok(())
}

/// Journals each item the run has not yet observed, in order, as `ObservationRecorded`, and returns
/// the observation of every item given, new or not.
fn observe(
    agentic: &mut Agentic,
    items: &[super::RetrievedObservation],
    batch: &mut Batch<'_>,
) -> Result<Vec<EventId>, RuntimeError> {
    let mut named = Vec::new();
    for item in items {
        if let Some(id) = agentic.observed.get(&item.item.item_id) {
            named.push(id.clone());
            continue;
        }
        let id = batch.journal("ObservationRecorded", None, observed(&item.observation)?)?;
        agentic
            .observed
            .insert(item.item.item_id.clone(), id.clone());
        agentic.observations.push(id.clone());
        agentic.items.push(item.item.clone());
        named.push(id);
    }
    Ok(named)
}

/// Asks for the next turn, or ends the run when the loop has none left.
fn next_turn(
    run: &mut Run,
    agentic: &mut Agentic,
    config: &ResearchConfig,
    state: &RuntimeState,
    now: RiskClock,
    batch: &mut Batch<'_>,
    out: &mut Out,
) -> Result<(), RuntimeError> {
    let left = agentic.deadline.secs().saturating_sub(now.secs());
    let time_left_ms = u64::try_from(left).unwrap_or(0).saturating_mul(1_000);
    let plan = agentic.state.turn(time_left_ms).map_err(loop_error)?;
    let call_id = batch.id_at(TURN_ORDINAL.saturating_add(plan.turn));
    let deployment = state.deployment();
    run.call_id = call_id.0.clone();
    agentic.stage = Stage::Turning { turn: plan.turn };
    out.requests
        .push(ResearchRequest::Turn(Box::new(TurnRequest {
            call: CallPlan {
                run_id: run.run_id.clone(),
                call_id: call_id.0,
                workspace_id: deployment.workspace.0.clone(),
                agent_id: deployment.agent.0.clone(),
                model: run.pin.clone(),
                as_of: UtcNanos::from_parts(run.cut_off.secs(), 0)?,
                deadline_ms: config.deadline_ms,
                max_output_tokens: config.max_output_tokens,
                observations: agentic.observations.iter().map(|id| id.0.clone()).collect(),
            },
            kind: agentic.state.started().kind,
            plan,
            transcript: agentic.transcript.clone(),
        })));
    Ok(())
}

/// The turn's reply (§6.9 step 4). `record` is the run's own call's (the caller checked it).
#[allow(
    clippy::too_many_arguments,
    reason = "one step's inputs, each read once; a struct would only rename them"
)]
pub(super) fn replied(
    run: &mut Run,
    agentic: &mut Agentic,
    record: &CallRecord,
    at: RiskClock,
    config: &ResearchConfig,
    context: &ResearchContext<'_>,
    state: &RuntimeState,
    batch: &mut Batch<'_>,
) -> Result<Out, RuntimeError> {
    let Stage::Turning { turn } = agentic.stage else {
        return Err(RuntimeError::NotInterpreted {
            what: "a reply for an agentic run with no turn out".to_owned(),
            story: "E19-13",
        });
    };
    let mut out = Out::default();
    let ok = record.outcome == Outcome::Ok;
    let menu = &agentic.state.started().menu;
    let calls: Option<Vec<ToolCall>> = record
        .tool_calls
        .iter()
        .map(|call| {
            menu.iter()
                .copied()
                .find(|tool| tool.name() == call.name)
                .map(|tool| ToolCall {
                    index: call.index,
                    tool,
                    arguments: String::from_utf8_lossy(&to_canonical(&call.arguments)).into_owned(),
                })
        })
        .collect();
    agentic.counts.turns = agentic.counts.turns.saturating_add(1);
    let Some(calls) = calls.filter(|_| ok) else {
        batch.journal(
            "ModelInvocationRecorded",
            None,
            super::invocation(record, &[])?,
        )?;
        let reason = if ok {
            EndReason::Error
        } else {
            failed(record.outcome)
        };
        end(agentic, reason, batch)?;
        out.ended = true;
        return Ok(out);
    };
    if calls.is_empty() {
        return final_output(run, agentic, record, turn, config, context, batch, out);
    }
    let reply = agentic
        .state
        .on_reply(&ReplyInput {
            turn,
            ok,
            cost: record.cost,
            tool_calls: calls.clone(),
            final_output: None,
        })
        .map_err(loop_error)?;
    let head = batch.journal(
        "ModelInvocationRecorded",
        None,
        super::invocation(record, &[])?,
    )?;
    for call in &calls {
        let arguments = call.arguments.clone().into_bytes();
        let body = payload::object(vec![
            ("call_id", payload::text(&record.call_id)),
            ("index", payload::count(u64::from(call.index), "index")?),
            ("tool", payload::text(call.tool.name())),
            ("class", payload::text(class_name(call.tool.class()))),
            ("arguments_ref", reference(&Digest::of(&arguments))),
            ("provenance", Value::Array(Vec::new())),
        ])?;
        out.artifacts.push(arguments);
        let id = batch.journal("ToolCallRecorded", Some(head.clone()), body)?;
        agentic.calls.insert(
            ToolCallId {
                turn,
                index: call.index,
            },
            id,
        );
        agentic.counts.tool_calls = agentic.counts.tool_calls.saturating_add(1);
    }
    let mut results = BTreeMap::new();
    for (call, code) in &reply.refused {
        let content = to_canonical(&payload::object(vec![(
            "error",
            payload::text(code.as_str()),
        )])?);
        let digest = Digest::of(&content);
        journal_result(
            agentic,
            *call,
            ToolOutcome::Refused,
            Some(*code),
            &digest,
            u32::try_from(content.len()).unwrap_or(u32::MAX),
            false,
            false,
            &[],
            batch,
        )?;
        out.artifacts.push(content);
        results.insert(call.index, digest);
    }
    let response = record.response_ref.unwrap_or_else(|| Digest::of(&[]));
    if let Some(reason) = reply.ends {
        end(agentic, reason, batch)?;
        out.ended = true;
        return Ok(out);
    }
    if reply.dispatch.is_empty() {
        agentic.transcript.push(TurnRef {
            response,
            results: results.into_values().collect(),
        });
        next_turn(run, agentic, config, state, at, batch, &mut out)?;
        return Ok(out);
    }
    let started = agentic.state.started();
    out.requests
        .push(ResearchRequest::Dispatch(DispatchRequest {
            run_id: run.run_id.clone(),
            turn,
            calls: reply.dispatch.clone(),
            result_bytes_left: agentic.state.result_bytes_left(),
            cut_off: run.cut_off,
            allowlist_version: started.allowlist_version,
        }));
    agentic.stage = Stage::Dispatching {
        turn,
        response,
        results,
        dispatched: reply
            .dispatch
            .iter()
            .map(|call| (call.index, call.tool))
            .collect(),
    };
    Ok(out)
}

/// A reply the run may no longer use (§5.3): journaled with no verdicts and no tool call, and the
/// run ends with `reason` in the same batch.
pub(super) fn abandoned(
    agentic: &mut Agentic,
    record: &CallRecord,
    reason: EndReason,
    batch: &mut Batch<'_>,
) -> Result<Out, RuntimeError> {
    batch.journal(
        "ModelInvocationRecorded",
        None,
        super::invocation(record, &[])?,
    )?;
    agentic.counts.turns = agentic.counts.turns.saturating_add(1);
    end(agentic, reason, batch)?;
    Ok(Out {
        ended: true,
        ..Out::default()
    })
}

/// A reply with no tool call is the final output (§6.9 step 4): judged once, as a single call's is,
/// invalidations first (DEC-2202), its verdicts on the turn's record; then the run ends `done`.
#[allow(
    clippy::too_many_arguments,
    reason = "one step's inputs, each read once; a struct would only rename them"
)]
fn final_output(
    run: &mut Run,
    agentic: &mut Agentic,
    record: &CallRecord,
    turn: u32,
    config: &ResearchConfig,
    context: &ResearchContext<'_>,
    batch: &mut Batch<'_>,
    mut out: Out,
) -> Result<Out, RuntimeError> {
    let called = Called {
        observations: &agentic.observations,
        items: &agentic.items,
    };
    let judged = if agentic.state.started().output == OutputMode::FinalOutput {
        judge::judge(
            run,
            &called,
            record,
            config,
            context,
            batch,
            agentic.state.started().kind,
        )
        .unwrap_or_default()
    } else {
        judge::Judged::default()
    };
    if agentic.state.started().kind == RunKind::Monitor && judged.refused {
        return refused_while_monitoring(agentic, record, turn, batch, out);
    }
    let (body, invalidations, theses) = match super::invocation(record, &judged.verdicts) {
        Ok(body) => (body, judged.invalidations, judged.theses),
        Err(_) => (super::invocation(record, &[])?, Vec::new(), Vec::new()),
    };
    let id = batch.journal("ModelInvocationRecorded", None, body)?;
    for invalidation in invalidations {
        batch.journal("ThesisInvalidated", Some(id.clone()), invalidation)?;
        agentic.counts.invalidated = agentic.counts.invalidated.saturating_add(1);
    }
    for thesis in theses {
        agentic.counts.proposed = agentic.counts.proposed.saturating_add(1);
        let admitted =
            thesis.output.is_some() && thesis.body.get("admitted") == Some(&Value::Bool(true));
        if admitted {
            agentic.counts.admitted = agentic.counts.admitted.saturating_add(1);
        }
        let thesis_record = batch.journal(thesis.event_type, Some(id.clone()), thesis.body)?;
        if let Some(output) = thesis.output {
            batch.journal("ModelOutputRecorded", Some(thesis_record), output)?;
        }
    }
    let reply = agentic
        .state
        .on_reply(&ReplyInput {
            turn,
            ok: true,
            cost: record.cost,
            tool_calls: Vec::new(),
            final_output: None,
        })
        .map_err(loop_error)?;
    end(agentic, reply.ends.unwrap_or(EndReason::Done(None)), batch)?;
    out.ended = true;
    Ok(out)
}

/// A `monitor` reply outside its schema, as one that proposes a thesis is (§6.9): the gateway pins
/// a monitoring run's output schema to `invalidations` alone, so such a reply never reaches judging
/// there; the core's strict read refuses it the same way, whatever the gateway did. The turn's
/// record is journaled with no verdicts, nothing of its output is used, and the run ends `error`,
/// as a `schema_invalid` reply's does.
fn refused_while_monitoring(
    agentic: &mut Agentic,
    record: &CallRecord,
    turn: u32,
    batch: &mut Batch<'_>,
    mut out: Out,
) -> Result<Out, RuntimeError> {
    batch.journal(
        "ModelInvocationRecorded",
        None,
        super::invocation(record, &[])?,
    )?;
    let reply = agentic
        .state
        .on_reply(&ReplyInput {
            turn,
            ok: false,
            cost: record.cost,
            tool_calls: Vec::new(),
            final_output: None,
        })
        .map_err(loop_error)?;
    end(agentic, reply.ends.unwrap_or(EndReason::Error), batch)?;
    out.ended = true;
    Ok(out)
}

/// A dispatch's results (§6.9 step 5): for each, in reply order, its new items' observations and its
/// `ToolResultRecorded`; then the next turn, or the run's end.
pub(super) fn dispatched(
    run: &mut Run,
    agentic: &mut Agentic,
    results: &[DispatchedResult],
    at: RiskClock,
    config: &ResearchConfig,
    state: &RuntimeState,
    batch: &mut Batch<'_>,
) -> Result<Out, RuntimeError> {
    let Stage::Dispatching {
        turn,
        response,
        results: mut contents,
        dispatched,
    } = agentic.stage.clone()
    else {
        return Err(RuntimeError::NotInterpreted {
            what: "results for an agentic run with no dispatch out".to_owned(),
            story: "E19-13",
        });
    };
    let answered: Vec<u32> = results.iter().map(|r| r.call.index).collect();
    let indices: Vec<u32> = dispatched.iter().map(|(index, _)| *index).collect();
    if answered != indices || results.iter().any(|r| r.call.turn != turn) {
        return Err(RuntimeError::NotInterpreted {
            what: "results that are not exactly the dispatch's calls, in order".to_owned(),
            story: "E19-13",
        });
    }
    let mut out = Out::default();
    let mut loop_results = Vec::new();
    for result in results {
        let named = observe(agentic, &result.items, batch)?;
        let mut observations: Vec<&str> = named.iter().map(|id| id.0.as_str()).collect();
        observations.sort_unstable();
        observations.dedup();
        if !result.items.is_empty() {
            read_instruments(agentic, &result.items)?;
        }
        journal_result(
            agentic,
            result.call,
            result.outcome,
            result.code,
            &result.content,
            result.bytes,
            result.truncated,
            result.untrusted,
            &observations,
            batch,
        )?;
        let read = dispatched
            .iter()
            .any(|(index, tool)| *index == result.call.index && tool.class() == ToolClass::Read);
        if read {
            agentic.counts.result_bytes = agentic
                .counts
                .result_bytes
                .saturating_add(u64::from(result.bytes));
        }
        contents.insert(result.call.index, result.content);
        loop_results.push(ToolResult {
            call: result.call,
            outcome: result.outcome,
            code: result.code,
            bytes: result.bytes,
            truncated: result.truncated,
            untrusted: result.untrusted,
            items: result
                .items
                .iter()
                .map(|item| item.item.item_id.clone())
                .collect(),
        });
    }
    agentic.transcript.push(TurnRef {
        response,
        results: contents.into_values().collect(),
    });
    match agentic
        .state
        .on_results(&loop_results)
        .map_err(loop_error)?
    {
        mandate_research_run::agentic::Next::Turn { .. } => {
            next_turn(run, agentic, config, state, at, batch, &mut out)?;
        }
        mandate_research_run::agentic::Next::End(reason) => {
            end(agentic, reason, batch)?;
            out.ended = true;
        }
    }
    Ok(out)
}

/// The gateway refused the turn out (DEC-2261): the run ends `error`; with no turn out, the refusal
/// is not interpreted.
pub(super) fn refused(
    _run: &mut Run,
    agentic: &mut Agentic,
    batch: &mut Batch<'_>,
) -> Result<Out, RuntimeError> {
    if !matches!(agentic.stage, Stage::Turning { .. }) {
        return Err(RuntimeError::NotInterpreted {
            what: "a refused turn for an agentic run with no turn out".to_owned(),
            story: "E19-13",
        });
    }
    end(agentic, EndReason::Error, batch)?;
    Ok(Out {
        ended: true,
        ..Out::default()
    })
}

/// One `ToolResultRecorded`, after its call's record.
#[allow(
    clippy::too_many_arguments,
    reason = "the record's members, each written once"
)]
fn journal_result(
    agentic: &Agentic,
    call: ToolCallId,
    outcome: ToolOutcome,
    code: Option<Code>,
    content: &Digest,
    bytes: u32,
    truncated: bool,
    untrusted: bool,
    observations: &[&str],
    batch: &mut Batch<'_>,
) -> Result<(), RuntimeError> {
    let Some(named) = agentic.calls.get(&call) else {
        return Err(RuntimeError::NotInterpreted {
            what: "a result for a tool call the run never recorded".to_owned(),
            story: "E19-13",
        });
    };
    let body = payload::object(vec![
        ("tool_call", payload::text(&named.0)),
        ("result", payload::text(outcome_name(outcome))),
        (
            "code",
            code.map_or(Value::Null, |code| payload::text(code.as_str())),
        ),
        ("untrusted", Value::Bool(untrusted)),
        ("content_ref", reference(content)),
        ("bytes", payload::count(u64::from(bytes), "bytes")?),
        ("truncated", Value::Bool(truncated)),
        (
            "observations",
            Value::Array(observations.iter().map(|id| payload::text(id)).collect()),
        ),
    ])?;
    batch.journal("ToolResultRecorded", Some(named.clone()), body)?;
    Ok(())
}

/// The run's end (§6.9 step 6): its summary, then `RunEnded` with the loop's reason. The pending
/// theses of an `action_tools` run are not yet mediated here (ADR-0005 slice 4), so a `final_output`
/// run discards nothing.
pub(super) fn end(
    agentic: &mut Agentic,
    reason: EndReason,
    batch: &mut Batch<'_>,
) -> Result<(), RuntimeError> {
    let plan = agentic.state.end(reason).map_err(loop_error)?;
    let mut discarded: Vec<&str> = plan
        .discarded
        .iter()
        .filter_map(|call| agentic.calls.get(call))
        .map(|id| id.0.as_str())
        .collect();
    discarded.sort_unstable();
    let summary = payload::object(vec![
        (
            "instruments_read",
            Value::Array(
                agentic
                    .instruments_read
                    .iter()
                    .map(|id| payload::text(id))
                    .collect(),
            ),
        ),
        ("watch", Value::Array(Vec::new())),
        (
            "proposed",
            payload::count(agentic.counts.proposed, "proposed")?,
        ),
        (
            "admitted",
            payload::count(agentic.counts.admitted, "admitted")?,
        ),
        (
            "invalidated",
            payload::count(agentic.counts.invalidated, "invalidated")?,
        ),
    ])?;
    batch.journal("RunSummaryRecorded", None, summary)?;
    let (name, bound) = end_names(plan.reason);
    let body = payload::object(vec![
        ("reason", payload::text(name)),
        ("bound", bound.map_or(Value::Null, payload::text)),
        ("turns", payload::count(agentic.counts.turns, "turns")?),
        (
            "tool_calls",
            payload::count(agentic.counts.tool_calls, "tool_calls")?,
        ),
        (
            "result_bytes",
            payload::count(agentic.counts.result_bytes, "result_bytes")?,
        ),
        (
            "discarded",
            Value::Array(discarded.iter().map(|id| payload::text(id)).collect()),
        ),
    ])?;
    batch.journal("RunEnded", None, body)?;
    Ok(())
}

/// The end an error outcome gives (§6.9 step 6): `cost` for a cap, `deadline` for a turn ended on
/// its deadline, `error` for every other.
fn failed(outcome: Outcome) -> EndReason {
    match outcome {
        Outcome::BudgetExhausted => EndReason::Cost,
        Outcome::DeadlineExceeded => EndReason::Deadline,
        _ => EndReason::Error,
    }
}

fn loop_error(error: mandate_research_run::ResearchRunError) -> RuntimeError {
    match error {
        mandate_research_run::ResearchRunError::Unimplemented { story } => {
            RuntimeError::Unimplemented { story }
        }
        other => RuntimeError::NotInterpreted {
            what: format!("the research loop refused a step ({})", other.code()),
            story: "E19-13",
        },
    }
}

/// An artifact's reference as a record names it: `sha256:` and the digest's hex (journal spec §3).
fn reference(digest: &Digest) -> Value {
    payload::text(&format!("sha256:{digest}"))
}

fn kind_name(kind: RunKind) -> &'static str {
    match kind {
        RunKind::Ideate => "ideate",
        RunKind::Monitor => "monitor",
    }
}

fn output_name(output: OutputMode) -> &'static str {
    match output {
        OutputMode::FinalOutput => "final_output",
        OutputMode::ActionTools => "action_tools",
    }
}

fn class_name(class: ToolClass) -> &'static str {
    match class {
        ToolClass::Read => "read",
        ToolClass::Think => "think",
        ToolClass::Action => "action",
    }
}

fn outcome_name(outcome: ToolOutcome) -> &'static str {
    match outcome {
        ToolOutcome::Ok => "ok",
        ToolOutcome::Refused => "refused",
        ToolOutcome::Failed => "failed",
    }
}

fn end_names(reason: EndReason) -> (&'static str, Option<&'static str>) {
    match reason {
        EndReason::Done(bound) => ("done", bound.map(bound_name)),
        EndReason::Bound(bound) => ("bound", Some(bound_name(bound))),
        EndReason::Deadline => ("deadline", None),
        EndReason::Cost => ("cost", None),
        EndReason::Mode => ("mode", None),
        EndReason::Error => ("error", None),
    }
}

fn bound_name(bound: Bound) -> &'static str {
    match bound {
        Bound::MaxTurns => "max_turns",
        Bound::MaxToolCalls => "max_tool_calls",
        Bound::MaxRepeatDenials => "max_repeat_denials",
        Bound::MaxTheses => "max_theses",
    }
}
