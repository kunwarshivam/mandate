//! The policy hierarchy ([mandate spec §4.3](../../../docs/specs/mandate.md#43-policy-hierarchy-dec-51-dec-98)):
//! platform, organization, workspace, mandate, where a child may only tighten.
//!
//! [`check`] does two things from one fold of the chain, because they are the same question asked at
//! two times: it reports the violations validation needs, and it returns the [`PolicyOverlay`] the
//! order path needs at runtime ("the stricter value governs, and `auto` evaluates as `ask` when
//! `auto_allowed` becomes false"). One fold means one definition of "stricter", tested once
//! (DEC-128 item 9).

use std::collections::{BTreeMap, BTreeSet};

use mandate_domain::AutonomyDecision;

use crate::document::Mandate;
use crate::{SchemaDec, SpecError};

/// A level of the hierarchy, outermost first when a chain is passed to [`check`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LevelName {
    Platform,
    Organization,
    Workspace,
    Mandate,
}

impl LevelName {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Platform => "platform",
            Self::Organization => "organization",
            Self::Workspace => "workspace",
            Self::Mandate => "mandate",
        }
    }
}

/// Every key §4.3 constrains, grouped by how it is compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PolicyKey {
    AllocationUsd,
    MaxLossFromAllocation,
    MaxPositionUsd,
    MaxPositionFraction,
    MaxGrossExposureUsd,
    MaxOrderUsd,
    MaxOrdersPerDay,
    MaxDailyLoss,
    MaxDrawdown,
    BreachConfirmS,
    MaxOutputAgeS,
    ExitThreshold,
    StopDistanceMax,
    ExitsOnlyAtMax,
    TwoApproverAboveUsd,
    MaxInstruments,
    ResearchWeight,
    ResearchCostCapUsdPerDay,
    MaxRevisionsPerLineage,
    EntryThreshold,
    RebalanceBand,
    Hysteresis,
    CadenceIntervalS,
    ApprovalTimeoutS,
    ReentryCooldownS,
    DailyBreachMinS,
    ScaleLiftAfterS,
    ResearchIntervalS,
    StaggerWindowS,
    LeveragedEtpsAllowed,
    AutoAllowed,
    ResearchAgentAllowed,
    AdmissionAutoAllowed,
    ProtectionRequired,
    IndependentApprovalRequired,
    AssetClasses,
    SignalModelTypes,
    GoalTypes,
    Channels,
    Environments,
}

/// How a key is compared against its ancestors (§4.3's five kinds).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeyKind {
    /// Child at most parent.
    Maximum,
    /// Child at least parent.
    Minimum,
    /// Child may be true only if every ancestor is.
    Permission,
    /// Once true at a level, every child is true.
    Requirement,
    /// Child a subset of parent.
    Set,
}

impl PolicyKey {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AllocationUsd => "allocation_usd",
            Self::MaxLossFromAllocation => "max_loss_from_allocation",
            Self::MaxPositionUsd => "max_position_usd",
            Self::MaxPositionFraction => "max_position_fraction",
            Self::MaxGrossExposureUsd => "max_gross_exposure_usd",
            Self::MaxOrderUsd => "max_order_usd",
            Self::MaxOrdersPerDay => "max_orders_per_day",
            Self::MaxDailyLoss => "max_daily_loss",
            Self::MaxDrawdown => "max_drawdown",
            Self::BreachConfirmS => "breach_confirm_s",
            Self::MaxOutputAgeS => "max_output_age_s",
            Self::ExitThreshold => "exit_threshold",
            Self::StopDistanceMax => "stop_distance_max",
            Self::ExitsOnlyAtMax => "exits_only_at_max",
            Self::TwoApproverAboveUsd => "two_approver_above_usd",
            Self::MaxInstruments => "max_instruments",
            Self::ResearchWeight => "research_weight",
            Self::ResearchCostCapUsdPerDay => "research_cost_cap_usd_per_day",
            Self::MaxRevisionsPerLineage => "max_revisions_per_lineage",
            Self::EntryThreshold => "entry_threshold",
            Self::RebalanceBand => "rebalance_band",
            Self::Hysteresis => "hysteresis",
            Self::CadenceIntervalS => "cadence_interval_s",
            Self::ApprovalTimeoutS => "approval_timeout_s",
            Self::ReentryCooldownS => "reentry_cooldown_s",
            Self::DailyBreachMinS => "daily_breach_min_s",
            Self::ScaleLiftAfterS => "scale_lift_after_s",
            Self::ResearchIntervalS => "research_interval_s",
            Self::StaggerWindowS => "stagger_window_s",
            Self::LeveragedEtpsAllowed => "leveraged_etps_allowed",
            Self::AutoAllowed => "auto_allowed",
            Self::ResearchAgentAllowed => "research_agent_allowed",
            Self::AdmissionAutoAllowed => "admission_auto_allowed",
            Self::ProtectionRequired => "protection_required",
            Self::IndependentApprovalRequired => "independent_approval_required",
            Self::AssetClasses => "asset_classes",
            Self::SignalModelTypes => "signal_model_types",
            Self::GoalTypes => "goal_types",
            Self::Channels => "channels",
            Self::Environments => "environments",
        }
    }

    /// Which comparison §4.3 gives the key.
    pub fn kind(self) -> KeyKind {
        match self {
            Self::AllocationUsd
            | Self::MaxLossFromAllocation
            | Self::MaxPositionUsd
            | Self::MaxPositionFraction
            | Self::MaxGrossExposureUsd
            | Self::MaxOrderUsd
            | Self::MaxOrdersPerDay
            | Self::MaxDailyLoss
            | Self::MaxDrawdown
            | Self::BreachConfirmS
            | Self::MaxOutputAgeS
            | Self::ExitThreshold
            | Self::StopDistanceMax
            | Self::ExitsOnlyAtMax
            | Self::TwoApproverAboveUsd
            | Self::MaxInstruments
            | Self::ResearchWeight
            | Self::ResearchCostCapUsdPerDay
            | Self::MaxRevisionsPerLineage => KeyKind::Maximum,
            Self::EntryThreshold
            | Self::RebalanceBand
            | Self::Hysteresis
            | Self::CadenceIntervalS
            | Self::ApprovalTimeoutS
            | Self::ReentryCooldownS
            | Self::DailyBreachMinS
            | Self::ScaleLiftAfterS
            | Self::ResearchIntervalS
            | Self::StaggerWindowS => KeyKind::Minimum,
            Self::LeveragedEtpsAllowed
            | Self::AutoAllowed
            | Self::ResearchAgentAllowed
            | Self::AdmissionAutoAllowed => KeyKind::Permission,
            Self::ProtectionRequired | Self::IndependentApprovalRequired => KeyKind::Requirement,
            Self::AssetClasses
            | Self::SignalModelTypes
            | Self::GoalTypes
            | Self::Channels
            | Self::Environments => KeyKind::Set,
        }
    }

    /// True for the two maximums where an **unset** mandate value is itself the violation, because
    /// "no limit" is looser than any limit (§4.3: "the mandate must set one at or below it").
    ///
    /// Every other key treats absence as "constrains nothing". That asymmetry is the one thing about
    /// §4.3 a reader is likely to get backwards, so it is named here rather than buried in a branch
    /// (DEC-128 item 10).
    pub fn absence_violates(self) -> bool {
        matches!(self, Self::TwoApproverAboveUsd | Self::StopDistanceMax)
    }
}

/// A value at a level, or its absence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyValue {
    Decimal(SchemaDec),
    Integer(u64),
    Flag(bool),
    Set(BTreeSet<String>),
    /// The level states nothing for this key.
    Absent,
}

/// One level's values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyLevel {
    pub name: LevelName,
    pub values: BTreeMap<PolicyKey, PolicyValue>,
}

/// One violation, naming the key, the level that broke it, and the **nearest** ancestor whose value it
/// breaks (§4.3).
///
/// Nearest, not first: an author needs the limit that actually binds, and the tightest ancestor is the
/// one to argue with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyViolation {
    pub key: PolicyKey,
    pub level: LevelName,
    pub value: PolicyValue,
    pub limit_level: LevelName,
    pub limit: PolicyValue,
}

/// What [`check`] returns: the violations, and the overlay the order path reads at runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyResult {
    pub violations: Vec<PolicyViolation>,
    pub overlay: PolicyOverlay,
}

/// The stricter of every ancestor's value for each key (§4.3's runtime overlay).
///
/// Read by `mandate-risk` for the ceiling of each limit it enforces, by `mandate-builder` through
/// [`PolicyOverlay::auto_allowed`] before it returns `auto`, and by the research agent for
/// `max_instruments`, `research_cost_cap_usd_per_day`, `research_interval_s`,
/// `max_revisions_per_lineage`, `research_agent_allowed`, `admission_auto_allowed`, and
/// `stagger_window_s` — §8.5's checks 4, 6, 7, 16, and 17 and §8.4's timing (DEC-128 item 9).
/// No [`Default`]: only [`check`] folds an overlay, so a caller cannot hold one that constrains
/// nothing and believe it means "no ceiling anywhere". An overlay that was never folded is not an
/// empty overlay, it is an unknown one, and the type does not let you make it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyOverlay {
    tightest: BTreeMap<PolicyKey, PolicyValue>,
}

impl PolicyOverlay {
    /// The value that governs: the stricter of the mandate's and every ancestor's.
    ///
    /// Fallible, so the stub cannot answer "no ceiling": a gate that read an absent ceiling from an
    /// unimplemented overlay would enforce nothing, which is the one way this type could fail open.
    pub fn effective(
        &self,
        key: PolicyKey,
        mandate_value: &PolicyValue,
    ) -> Result<PolicyValue, SpecError> {
        let _ = (key, mandate_value);
        Err(SpecError::Unimplemented)
    }

    /// False when any ancestor forbids `auto`, in which case §4.3 makes every `auto` evaluate as
    /// `ask` — a tightening the order path applies without a new mandate version.
    ///
    /// Infallible and fail-closed: the stub's `false` narrows every `auto` to `ask`, which is the safe
    /// direction, so this one does not need to be a `Result`.
    pub fn auto_allowed(&self) -> bool {
        false
    }

    /// `auto` narrowed to `ask` where the overlay forbids it, and otherwise unchanged.
    pub fn narrow(&self, decision: AutonomyDecision) -> AutonomyDecision {
        if decision == AutonomyDecision::Auto && !self.auto_allowed() {
            AutonomyDecision::Ask
        } else {
            decision
        }
    }

    pub fn tightest(&self) -> &BTreeMap<PolicyKey, PolicyValue> {
        &self.tightest
    }
}

/// The mandate's own values for every key §4.3 constrains, which is what the chain compares against.
pub fn values_of(mandate: &Mandate) -> Result<BTreeMap<PolicyKey, PolicyValue>, SpecError> {
    let _ = mandate;
    Err(SpecError::Unimplemented)
}

/// Checks a mandate against a chain of levels, outermost first, and folds the chain into an overlay.
pub fn check(mandate: &Mandate, levels: &[PolicyLevel]) -> Result<PolicyResult, SpecError> {
    let _ = (mandate, levels);
    Err(SpecError::Unimplemented)
}

/// The platform base of §4.3: `max_loss_from_allocation` at most 0.5, `breach_confirm_s` at most 300,
/// `max_instruments` at most 20, `stagger_window_s` at least 900 (DEC-117, DEC-123).
pub fn platform_base() -> Result<PolicyLevel, SpecError> {
    Err(SpecError::Unimplemented)
}

/// The retail profile of §4.3 (DEC-98), including the conservative placeholders that stand until
/// counsel answers: `approval_timeout_s` at least 120 and `max_loss_from_allocation` at most 0.2.
pub fn retail_profile() -> Result<PolicyLevel, SpecError> {
    Err(SpecError::Unimplemented)
}

/// The internal research profile of §4.3 (DEC-103's thin slice): the research agent on, paper only,
/// every admission `ask`, at most three revisions per lineage.
pub fn internal_research_profile() -> Result<PolicyLevel, SpecError> {
    Err(SpecError::Unimplemented)
}
