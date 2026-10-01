//! E17-6's correlated-flow controls, the pure halves (DEC-100, DEC-123, DEC-293): the
//! aggregate-flow monitor that lets an operator **see** research-agent exposure concentrate, and
//! the operator per-thesis halt that lets them **stop** it. The admission half of the halt is
//! already live — §8.5 check 9 refuses a thesis for a halted instrument, the halt set arriving as
//! an `AdmissionFacts` field (E17-3, MC-N10) — so this module owns the two controls that half
//! does not: the sums and the removal.
//!
//! **The monitor is a pure function, and that is the safety property.** DEC-100 places it "in the
//! workspace deployment ..., outside the trade path", where it "writes to none of them, and sends
//! nothing to the global control plane" (DEC-10); as code that is a function over typed rows
//! returning a report value — no entry point of this crate accepts a journal, a stream, or a
//! workspace handle to write through, so the boundary is representable rather than promised. The
//! operator *service* that gathers rows and shows the report is wiring outside this crate, exactly
//! as E17-2's model invocation and E17-5's escalation seam are. The monitor never halts by
//! itself (DEC-123): its output is a report, and only the operator's halt changes anything.
//!
//! **Rows are per agent, grouped by workspace, and a pinned agent contributes nothing.** For a
//! dynamic mandate every instrument in the working universe is research-admitted (§2.3), so an
//! agent's exposure over its working universe is its research flow; the caller derives the dollar
//! values, the crate's house pattern for facts produced elsewhere. A pinned agent is
//! bring-your-own-strategy — the owner's own universe, per-account controls only — so its
//! instruments are not correlated research flow and never enter the aggregate, and a halt never
//! touches its universe (MI-20's mirror: a pinned mandate has no research flow to stop).
//!
//! **The threshold is DEC-123's, and the alert is strictly above it**: the lower of 1% of the
//! instrument's 20-day average daily dollar volume and 1,000,000 USD per instrument per
//! deployment — at the bar the row is quiet, one cent above it alerts. The share of ADV is
//! reported (one rounding of one quotient at the 12-place report scale), and an instrument whose
//! ADV is missing keeps the share `None` and alerts, because the threshold is undecidable and
//! reading missing data as quiet is the fail-open direction AGENTS.md rule 3 bars.
//!
//! **The halt only removes permissions** (DEC-100: "never permits anything a workspace's own
//! limits deny"): its application produces `UniverseChanged` removals with reason `operator_halt`
//! and `RemovedInstrument` restrictions — the exits-only semantics of §2.2 — and nothing else.
//! It reaches a workspace only through that workspace's own resolved halt set, derived from its
//! own journaled `PlatformOperatorAction`, which is what makes it the one sanctioned way one
//! workspace's situation changes another's decisions (DEC-09): no state crosses, only the
//! operator's recorded action.

use std::collections::{BTreeMap, BTreeSet};

use mandate_num::{Ratio, Usd};

use crate::{
    AssetId, ContentHash, InstrumentRestriction, ResearchError, ResearchEvent, UniverseEntry,
    WorkingUniverse, WorkspaceId,
};

/// The scale the report's share of ADV rounds at (DEC-293, the E4-2 report discipline of one
/// rounding of one quotient) — the same scale the `score` module's figures round at.
pub const REPORT_SCALE: u32 = 12;

/// One agent's research flow in its deployment (DEC-100): the workspace it runs in, whether its
/// mandate pins the universe (bring-your-own-strategy, which contributes nothing and is never
/// halted), and its research exposure per instrument in dollars — the positions it holds in the
/// instruments its working universe admitted, as the caller values them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentFlow<'a> {
    /// The workspace hosting the agent; several agents may share one.
    pub workspace: &'a WorkspaceId,
    /// The agent's mandate pins its universe, so its flow is not research flow.
    pub pinned: bool,
    /// The agent's research exposure per instrument, in dollars.
    pub exposure: &'a BTreeMap<AssetId, Usd>,
}

/// Everything one aggregate-flow pass reads (DEC-100): the deployment's agents, and each
/// instrument's 20-day average daily dollar volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowInput<'a> {
    /// The deployment's agents, pinned ones included (they are skipped, not filtered by the
    /// caller, so the bring-your-own-strategy exclusion is this crate's own invariant).
    pub agents: &'a [AgentFlow<'a>],
    /// Each instrument's 20-day average daily dollar volume, in dollars.
    pub average_daily_dollar_volume: &'a BTreeMap<AssetId, Usd>,
}

/// One instrument's aggregate research flow over a deployment (DEC-123): what the operator sees
/// before deciding to halt. Every figure is exact or the one named rounding, and the row carries
/// everything a reader needs to recompute the alert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowRow {
    /// The instrument the flow concentrates in.
    pub instrument: AssetId,
    /// The deployment's summed research exposure to it, in dollars.
    pub exposure_usd: Usd,
    /// The exposure as a share of the instrument's 20-day average daily dollar volume, rounded
    /// once at [`REPORT_SCALE`]; `None` when the volume is missing or zero, so the share is
    /// named absent rather than guessed.
    pub share_of_adv: Option<Ratio>,
    /// Whether the row alerts: exposure strictly above the lower of 1% of average daily dollar
    /// volume and 1,000,000 USD (DEC-123). A missing volume alerts, because the threshold is
    /// undecidable and quiet would be the fail-open reading.
    pub alerts: bool,
    /// The workspaces whose research agents hold the instrument, in order, each once.
    pub contributing_workspaces: Vec<WorkspaceId>,
}

/// The aggregate-flow report of one deployment: one row per instrument with research exposure
/// above zero, ordered by instrument id, so the same rows give the same report every run (ES-21).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowReport {
    /// The rows, ordered by instrument id.
    pub rows: Vec<FlowRow>,
}

/// One operator per-thesis halt (DEC-100): an instrument, and optionally the research-agent
/// version — its pinned content hash (DEC-67) — the halt is scoped to. An unscoped halt matches
/// every workspace; a hash-scoped one matches only a workspace whose admitting model carries that
/// hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorHalt {
    /// The instrument the halt names.
    pub instrument: AssetId,
    /// The research agent's pinned content hash, when the halt is scoped to one version.
    pub research_agent: Option<ContentHash>,
}

/// The outcome of applying the operator's halts to one workspace's working universe: what was
/// removed, the entries to journal, and the universe and restrictions after — [`Expiry`]'s shape,
/// because a halt and a thesis expiry are both removals from the same fold.
///
/// [`Expiry`]: crate::Expiry
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HaltOutcome {
    /// The working universe after the halts.
    pub universe: WorkingUniverse,
    /// The instruments removed, in instrument order.
    pub removed: Vec<AssetId>,
    /// The exits-only restriction each removal earns (§2.2): `RemovedInstrument`, never a mode
    /// change and never a permission.
    pub instrument_restrictions: BTreeMap<AssetId, InstrumentRestriction>,
    /// The `UniverseChanged` entries to journal, one per removal, citing the halted instrument's
    /// thesis and lineage (journal spec §9's row requires both for every reason).
    pub journal: Vec<ResearchEvent>,
}

/// DEC-100's matching rule, resolved for one workspace: the instruments its admitting model's
/// theses are halted for. An unscoped halt matches every workspace; a hash-scoped halt matches
/// only when `admitting_model` is that hash, so a pinned mandate (no admitting model) is matched
/// by none of them. The admitting hash arrives as an argument — a fact the caller reads from the
/// mandate — because the crate's narrow envelope view carries no model triple.
///
/// The matching itself is total; the `Result` exists for the DEC-77 stub protocol and always
/// returns `Ok` once implemented.
///
/// # Errors
/// Returns [`ResearchError::Unimplemented`] until E17-6's implementation lands.
pub fn halted_instruments(
    halts: &[OperatorHalt],
    admitting_model: Option<&ContentHash>,
) -> Result<BTreeSet<AssetId>, ResearchError> {
    let _ = (halts, admitting_model);
    Err(ResearchError::Unimplemented(
        "flow::halted_instruments",
        "E17-6",
    ))
}

/// DEC-100's halt applied to one workspace's fold. An unread universe is an error, never a
/// no-op: the fold cannot know what it holds, and a silently dropped halt would be the fail-open
/// direction. A pinned universe is returned unchanged with an empty journal — the pinned list is
/// the owner's, and a pinned mandate has no research flow to stop (MI-20's mirror). A dynamic
/// universe: every halted instrument the entries hold is removed, journaled as `UniverseChanged`
/// (removed, `operator_halt`) with `universe_size_after` sinking by one per removal, in
/// instrument order; a halted instrument the entries do not hold removes nothing and journals
/// nothing, because check 9 already holds its admissions. The removal is exits-only (§2.2), and
/// the application produces removals only — it never permits anything a workspace's own limits
/// deny.
///
/// # Errors
/// Returns [`ResearchError::UniverseUnavailable`] for an unread working universe,
/// [`ResearchError::DuplicateInstrument`] for an entry list naming one instrument twice, and
/// [`ResearchError::Unimplemented`] until E17-6's implementation lands.
pub fn apply_operator_halts(
    halted: &BTreeSet<AssetId>,
    entries: &[UniverseEntry],
    universe: &WorkingUniverse,
) -> Result<HaltOutcome, ResearchError> {
    let _ = (halted, entries, universe);
    Err(ResearchError::Unimplemented(
        "flow::apply_operator_halts",
        "E17-6",
    ))
}

/// DEC-100's aggregate-flow monitor over one deployment: the research agents' exposure summed per
/// instrument (pinned agents contribute nothing), each row carrying its dollars, its share of
/// 20-day average daily dollar volume, its contributing workspaces, and whether it alerts above
/// DEC-123's threshold — the lower of 1% of that volume and 1,000,000 USD per instrument per
/// deployment, strictly above. Instruments with no research exposure are absent; the rows are
/// ordered by instrument id. The report is a value: the monitor writes to no workspace, sends
/// nothing anywhere, and halts nothing (DEC-123: the operator issues the halt).
///
/// # Errors
/// Returns [`ResearchError::NegativeExposure`] for a negative exposure row (v1 is long-only, and
/// a negative row would under-state the aggregate) and [`ResearchError::Unimplemented`] until
/// E17-6's implementation lands.
pub fn aggregate_flow(input: &FlowInput<'_>) -> Result<FlowReport, ResearchError> {
    let _ = input;
    Err(ResearchError::Unimplemented(
        "flow::aggregate_flow",
        "E17-6",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stable code (ES-09) of the one error this module mints, pinned live because the
    /// mutation gate runs only live tests: a `code()` arm no live test reads would be a mutant
    /// nothing catches, and the pending tests do not run under the gate.
    #[test]
    fn the_negative_exposure_code_is_pinned() {
        assert_eq!(ResearchError::NegativeExposure.code(), "negative_exposure");
    }
}
