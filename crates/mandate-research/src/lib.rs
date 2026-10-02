#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]
//! The research-agent contract of [mandate spec §8](../../../docs/specs/mandate.md#8-signal-models-and-the-order-builder)
//! as code: a thesis in, and an admission, a lineage fold, a removal, a stagger offset, an
//! input-drift verdict, a forward-paper scorecard, or a correlated-flow report or operator halt
//! out (backlog E17-2, E17-3, E17-5, E17-6's monitoring and halt half, E17-7, E17-8's scoring
//! half, and E17-9's fold; [task brief](../../../docs/project/tasks/M5-J-research-thin-slice.md),
//! DEC-132, DEC-281, DEC-293).
//!
//! **This crate never calls a model.** A model output arrives as a [`Thesis`] value that something
//! else produced, and there is no HTTP client, no prompt, no artifact store, and no provider here.
//! That is what makes the whole §8.5 decision a pure function a fuzzer can drive, which E17-3's
//! acceptance clause asks for, and it is why the LLM is out of scope (DEC-132 item 2). The platform
//! boundary that turns a provider's JSON into a [`Thesis`] is E17-2's shell story;
//! `python/research_spike/` is the spike that found the shape.
//!
//! Every entry point is pure: it reads only its arguments, keeps no state between calls, touches no
//! clock, no filesystem and no randomness, orders every collection, and returns the same value for
//! the same inputs, so a replay reproduces every admission (ADR-0001 ES-21). None of them appends an
//! event: [`ResearchEvent`] values are returned and the executor journals them, with the
//! `causation_id` that ties a [`ResearchEvent::UniverseChanged`] in the account stream to its thesis
//! entry in the agent stream (journal spec §1, §2, §9; DEC-132 item 17).
//!
//! **What the agent may not decide about itself.** A [`Thesis`] carries what the agent said. Every
//! fact about the *instrument* is in [`InstrumentFacts`] and every fact about the *evidence* is in
//! [`ProposedThesis::corroboration`], both filled from platform data, because a model that described
//! its own asset class, its own leveraged-ETP status, or its own corroboration would decide §8.5
//! checks 10, 11, and 15 for itself — which MI-16 forbids and DEC-101 exists to prevent (DEC-132
//! items 6 and 7). A [`ProposedThesis`] binds the three together, so there is no id to disagree and
//! no thesis without its facts.
//!
//! **The order of the checks is the contract.** [`checks`] evaluates all seventeen §8.5 predicates,
//! in the spec's order, each with its own verdict; [`admit`] takes the lowest-numbered failure and
//! that reason is what gets journaled. Evaluating all of them is safe because every predicate is
//! total, and it is what makes "in order" testable rather than merely asserted (DEC-132 item 4).
//!
//! **A refusal admits nothing**, and changes the working universe in exactly one case: a refusal
//! whose reason is [`RefusalReason::LineageRetired`] retires the lineage, and retirement removes the
//! instrument that lineage holds (§8.6 item 4). That is a removal, which is exits-only and adds no
//! risk (MI-19), and it is the only path by which anything here shrinks the universe: a lowered
//! `max_instruments` refuses further admissions and never removes (DEC-132 item 14).
//!
//! Every entry point has its implementation and returns its own typed errors (DEC-77);
//! [`ResearchError::Unimplemented`] stays only as a pinned ES-09 code.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::Digest;
use mandate_num::Usd;
use mandate_time::UtcNanos;

pub mod drift;
pub mod flow;
pub mod score;
pub mod spec_types;

pub use spec_types::{
    AssetClass, AssetId, AutonomyDecision, ContentHash, GroupId, InstrumentGroup,
    InstrumentRestriction, MandateEnvelope, ModelId, ModelVersion, PolicyOverlay, ResearchEnvelope,
    SchemaDec, SpecTypeError, UniverseChange, UniverseChangeReason, ValidatedMandate,
    WorkingUniverse, group_of,
};

/// A thesis's own id (journal spec §9).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ThesisId(String);

impl ThesisId {
    /// # Errors
    /// Returns [`ResearchError::EmptyId`] for an empty id.
    pub fn new(id: &str) -> Result<Self, ResearchError> {
        if id.is_empty() {
            return Err(ResearchError::EmptyId);
        }
        Ok(Self(id.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The first thesis's id in a lineage (§8.4). `revision` 0's `lineage_id` equals its `thesis_id`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LineageId(String);

impl LineageId {
    /// # Errors
    /// Returns [`ResearchError::EmptyId`] for an empty id.
    pub fn new(id: &str) -> Result<Self, ResearchError> {
        if id.is_empty() {
            return Err(ResearchError::EmptyId);
        }
        Ok(Self(id.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An entry on the vetted source allowlist (DEC-101), which is versioned configuration.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceId(String);

impl SourceId {
    /// # Errors
    /// Returns [`ResearchError::EmptyId`] for an empty id.
    pub fn new(id: &str) -> Result<Self, ResearchError> {
        if id.is_empty() {
            return Err(ResearchError::EmptyId);
        }
        Ok(Self(id.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The workspace this agent runs in, which the §8.4 stagger offset is derived from so that accounts
/// do not act at once without sharing any state (DEC-100).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct WorkspaceId(String);

impl WorkspaceId {
    /// # Errors
    /// Returns [`ResearchError::EmptyId`] for an empty id.
    pub fn new(id: &str) -> Result<Self, ResearchError> {
        if id.is_empty() {
            return Err(ResearchError::EmptyId);
        }
        Ok(Self(id.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The allowlist version in effect, recorded in every `ThesisProposed` (DEC-101).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct AllowlistVersion(pub u32);

/// §8.2's `direction`. `Long` only in v1 (DEC-32); anything else is an ignored output, so the enum
/// keeps a variant for what a model may send rather than refusing to hold it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Direction {
    Long,
    Other,
}

/// How the **platform** corroborated a thesis (DEC-101): an independent allowlisted source, or market
/// data consistent with it. Never what the model asserted (DEC-132 item 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Corroboration {
    IndependentSource,
    MarketData,
}

impl Corroboration {
    /// The journaled kind (journal spec §9).
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::IndependentSource => "independent_source",
            Self::MarketData => "market_data",
        }
    }
}

/// §8.1's pinned identity and §8.2's freshness envelope.
///
/// Whether an output is *fresh* (`as_of <= now < expires_at` and `now - as_of <= max_output_age_s`,
/// latest per model) belongs to stream H's combine step: a thesis's **lifetime** and an output's
/// **freshness** are different clocks, as §8.6's last bullet says. This crate reads `as_of` and
/// `expires_at` for check 2 and nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputEnvelope {
    pub model_id: ModelId,
    pub model_version: ModelVersion,
    pub content_hash: ContentHash,
    pub as_of: UtcNanos,
    pub expires_at: UtcNanos,
}

/// §8.6's invalidation conditions, kept as the text the journal and the approval screen carry
/// (DEC-126). Evaluating a condition against market data is the caller's (DEC-132 item 13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invalidation(String);

impl Invalidation {
    /// # Errors
    /// Returns [`ResearchError::EmptyInvalidation`] for empty text: §8.4 requires a thesis to say
    /// what would end it.
    pub fn new(text: &str) -> Result<Self, ResearchError> {
        if text.trim().is_empty() {
            return Err(ResearchError::EmptyInvalidation);
        }
        Ok(Self(text.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// §8.4's thesis: what the research agent said about one instrument.
///
/// It carries no `asset_class` and no leveraged-ETP flag: both are instrument reference data, in
/// [`InstrumentFacts`] (DEC-132 item 6). It carries no corroboration either (item 7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thesis {
    pub thesis_id: ThesisId,
    pub lineage_id: LineageId,
    pub revision: u32,
    pub predecessor_thesis_id: Option<ThesisId>,
    pub instrument_id: AssetId,
    pub output: OutputEnvelope,
    pub direction: Direction,
    pub horizon_s: u32,
    pub conviction: SchemaDec,
    pub confidence: SchemaDec,
    pub evidence: Option<Digest>,
    pub evidence_sources: Vec<SourceId>,
    pub invalidation: Invalidation,
}

/// Instrument reference data (trading spec §3.2). It holds no id: the facts reach a check only
/// through the [`ProposedThesis`] that binds them to their thesis, so there is no pair of ids that
/// could disagree (DEC-132 item 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstrumentFacts {
    pub asset_class: AssetClass,
    pub leveraged_or_inverse_etp: bool,
}

/// One thesis with the platform's facts about it: the instrument's reference data, and the
/// corroboration the platform found, per thesis rather than per fold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposedThesis {
    pub thesis: Thesis,
    pub instrument: InstrumentFacts,
    pub corroboration: Option<Corroboration>,
}

/// The vetted source allowlist and its version (DEC-101, E17-7), shared by every thesis in a fold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceAllowlist {
    pub version: AllowlistVersion,
    pub sources: BTreeSet<SourceId>,
}

/// The facts checks 7 to 13 read, each produced elsewhere: the eligibility failures by
/// `mandate-risk`'s floor (E6-7), the group map and the claims by the account ledger (trading spec
/// §7.1) in the shape stream F's `ValidationContext` supplies, the halts by the operator service
/// (E17-6, DEC-100), the disclosures by the consent record, the data universe by the profile
/// (DEC-103), and the spend by the cost accounting (DEC-120), whose day boundary is
/// `mandate_spec::risk_day`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmissionFacts {
    pub allowlist: SourceAllowlist,
    pub eligibility_failures: BTreeSet<AssetId>,
    pub instrument_groups: BTreeMap<AssetId, GroupId>,
    pub claimed_by_other_agents: BTreeSet<AssetId>,
    pub halted_instruments: BTreeSet<AssetId>,
    pub disclosures_accepted: BTreeSet<Digest>,
    /// `Some` pins a data universe (the research basket); `None` pins none.
    pub data_universe: Option<BTreeSet<AssetId>>,
    pub research_spend_usd_today: Usd,
}

#[derive(Debug, Clone, Copy)]
pub struct AdmissionInput<'a> {
    pub mandate: &'a ValidatedMandate,
    pub overlay: &'a PolicyOverlay,
    pub universe: &'a WorkingUniverse,
    pub proposal: &'a ProposedThesis,
    pub facts: &'a AdmissionFacts,
    pub lineages: &'a LineageState,
}

/// One §8.5 check with its ordinal and its verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Check {
    pub number: u8,
    pub reason: RefusalReason,
    pub failed: bool,
}

/// The seventeen §8.5 reasons, in the spec's order. The discriminant order *is* the check order, so
/// `Ord` sorts by ordinal and the first failure is the minimum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RefusalReason {
    DirectionNotAllowed,
    HorizonMismatch,
    RevisionWithoutPredecessor,
    ResearchDisabled,
    UniversePinned,
    AdmissionDenied,
    CostCapReached,
    NotInDataUniverse,
    OperatorHalt,
    NotAllowedAssetClass,
    LeveragedEtpNotEnabled,
    EligibilityFloor,
    InstrumentGroupClaimed,
    SourceNotAllowlisted,
    NoCorroboration,
    LineageRetired,
    UniverseFull,
}

impl RefusalReason {
    /// The journaled reason (ES-09).
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::DirectionNotAllowed => "direction_not_allowed",
            Self::HorizonMismatch => "horizon_mismatch",
            Self::RevisionWithoutPredecessor => "revision_without_predecessor",
            Self::ResearchDisabled => "research_disabled",
            Self::UniversePinned => "universe_pinned",
            Self::AdmissionDenied => "admission_denied",
            Self::CostCapReached => "cost_cap_reached",
            Self::NotInDataUniverse => "not_in_data_universe",
            Self::OperatorHalt => "operator_halt",
            Self::NotAllowedAssetClass => "not_allowed_asset_class",
            Self::LeveragedEtpNotEnabled => "leveraged_etp_not_enabled",
            Self::EligibilityFloor => "eligibility_floor",
            Self::InstrumentGroupClaimed => "instrument_group_claimed",
            Self::SourceNotAllowlisted => "source_not_allowlisted",
            Self::NoCorroboration => "no_corroboration",
            Self::LineageRetired => "lineage_retired",
            Self::UniverseFull => "universe_full",
        }
    }

    /// The §8.5 ordinal, 1 to 17, written down in exactly one place.
    #[must_use]
    pub fn check_number(self) -> u8 {
        match self {
            Self::DirectionNotAllowed => 1,
            Self::HorizonMismatch => 2,
            Self::RevisionWithoutPredecessor => 3,
            Self::ResearchDisabled => 4,
            Self::UniversePinned => 5,
            Self::AdmissionDenied => 6,
            Self::CostCapReached => 7,
            Self::NotInDataUniverse => 8,
            Self::OperatorHalt => 9,
            Self::NotAllowedAssetClass => 10,
            Self::LeveragedEtpNotEnabled => 11,
            Self::EligibilityFloor => 12,
            Self::InstrumentGroupClaimed => 13,
            Self::SourceNotAllowlisted => 14,
            Self::NoCorroboration => 15,
            Self::LineageRetired => 16,
            Self::UniverseFull => 17,
        }
    }

    /// §8.2's ignored outputs: checks 1 to 3 are refusals *and* ignored. One predicate, so no caller
    /// re-lists the three codes (DEC-132 item 5).
    #[must_use]
    pub fn is_ignored_output(self) -> bool {
        matches!(
            self,
            Self::DirectionNotAllowed | Self::HorizonMismatch | Self::RevisionWithoutPredecessor
        )
    }

    /// Every reason, in §8.5 order. The one list `checks` walks.
    #[must_use]
    pub fn all() -> [Self; 17] {
        [
            Self::DirectionNotAllowed,
            Self::HorizonMismatch,
            Self::RevisionWithoutPredecessor,
            Self::ResearchDisabled,
            Self::UniversePinned,
            Self::AdmissionDenied,
            Self::CostCapReached,
            Self::NotInDataUniverse,
            Self::OperatorHalt,
            Self::NotAllowedAssetClass,
            Self::LeveragedEtpNotEnabled,
            Self::EligibilityFloor,
            Self::InstrumentGroupClaimed,
            Self::SourceNotAllowlisted,
            Self::NoCorroboration,
            Self::LineageRetired,
            Self::UniverseFull,
        ]
    }
}

/// Whether an admitted thesis entered the universe or replaced an active instrument's thesis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionChange {
    Admitted,
    Renewed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionDecision {
    Admitted { change: AdmissionChange },
    Refused { reason: RefusalReason },
}

impl AdmissionDecision {
    #[must_use]
    pub fn admitted(self) -> bool {
        matches!(self, Self::Admitted { .. })
    }

    #[must_use]
    pub fn reason(self) -> Option<RefusalReason> {
        match self {
            Self::Admitted { .. } => None,
            Self::Refused { reason } => Some(reason),
        }
    }

    /// §8.2: an ignored output is a refusal by one of the first three checks.
    #[must_use]
    pub fn ignored(self) -> bool {
        self.reason().is_some_and(RefusalReason::is_ignored_output)
    }
}

/// What stream H's `classify` needs for the first order in the instrument (§8.5, §6.2).
///
/// Admission does not decide it: §8.5 says the first order "is then decided by the autonomy rules",
/// and §6.2's evaluation is stream H's, which reads these two flags from this stream (DEC-132
/// item 3). `new_instrument` is `true` on a renewal too, because MC-N14 expects the ceiling there.
/// H's `ActionContext` takes `thesis_confidence` as its own unit-interval type, so the composition
/// point converts `confidence.as_str()` through that type's parse; this crate carries the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirstOrderFacts {
    pub new_instrument: bool,
    pub thesis_confidence: SchemaDec,
    pub admission_ceiling: AutonomyDecision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Admission {
    pub decision: AdmissionDecision,
    pub universe: WorkingUniverse,
    pub journal: Vec<ResearchEvent>,
    /// `Some` exactly when the decision is [`AdmissionDecision::Admitted`].
    pub first_order: Option<FirstOrderFacts>,
}

/// Every §8.5 predicate, in the spec's order, each with its own verdict.
///
/// # Errors
/// Returns [`ResearchError::UniverseUnavailable`] for an unread working universe, which is never an
/// admission, and [`ResearchError::TimeOutOfRange`] when `as_of + horizon_s` leaves `UtcNanos`'s
/// range, so check 2 has no instant to compare with.
pub fn checks(input: &AdmissionInput<'_>) -> Result<Vec<Check>, ResearchError> {
    let (active, _) = known_instruments(input.universe)?;
    let thesis = &input.proposal.thesis;
    let horizon_end = plus_seconds(thesis.output.as_of, thesis.horizon_s)?;
    let verdicts = CheckInputs {
        input,
        active,
        horizon_end,
    };
    Ok(RefusalReason::all()
        .into_iter()
        .map(|reason| Check {
            number: reason.check_number(),
            reason,
            failed: verdicts.fails(reason),
        })
        .collect())
}

/// §8.5: decides one thesis against the envelope. Admission never loosens an envelope field (MI-16).
///
/// The first failing check is the journaled reason. An admitted thesis whose instrument is already
/// active is a renewal: it journals its thesis entry alone and leaves the universe as it was
/// (DEC-132 item 11). A first admission adds the instrument and journals `UniverseChanged` with
/// the size counted after the change.
///
/// # Errors
/// Returns [`ResearchError`] for the same reasons as [`checks`].
pub fn admit(input: &AdmissionInput<'_>) -> Result<Admission, ResearchError> {
    let verdicts = checks(input)?;
    let reason = verdicts
        .iter()
        .find(|check| check.failed)
        .map(|check| check.reason);
    let (active, pinned) = known_instruments(input.universe)?;
    let proposal = input.proposal;
    let thesis = &proposal.thesis;
    let mut instruments = active.clone();
    let mut journal = vec![thesis_event(proposal, &input.facts.allowlist, reason)];
    let decision = match reason {
        Some(reason) => AdmissionDecision::Refused { reason },
        None if active.contains(&thesis.instrument_id) => AdmissionDecision::Admitted {
            change: AdmissionChange::Renewed,
        },
        None => {
            instruments.insert(thesis.instrument_id.clone());
            journal.push(universe_changed(
                &thesis.instrument_id,
                UniverseChange::Admitted,
                UniverseChangeReason::ThesisAdmitted,
                (&thesis.thesis_id, &thesis.lineage_id),
                instruments.len(),
            ));
            AdmissionDecision::Admitted {
                change: AdmissionChange::Admitted,
            }
        }
    };
    let first_order = decision.admitted().then(|| FirstOrderFacts {
        new_instrument: true,
        thesis_confidence: thesis.confidence.clone(),
        admission_ceiling: input
            .overlay
            .effective_admission(input.mandate.envelope().admission),
    });
    Ok(Admission {
        decision,
        universe: WorkingUniverse::Known {
            instruments,
            pinned,
        },
        journal,
        first_order,
    })
}

/// The members and the pinned flag of a universe that has been read; an unread one is an error,
/// never an empty set (§2.3, DEC-132 item 18).
fn known_instruments(
    universe: &WorkingUniverse,
) -> Result<(&BTreeSet<AssetId>, bool), ResearchError> {
    match universe {
        WorkingUniverse::Known {
            instruments,
            pinned,
        } => Ok((instruments, *pinned)),
        WorkingUniverse::Unavailable => Err(ResearchError::UniverseUnavailable),
    }
}

/// One `UniverseChanged` entry (journal spec §9), naming the thesis and lineage it follows from.
fn universe_changed(
    instrument: &AssetId,
    change: UniverseChange,
    reason: UniverseChangeReason,
    (thesis_id, lineage_id): (&ThesisId, &LineageId),
    universe_size_after: usize,
) -> ResearchEvent {
    ResearchEvent::UniverseChanged(UniverseChangedEntry {
        instrument: instrument.clone(),
        change,
        reason,
        thesis_id: thesis_id.clone(),
        lineage_id: lineage_id.clone(),
        universe_size_after,
    })
}

/// `at + seconds`, or [`ResearchError::TimeOutOfRange`] past `UtcNanos`'s range.
fn plus_seconds(at: UtcNanos, seconds: u32) -> Result<UtcNanos, ResearchError> {
    at.secs()
        .checked_add(i64::from(seconds))
        .and_then(|secs| UtcNanos::from_parts(secs, at.nanos()).ok())
        .ok_or(ResearchError::TimeOutOfRange)
}

/// What the seventeen predicates read, resolved once so each predicate is one total expression.
struct CheckInputs<'a> {
    input: &'a AdmissionInput<'a>,
    active: &'a BTreeSet<AssetId>,
    horizon_end: UtcNanos,
}

impl CheckInputs<'_> {
    /// One §8.5 predicate: `true` when the check fails. Every one reads typed facts only, so no text
    /// a model wrote can move a verdict (DEC-101, R-05).
    ///
    /// Check 4 asks for a research envelope on a mandate whose model admits instruments, as one
    /// expression: V-036 makes "no admitting model" and "no research envelope" the same condition
    /// for a validated mandate, so two separate disjuncts would differ only on inputs validation
    /// rejects, which no test can reach.
    ///
    /// Check 6 reads the overlay's effective `autonomy.admission` although today's overlay only
    /// tightens `auto` to `ask` and never to `deny`, so the read changes no verdict and no test can
    /// tell it from the mandate's own value. It stays so that a policy able to deny admission binds
    /// here without a code change (DEC-132 item 22).
    ///
    /// Check 17 refuses if the ceiling does not fit a `usize`, which no supported target reaches:
    /// an impossible input resolves toward refusal (AGENTS.md rule 3).
    fn fails(&self, reason: RefusalReason) -> bool {
        let input = self.input;
        let envelope = input.mandate.envelope();
        let overlay = input.overlay;
        let facts = input.facts;
        let proposal = input.proposal;
        let thesis = &proposal.thesis;
        let instrument = &thesis.instrument_id;
        match reason {
            RefusalReason::DirectionNotAllowed => thesis.direction != Direction::Long,
            RefusalReason::HorizonMismatch => thesis.output.expires_at != self.horizon_end,
            RefusalReason::RevisionWithoutPredecessor => {
                (thesis.revision > 0) != thesis.predecessor_thesis_id.is_some()
            }
            RefusalReason::ResearchDisabled => {
                !overlay.research_agent_allowed
                    || envelope
                        .research
                        .as_ref()
                        .filter(|_| envelope.admits_instruments)
                        .is_none()
            }
            RefusalReason::UniversePinned => envelope.universe_pinned,
            RefusalReason::AdmissionDenied => {
                overlay.effective_admission(envelope.admission) == AutonomyDecision::Deny
            }
            RefusalReason::CostCapReached => envelope.research.as_ref().is_some_and(|research| {
                facts.research_spend_usd_today
                    >= overlay.effective_cost_cap(research.cost_cap_usd_per_day)
            }),
            RefusalReason::NotInDataUniverse => facts
                .data_universe
                .as_ref()
                .is_some_and(|basket| !basket.contains(instrument)),
            RefusalReason::OperatorHalt => facts.halted_instruments.contains(instrument),
            RefusalReason::NotAllowedAssetClass => !envelope
                .asset_classes
                .contains(&proposal.instrument.asset_class),
            RefusalReason::LeveragedEtpNotEnabled => {
                proposal.instrument.leveraged_or_inverse_etp
                    && !(envelope.leveraged_etps_enabled
                        && envelope
                            .leveraged_etp_disclosure_version
                            .as_ref()
                            .is_some_and(|version| facts.disclosures_accepted.contains(version)))
            }
            RefusalReason::EligibilityFloor => facts.eligibility_failures.contains(instrument),
            RefusalReason::InstrumentGroupClaimed => {
                let own = group_of(instrument, &facts.instrument_groups);
                facts
                    .claimed_by_other_agents
                    .iter()
                    .any(|claimed| group_of(claimed, &facts.instrument_groups) == own)
            }
            RefusalReason::SourceNotAllowlisted => thesis
                .evidence_sources
                .iter()
                .any(|source| !facts.allowlist.sources.contains(source)),
            RefusalReason::NoCorroboration => proposal.corroboration.is_none(),
            RefusalReason::LineageRetired => {
                input
                    .lineages
                    .lineage(&thesis.lineage_id)
                    .is_some_and(|lineage| lineage.retired)
                    || thesis.revision > self.revision_cap()
            }
            RefusalReason::UniverseFull => {
                !self.active.contains(instrument)
                    && usize::try_from(overlay.effective_max_instruments(envelope.max_instruments))
                        .map_or(true, |ceiling| self.active.len() >= ceiling)
            }
        }
    }

    /// The effective `max_revisions_per_lineage`; with no research envelope nothing may be revised,
    /// so the cap is 0, as `ref.py` reads it.
    fn revision_cap(&self) -> u32 {
        let input = self.input;
        input
            .mandate
            .envelope()
            .research
            .as_ref()
            .map_or(0, |research| {
                input
                    .overlay
                    .effective_max_revisions_per_lineage(research.max_revisions_per_lineage)
            })
    }
}

/// The thesis entry every decision journals, refused or not. Its type follows the revision number,
/// never the verdict (journal spec §9).
fn thesis_event(
    proposal: &ProposedThesis,
    allowlist: &SourceAllowlist,
    reason: Option<RefusalReason>,
) -> ResearchEvent {
    let thesis = &proposal.thesis;
    let entry = ThesisEntry {
        thesis_id: thesis.thesis_id.clone(),
        lineage_id: thesis.lineage_id.clone(),
        revision: thesis.revision,
        predecessor_thesis_id: thesis.predecessor_thesis_id.clone(),
        instrument: thesis.instrument_id.clone(),
        asset_class: proposal.instrument.asset_class,
        direction: thesis.direction,
        horizon_s: thesis.horizon_s,
        conviction: thesis.conviction.clone(),
        confidence: thesis.confidence.clone(),
        evidence: thesis.evidence,
        evidence_sources: thesis.evidence_sources.clone(),
        corroboration: proposal.corroboration,
        invalidation: thesis.invalidation.clone(),
        allowlist_version: allowlist.version,
        admitted: reason.is_none(),
        reason,
    };
    if thesis.revision > 0 {
        ResearchEvent::ThesisRevised(entry)
    } else {
        ResearchEvent::ThesisProposed(entry)
    }
}

/// One lineage's folded state: the highest revision it admitted, how many admissions it has, and
/// whether it retired. The default is a lineage the fold has just met.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Lineage {
    pub revisions: u32,
    pub admitted: u32,
    pub retired: bool,
}

/// §8.6's folded lineage state, and which instrument each lineage holds.
///
/// Retirement is read from here, never asserted per instrument, so nothing can be removed for a
/// reason the journal does not carry. A lineage keeps its holder entry after it retires — the map
/// records which lineage held what, not what the universe holds now — and loses it the moment
/// another lineage's thesis for that instrument is admitted (DEC-132 items 9 and 10).
///
/// An empty state is `LineageState::default()`: there is no `empty()` beside it, because a function
/// whose body is `Self::default()` can be mutated to `Default::default()` with no visible effect, and
/// no test could tell the two apart.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineageState {
    lineages: BTreeMap<LineageId, Lineage>,
    holders: BTreeMap<LineageId, AssetId>,
}

impl LineageState {
    /// Builds a state from folded events. The narrow constructor tests use; the runtime folds
    /// `ThesisProposed`, `ThesisRevised`, and `UniverseChanged` instead.
    #[must_use]
    pub fn from_parts(
        lineages: BTreeMap<LineageId, Lineage>,
        holders: BTreeMap<LineageId, AssetId>,
    ) -> Self {
        Self { lineages, holders }
    }

    #[must_use]
    pub fn lineage(&self, id: &LineageId) -> Option<&Lineage> {
        self.lineages.get(id)
    }

    #[must_use]
    pub fn holder_of(&self, id: &LineageId) -> Option<&AssetId> {
        self.holders.get(id)
    }

    #[must_use]
    pub fn held_by(&self, instrument: &AssetId) -> Option<&LineageId> {
        self.holders
            .iter()
            .find(|(_, held)| *held == instrument)
            .map(|(lineage, _)| lineage)
    }

    #[must_use]
    pub fn lineages(&self) -> &BTreeMap<LineageId, Lineage> {
        &self.lineages
    }

    #[must_use]
    pub fn holders(&self) -> &BTreeMap<LineageId, AssetId> {
        &self.holders
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FoldInput<'a> {
    pub mandate: &'a ValidatedMandate,
    pub overlay: &'a PolicyOverlay,
    pub universe: &'a WorkingUniverse,
    pub proposals: &'a [ProposedThesis],
    pub facts: &'a AdmissionFacts,
    pub lineages: &'a LineageState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldStep {
    pub thesis_id: ThesisId,
    pub decision: AdmissionDecision,
    pub lineage_revisions: u32,
    pub lineage_retired: bool,
    pub journal: Vec<ResearchEvent>,
}

impl FoldStep {
    /// MI-18: always false. No type in this crate accepts a predecessor's score or scorecard, so
    /// carrying one forward is unrepresentable rather than merely tested (DEC-132 item 16). Scoring
    /// itself is E17-8's.
    #[must_use]
    pub fn score_carried_forward(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fold {
    pub steps: Vec<FoldStep>,
    pub lineages: LineageState,
    pub universe: WorkingUniverse,
}

/// §8.6 (DEC-111): folds a sequence of proposals, capping revisions per lineage.
///
/// Retiring a lineage removes the instrument it holds at once and in the same step: the platform has
/// failed on the idea `max_revisions_per_lineage` times and no renewal can be admitted, so leaving
/// the position in the universe would leave it with no path back. Removal is exits-only, so it adds
/// no risk (MI-19). Retirement follows the **journaled refusal reason**, never the revision number
/// alone, so a thesis an earlier check refused retires nothing.
///
/// # Errors
/// Returns [`ResearchError`] for an unread universe, or a proposal sequence naming one thesis twice.
pub fn fold_theses(input: &FoldInput<'_>) -> Result<Fold, ResearchError> {
    let mut seen = BTreeSet::new();
    for proposal in input.proposals {
        if !seen.insert(&proposal.thesis.thesis_id) {
            return Err(ResearchError::DuplicateThesisId);
        }
    }
    known_instruments(input.universe)?;
    let mut universe = input.universe.clone();
    let mut lineages = input.lineages.clone();
    let mut steps = Vec::with_capacity(input.proposals.len());
    for proposal in input.proposals {
        let admission = admit(&AdmissionInput {
            mandate: input.mandate,
            overlay: input.overlay,
            universe: &universe,
            proposal,
            facts: input.facts,
            lineages: &lineages,
        })?;
        universe = admission.universe;
        let mut journal = admission.journal;
        let thesis = &proposal.thesis;
        let (lineage, retired_now) = lineages.step(thesis, admission.decision);
        if retired_now && let Some(removal) = retire_holder(&mut universe, &lineages, thesis) {
            journal.push(removal);
        }
        steps.push(FoldStep {
            thesis_id: thesis.thesis_id.clone(),
            decision: admission.decision,
            lineage_revisions: lineage.revisions,
            lineage_retired: lineage.retired,
            journal,
        });
    }
    Ok(Fold {
        steps,
        lineages,
        universe,
    })
}

impl LineageState {
    /// §8.6: folds one decision into its lineage. A `lineage_retired` refusal retires a live lineage;
    /// an admission raises the highest revision, counts the admission, and moves the instrument's
    /// holder to this lineage. Every other refusal leaves the lineage as it was, although the lineage
    /// becomes known (MC-N19). Returns the lineage after the step, and whether this step retired it.
    fn step(&mut self, thesis: &Thesis, decision: AdmissionDecision) -> (Lineage, bool) {
        let lineage = self.lineages.entry(thesis.lineage_id.clone()).or_default();
        let retired_now =
            decision.reason() == Some(RefusalReason::LineageRetired) && !lineage.retired;
        if retired_now {
            lineage.retired = true;
        }
        if decision.admitted() {
            lineage.revisions = lineage.revisions.max(thesis.revision);
            lineage.admitted = lineage.admitted.saturating_add(1);
        }
        let lineage = *lineage;
        if decision.admitted() {
            self.holders.retain(|_, held| *held != thesis.instrument_id);
            self.holders
                .insert(thesis.lineage_id.clone(), thesis.instrument_id.clone());
        }
        (lineage, retired_now)
    }
}

/// §8.6 item 4: removes the instrument a retiring lineage holds, if the universe still holds it, and
/// returns the removal's `UniverseChanged`. A lineage that lost its holder to another lineage's
/// admission removes nothing (MC-N28).
fn retire_holder(
    universe: &mut WorkingUniverse,
    lineages: &LineageState,
    thesis: &Thesis,
) -> Option<ResearchEvent> {
    let held = lineages.holder_of(&thesis.lineage_id)?;
    let WorkingUniverse::Known { instruments, .. } = universe else {
        return None;
    };
    instruments.remove(held).then(|| {
        universe_changed(
            held,
            UniverseChange::Removed,
            UniverseChangeReason::LineageRetired,
            (&thesis.thesis_id, &thesis.lineage_id),
            instruments.len(),
        )
    })
}

/// One instrument's current thesis, as the universe fold holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniverseEntry {
    pub instrument: AssetId,
    pub thesis_id: ThesisId,
    pub lineage_id: LineageId,
    pub revision: u32,
    pub expires_at: UtcNanos,
    /// The caller's verdict on §8.6's invalidation conditions (DEC-132 item 13).
    pub invalidated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expiry {
    pub universe: WorkingUniverse,
    pub removed: Vec<AssetId>,
    pub instrument_restrictions: BTreeMap<AssetId, InstrumentRestriction>,
    pub journal: Vec<ResearchEvent>,
}

/// §8.6, DEC-118: at its horizon a thesis is not renewed; its instrument becomes removed.
///
/// The reason is the first that holds: invalidated, then the lineage retired, then
/// `now >= expires_at` — inclusive, so a thesis does not outlive its own horizon by an instant.
/// Retirement is read from the folded [`LineageState`], never from a per-entry flag.
///
/// # Errors
/// Returns [`ResearchError::DuplicateInstrument`] for an entry list naming one instrument twice.
pub fn expire_theses(
    now: UtcNanos,
    entries: &[UniverseEntry],
    lineages: &LineageState,
) -> Result<Expiry, ResearchError> {
    let mut by_instrument: BTreeMap<&AssetId, &UniverseEntry> = BTreeMap::new();
    for entry in entries {
        if by_instrument.insert(&entry.instrument, entry).is_some() {
            return Err(ResearchError::DuplicateInstrument);
        }
    }
    let mut kept = BTreeSet::new();
    let mut ended = Vec::new();
    for (instrument, entry) in by_instrument {
        match removal_reason(now, entry, lineages) {
            Some(reason) => ended.push((entry, reason)),
            None => {
                kept.insert(instrument.clone());
            }
        }
    }
    let mut remaining = entries.len();
    let mut journal = Vec::new();
    for (entry, reason) in &ended {
        remaining = remaining.saturating_sub(1);
        journal.push(universe_changed(
            &entry.instrument,
            UniverseChange::Removed,
            *reason,
            (&entry.thesis_id, &entry.lineage_id),
            remaining,
        ));
    }
    let removed: Vec<AssetId> = ended
        .iter()
        .map(|(entry, _)| entry.instrument.clone())
        .collect();
    Ok(Expiry {
        universe: WorkingUniverse::Known {
            instruments: kept,
            pinned: false,
        },
        instrument_restrictions: removed
            .iter()
            .map(|instrument| (instrument.clone(), InstrumentRestriction::RemovedInstrument))
            .collect(),
        removed,
        journal,
    })
}

/// §8.6's removal reasons, first that holds: invalidated, then the lineage retired, then the
/// horizon reached (`now >= expires_at`). `None` keeps the entry.
fn removal_reason(
    now: UtcNanos,
    entry: &UniverseEntry,
    lineages: &LineageState,
) -> Option<UniverseChangeReason> {
    if entry.invalidated {
        Some(UniverseChangeReason::ThesisInvalidated)
    } else if lineages
        .lineage(&entry.lineage_id)
        .is_some_and(|lineage| lineage.retired)
    {
        Some(UniverseChangeReason::LineageRetired)
    } else if now >= entry.expires_at {
        Some(UniverseChangeReason::ThesisExpired)
    } else {
        None
    }
}

/// §8.4, DEC-123: the stagger window, a policy minimum of 900 s. Zero means no wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct StaggerWindow(pub u32);

/// §8.4: `SHA-256(workspace_id ‖ 0x00 ‖ thesis_id)`, read as a big-endian integer, modulo the
/// window, in whole seconds.
///
/// The digest comes from `mandate_canon`, so no hashing dependency is added, and the reduction is a
/// byte fold in `u64`: with `acc < window <= u32::MAX`, `acc * 256 + 255 < 2^40`, so every step is
/// exact and cannot overflow (DEC-132 item 8).
///
/// # Errors
/// Returns [`ResearchError::WindowTooLarge`] if a step of the reduction left `u64`, which the bound
/// above rules out; the error keeps the arithmetic checked rather than assumed.
pub fn stagger_offset(
    workspace: &WorkspaceId,
    thesis: &ThesisId,
    window: StaggerWindow,
) -> Result<u32, ResearchError> {
    let modulus = u64::from(window.0);
    if modulus == 0 {
        return Ok(0);
    }
    let digest = Digest::of_parts(&[
        workspace.as_str().as_bytes(),
        &[0x00],
        thesis.as_str().as_bytes(),
    ]);
    let mut acc: u64 = 0;
    for byte in digest.as_bytes() {
        acc = acc
            .checked_mul(256)
            .and_then(|shifted| shifted.checked_add(u64::from(*byte)))
            .and_then(|value| value.checked_rem(modulus))
            .ok_or(ResearchError::WindowTooLarge)?;
    }
    u32::try_from(acc).map_err(|_| ResearchError::WindowTooLarge)
}

/// §8.4: when the first opening order on a newly admitted thesis may go.
///
/// For an equity the offset is counted from the **later** of the admission and the next
/// regular-session open; crypto trades continuously, so it is counted from the admission.
///
/// # Errors
/// Returns [`ResearchError::SessionCalendarMissing`] when an equity has no next regular open, and
/// [`ResearchError::TimeOutOfRange`] when the release leaves `UtcNanos`'s range.
pub fn stagger_release_at(
    asset_class: AssetClass,
    admitted_at: UtcNanos,
    next_regular_open: Option<UtcNanos>,
    offset_s: u32,
) -> Result<UtcNanos, ResearchError> {
    let anchor = match asset_class {
        AssetClass::Crypto => admitted_at,
        AssetClass::UsEquity => next_regular_open
            .ok_or(ResearchError::SessionCalendarMissing)?
            .max(admitted_at),
    };
    plus_seconds(anchor, offset_s)
}

/// §8.4's `behavior.research.interval_s`, a policy minimum: the earliest instant the agent may
/// propose again. `None` before it has ever proposed, which is "now" for the caller's own clock.
///
/// The scheduler (stream I) calls this; the crate reads no clock (ES-21).
///
/// # Errors
/// Returns [`ResearchError::TimeOutOfRange`] when the next instant leaves `UtcNanos`'s range.
pub fn next_proposal_at(
    last_proposal: Option<UtcNanos>,
    interval_s: u32,
) -> Result<Option<UtcNanos>, ResearchError> {
    last_proposal
        .map(|last| plus_seconds(last, interval_s))
        .transpose()
}

/// journal spec §9. The crate returns values; the executor appends them, to the agent stream for a
/// thesis entry and the account stream for a universe change, with the `causation_id` tying the two
/// (DEC-132 item 17).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResearchEvent {
    ThesisProposed(ThesisEntry),
    ThesisRevised(ThesisEntry),
    UniverseChanged(UniverseChangedEntry),
}

/// `ThesisProposed` and `ThesisRevised` (journal spec §9). The entry type follows the **revision
/// number**, not the verdict, so an ignored revision is still a `ThesisRevised`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThesisEntry {
    pub thesis_id: ThesisId,
    pub lineage_id: LineageId,
    pub revision: u32,
    /// Present exactly when `revision > 0` (check 3).
    pub predecessor_thesis_id: Option<ThesisId>,
    pub instrument: AssetId,
    /// From instrument reference data, never the thesis's claim (DEC-132 item 6).
    pub asset_class: AssetClass,
    pub direction: Direction,
    pub horizon_s: u32,
    pub conviction: SchemaDec,
    pub confidence: SchemaDec,
    pub evidence: Option<Digest>,
    pub evidence_sources: Vec<SourceId>,
    pub corroboration: Option<Corroboration>,
    pub invalidation: Invalidation,
    pub allowlist_version: AllowlistVersion,
    pub admitted: bool,
    pub reason: Option<RefusalReason>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniverseChangedEntry {
    pub instrument: AssetId,
    pub change: UniverseChange,
    pub reason: UniverseChangeReason,
    pub thesis_id: ThesisId,
    pub lineage_id: LineageId,
    pub universe_size_after: usize,
}

/// Every way an input can make a decision impossible rather than negative (ES-09).
#[derive(Debug, thiserror::Error)]
pub enum ResearchError {
    #[error("the working universe has not been read, so nothing may be admitted")]
    UniverseUnavailable,
    #[error("an input holds the same instrument twice")]
    DuplicateInstrument,
    #[error("a proposal sequence names the same thesis twice")]
    DuplicateThesisId,
    #[error("an equity stagger anchor needs the next regular-session open")]
    SessionCalendarMissing,
    #[error("a stagger window above the representable bound")]
    WindowTooLarge,
    #[error("a proposal interval above the representable bound")]
    IntervalTooLarge,
    #[error("an instant outside the representable range")]
    TimeOutOfRange,
    #[error("a decimal outside the arithmetic range")]
    OutOfRange,
    #[error("an id is never empty")]
    EmptyId,
    #[error("a thesis states what would invalidate it")]
    EmptyInvalidation,
    #[error("an observation's timestamp precedes the last one already folded for its source")]
    ObservationOutOfOrder,
    /// A close series must hold at least one close
    #[error("a close series must hold at least one close")]
    EmptyCloses,
    /// A close series's instants must strictly advance
    #[error("a close series's instants must strictly advance")]
    ClosesOutOfOrder,
    /// No close of the series falls strictly after the instant
    #[error("no close of the series falls strictly after the instant")]
    NoCloseAfter,
    /// No close of the series falls at or before the instant
    #[error("no close of the series falls at or before the instant")]
    NoCloseOnOrBefore,
    /// The registered window has not closed: fewer scoreable theses than the minimum
    #[error("the registered window has not closed: fewer scoreable theses than the minimum")]
    WindowNotClosed,
    /// A thesis's horizon closes outside the registered window
    #[error("a thesis's horizon closes outside the registered window")]
    ThesisOutsideWindow,
    /// A research exposure is never negative: v1 is long-only, and a negative row would
    /// under-state the aggregate flow the monitor exists to see (AGENTS.md rule 12)
    #[error("a research exposure is never negative")]
    NegativeExposure,
    /// An evaluation whose scoreable set is empty — no theses at all, or every thesis
    /// unscoreable — has nothing to aggregate, and refuses under its own name rather than the
    /// nameless division the aggregate's mean of nothing reached (#410 review minor 3,
    /// DEC-282 item 9, DEC-335)
    #[error("an evaluation's scoreable set is empty, so there is nothing to aggregate")]
    EmptyScoreableSet,
    /// A basket with no members has no equal-weighted mean, and refuses under its own name
    /// rather than the nameless division its mean of nothing reached (#435 review minor 3,
    /// DEC-380)
    #[error("the basket has no members, so it has no equal-weighted mean")]
    EmptyBasket,
    /// Returned by no entry point of this crate since the empty-basket follow-up's
    /// implementation landed its stub (DEC-380); kept because ES-09's registry is add-only and
    /// the `unimplemented` code is pinned live, so a staged story can name itself and its story
    /// (DEC-77, DEC-294).
    #[error("{0} is not implemented yet (pending {1})")]
    Unimplemented(&'static str, &'static str),
    #[error(transparent)]
    SpecType(#[from] SpecTypeError),
    #[error(transparent)]
    Num(#[from] mandate_num::NumError),
    #[error(transparent)]
    Time(#[from] mandate_time::TimeError),
}

impl ResearchError {
    /// The stable reason code (ES-09).
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::UniverseUnavailable => "universe_unavailable",
            Self::DuplicateInstrument => "duplicate_instrument",
            Self::DuplicateThesisId => "duplicate_thesis_id",
            Self::SessionCalendarMissing => "session_calendar_missing",
            Self::WindowTooLarge => "window_too_large",
            Self::IntervalTooLarge => "interval_too_large",
            Self::TimeOutOfRange => "time_out_of_range",
            Self::OutOfRange => "out_of_range",
            Self::EmptyId => "empty_id",
            Self::EmptyInvalidation => "empty_invalidation",
            Self::ObservationOutOfOrder => "observation_out_of_order",
            Self::EmptyCloses => "empty_closes",
            Self::ClosesOutOfOrder => "closes_out_of_order",
            Self::NoCloseAfter => "no_close_after",
            Self::NoCloseOnOrBefore => "no_close_on_or_before",
            Self::WindowNotClosed => "window_not_closed",
            Self::ThesisOutsideWindow => "thesis_outside_window",
            Self::NegativeExposure => "negative_exposure",
            Self::EmptyScoreableSet => "empty_scoreable_set",
            Self::EmptyBasket => "empty_basket",
            Self::Unimplemented(_, _) => "unimplemented",
            Self::SpecType(_) => "spec_type",
            Self::Num(e) => e.code(),
            Self::Time(e) => e.code(),
        }
    }
}

/// What the tests PR's files cannot pin on the implementation, each shown by a planted bug every
/// live test passed (the post-merge review of #158): check 7 compares against the overlay's
/// `research_cost_cap_usd_per_day` when it is below the mandate's own cap, which is DEC-132
/// item 22's claim that a lowered ceiling binds at check 7.
#[cfg(test)]
mod tests {
    use super::*;

    /// MC-N01's admitting inputs, with the mandate's cap at 5 USD and the overlay's at 2.5 USD.
    struct Owned {
        mandate: ValidatedMandate,
        overlay: PolicyOverlay,
        universe: WorkingUniverse,
        proposal: ProposedThesis,
        facts: AdmissionFacts,
        lineages: LineageState,
    }

    impl Owned {
        fn new(spend: &str) -> Result<Self, ResearchError> {
            let source = SourceId::new("src.filings")?;
            let thesis = Thesis {
                thesis_id: ThesisId::new("th-1")?,
                lineage_id: LineageId::new("th-1")?,
                revision: 0,
                predecessor_thesis_id: None,
                instrument_id: AssetId::new("7b4a1c2e-5555-4a2b-9c3d-000000000005")?,
                output: OutputEnvelope {
                    model_id: ModelId::new("llm.research_agent")?,
                    model_version: ModelVersion::new("0.1.0"),
                    content_hash: ContentHash::new(Digest::of_parts(&[b"model"])),
                    as_of: UtcNanos::parse("2026-09-22T14:00:00.000000000Z")?,
                    expires_at: UtcNanos::parse("2026-09-23T14:00:00.000000000Z")?,
                },
                direction: Direction::Long,
                horizon_s: 86_400,
                conviction: SchemaDec::from_checked_text("0.7"),
                confidence: SchemaDec::from_checked_text("0.8"),
                evidence: None,
                evidence_sources: vec![source.clone()],
                invalidation: Invalidation::new("Guidance is cut.")?,
            };
            Ok(Self {
                mandate: ValidatedMandate::from_validated_envelope(MandateEnvelope {
                    universe_pinned: false,
                    max_instruments: 5,
                    asset_classes: BTreeSet::from([AssetClass::UsEquity]),
                    leveraged_etps_enabled: false,
                    leveraged_etp_disclosure_version: None,
                    admits_instruments: true,
                    research: Some(ResearchEnvelope {
                        interval_s: 3_600,
                        cost_cap_usd_per_day: Usd::parse("5")?,
                        max_revisions_per_lineage: 3,
                    }),
                    admission: AutonomyDecision::Ask,
                }),
                overlay: PolicyOverlay {
                    research_cost_cap_usd_per_day: Some(Usd::parse("2.5")?),
                    ..PolicyOverlay::permissive()
                },
                universe: WorkingUniverse::Known {
                    instruments: BTreeSet::new(),
                    pinned: false,
                },
                proposal: ProposedThesis {
                    thesis,
                    instrument: InstrumentFacts {
                        asset_class: AssetClass::UsEquity,
                        leveraged_or_inverse_etp: false,
                    },
                    corroboration: Some(Corroboration::IndependentSource),
                },
                facts: AdmissionFacts {
                    allowlist: SourceAllowlist {
                        version: AllowlistVersion(1),
                        sources: BTreeSet::from([source]),
                    },
                    eligibility_failures: BTreeSet::new(),
                    instrument_groups: BTreeMap::new(),
                    claimed_by_other_agents: BTreeSet::new(),
                    halted_instruments: BTreeSet::new(),
                    disclosures_accepted: BTreeSet::new(),
                    data_universe: None,
                    research_spend_usd_today: Usd::parse(spend)?,
                },
                lineages: LineageState::default(),
            })
        }

        fn decide(&self) -> Result<AdmissionDecision, ResearchError> {
            admit(&AdmissionInput {
                mandate: &self.mandate,
                overlay: &self.overlay,
                universe: &self.universe,
                proposal: &self.proposal,
                facts: &self.facts,
                lineages: &self.lineages,
            })
            .map(|admission| admission.decision)
        }
    }

    #[test]
    fn check_7_a_policy_cap_below_the_mandates_binds_at_equality() -> Result<(), ResearchError> {
        assert_eq!(
            Owned::new("2.5")?.decide()?,
            AdmissionDecision::Refused {
                reason: RefusalReason::CostCapReached
            },
            "the overlay's 2.5 USD is the stricter cap, so a spend of 2.5 has reached it"
        );
        Ok(())
    }

    #[test]
    fn check_7_a_cent_below_the_policy_cap_still_admits() -> Result<(), ResearchError> {
        assert!(
            Owned::new("2.49")?.decide()?.admitted(),
            "below the stricter cap, and the mandate's own 5 USD is further away still"
        );
        Ok(())
    }
}
