//! Semantic validation ([mandate spec §4.1](../../../docs/specs/mandate.md#41-v-rules), §4.2) and the
//! one way a validated mandate is made.
//!
//! [`validate`] returns **every** code the document breaks, sorted, because §4 says so: an author
//! fixes one round of findings, not one finding. `Err` is reserved for a document that cannot be
//! evaluated at all — a decimal no exact type can hold (DEC-128 item 4) — which is not a rule
//! violation and must never be reported as one.

use std::collections::{BTreeMap, BTreeSet};

use mandate_domain::{AssetClass, AssetId, Environment};
use mandate_num::Usd;
use mandate_time::Date;

use crate::document::{ConnectionId, Mandate, ModelId, ProvenanceMap};
use crate::policy::{PolicyLevel, PolicyViolation};
use crate::{SchemaDec, SpecError};

/// A semantic rule of §4.1, by the code the spec gives it.
///
/// There is no V-004, V-019, V-021, or V-025 to V-029: §4.1 does not define them, and v0.1's V-021 was
/// withdrawn in the v0.6 rewrite. The enum has no variant for a code the spec does not state, so a gap
/// in the numbering cannot read as a rule someone forgot to implement.
///
/// The type is `Violation` and not `Rule` because [`Rule`](crate::document::Rule) is
/// `autonomy.rules[]`, which stream H reads by that name (DEC-128 item 21).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Violation {
    V001,
    V002,
    V003,
    V005,
    V006,
    V007,
    V008,
    V009,
    V010,
    V011,
    V012,
    V013,
    V014,
    V015,
    V016,
    V017,
    V018,
    V020,
    V022,
    V023,
    V024,
    V030,
    V031,
    V032,
    V033,
    V034,
    V035,
    V036,
    V037,
    V038,
    V039,
}

impl Violation {
    /// The code as §4.1 writes it, which is also the reference cases' (ES-09).
    pub fn code(self) -> &'static str {
        match self {
            Self::V001 => "V-001",
            Self::V002 => "V-002",
            Self::V003 => "V-003",
            Self::V005 => "V-005",
            Self::V006 => "V-006",
            Self::V007 => "V-007",
            Self::V008 => "V-008",
            Self::V009 => "V-009",
            Self::V010 => "V-010",
            Self::V011 => "V-011",
            Self::V012 => "V-012",
            Self::V013 => "V-013",
            Self::V014 => "V-014",
            Self::V015 => "V-015",
            Self::V016 => "V-016",
            Self::V017 => "V-017",
            Self::V018 => "V-018",
            Self::V020 => "V-020",
            Self::V022 => "V-022",
            Self::V023 => "V-023",
            Self::V024 => "V-024",
            Self::V030 => "V-030",
            Self::V031 => "V-031",
            Self::V032 => "V-032",
            Self::V033 => "V-033",
            Self::V034 => "V-034",
            Self::V035 => "V-035",
            Self::V036 => "V-036",
            Self::V037 => "V-037",
            Self::V038 => "V-038",
            Self::V039 => "V-039",
        }
    }
}

impl core::fmt::Display for Violation {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.code())
    }
}

/// A warning of §4.2. Warnings never block; each is acknowledged on the confirmation screen and
/// recorded in `MandateConfirmed`, which is the authoring flow's job, not this crate's.
///
/// There is no W-004: §4.2 does not define one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Warning {
    W001,
    W002,
    W003,
    W005,
    W006,
}

impl Warning {
    pub fn code(self) -> &'static str {
        match self {
            Self::W001 => "W-001",
            Self::W002 => "W-002",
            Self::W003 => "W-003",
            Self::W005 => "W-005",
            Self::W006 => "W-006",
        }
    }
}

impl core::fmt::Display for Warning {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.code())
    }
}

/// A registered signal model, as V-007 compares it (§8.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredModel {
    pub version: String,
    pub content_hash: mandate_canon::Digest,
    /// Exactly the parameters the model declares, sorted.
    pub params: BTreeSet<String>,
    pub admits_instruments: bool,
}

/// Everything the V-rules need that is not in the document.
///
/// All of it is supplied by the caller: this crate reads no registry, no account, and no clock. A
/// `None` registry means V-007 is not checked, which is how the reference cases express "not stated".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationContext {
    pub account_equity_usd: Usd,
    pub other_allocations_usd: Usd,
    pub validation_date: Date,
    pub registry: Option<BTreeMap<ModelId, RegisteredModel>>,
    pub provenance: ProvenanceMap,
    pub workspace_users: u32,
    pub approver_users: u32,
    pub disclosures_accepted: BTreeSet<mandate_canon::Digest>,
    /// Which instrument group each instrument belongs to (trading spec §7.1); an instrument absent
    /// from the map is its own group.
    pub instrument_groups: BTreeMap<AssetId, GroupId>,
    pub claimed_by_other_agents: BTreeSet<AssetId>,
    /// The environment the connection actually is, for V-001. `None` means it matches.
    pub connection_environment: Option<Environment>,
    pub connection_loss_carry_usd: Usd,
    /// Instruments that fail the eligibility floor at validation time, which warns rather than
    /// blocks (W-001); the gate enforces at runtime.
    pub eligibility_failures: BTreeSet<AssetId>,
    pub previous_version: Option<PreviousVersion>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GroupId(String);

impl GroupId {
    pub fn new(id: &str) -> Self {
        Self(id.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What V-031 compares a new version against: the two fields that may never change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviousVersion {
    pub environment: Environment,
    pub connection_id: ConnectionId,
}

/// The four dollar figures §4.2 puts on the confirmation screen.
///
/// `one_position_at_stop_usd` is `None` exactly when protection is disabled, because there is then no
/// stop to lose at — which W-003 warns about separately rather than reporting as a zero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorstCase {
    pub one_position_at_stop_usd: Option<Usd>,
    pub daily_loss_budget_usd: Usd,
    pub flatten_trigger_loss_usd: Usd,
    pub lifetime_floor_loss_usd: Usd,
}

/// What §4 reports: every violated code, every warning, and the worst-case figures.
///
/// The two sets are disjoint: a warning never appears among the violations and never blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationReport {
    pub violations: BTreeSet<Violation>,
    pub warnings: BTreeSet<Warning>,
    pub worst_case: WorstCase,
}

impl ValidationReport {
    pub fn is_valid(&self) -> bool {
        self.violations.is_empty()
    }
}

/// Runs the schema-independent rules of §4.1 and the warnings of §4.2 over a parsed document.
///
/// Returns every code the document breaks, not the first. `Err` means the document could not be
/// evaluated — a decimal outside the range of the arithmetic a rule needs — which is
/// [`SpecError::OutOfRange`] naming the pointer, never a V-code (DEC-128 item 4).
pub fn validate(
    mandate: &Mandate,
    context: &ValidationContext,
) -> Result<ValidationReport, SpecError> {
    let _ = (mandate, context);
    Err(SpecError::Unimplemented)
}

/// A mandate that passed the schema, every V-rule, and the policy hierarchy.
///
/// The only constructor is [`ValidatedMandate::new`], so no caller can build one from a document
/// nobody validated. `mandate-risk`, `mandate-builder`, and the research agent all take
/// `&ValidatedMandate`, which is what moves AGENTS.md rule 1 from a convention a reviewer checks to
/// something the types will not express (DEC-128 item 8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedMandate {
    mandate: Mandate,
}

impl ValidatedMandate {
    /// The document, if it breaks no rule and no policy above it.
    ///
    /// `Err(Rejected)` carries the whole report and every policy violation rather than one code, so a
    /// caller can show the author everything at once.
    pub fn new(
        mandate: Mandate,
        context: &ValidationContext,
        policies: &[PolicyLevel],
    ) -> Result<Self, Rejected> {
        let _ = (&mandate, context, policies);
        Err(Rejected::NotEvaluated(SpecError::Unimplemented))
    }

    pub fn mandate(&self) -> &Mandate {
        &self.mandate
    }
}

/// Why a document is not a [`ValidatedMandate`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Rejected {
    /// The report is boxed so that returning `Result<ValidatedMandate, Rejected>` stays cheap: the
    /// success path is the common one and must not carry the size of a whole report.
    #[error("the mandate breaks {} rule(s) and {} policy limit(s)", report.violations.len(), policy.len())]
    Rules {
        report: Box<ValidationReport>,
        policy: Vec<PolicyViolation>,
    },
    #[error(transparent)]
    NotEvaluated(#[from] SpecError),
}

/// The closed list of §7: the only paths a `platform_default` may appear on, each with the only value
/// it may take there (`None` meaning any value).
///
/// V-020 reads this list and nothing wider. A default anywhere else, or with another value, is a
/// violation — which is what stops the platform from quietly choosing a limit for an owner.
pub fn platform_defaultable() -> BTreeMap<&'static str, Option<&'static str>> {
    BTreeMap::from([
        ("/name", None),
        ("/notifications", None),
        ("/autonomy/approval/on_timeout", Some("skip")),
        ("/autonomy/approval/approvers", None),
        ("/autonomy/default", Some("ask")),
        ("/autonomy/admission", Some("ask")),
        ("/universe/leveraged_etps_enabled", Some("false")),
        ("/universe/leveraged_etp_disclosure_version", Some("null")),
        ("/environment", Some("paper")),
    ])
}

/// The three paths that may never be `platform_proposed` (V-038): bring-your-own-strategy means the
/// owner's own universe, and the research agent is the path for the platform's ideas.
pub const NEVER_PROPOSED: [&str; 3] = [
    "/universe/pinned_instruments",
    "/environment",
    "/connection_id",
];

/// What the compiler proposes when the owner has not stated it (§7): both shown as proposed, both
/// needing confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlatformProposals {
    /// `universe.max_instruments`, 5 (DEC-117).
    pub max_instruments: u32,
    /// `behavior.research.max_revisions_per_lineage`, 3 (DEC-111).
    pub max_revisions_per_lineage: u32,
}

impl PlatformProposals {
    pub const DEFAULT: Self = Self {
        max_instruments: 5,
        max_revisions_per_lineage: 3,
    };
}

/// Whether an instrument's class is one the universe allows (V-039), exposed because admission asks
/// the same question at runtime (§8.5 check 10).
pub fn class_allowed(class: AssetClass, allowed: &BTreeSet<AssetClass>) -> bool {
    allowed.contains(&class)
}

/// The stop distance a worst-case figure uses: the stop, plus the crypto stop-limit offset when the
/// universe admits crypto (§4.2 W-002).
pub fn worst_case_stop_distance(mandate: &Mandate) -> Result<Option<SchemaDec>, SpecError> {
    let _ = mandate;
    Err(SpecError::Unimplemented)
}
