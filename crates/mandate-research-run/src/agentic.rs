//! The agentic research loop's pure state ([agent harness spec](../../../docs/specs/agent-harness.md)
//! v0.2 §6.9 to §6.11, ADR-0005, DEC-2203; backlog E19-13): the run kind, the tool menu fixed at the
//! start, the loop's bounds, and the fold of a run's own records that decides each next step.
//!
//! The runtime core (`mandate-runtime`, lane I2) owns the journal and the mediation of actions; this
//! module only decides, from journaled facts, what the next step is, so a replay decides the same
//! (HI-3). It reads no clock and does no I/O: the run's time left arrives as a duration.
//!
//! Slice 3 decides everything a `final_output` run needs: each turn's offer, a reply's total order,
//! the screening of reads against the run's frozen scope, the in-batch bounds, a dispatch's
//! results, and the end. An `action_tools` run's actions are ordered and counted against
//! `max_theses` here, but their verdicts, the repeated refusals among them, and the pending theses
//! a `done` end commits are the core's mediation, slice 4's (ADR-0005).

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::Value;
use mandate_gateway::money::Usd6;
use mandate_time::UtcNanos;

use crate::error::ResearchRunError;
use crate::item::{ItemId, ItemKind, Symbol};
use crate::output::CandidateOutput;

/// The run kind, fixed at the start from the trigger and folded state (§6.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RunKind {
    Ideate,
    Monitor,
}

impl RunKind {
    /// The kind's spelling in the gateway's request and the run's records (§10.1, DEC-2256 item 2).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ideate => "ideate",
            Self::Monitor => "monitor",
        }
    }
}

/// How the run gives its opinion: the pinned `output` (§6.9, inference spec §4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    FinalOutput,
    ActionTools,
}

/// A tool's class (§6.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ToolClass {
    Read,
    Think,
    Action,
}

/// The closed tool set of §6.10. A tool outside it cannot be named (HI-4, rung 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tool {
    MarketBars,
    MarketQuote,
    NewsSearch,
    NewsGet,
    FilingsSearch,
    FilingsGet,
    MemoryPositions,
    MemoryTheses,
    MemoryRuns,
    RecordsQuery,
    MandateRead,
    Note,
    Plan,
    ProposeThesis,
    Invalidate,
    ProposeExit,
}

impl Tool {
    /// Every tool, in declaration order.
    pub const ALL: [Self; 16] = [
        Self::MarketBars,
        Self::MarketQuote,
        Self::NewsSearch,
        Self::NewsGet,
        Self::FilingsSearch,
        Self::FilingsGet,
        Self::MemoryPositions,
        Self::MemoryTheses,
        Self::MemoryRuns,
        Self::RecordsQuery,
        Self::MandateRead,
        Self::Note,
        Self::Plan,
        Self::ProposeThesis,
        Self::Invalidate,
        Self::ProposeExit,
    ];

    /// The tool a declared name names, the inverse of [`Tool::name`]; `None` for any other text.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tool| tool.name() == name)
    }

    /// The declared name, as the pinned entry and `ToolCallRecorded.tool` carry it.
    pub const fn name(self) -> &'static str {
        match self {
            Self::MarketBars => "market.bars",
            Self::MarketQuote => "market.quote",
            Self::NewsSearch => "news.search",
            Self::NewsGet => "news.get",
            Self::FilingsSearch => "filings.search",
            Self::FilingsGet => "filings.get",
            Self::MemoryPositions => "memory.positions",
            Self::MemoryTheses => "memory.theses",
            Self::MemoryRuns => "memory.runs",
            Self::RecordsQuery => "records.query",
            Self::MandateRead => "mandate.read",
            Self::Note => "note",
            Self::Plan => "plan",
            Self::ProposeThesis => "propose_thesis",
            Self::Invalidate => "invalidate",
            Self::ProposeExit => "propose_exit",
        }
    }

    pub const fn class(self) -> ToolClass {
        match self {
            Self::Note | Self::Plan => ToolClass::Think,
            Self::ProposeThesis | Self::Invalidate | Self::ProposeExit => ToolClass::Action,
            Self::MarketBars
            | Self::MarketQuote
            | Self::NewsSearch
            | Self::NewsGet
            | Self::FilingsSearch
            | Self::FilingsGet
            | Self::MemoryPositions
            | Self::MemoryTheses
            | Self::MemoryRuns
            | Self::RecordsQuery
            | Self::MandateRead => ToolClass::Read,
        }
    }
}

/// The pinned loop bounds (§6.11). Money is not here: the run's ceiling is configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopBounds {
    pub max_turns: u32,
    pub max_tool_calls: u32,
    pub max_tool_calls_per_turn: u32,
    pub max_theses: u32,
    pub max_result_bytes: u32,
    pub max_run_result_bytes: u32,
    pub max_repeat_denials: u32,
    pub max_run_s: u32,
    pub tool_timeout_s: u32,
}

impl LoopBounds {
    /// §6.11's defaults (DEC-2203 item 8).
    pub const fn v0_2() -> Self {
        Self {
            max_turns: 8,
            max_tool_calls: 24,
            max_tool_calls_per_turn: 4,
            max_theses: 4,
            max_result_bytes: 24_000,
            max_run_result_bytes: 96_000,
            max_repeat_denials: 2,
            max_run_s: 900,
            tool_timeout_s: 10,
        }
    }
}

/// What the run froze at its start and `RunStarted` journals (§6.9 step 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunStart {
    pub kind: RunKind,
    pub output: OutputMode,
    pub menu: BTreeSet<Tool>,
    pub bounds: LoopBounds,
    pub cost_ceiling: Usd6,
    pub cut_off: UtcNanos,
    pub deadline: UtcNanos,
    pub allowlist_version: u32,
    pub data_universe: BTreeSet<Symbol>,
    /// The `event_id`s of the memory placed in the prompt's prefix (`RunStarted.memory`).
    pub memory: Vec<String>,
}

/// One tool call of a reply: its position in the reply, the tool, and its canonical arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub index: u32,
    pub tool: Tool,
    pub arguments: String,
}

/// A tool call's place in the run: the turn (from 1) and its index in that turn's reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ToolCallId {
    pub turn: u32,
    pub index: u32,
}

/// The closed set of model-visible codes (§6.13 item 5, journal spec §9.22 `code`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    Admissible,
    Removed,
    ExitRequested,
    NotHeld,
    UnknownInstrument,
    UnknownEvidence,
    DuplicateInstrument,
    ForbiddenLanguage,
    RevisionsNotEnabled,
    UnknownThesis,
    DirectionNotAllowed,
    AssetClassNotAllowed,
    NotEligible,
    SourceNotAllowed,
    NotCorroborated,
    UniverseFull,
    NotAdmitted,
    OutOfUniverse,
    UnknownItem,
    Unavailable,
    CallBudget,
    MaxTheses,
    MaxRepeatDenials,
    RunEnded,
}

impl Code {
    /// Every code, in declaration order.
    pub const ALL: [Self; 24] = [
        Self::Admissible,
        Self::Removed,
        Self::ExitRequested,
        Self::NotHeld,
        Self::UnknownInstrument,
        Self::UnknownEvidence,
        Self::DuplicateInstrument,
        Self::ForbiddenLanguage,
        Self::RevisionsNotEnabled,
        Self::UnknownThesis,
        Self::DirectionNotAllowed,
        Self::AssetClassNotAllowed,
        Self::NotEligible,
        Self::SourceNotAllowed,
        Self::NotCorroborated,
        Self::UniverseFull,
        Self::NotAdmitted,
        Self::OutOfUniverse,
        Self::UnknownItem,
        Self::Unavailable,
        Self::CallBudget,
        Self::MaxTheses,
        Self::MaxRepeatDenials,
        Self::RunEnded,
    ];

    /// The journal spec §9.22 spelling of the code.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Admissible => "admissible",
            Self::Removed => "removed",
            Self::ExitRequested => "exit_requested",
            Self::NotHeld => "not_held",
            Self::UnknownInstrument => "unknown_instrument",
            Self::UnknownEvidence => "unknown_evidence",
            Self::DuplicateInstrument => "duplicate_instrument",
            Self::ForbiddenLanguage => "forbidden_language",
            Self::RevisionsNotEnabled => "revisions_not_enabled",
            Self::UnknownThesis => "unknown_thesis",
            Self::DirectionNotAllowed => "direction_not_allowed",
            Self::AssetClassNotAllowed => "asset_class_not_allowed",
            Self::NotEligible => "not_eligible",
            Self::SourceNotAllowed => "source_not_allowed",
            Self::NotCorroborated => "not_corroborated",
            Self::UniverseFull => "universe_full",
            Self::NotAdmitted => "not_admitted",
            Self::OutOfUniverse => "out_of_universe",
            Self::UnknownItem => "unknown_item",
            Self::Unavailable => "unavailable",
            Self::CallBudget => "call_budget",
            Self::MaxTheses => "max_theses",
            Self::MaxRepeatDenials => "max_repeat_denials",
            Self::RunEnded => "run_ended",
        }
    }

    /// The code a §9.22 spelling names, the inverse of [`Code::as_str`]; `None` for any other text.
    pub fn from_spelling(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|code| code.as_str() == text)
    }
}

/// A tool result's outcome (§6.10's pairing table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolOutcome {
    Ok,
    Refused,
    Failed,
}

/// One tool call's result, as `ToolResultRecorded` journals it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResult {
    pub call: ToolCallId,
    pub outcome: ToolOutcome,
    pub code: Option<Code>,
    pub bytes: u32,
    pub truncated: bool,
    pub untrusted: bool,
    pub items: Vec<ItemId>,
}

/// What the next turn's call must carry (§6.9 step 3, §10.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnPlan {
    pub turn: u32,
    pub tools: BTreeSet<Tool>,
    pub spend_ceiling: Usd6,
    /// The run's time left on the risk clock, in milliseconds; the worker's wall deadline is its
    /// dispatch plus the lesser of this and the entry's `deadline_ms`.
    pub time_left_ms: u64,
    pub turns_left: u32,
    pub tool_calls_left: u32,
}

/// A turn's reply, as the gateway's call record gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyInput {
    pub turn: u32,
    pub ok: bool,
    pub cost: Usd6,
    pub tool_calls: Vec<ToolCall>,
    pub final_output: Option<CandidateOutput>,
}

/// What the core does with a reply (§6.9 step 4): the calls in the batch's total order, the ones
/// refused before anything runs, and whether the run ends here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub ordered: Vec<ToolCall>,
    pub refused: Vec<(ToolCallId, Code)>,
    pub dispatch: Vec<ToolCall>,
    pub ends: Option<EndReason>,
}

/// The step after a dispatch's results (§6.9 step 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    Turn { result_bytes_left: u32 },
    End(EndReason),
}

/// A bound that ends a run (`RunEnded.bound`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    MaxTurns,
    MaxToolCalls,
    MaxRepeatDenials,
    MaxTheses,
}

/// Why a run ended (`RunEnded.reason`, with its `bound`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    /// The model ended it, or its announced shape ran out (`Some(MaxTurns | MaxToolCalls)`).
    Done(Option<Bound>),
    Bound(Bound),
    Deadline,
    Cost,
    Mode,
    Error,
}

/// A thesis whose provisional result was `admissible`, waiting for a `done` end (§6.9 step 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingProposal {
    pub call: ToolCallId,
    pub arguments: String,
}

/// What the end batch does with the pending theses: commit them on `done`, else discard them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndPlan {
    pub reason: EndReason,
    pub commit: Vec<PendingProposal>,
    pub discarded: Vec<ToolCallId>,
}

/// A record of the run as the core folds it back, so replay rebuilds the state (HI-3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FoldedEvent {
    Started(RunStart),
    Seeded(Vec<(ItemKind, ItemId)>),
    Turn(ReplyInput),
    Result(ToolResult),
    Ended(EndReason),
}

/// A refused or failed result's identity for `max_repeat_denials`: the tool, its canonical
/// arguments, and the code (§6.11).
type DenialKey = (Tool, String, &'static str);

/// The run's loop state: a pure fold of its own records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunState {
    start: RunStart,
    turns: u32,
    tool_calls: u32,
    theses: u32,
    read_bytes: u32,
    spent: Usd6,
    received: BTreeSet<(ItemKind, ItemId)>,
    denials: BTreeMap<DenialKey, u32>,
    outstanding: Vec<ToolCallId>,
    dispatched: BTreeMap<ToolCallId, ToolCall>,
    refused_last: BTreeMap<ToolCallId, Code>,
    denied_at_dispatch: bool,
    pending: Vec<PendingProposal>,
    stop: Option<EndReason>,
    ended: Option<EndReason>,
}

impl RunState {
    /// The state of a run that has just started (§6.9 step 1).
    pub fn start(start: RunStart) -> Self {
        Self {
            start,
            turns: 0,
            tool_calls: 0,
            theses: 0,
            read_bytes: 0,
            spent: Usd6::ZERO,
            received: BTreeSet::new(),
            denials: BTreeMap::new(),
            outstanding: Vec::new(),
            dispatched: BTreeMap::new(),
            refused_last: BTreeMap::new(),
            denied_at_dispatch: false,
            pending: Vec::new(),
            stop: None,
            ended: None,
        }
    }

    /// What the run froze at its start.
    pub fn started(&self) -> &RunStart {
        &self.start
    }

    /// The read result bytes the run has left, which a dispatch carries (§6.9 step 5).
    pub fn result_bytes_left(&self) -> u32 {
        self.start
            .bounds
            .max_run_result_bytes
            .saturating_sub(self.read_bytes)
    }

    /// Takes the seed's items (§6.9 step 2), so a later `news.get` or `filings.get` may name them.
    ///
    /// # Errors
    /// [`ResearchRunError::LoopOrder`] once a turn has been taken or the run has ended.
    pub fn on_seed(&mut self, items: &[(ItemKind, ItemId)]) -> Result<(), ResearchRunError> {
        if self.turns > 0 || self.ended.is_some() {
            return Err(ResearchRunError::LoopOrder {
                what: "a seed after the run's first turn",
            });
        }
        self.received.extend(items.iter().cloned());
        Ok(())
    }

    /// The next turn's plan, given the run's time left on the risk clock (§6.9 step 3): the menu,
    /// only the action tools in the last turn, and no tool once no tool call is left.
    ///
    /// # Errors
    /// [`ResearchRunError::LoopOrder`] when the run has stopped, a dispatch is out, or no turn is
    /// left.
    pub fn turn(&self, time_left_ms: u64) -> Result<TurnPlan, ResearchRunError> {
        self.ready("a turn")?;
        let bounds = &self.start.bounds;
        if self.turns >= bounds.max_turns {
            return Err(ResearchRunError::LoopOrder {
                what: "a turn past max_turns",
            });
        }
        let turn = self.turns.saturating_add(1);
        Ok(TurnPlan {
            turn,
            tools: self.offer(turn),
            spend_ceiling: self.start.cost_ceiling.saturating_sub(self.spent),
            time_left_ms,
            turns_left: bounds.max_turns.saturating_sub(self.turns),
            tool_calls_left: self.calls_left(),
        })
    }

    /// Takes a turn's reply: orders its calls, screens reads, applies the in-batch bounds (§6.9 step 4).
    /// The refusals it returns in [`Reply::refused`] count toward `max_repeat_denials` here;
    /// [`RunState::on_results`] counts only the dispatched results.
    ///
    /// # Errors
    /// [`ResearchRunError::LoopOrder`] for a reply to a turn that was not asked for.
    pub fn on_reply(&mut self, reply: &ReplyInput) -> Result<Reply, ResearchRunError> {
        self.ready("a reply")?;
        let turn = self.turns.saturating_add(1);
        if reply.turn != turn || turn > self.start.bounds.max_turns {
            return Err(ResearchRunError::LoopOrder {
                what: "a reply to a turn that was not asked for",
            });
        }
        let offered = self.offer(turn);
        let calls_left = self.calls_left();
        self.turns = turn;
        self.spent = self
            .spent
            .checked_add(reply.cost)
            .unwrap_or(Usd6::from_micros(u64::MAX));
        self.refused_last.clear();
        if !reply.ok {
            return Ok(self.stopping(Vec::new(), Vec::new(), EndReason::Error));
        }
        let calls = &reply.tool_calls;
        let count = u32::try_from(calls.len()).unwrap_or(u32::MAX);
        self.tool_calls = self.tool_calls.saturating_add(count);
        if calls.is_empty() {
            let shape = if turn >= self.start.bounds.max_turns {
                Some(Bound::MaxTurns)
            } else if calls_left == 0 {
                Some(Bound::MaxToolCalls)
            } else {
                None
            };
            return Ok(self.stopping(Vec::new(), Vec::new(), EndReason::Done(shape)));
        }
        let ordered = total_order(calls);
        let well_formed = count <= self.start.bounds.max_tool_calls_per_turn
            && calls.iter().all(|call| offered.contains(&call.tool))
            && calls
                .iter()
                .enumerate()
                .all(|(at, call)| usize::try_from(call.index).is_ok_and(|index| index == at));
        if !well_formed {
            let refused = refuse_rest(&ordered, turn, &BTreeMap::new(), 0);
            return Ok(self.stopping(ordered, refused, EndReason::Error));
        }
        if count > calls_left {
            let refused = ordered
                .iter()
                .map(|call| (id(turn, call), Code::CallBudget))
                .collect();
            return Ok(self.stopping(ordered, refused, EndReason::Bound(Bound::MaxToolCalls)));
        }
        let mut codes: BTreeMap<u32, Code> = BTreeMap::new();
        let mut bound = None;
        let mut taken = 0;
        for (at, call) in ordered.iter().enumerate() {
            taken = at.saturating_add(1);
            match call.tool.class() {
                ToolClass::Action if call.tool == Tool::ProposeThesis => {
                    self.theses = self.theses.saturating_add(1);
                    if self.theses > self.start.bounds.max_theses {
                        codes.insert(call.index, Code::MaxTheses);
                        bound = Some(Bound::MaxTheses);
                    }
                }
                ToolClass::Read => {
                    if let Some(code) = self.screen(call) {
                        if self.deny(call, code) {
                            codes.insert(call.index, Code::MaxRepeatDenials);
                            bound = Some(Bound::MaxRepeatDenials);
                        } else {
                            codes.insert(call.index, code);
                        }
                    }
                }
                ToolClass::Action | ToolClass::Think => {}
            }
            if bound.is_some() {
                break;
            }
        }
        if let Some(bound) = bound {
            let refused = refuse_rest(&ordered, turn, &codes, taken);
            return Ok(self.stopping(ordered, refused, EndReason::Bound(bound)));
        }
        let refused: Vec<(ToolCallId, Code)> = ordered
            .iter()
            .filter_map(|call| codes.get(&call.index).map(|code| (id(turn, call), *code)))
            .collect();
        self.refused_last = refused.iter().copied().collect();
        let dispatch: Vec<ToolCall> = calls
            .iter()
            .filter(|call| call.tool.class() != ToolClass::Action)
            .filter(|call| !codes.contains_key(&call.index))
            .cloned()
            .collect();
        let mut ends = None;
        if dispatch.is_empty() {
            if turn >= self.start.bounds.max_turns {
                ends = Some(EndReason::Done(Some(Bound::MaxTurns)));
                self.stop = ends;
            }
        } else {
            self.outstanding = dispatch.iter().map(|call| id(turn, call)).collect();
            self.dispatched = dispatch
                .iter()
                .map(|call| (id(turn, call), call.clone()))
                .collect();
        }
        Ok(Reply {
            ordered,
            refused,
            dispatch,
            ends,
        })
    }

    /// Takes a dispatch's results, counting bytes and repeated refusals (§6.9 step 5).
    ///
    /// # Errors
    /// [`ResearchRunError::LoopOrder`] unless the results are exactly the dispatch's calls, in order.
    pub fn on_results(&mut self, results: &[ToolResult]) -> Result<Next, ResearchRunError> {
        let answered: Vec<ToolCallId> = results.iter().map(|result| result.call).collect();
        if self.ended.is_some() || self.outstanding.is_empty() || answered != self.outstanding {
            return Err(ResearchRunError::LoopOrder {
                what: "results that are not exactly the dispatch's calls, in order",
            });
        }
        for result in results {
            self.take_result(result)?;
        }
        Ok(self.after_dispatch())
    }

    /// Ends the run: which pending theses to commit, which to discard (§6.9 step 6). A `done` end
    /// is only the one the loop itself decided, so no caller can commit theses on a run a bound, an
    /// error, or a cancellation ended (HI-9); any other end discards, and is always accepted.
    ///
    /// # Errors
    /// [`ResearchRunError::LoopOrder`] for a run that has already ended, or a `done` end the loop
    /// did not decide.
    pub fn end(&mut self, reason: EndReason) -> Result<EndPlan, ResearchRunError> {
        if self.ended.is_some() {
            return Err(ResearchRunError::LoopOrder {
                what: "a second end",
            });
        }
        if matches!(reason, EndReason::Done(_)) && self.stop != Some(reason) {
            return Err(ResearchRunError::LoopOrder {
                what: "a done end the loop did not decide",
            });
        }
        self.ended = Some(reason);
        self.outstanding.clear();
        let (commit, discarded) = if matches!(reason, EndReason::Done(_)) {
            (self.pending.clone(), Vec::new())
        } else {
            (
                Vec::new(),
                self.pending.iter().map(|pending| pending.call).collect(),
            )
        };
        Ok(EndPlan {
            reason,
            commit,
            discarded,
        })
    }

    /// Folds one of the run's records back, so a replay rebuilds the state (HI-3). A refusal the
    /// reply's own fold already produced is checked and skipped; a dispatched result is taken, and
    /// the last one of a dispatch decides the step after it.
    ///
    /// # Errors
    /// [`ResearchRunError::LoopOrder`] for a record out of the run's order, or a refusal that is
    /// not the one the reply's fold produced.
    pub fn fold(&mut self, event: &FoldedEvent) -> Result<(), ResearchRunError> {
        match event {
            FoldedEvent::Started(start) => {
                if self.turns > 0 || self.ended.is_some() {
                    return Err(ResearchRunError::LoopOrder {
                        what: "a second start",
                    });
                }
                *self = Self::start(start.clone());
            }
            FoldedEvent::Seeded(items) => self.on_seed(items)?,
            FoldedEvent::Turn(reply) => {
                self.on_reply(reply)?;
            }
            FoldedEvent::Result(result) => {
                if let Some(code) = self.refused_last.get(&result.call) {
                    if result.outcome != ToolOutcome::Refused || result.code != Some(*code) {
                        return Err(ResearchRunError::LoopOrder {
                            what: "a refusal the reply's fold did not produce",
                        });
                    }
                } else if self.outstanding.first() == Some(&result.call) {
                    self.take_result(result)?;
                    if self.outstanding.is_empty() {
                        self.after_dispatch();
                    }
                } else {
                    return Err(ResearchRunError::LoopOrder {
                        what: "a result for no call the run has out",
                    });
                }
            }
            FoldedEvent::Ended(reason) => {
                self.end(*reason)?;
            }
        }
        Ok(())
    }

    fn ready(&self, what: &'static str) -> Result<(), ResearchRunError> {
        if self.ended.is_some() || self.stop.is_some() || !self.outstanding.is_empty() {
            return Err(ResearchRunError::LoopOrder { what });
        }
        Ok(())
    }

    fn calls_left(&self) -> u32 {
        self.start
            .bounds
            .max_tool_calls
            .saturating_sub(self.tool_calls)
    }

    /// The tools turn `turn` offers (§6.9 step 3).
    fn offer(&self, turn: u32) -> BTreeSet<Tool> {
        if self.calls_left() == 0 {
            return BTreeSet::new();
        }
        if turn >= self.start.bounds.max_turns {
            return self
                .start
                .menu
                .iter()
                .copied()
                .filter(|tool| tool.class() == ToolClass::Action)
                .collect();
        }
        self.start.menu.clone()
    }

    fn stopping(
        &mut self,
        ordered: Vec<ToolCall>,
        refused: Vec<(ToolCallId, Code)>,
        reason: EndReason,
    ) -> Reply {
        self.stop = Some(reason);
        self.refused_last = refused.iter().copied().collect();
        Reply {
            ordered,
            refused,
            dispatch: Vec::new(),
            ends: Some(reason),
        }
    }

    /// The code a read is refused before it runs, if any: a symbol outside the run's frozen scope,
    /// or an item the run never received of the kind the tool takes (§6.10). Arguments the
    /// gateway's schema check should already have refused (unparsable, a member missing or of the
    /// wrong type, no symbol) prove nothing in scope, so they are refused the same way.
    fn screen(&self, call: &ToolCall) -> Option<Code> {
        let arguments = mandate_canon::parse(call.arguments.as_bytes()).ok();
        let member = |name: &str| arguments.as_ref().and_then(|value| value.get(name));
        match call.tool {
            Tool::MarketBars => {
                let symbol = member("symbol").and_then(Value::as_str);
                (!symbol.is_some_and(|symbol| self.in_scope(symbol))).then_some(Code::OutOfUniverse)
            }
            Tool::MarketQuote | Tool::NewsSearch | Tool::FilingsSearch => {
                let symbols = member("symbols").and_then(Value::as_array);
                let all_in = symbols.is_some_and(|symbols| {
                    !symbols.is_empty()
                        && symbols
                            .iter()
                            .all(|symbol| symbol.as_str().is_some_and(|s| self.in_scope(s)))
                });
                (!all_in).then_some(Code::OutOfUniverse)
            }
            Tool::NewsGet => self.unreceived(member("item"), ItemKind::News),
            Tool::FilingsGet => self.unreceived(member("item"), ItemKind::Filing),
            Tool::MemoryPositions
            | Tool::MemoryTheses
            | Tool::MemoryRuns
            | Tool::RecordsQuery
            | Tool::MandateRead
            | Tool::Note
            | Tool::Plan
            | Tool::ProposeThesis
            | Tool::Invalidate
            | Tool::ProposeExit => None,
        }
    }

    fn in_scope(&self, symbol: &str) -> bool {
        Symbol::new(symbol).is_ok_and(|symbol| self.start.data_universe.contains(&symbol))
    }

    fn unreceived(&self, item: Option<&Value>, kind: ItemKind) -> Option<Code> {
        let known = item
            .and_then(Value::as_str)
            .and_then(|text| ItemId::new(text).ok())
            .is_some_and(|item| self.received.contains(&(kind, item)));
        (!known).then_some(Code::UnknownItem)
    }

    /// Counts one refusal; true when it is the repeat that reaches `max_repeat_denials` (§6.11).
    fn deny(&mut self, call: &ToolCall, code: Code) -> bool {
        let seen = self
            .denials
            .entry((call.tool, call.arguments.clone(), code.as_str()))
            .or_insert(0);
        *seen = seen.saturating_add(1);
        *seen >= self.start.bounds.max_repeat_denials
    }

    fn take_result(&mut self, result: &ToolResult) -> Result<(), ResearchRunError> {
        if self.outstanding.first() != Some(&result.call) {
            return Err(ResearchRunError::LoopOrder {
                what: "a result out of the dispatch's order",
            });
        }
        self.outstanding.remove(0);
        let Some(call) = self.dispatched.remove(&result.call) else {
            return Err(ResearchRunError::LoopOrder {
                what: "a result for no dispatched call",
            });
        };
        if call.tool.class() == ToolClass::Read {
            self.read_bytes = self.read_bytes.saturating_add(result.bytes);
        }
        let kind = match call.tool {
            Tool::NewsSearch | Tool::NewsGet => Some(ItemKind::News),
            Tool::FilingsSearch => Some(ItemKind::Filing),
            _ => None,
        };
        if let Some(kind) = kind.filter(|_| result.outcome == ToolOutcome::Ok) {
            self.received
                .extend(result.items.iter().cloned().map(|item| (kind, item)));
        }
        let denied = match (result.outcome, result.code) {
            (
                ToolOutcome::Ok,
                None | Some(Code::Admissible | Code::Removed | Code::ExitRequested),
            ) => None,
            (_, Some(code)) => Some(code),
            (_, None) => Some(Code::Unavailable),
        };
        if let Some(code) = denied
            && self.deny(&call, code)
        {
            self.denied_at_dispatch = true;
        }
        Ok(())
    }

    /// The step after a dispatch's last result: the bound a repeated refusal reached, the end of
    /// the last turn, or the next turn.
    fn after_dispatch(&mut self) -> Next {
        let ends = if std::mem::take(&mut self.denied_at_dispatch) {
            Some(EndReason::Bound(Bound::MaxRepeatDenials))
        } else if self.turns >= self.start.bounds.max_turns {
            Some(EndReason::Done(Some(Bound::MaxTurns)))
        } else {
            None
        };
        if let Some(reason) = ends {
            self.stop = Some(reason);
            return Next::End(reason);
        }
        Next::Turn {
            result_bytes_left: self.result_bytes_left(),
        }
    }
}

fn id(turn: u32, call: &ToolCall) -> ToolCallId {
    ToolCallId {
        turn,
        index: call.index,
    }
}

/// A reply's calls in the batch's one total order (§6.9 step 4): invalidations, then exit
/// requests, then theses, then reads and thinks, each group in reply order.
fn total_order(calls: &[ToolCall]) -> Vec<ToolCall> {
    let rank = |tool: Tool| match tool {
        Tool::Invalidate => 0,
        Tool::ProposeExit => 1,
        Tool::ProposeThesis => 2,
        _ => 3,
    };
    let mut ordered = calls.to_vec();
    ordered.sort_by_key(|call| rank(call.tool));
    ordered
}

/// The reply's refusals in the batch's order when a bound ends it there (§6.9 step 4, HI-24): the
/// codes already decided, and `run_ended` for every call with no result yet. An action taken
/// before the bound, among the first `mediated` calls of the order, has the core's verdict as its
/// result and is not refused.
fn refuse_rest(
    ordered: &[ToolCall],
    turn: u32,
    codes: &BTreeMap<u32, Code>,
    mediated: usize,
) -> Vec<(ToolCallId, Code)> {
    ordered
        .iter()
        .enumerate()
        .filter_map(|(at, call)| match codes.get(&call.index) {
            Some(code) => Some((id(turn, call), *code)),
            None if at < mediated && call.tool.class() == ToolClass::Action => None,
            None => Some((id(turn, call), Code::RunEnded)),
        })
        .collect()
}
