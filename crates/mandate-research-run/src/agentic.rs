//! The agentic research loop's pure state ([agent harness spec](../../../docs/specs/agent-harness.md)
//! v0.2 §6.9 to §6.11, ADR-0005, DEC-2203; backlog E19-13): the run kind, the tool menu fixed at the
//! start, the loop's bounds, and the fold of a run's own records that decides each next step.
//!
//! The runtime core (`mandate-runtime`, lane I2) owns the journal and the mediation of actions; this
//! module only decides, from journaled facts, what the next step is, so a replay decides the same
//! (HI-3). It reads no clock and does no I/O: the run's time left arrives as a duration.
//!
//! **Signatures only.** Every body that decides something returns
//! [`ResearchRunError::Unimplemented`] until slice 3 lands (DEC-990 item 3), so the core's tests can
//! be written against this shape first.

use std::collections::BTreeSet;

use mandate_gateway::money::Usd6;
use mandate_time::UtcNanos;

use crate::error::ResearchRunError;
use crate::item::{ItemId, Symbol};
use crate::output::CandidateOutput;

const STORY: &str = "E19-13";

/// The run kind, fixed at the start from the trigger and folded state (§6.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RunKind {
    Ideate,
    Monitor,
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
    Turn(ReplyInput),
    Result(ToolResult),
    Ended(EndReason),
}

/// The run's loop state: a pure fold of its own records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunState {
    start: RunStart,
    turns: u32,
    tool_calls: u32,
    result_bytes: u32,
    spent: Usd6,
    pending: Vec<PendingProposal>,
    ended: Option<EndReason>,
}

impl RunState {
    /// The state of a run that has just started (§6.9 step 1).
    pub fn start(start: RunStart) -> Self {
        Self {
            start,
            turns: 0,
            tool_calls: 0,
            result_bytes: 0,
            spent: Usd6::default(),
            pending: Vec::new(),
            ended: None,
        }
    }

    /// What the run froze at its start.
    pub fn started(&self) -> &RunStart {
        &self.start
    }

    /// The next turn's plan, given the run's time left on the risk clock (§6.9 step 3).
    ///
    /// # Errors
    /// [`ResearchRunError::Unimplemented`] until slice 3.
    pub fn turn(&self, time_left_ms: u64) -> Result<TurnPlan, ResearchRunError> {
        let _ = (time_left_ms, self.turns, self.spent, self.ended);
        Err(ResearchRunError::Unimplemented { story: STORY })
    }

    /// Takes a turn's reply: orders its calls, screens reads, applies the in-batch bounds (§6.9 step 4).
    /// The refusals it returns in [`Reply::refused`] count toward `max_repeat_denials` here;
    /// [`RunState::on_results`] counts only the dispatched results.
    ///
    /// # Errors
    /// [`ResearchRunError::Unimplemented`] until slice 3.
    pub fn on_reply(&mut self, reply: &ReplyInput) -> Result<Reply, ResearchRunError> {
        let _ = (reply, self.tool_calls);
        Err(ResearchRunError::Unimplemented { story: STORY })
    }

    /// Takes a dispatch's results, counting bytes and repeated refusals (§6.9 step 5).
    ///
    /// # Errors
    /// [`ResearchRunError::Unimplemented`] until slice 3.
    pub fn on_results(&mut self, results: &[ToolResult]) -> Result<Next, ResearchRunError> {
        let _ = (results, self.result_bytes);
        Err(ResearchRunError::Unimplemented { story: STORY })
    }

    /// Ends the run: which pending theses to commit, which to discard (§6.9 step 6).
    ///
    /// # Errors
    /// [`ResearchRunError::Unimplemented`] until slice 3.
    pub fn end(&mut self, reason: EndReason) -> Result<EndPlan, ResearchRunError> {
        let _ = (reason, &self.pending);
        Err(ResearchRunError::Unimplemented { story: STORY })
    }

    /// Folds one of the run's records back, so a replay rebuilds the state (HI-3).
    ///
    /// # Errors
    /// [`ResearchRunError::Unimplemented`] until slice 3.
    pub fn fold(&mut self, event: &FoldedEvent) -> Result<(), ResearchRunError> {
        let _ = event;
        Err(ResearchRunError::Unimplemented { story: STORY })
    }
}
