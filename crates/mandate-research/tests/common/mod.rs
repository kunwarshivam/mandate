//! Builders for the family-N shapes, so each test states only the field it is about.
//!
//! [`Scenario::admitting`] is the `research_equity` base of
//! `docs/specs/reference-cases/mandate.yaml` with MC-N01's facts: an empty universe,
//! `max_instruments` 5, `autonomy.admission` `ask`, a research envelope with a 5 USD daily cap and a
//! revision cap of 3, and one allowlisted-and-corroborated long thesis on instrument 5. Every test
//! starts there and breaks exactly one thing, which is what makes a failure name the check that
//! moved rather than the whole struct.

#![allow(dead_code, reason = "each test binary uses a different subset")]

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::Digest;
use mandate_num::Usd;
use mandate_research::{
    AdmissionFacts, AdmissionInput, AllowlistVersion, AssetClass, AssetId, AutonomyDecision,
    ContentHash, Corroboration, Direction, FoldInput, InstrumentFacts, Invalidation, LineageId,
    LineageState, MandateEnvelope, ModelId, ModelVersion, OutputEnvelope, PolicyOverlay,
    ProposedThesis, ResearchEnvelope, SchemaDec, SourceAllowlist, SourceId, Thesis, ThesisId,
    UniverseEntry, ValidatedMandate, WorkingUniverse, WorkspaceId,
};
use mandate_time::UtcNanos;

pub const INSTRUMENT_1: &str = "7b4a1c2e-1111-4a2b-9c3d-000000000001";
pub const INSTRUMENT_2: &str = "7b4a1c2e-2222-4a2b-9c3d-000000000002";
pub const INSTRUMENT_3: &str = "7b4a1c2e-3333-4a2b-9c3d-000000000003";
pub const INSTRUMENT_4: &str = "7b4a1c2e-4444-4a2b-9c3d-000000000004";
pub const INSTRUMENT_5: &str = "7b4a1c2e-5555-4a2b-9c3d-000000000005";
pub const INSTRUMENT_9: &str = "7b4a1c2e-9999-4a2b-9c3d-000000000009";

/// The `research_equity_etp` base's accepted disclosure version, and a different one, so the
/// exact-version condition of V-005 has both sides (MC-N25, MC-N26).
pub const DISCLOSURE_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
pub const DISCLOSURE_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

pub const AS_OF: &str = "2026-09-22T14:00:00.000000000Z";
pub const EXPIRES_AT: &str = "2026-09-23T14:00:00.000000000Z";
pub const HORIZON_S: u32 = 86_400;

#[must_use]
pub fn asset(id: &str) -> AssetId {
    AssetId::new(id).expect("a fixture asset id is not empty")
}

#[must_use]
pub fn thesis_id(id: &str) -> ThesisId {
    ThesisId::new(id).expect("a fixture thesis id is not empty")
}

#[must_use]
pub fn lineage_id(id: &str) -> LineageId {
    LineageId::new(id).expect("a fixture lineage id is not empty")
}

#[must_use]
pub fn source(id: &str) -> SourceId {
    SourceId::new(id).expect("a fixture source id is not empty")
}

#[must_use]
pub fn workspace(id: &str) -> WorkspaceId {
    WorkspaceId::new(id).expect("a fixture workspace id is not empty")
}

#[must_use]
pub fn at(text: &str) -> UtcNanos {
    UtcNanos::parse(text).expect("a fixture instant parses")
}

#[must_use]
pub fn usd(text: &str) -> Usd {
    Usd::parse(text).expect("a fixture amount parses")
}

#[must_use]
pub fn digest(hex: &str) -> Digest {
    Digest::from_hex(hex).expect("a fixture digest is 32 hex bytes")
}

#[must_use]
pub fn dec(text: &str) -> SchemaDec {
    SchemaDec::from_checked_text(text)
}

#[must_use]
pub fn universe_of(ids: &[&str]) -> WorkingUniverse {
    WorkingUniverse::Known {
        instruments: ids.iter().map(|id| asset(id)).collect(),
        pinned: false,
    }
}

/// The `llm.research_agent` model the `research_equity` base pins.
#[must_use]
pub fn research_output() -> OutputEnvelope {
    OutputEnvelope {
        model_id: ModelId::new("llm.research_agent").expect("a fixture model id is not empty"),
        model_version: ModelVersion::new("0.1.0"),
        content_hash: ContentHash::new(digest(
            "4444444444444444444444444444444444444444444444444444444444444444",
        )),
        as_of: at(AS_OF),
        expires_at: at(EXPIRES_AT),
    }
}

/// MC-N01's thesis: revision 0, long, one-day horizon, two allowlisted sources.
#[must_use]
pub fn thesis(id: &str, instrument: &str) -> Thesis {
    Thesis {
        thesis_id: thesis_id(id),
        lineage_id: lineage_id(id),
        revision: 0,
        predecessor_thesis_id: None,
        instrument_id: asset(instrument),
        output: research_output(),
        direction: Direction::Long,
        horizon_s: HORIZON_S,
        conviction: dec("0.7"),
        confidence: dec("0.8"),
        evidence: None,
        evidence_sources: vec![source("src.filings"), source("src.newswire")],
        invalidation: Invalidation::new("Guidance is cut, or the 50-day trend breaks.")
            .expect("a fixture invalidation is not empty"),
    }
}

/// The same thesis as a revision in an existing lineage.
#[must_use]
pub fn revision(
    id: &str,
    lineage: &str,
    revision: u32,
    predecessor: &str,
    instrument: &str,
) -> Thesis {
    Thesis {
        lineage_id: lineage_id(lineage),
        revision,
        predecessor_thesis_id: Some(thesis_id(predecessor)),
        ..thesis(id, instrument)
    }
}

/// A plain US equity that is not a leveraged or inverse ETP.
#[must_use]
pub fn equity_facts() -> InstrumentFacts {
    InstrumentFacts {
        asset_class: AssetClass::UsEquity,
        leveraged_or_inverse_etp: false,
    }
}

/// One thesis with the platform's facts about it, corroborated by an independent source.
#[must_use]
pub fn proposal(t: Thesis) -> ProposedThesis {
    ProposedThesis {
        thesis: t,
        instrument: equity_facts(),
        corroboration: Some(Corroboration::IndependentSource),
    }
}

/// The `research_equity` base's envelope: the fields §8.5 and §8.6 read.
#[must_use]
pub fn research_equity_envelope() -> MandateEnvelope {
    MandateEnvelope {
        universe_pinned: false,
        max_instruments: 5,
        asset_classes: BTreeSet::from([AssetClass::UsEquity]),
        leveraged_etps_enabled: false,
        leveraged_etp_disclosure_version: None,
        admits_instruments: true,
        research: Some(ResearchEnvelope {
            interval_s: 3_600,
            cost_cap_usd_per_day: usd("5"),
            max_revisions_per_lineage: 3,
        }),
        admission: AutonomyDecision::Ask,
    }
}

/// One admission's inputs, owned so a test can edit any field before calling.
pub struct Scenario {
    pub mandate: ValidatedMandate,
    pub overlay: PolicyOverlay,
    pub universe: WorkingUniverse,
    pub proposal: ProposedThesis,
    pub facts: AdmissionFacts,
    pub lineages: LineageState,
}

impl Scenario {
    /// MC-N01: every check passes and the thesis is admitted.
    #[must_use]
    pub fn admitting() -> Self {
        Self {
            mandate: ValidatedMandate::from_validated_envelope(research_equity_envelope()),
            overlay: PolicyOverlay::permissive(),
            universe: universe_of(&[]),
            proposal: proposal(thesis("th-1", INSTRUMENT_5)),
            facts: AdmissionFacts {
                allowlist: SourceAllowlist {
                    version: AllowlistVersion(1),
                    sources: BTreeSet::from([source("src.filings"), source("src.newswire")]),
                },
                eligibility_failures: BTreeSet::new(),
                instrument_groups: BTreeMap::new(),
                claimed_by_other_agents: BTreeSet::new(),
                halted_instruments: BTreeSet::new(),
                disclosures_accepted: BTreeSet::new(),
                data_universe: None,
                research_spend_usd_today: usd("0"),
            },
            lineages: LineageState::empty(),
        }
    }

    /// Rebuilds the envelope through a closure, so a test edits one envelope field in one line.
    pub fn with_envelope(mut self, edit: impl FnOnce(&mut MandateEnvelope)) -> Self {
        let mut envelope = self.mandate.envelope().clone();
        edit(&mut envelope);
        self.mandate = ValidatedMandate::from_validated_envelope(envelope);
        self
    }

    #[must_use]
    pub fn input(&self) -> AdmissionInput<'_> {
        AdmissionInput {
            mandate: &self.mandate,
            overlay: &self.overlay,
            universe: &self.universe,
            proposal: &self.proposal,
            facts: &self.facts,
            lineages: &self.lineages,
        }
    }
}

/// A fold's inputs, sharing the admission facts of a [`Scenario`].
pub struct FoldScenario {
    pub scenario: Scenario,
    pub proposals: Vec<ProposedThesis>,
}

impl FoldScenario {
    /// The cap-one base (`research_equity_cap_one`) or the cap-three base, with a thesis sequence.
    #[must_use]
    pub fn new(cap: u32, theses: Vec<Thesis>) -> Self {
        Self {
            scenario: Scenario::admitting().with_envelope(|e| {
                if let Some(research) = e.research.as_mut() {
                    research.max_revisions_per_lineage = cap;
                }
            }),
            proposals: theses.into_iter().map(proposal).collect(),
        }
    }

    #[must_use]
    pub fn input(&self) -> FoldInput<'_> {
        FoldInput {
            mandate: &self.scenario.mandate,
            overlay: &self.scenario.overlay,
            universe: &self.scenario.universe,
            proposals: &self.proposals,
            facts: &self.scenario.facts,
            lineages: &self.scenario.lineages,
        }
    }
}

/// One universe entry for the expiry cases.
#[must_use]
pub fn entry(
    instrument: &str,
    id: &str,
    lineage: &str,
    expires_at: &str,
    invalidated: bool,
) -> UniverseEntry {
    UniverseEntry {
        instrument: asset(instrument),
        thesis_id: thesis_id(id),
        lineage_id: lineage_id(lineage),
        revision: 0,
        expires_at: at(expires_at),
        invalidated,
    }
}
