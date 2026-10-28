//! The model-gateway service's own record of one gate run (DEC-2217 item 4, DEC-2218): the run's
//! seed retrieved from its corpus slice, its loop driven by the replies the service itself gave, and
//! its reads dispatched over the slice, through the same pure steps the research worker and the
//! runtime core take (`retrieve`, [`RunState`], [`dispatch`], [`TurnCall`]). Each call the gate Job
//! sends is held to the turn this record expects: every member equal to the one it would build,
//! except the runtime's own journal ids (each observation, and each memory thesis's id), which are
//! held to their form and to staying the same across the run's turns. A call that differs in
//! anything else is [`Unmatched`], so a Job can end a gate early but never steer the model.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::{Key, Object, Value, is_ulid, parse, to_canonical};
use mandate_gateway::kinds::{KeyOwner, Outcome, Purpose};
use mandate_gateway::{CallRecord, CallRequest};
use mandate_time::UtcNanos;

use crate::agentic::{
    EndReason, LoopBounds, Next, OutputMode, ReplyInput, RunKind, RunStart, RunState, Tool,
    ToolCall, ToolClass, ToolResult, TurnPlan,
};
use crate::call::{CallContext, CallPlan, MemoryThesis};
use crate::dispatch::{DispatchScope, dispatch};
use crate::entry::ResearchEntry;
use crate::gate::{ProbeContext, ProbeRun, ProbeSet};
use crate::item::{Allowlist, ItemId, ItemKind};
use crate::plan::Scope;
use crate::plane::RecordedPlane;
use crate::ports::Readers;
use crate::retrieve::{Retrieval, retrieve};
use crate::turn::{EarlierTurn, TurnCall};

/// The agent id the service's record builds with. A gate call names no agent, so this never
/// leaves the record: the expected request is compared with the agent and purpose set aside.
const NO_AGENT: &str = "gate";

/// Why a call does not fit the run, or a run cannot be shadowed: the member that differs, by its
/// request name, or the step that failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unmatched(pub &'static str);

/// What the record reads of the service's own record of a turn's call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answered {
    pub ok: bool,
    pub cost: mandate_gateway::money::Usd6,
    pub tool_calls: Vec<mandate_gateway::tools::ToolCall>,
    /// The response bytes, as the next turn's transcript carries them.
    pub response: Vec<u8>,
    /// The validated final output, when the reply gave one.
    pub output: Option<Value>,
}

impl From<&CallRecord> for Answered {
    fn from(record: &CallRecord) -> Self {
        Self {
            ok: record.outcome == Outcome::Ok,
            cost: record.cost,
            tool_calls: record.tool_calls.clone(),
            response: record.response.clone().unwrap_or_default(),
            output: record.output.clone(),
        }
    }
}

/// How a reply leaves the run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// The run takes another turn, whose call [`Shadow::check`] now expects.
    Turn,
    /// The run ended, with the final output when the reply gave one.
    Ended {
        reason: EndReason,
        output: Option<Value>,
    },
}

/// The service's record of one gate run.
#[derive(Debug, Clone)]
pub struct Shadow {
    run: ProbeRun,
    plane: RecordedPlane,
    entry: ResearchEntry,
    workspace: String,
    description: String,
    context: ProbeContext,
    scope: Scope,
    bounds: LoopBounds,
    seed: Retrieval,
    state: RunState,
    plan: TurnPlan,
    turn: u32,
    observations: Vec<String>,
    new_items: usize,
    /// Every item the run's dispatches read, in order.
    reads: Vec<ItemId>,
    thesis_ids: Option<Vec<String>>,
    transcript: Vec<EarlierTurn>,
    awaiting_reply: bool,
    ended: bool,
}

impl Shadow {
    /// The record of `set`'s run `index` for `entry`, researched in `workspace`, before its first
    /// turn.
    ///
    /// # Errors
    /// [`Unmatched`] naming the step that failed: no such run, an entry with no agentic loop, or a
    /// seed that does not retrieve.
    pub fn start(
        set: &ProbeSet,
        index: usize,
        entry: &ResearchEntry,
        workspace: &str,
    ) -> Result<Self, Unmatched> {
        let run = set.runs.get(index).ok_or(Unmatched("run"))?.clone();
        let plane = set.plane(&run).ok_or(Unmatched("corpus"))?.clone();
        let bounds = entry.agentic.ok_or(Unmatched("agentic"))?;
        let scope = Scope {
            workspace: workspace.to_owned(),
            agent: NO_AGENT.to_owned(),
            cut_off: run.cut_off,
            data_universe: run.universe.clone(),
            held: BTreeSet::new(),
            allowlist: Allowlist {
                version: set.allowlist_version,
                sources: set.sources.clone(),
            },
        };
        let seed = retrieve(&entry.plan, &scope, readers(&plane)).map_err(|_| Unmatched("seed"))?;
        let deadline = after_secs(run.cut_off, bounds.max_run_s)?;
        let mut state = RunState::start(RunStart {
            kind: run.kind,
            output: OutputMode::FinalOutput,
            menu: menu(entry)?,
            bounds,
            cost_ceiling: set.context.run_usd,
            cut_off: run.cut_off,
            deadline,
            allowlist_version: set.allowlist_version,
            data_universe: run.universe.clone(),
            memory: Vec::new(),
        });
        let seeded: Vec<(ItemKind, ItemId)> = seed
            .items
            .iter()
            .filter(|item| matches!(item.kind(), ItemKind::News | ItemKind::Filing))
            .map(|item| Ok((item.kind(), item.item_id().map_err(|_| Unmatched("seed"))?)))
            .collect::<Result<_, Unmatched>>()?;
        state.on_seed(&seeded).map_err(|_| Unmatched("seed"))?;
        let plan = state
            .turn(time_left(&bounds))
            .map_err(|_| Unmatched("turn"))?;
        Ok(Self {
            new_items: seed.items.len(),
            run,
            plane,
            entry: entry.clone(),
            workspace: workspace.to_owned(),
            description: set.context.description.clone(),
            context: set.context.clone(),
            scope,
            bounds,
            seed,
            state,
            plan,
            turn: 1,
            observations: Vec::new(),
            reads: Vec::new(),
            thesis_ids: None,
            transcript: Vec::new(),
            awaiting_reply: false,
            ended: false,
        })
    }

    /// Whether the run has ended.
    pub fn ended(&self) -> bool {
        self.ended
    }

    /// The run's kind.
    pub fn kind(&self) -> RunKind {
        self.run.kind
    }

    /// How many new observations the next call must add: one per seed item on the first turn, then
    /// one per item the last dispatch read first.
    pub fn observations_due(&self) -> usize {
        self.new_items
    }

    /// The request this record expects for its next turn, under the runtime's `call_id`, its
    /// `observations` and its memory `thesis_ids`, dispatched at `at`: the call the research worker
    /// builds for the turn, as an evaluation, with no agent.
    ///
    /// # Errors
    /// [`Unmatched`] when the record cannot build the turn's call.
    pub fn expected(
        &self,
        call_id: &str,
        observations: Vec<String>,
        thesis_ids: &[String],
        at: UtcNanos,
    ) -> Result<CallRequest, Unmatched> {
        if thesis_ids.len() != self.run.theses.len() {
            return Err(Unmatched("inputs"));
        }
        let content = self.entry.content();
        let plan = CallPlan {
            run_id: String::new(),
            call_id: call_id.to_owned(),
            workspace_id: self.workspace.clone(),
            agent_id: NO_AGENT.to_owned(),
            model: content.model_ref(),
            as_of: self.run.cut_off,
            deadline_ms: u32::try_from(self.entry.deadline_ms)
                .map_err(|_| Unmatched("deadline"))?,
            max_output_tokens: self.entry.max_output_tokens,
            observations,
        };
        let universe: Vec<_> = self.run.universe.iter().cloned().collect();
        let context = CallContext {
            description: self.description.clone(),
            data_universe: universe.clone(),
            asset_classes: self.context.asset_classes.clone(),
            max_instruments: self.context.max_instruments,
            working_universe: if self.run.kind == RunKind::Monitor {
                universe
            } else {
                Vec::new()
            },
            held: Vec::new(),
            theses: self
                .run
                .theses
                .iter()
                .zip(thesis_ids)
                .map(|(probe, id)| MemoryThesis {
                    thesis_id: id.clone(),
                    symbol: probe.symbol.clone(),
                    expires_at: probe.expires_at,
                })
                .collect(),
        };
        let mut request = TurnCall::build(
            &plan,
            &self.plan,
            self.run.kind,
            &self.seed,
            context,
            self.transcript.clone(),
            at,
        )
        .and_then(|call| call.request())
        .map_err(|_| Unmatched("turn"))?;
        request.purpose = Purpose::Evaluation;
        request.agent_id = None;
        Ok(request)
    }

    /// Holds `request` to the turn this record expects (DEC-2217 item 4): every member equal to the
    /// one it would build, the journal ids held to their form and constancy (DEC-2218). On a match
    /// the record takes the request's ids and awaits the turn's reply.
    ///
    /// # Errors
    /// [`Unmatched`] naming the first member that differs, or `turn` when no call is due.
    pub fn check(&mut self, request: &CallRequest) -> Result<(), Unmatched> {
        if self.ended || self.awaiting_reply {
            return Err(Unmatched("turn"));
        }
        if request.purpose != Purpose::Evaluation {
            return Err(Unmatched("purpose"));
        }
        if request.agent_id.is_some() {
            return Err(Unmatched("agent_id"));
        }
        if request.key_owner != KeyOwner::Platform {
            return Err(Unmatched("key_owner"));
        }
        let observations = self.observations_of(request)?;
        let theses = self.memory_of(request)?;
        let ids: Vec<String> = theses.iter().map(|t| t.thesis_id.clone()).collect();
        let expected =
            self.expected(&request.call_id, observations.clone(), &ids, request.as_of)?;
        let members: [(&'static str, bool); 8] = [
            (
                "workspace_id",
                request.workspace_id == expected.workspace_id,
            ),
            ("model", request.model == expected.model),
            ("as_of", request.as_of == expected.as_of),
            ("run_kind", request.run_kind == expected.run_kind),
            ("inputs", request.inputs == expected.inputs),
            ("turns", request.turns == expected.turns),
            ("tools", request.tools == expected.tools),
            (
                "spend_ceiling",
                request.spend_ceiling == expected.spend_ceiling,
            ),
        ];
        if let Some((member, _)) = members.iter().find(|(_, same)| !same) {
            return Err(Unmatched(member));
        }
        self.observations = observations;
        self.new_items = 0;
        self.thesis_ids = Some(ids);
        self.awaiting_reply = true;
        Ok(())
    }

    /// Takes the service's own `record` of the turn's call: the run's loop decides from it as the
    /// runtime core decides, the record dispatching each read over the corpus itself.
    ///
    /// # Errors
    /// [`Unmatched`] when no reply is due, or a loop step fails.
    pub fn reply(&mut self, record: &Answered) -> Result<Step, Unmatched> {
        if self.ended || !self.awaiting_reply {
            return Err(Unmatched("turn"));
        }
        self.awaiting_reply = false;
        let turn = self.turn;
        let ok = record.ok;
        let menu = &self.state.started().menu;
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
                        arguments: String::from_utf8_lossy(&to_canonical(&call.arguments))
                            .into_owned(),
                    })
            })
            .collect();
        let Some(calls) = calls.filter(|_| ok) else {
            return Ok(self.end(EndReason::Error, None));
        };
        if calls.is_empty() {
            let reply = self
                .state
                .on_reply(&ReplyInput {
                    turn,
                    ok: true,
                    cost: record.cost,
                    tool_calls: Vec::new(),
                    final_output: None,
                })
                .map_err(|_| Unmatched("reply"))?;
            return Ok(self.end(
                reply.ends.unwrap_or(EndReason::Done(None)),
                record.output.clone(),
            ));
        }
        let reply = self
            .state
            .on_reply(&ReplyInput {
                turn,
                ok,
                cost: record.cost,
                tool_calls: calls,
                final_output: None,
            })
            .map_err(|_| Unmatched("reply"))?;
        let mut contents: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
        for (call, code) in &reply.refused {
            contents.insert(call.index, refusal(code.as_str())?);
        }
        if let Some(reason) = reply.ends {
            return Ok(self.end(reason, None));
        }
        let response = record.response.clone();
        if reply.dispatch.is_empty() {
            self.transcript.push(EarlierTurn {
                response,
                results: contents.into_values().collect(),
            });
            return self.next_turn();
        }
        let ran = dispatch(
            turn,
            &reply.dispatch,
            DispatchScope {
                scope: &self.scope,
                bounds: &self.bounds,
                result_bytes_left: self.state.result_bytes_left(),
            },
            readers(&self.plane),
        )
        .map_err(|_| Unmatched("dispatch"))?;
        let mut seen: BTreeSet<ItemId> = self
            .seed
            .items
            .iter()
            .filter_map(|item| item.item_id().ok())
            .collect();
        seen.extend(self.read_items());
        let mut results = Vec::with_capacity(ran.results.len());
        let mut fresh = 0_usize;
        for result in ran.results {
            let items: Vec<ItemId> = result
                .items
                .iter()
                .map(|item| item.item_id().map_err(|_| Unmatched("dispatch")))
                .collect::<Result<_, _>>()?;
            for item in &items {
                if seen.insert(item.clone()) {
                    fresh = fresh.saturating_add(1);
                }
            }
            results.push(ToolResult {
                call: result.call,
                outcome: result.outcome,
                code: result.code,
                bytes: u32::try_from(result.content.len()).unwrap_or(u32::MAX),
                truncated: result.truncated,
                untrusted: result.untrusted,
                items,
            });
            contents.insert(result.call.index, result.content);
        }
        self.reads.extend(
            results
                .iter()
                .flat_map(|result| result.items.iter().cloned()),
        );
        self.new_items = fresh;
        self.transcript.push(EarlierTurn {
            response,
            results: contents.into_values().collect(),
        });
        match self
            .state
            .on_results(&results)
            .map_err(|_| Unmatched("results"))?
        {
            Next::Turn { .. } => self.next_turn(),
            Next::End(reason) => Ok(self.end(reason, None)),
        }
    }

    fn next_turn(&mut self) -> Result<Step, Unmatched> {
        self.plan = self
            .state
            .turn(time_left(&self.bounds))
            .map_err(|_| Unmatched("turn"))?;
        self.turn = self.plan.turn;
        Ok(Step::Turn)
    }

    fn end(&mut self, reason: EndReason, output: Option<Value>) -> Step {
        let _ended_once = self.state.end(reason);
        self.ended = true;
        Step::Ended { reason, output }
    }

    /// The request's observations, held to the record's: on the first turn one per seed item, each
    /// a ULID; on a later one the earlier turn's, then one per item the last dispatch read first.
    /// No two are the same.
    fn observations_of(&self, request: &CallRequest) -> Result<Vec<String>, Unmatched> {
        let held = &request.observations;
        let expected = self.observations.len().saturating_add(self.new_items);
        let unique: BTreeSet<&String> = held.iter().collect();
        let fits = held.len() == expected
            && held.starts_with(&self.observations)
            && held.iter().all(|id| is_ulid(id))
            && unique.len() == held.len();
        if fits {
            Ok(held.clone())
        } else {
            Err(Unmatched("observations"))
        }
    }

    /// The request's memory, held to the run's theses: each the probe thesis's symbol and expiry in
    /// order, its id the runtime's own, a ULID, kept from the first turn on.
    fn memory_of(&self, request: &CallRequest) -> Result<Vec<MemoryThesis>, Unmatched> {
        let memory = request
            .inputs
            .get("memory")
            .and_then(|text| parse(text.as_bytes()).ok())
            .ok_or(Unmatched("inputs"))?;
        let entries = memory.as_array().ok_or(Unmatched("inputs"))?;
        if entries.len() != self.run.theses.len() {
            return Err(Unmatched("inputs"));
        }
        let theses: Vec<MemoryThesis> = entries
            .iter()
            .zip(&self.run.theses)
            .map(|(entry, probe)| {
                let id = entry
                    .get("thesis_id")
                    .and_then(Value::as_str)
                    .filter(|id| is_ulid(id))
                    .ok_or(Unmatched("inputs"))?;
                Ok(MemoryThesis {
                    thesis_id: id.to_owned(),
                    symbol: probe.symbol.clone(),
                    expires_at: probe.expires_at,
                })
            })
            .collect::<Result<_, Unmatched>>()?;
        if let Some(kept) = &self.thesis_ids
            && theses.iter().map(|t| &t.thesis_id).ne(kept.iter())
        {
            return Err(Unmatched("inputs"));
        }
        Ok(theses)
    }

    fn read_items(&self) -> impl Iterator<Item = ItemId> + '_ {
        self.reads.iter().cloned()
    }
}

/// The run's menu: the entry's declared tools but actions, which a `final_output` loop never
/// offers, on both run kinds, as the cell's runtime builds it.
fn menu(entry: &ResearchEntry) -> Result<BTreeSet<Tool>, Unmatched> {
    let content = entry.content();
    let declared = content.agentic().ok_or(Unmatched("agentic"))?;
    Ok(declared
        .tools
        .iter()
        .filter_map(|tool| Tool::from_name(&tool.name))
        .filter(|tool| tool.class() != ToolClass::Action)
        .collect())
}

/// A refused call's result, as the runtime core writes it: `{"error": <code>}`, canonical.
fn refusal(code: &str) -> Result<Vec<u8>, Unmatched> {
    let key = Key::new("error").map_err(|_| Unmatched("results"))?;
    let mut object = Object::new();
    object.insert(key, Value::Str(code.to_owned()));
    Ok(to_canonical(&Value::Object(object)))
}

fn readers(plane: &RecordedPlane) -> Readers<'_> {
    Readers {
        bars: plane,
        news: plane,
        filings: plane,
        instruments: plane,
    }
}

/// The whole run's time, in milliseconds: the record never runs out of time itself, since a gate
/// session's own bound ends it (DEC-2217 item 2).
fn time_left(bounds: &LoopBounds) -> u64 {
    u64::from(bounds.max_run_s).saturating_mul(1_000)
}

fn after_secs(at: UtcNanos, secs: u32) -> Result<UtcNanos, Unmatched> {
    let total = at
        .secs()
        .checked_add(i64::from(secs))
        .ok_or(Unmatched("deadline"))?;
    UtcNanos::from_parts(total, at.nanos()).map_err(|_| Unmatched("deadline"))
}
