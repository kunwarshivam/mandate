//! The minimal stream-F shapes this crate needs before `mandate-spec` and `mandate-domain` exist.
//!
//! Every name here is the one stream F's brief fixes (DEC-128 item 21), so nothing renames when
//! those crates land: [`AssetId`] is the schema's `$defs/uuid` id, [`WorkingUniverse`] is
//! `mandate-domain`'s over it, [`ValidatedMandate`] is the only way in, and [`PolicyOverlay`] is the
//! §4.3 runtime overlay whose effective values this crate reads rather than recomputing.
//!
//! This module is temporary by construction. The first `mandate-research` implementation PR after
//! stream F's tests PR merges **deletes it** and takes the real types; nothing here is a second
//! design, only the subset of fields §8.5 and §8.6 read. Stream G's `mandate-risk` carries the same
//! module for the same reason, so the two agree field for field on what they share.
//!
//! [`ModelVersion`] and [`ContentHash`] are the exception: stream H's brief names them and stream H
//! is a sibling at layer 5, which this crate may not depend on, so they are declared here and the
//! coordinator decides whether they move to `mandate-domain` when F's crate lands (DEC-132 item 15).

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::Digest;
use mandate_num::Usd;

/// The schema's `$defs/uuid` id, which every mandate rule and every reference case names.
/// `mandate_accounting::InstrumentId` stays the wider broker-facing id (DEC-128 item 21).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AssetId(String);

impl AssetId {
    /// # Errors
    /// Returns [`SpecTypeError::EmptyAssetId`] for an empty id.
    pub fn new(id: &str) -> Result<Self, SpecTypeError> {
        if id.is_empty() {
            return Err(SpecTypeError::EmptyAssetId);
        }
        Ok(Self(id.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An instrument group's own id (trading spec §7.1). Distinct from [`AssetId`] so that a group whose
/// id spells an asset id cannot claim that instrument (DEC-132 item 12).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct GroupId(String);

impl GroupId {
    /// # Errors
    /// Returns [`SpecTypeError::EmptyGroupId`] for an empty id.
    pub fn new(id: &str) -> Result<Self, SpecTypeError> {
        if id.is_empty() {
            return Err(SpecTypeError::EmptyGroupId);
        }
        Ok(Self(id.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SpecTypeError {
    #[error("an asset id is never empty")]
    EmptyAssetId,
    #[error("a group id is never empty")]
    EmptyGroupId,
    #[error("a model id is never empty")]
    EmptyModelId,
}

/// A decimal field held as the text that satisfied its schema `$def`, never as a rounded number
/// (DEC-128 item 3). This crate only carries, orders, and hands on such values; the grammars and the
/// conversions to arithmetic types are `mandate-spec`'s.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SchemaDec(String);

impl SchemaDec {
    /// The narrow view takes text stream F has already checked against the field's whole `$def`.
    #[must_use]
    pub fn from_checked_text(text: &str) -> Self {
        Self(text.to_owned())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AssetClass {
    UsEquity,
    Crypto,
}

/// AUTO, ASK, or DENY (§6.1). `Ord` is strictness ascending, which is what the admission ceiling and
/// the overlay both need.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AutonomyDecision {
    Auto,
    Ask,
    Deny,
}

/// The set of instruments the agent may open or increase now (mandate spec §2.3).
///
/// [`WorkingUniverse::Unavailable`] is a real state and not an empty set: nothing may be admitted
/// while the `UniverseChanged` fold has not been read, where an empty `Known` is a universe that is
/// known and holds nothing. There is no `Default` and no constructor that guesses, so "we did not
/// read it" cannot be mistaken for "nothing is admitted" (DEC-132 item 18, DEC-129 item 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkingUniverse {
    Known {
        instruments: BTreeSet<AssetId>,
        pinned: bool,
    },
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum InstrumentRestriction {
    StaleMark,
    RemovedInstrument,
}

/// Which way the working universe changed (journal spec §9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UniverseChange {
    Admitted,
    Removed,
}

/// Every reason journal spec §9's `UniverseChanged` row lists. Stream F's brief names its risk-state
/// enum `RemovalReason`, which cannot carry [`UniverseChangeReason::ThesisAdmitted`]; one enum with
/// all seven is what both streams need, and the coordinator settles the name (DEC-132 item 15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UniverseChangeReason {
    ThesisAdmitted,
    ThesisExpired,
    ThesisInvalidated,
    LineageRetired,
    EligibilityLost,
    OperatorHalt,
    VersionApplied,
}

impl UniverseChangeReason {
    /// The journaled reason (ES-09).
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::ThesisAdmitted => "thesis_admitted",
            Self::ThesisExpired => "thesis_expired",
            Self::ThesisInvalidated => "thesis_invalidated",
            Self::LineageRetired => "lineage_retired",
            Self::EligibilityLost => "eligibility_lost",
            Self::OperatorHalt => "operator_halt",
            Self::VersionApplied => "version_applied",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ModelId(String);

impl ModelId {
    /// # Errors
    /// Returns [`SpecTypeError::EmptyModelId`] for an empty id.
    pub fn new(id: &str) -> Result<Self, SpecTypeError> {
        if id.is_empty() {
            return Err(SpecTypeError::EmptyModelId);
        }
        Ok(Self(id.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A signal model's semantic version (§8.1).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ModelVersion(String);

impl ModelVersion {
    #[must_use]
    pub fn new(version: &str) -> Self {
        Self(version.to_owned())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The content hash of a model's code, prompt, parameter schema, and underlying model identity
/// (§8.1), which the mandate pins and which the gateway may never substitute (DEC-67).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ContentHash(Digest);

impl ContentHash {
    #[must_use]
    pub fn new(digest: Digest) -> Self {
        Self(digest)
    }

    #[must_use]
    pub fn digest(&self) -> &Digest {
        &self.0
    }
}

/// `behavior.research` (§8.4): the research agent's envelope fields. Null exactly when no model
/// admits instruments (V-036).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResearchEnvelope {
    pub interval_s: u32,
    pub cost_cap_usd_per_day: Usd,
    pub max_revisions_per_lineage: u32,
}

/// The envelope fields §8.5 and §8.6 read. Every one is user-sourced and confirmed (MI-12); nothing
/// this crate does can change any of them (MI-16).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MandateEnvelope {
    pub universe_pinned: bool,
    pub max_instruments: u32,
    pub asset_classes: BTreeSet<AssetClass>,
    pub leveraged_etps_enabled: bool,
    pub leveraged_etp_disclosure_version: Option<Digest>,
    pub admits_instruments: bool,
    pub research: Option<ResearchEnvelope>,
    pub admission: AutonomyDecision,
}

/// A mandate that passed validation and the policy chain. It has no other constructor in
/// `mandate-spec`, so admission cannot be handed a mandate nobody validated (DEC-128 item 8); the
/// narrow view keeps that shape so no signature changes when the real type lands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedMandate(MandateEnvelope);

impl ValidatedMandate {
    /// The narrow view's stand-in for `ValidatedMandate::new`, which validates. Tests build
    /// envelopes directly; the implementation PR deletes this with the module.
    #[must_use]
    pub fn from_validated_envelope(envelope: MandateEnvelope) -> Self {
        Self(envelope)
    }

    #[must_use]
    pub fn envelope(&self) -> &MandateEnvelope {
        &self.0
    }
}

/// The policy ceilings this crate reads, as the §4.3 overlay resolves them. A level that states no
/// value constrains nothing (DEC-128 item 10), so every accessor takes the mandate's own value and
/// returns the stricter of the two: lower for a maximum, higher for a minimum, and the stricter
/// autonomy decision for `admission`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PolicyOverlay {
    pub max_instruments: Option<u32>,
    pub max_revisions_per_lineage: Option<u32>,
    pub research_cost_cap_usd_per_day: Option<Usd>,
    pub stagger_window_s: Option<u32>,
    pub research_interval_s: Option<u32>,
    pub research_agent_allowed: bool,
    pub admission_auto_allowed: bool,
}

impl PolicyOverlay {
    /// An overlay that constrains nothing beyond the mandate, with both permissions granted.
    #[must_use]
    pub fn permissive() -> Self {
        Self {
            research_agent_allowed: true,
            admission_auto_allowed: true,
            ..Self::default()
        }
    }

    /// DEC-103's internal research profile: the research agent on, every admission `ask`, and at
    /// most three revisions per lineage.
    #[must_use]
    pub fn internal_research_profile() -> Self {
        Self {
            max_revisions_per_lineage: Some(3),
            stagger_window_s: Some(900),
            research_agent_allowed: true,
            admission_auto_allowed: false,
            ..Self::default()
        }
    }

    /// `max_instruments`, a maximum: the lower of the two bounds (check 17, MI-15).
    #[must_use]
    pub fn effective_max_instruments(&self, mandate_value: u32) -> u32 {
        match self.max_instruments {
            Some(ceiling) => ceiling.min(mandate_value),
            None => mandate_value,
        }
    }

    /// `max_revisions_per_lineage`, a maximum: the lower of the two (check 16, DEC-111 item 4).
    #[must_use]
    pub fn effective_max_revisions_per_lineage(&self, mandate_value: u32) -> u32 {
        match self.max_revisions_per_lineage {
            Some(ceiling) => ceiling.min(mandate_value),
            None => mandate_value,
        }
    }

    /// `research_cost_cap_usd_per_day`, a maximum: the lower of the two (check 7, DEC-120).
    #[must_use]
    pub fn effective_cost_cap(&self, mandate_value: Usd) -> Usd {
        match self.research_cost_cap_usd_per_day {
            Some(ceiling) if ceiling < mandate_value => ceiling,
            _ => mandate_value,
        }
    }

    /// `stagger_window_s`, a **minimum**: the higher of the two (§8.4, DEC-123).
    #[must_use]
    pub fn effective_stagger_window_s(&self, mandate_value: u32) -> u32 {
        match self.stagger_window_s {
            Some(floor) => floor.max(mandate_value),
            None => mandate_value,
        }
    }

    /// `research_interval_s`, a **minimum**: the higher of the two (§8.4).
    #[must_use]
    pub fn effective_research_interval_s(&self, mandate_value: u32) -> u32 {
        match self.research_interval_s {
            Some(floor) => floor.max(mandate_value),
            None => mandate_value,
        }
    }

    /// `autonomy.admission`: `auto` evaluates as `ask` once `admission_auto_allowed` is false
    /// (§4.3), and the overlay never loosens a stricter mandate value (MI-17).
    #[must_use]
    pub fn effective_admission(&self, mandate_value: AutonomyDecision) -> AutonomyDecision {
        if self.admission_auto_allowed {
            mandate_value
        } else {
            mandate_value.max(AutonomyDecision::Ask)
        }
    }
}

/// A named group, or an instrument standing as its own group because no policy names one. Making the
/// default a distinct case is what stops a [`GroupId`] spelling an [`AssetId`] from claiming that
/// instrument (DEC-132 item 12).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum InstrumentGroup {
    Named(GroupId),
    Ungrouped(AssetId),
}

/// Trading spec §7.1's claim lookup, in the shape stream F's `ValidationContext` already supplies:
/// a map of the instruments that have a named group, and the instruments other agents hold.
#[must_use]
pub fn group_of(instrument: &AssetId, groups: &BTreeMap<AssetId, GroupId>) -> InstrumentGroup {
    match groups.get(instrument) {
        Some(group) => InstrumentGroup::Named(group.clone()),
        None => InstrumentGroup::Ungrouped(instrument.clone()),
    }
}
