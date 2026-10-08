//! The policy hierarchy ([mandate spec §4.3](../../../docs/specs/mandate.md#43-policy-hierarchy-dec-51-dec-98)):
//! platform, organization, workspace, mandate, where a child may only tighten.
//!
//! [`check`] does two things from one fold of the chain, because they are the same question asked at
//! two times: it reports the violations validation needs, and it returns the [`PolicyOverlay`] the
//! order path needs at runtime ("the stricter value governs, and `auto` evaluates as `ask` when
//! `auto_allowed` becomes false"). One fold means one definition of "stricter", tested once
//! (DEC-128 item 9).

use std::collections::{BTreeMap, BTreeSet};

use core::cmp::Ordering;

use mandate_canon::Value;
use mandate_domain::{AutonomyDecision, Environment, Purpose};

use crate::document::{Channel, LadderAction, Mandate};
use crate::{DecGrammar, SchemaDec, SpecError};

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

impl PolicyLevel {
    /// One `schemas/policy.schema.json` document read strictly, as `Mandate::parse` reads the
    /// mandate schema: an unknown member, a missing one, a value of another type, off its enum, off
    /// its decimal grammar, out of its integer bounds, or a set repeating an item is a
    /// [`ParseError`](crate::ParseError) naming the pointer. `profile` is checked and not kept
    /// (journal spec §9, DEC-484 item 4).
    pub fn parse(document: &Value) -> Result<Self, SpecError> {
        let _ = document;
        Err(SpecError::Unimplemented)
    }
}

/// A `policy_set` configuration object (journal spec §9, DEC-484 item 4): exactly `kind:
/// "policy_set"`, `policy_set_version: 1` and `levels`, each a [`PolicyLevel::parse`] document,
/// returned outermost first. A level out of the platform, organization, workspace order, or
/// repeated, is `invalid_input`; a member or a level that breaks the schema is its
/// [`ParseError`](crate::ParseError), its pointer from the object's root.
pub fn parse_policy_set(object: &Value) -> Result<Vec<PolicyLevel>, SpecError> {
    let _ = object;
    Err(SpecError::Unimplemented)
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
        match (self.tightest.get(&key), mandate_value) {
            (None, value) => Ok(value.clone()),
            (Some(ceiling), PolicyValue::Absent) => Ok(ceiling.clone()),
            (Some(ceiling), value) => tighten(key, value, ceiling),
        }
    }

    /// False when any ancestor forbids `auto`, in which case §4.3 makes every `auto` evaluate as
    /// `ask` — a tightening the order path applies without a new mandate version.
    ///
    /// Infallible and fail-closed: the stub's `false` narrows every `auto` to `ask`, which is the safe
    /// direction, so this one does not need to be a `Result`.
    pub fn auto_allowed(&self) -> bool {
        self.tightest.get(&PolicyKey::AutoAllowed) != Some(&PolicyValue::Flag(false))
    }

    /// `auto` narrowed to `ask` where the overlay forbids it, and otherwise unchanged: `ask` and
    /// `deny` are never raised, whatever the overlay says.
    ///
    /// Only an action that adds risk can be narrowed, and the type says so: `adds` is an
    /// [`AddingPurpose`], which has no exit variant, so a `risk_exit`, `protective`,
    /// `discretionary_exit`, or `owner_exit` AUTO (§6.2 step 3) cannot be passed here at all.
    /// AGENTS.md rules 2 and 13: reducing risk never needs approval (#263 round 1).
    pub fn narrow(&self, adds: AddingPurpose, decision: AutonomyDecision) -> AutonomyDecision {
        let _ = adds;
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

/// The purpose of an action that adds risk (§6.1): the only actions the overlay may narrow. There is
/// no exit variant, so an exit's built-in AUTO can never reach [`PolicyOverlay::narrow`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AddingPurpose {
    Open,
    Increase,
}

impl TryFrom<Purpose> for AddingPurpose {
    /// The purpose given back: an exit, which no policy may narrow.
    type Error = Purpose;

    fn try_from(purpose: Purpose) -> Result<Self, Purpose> {
        match purpose {
            Purpose::Open => Ok(Self::Open),
            Purpose::Increase => Ok(Self::Increase),
            Purpose::DiscretionaryExit
            | Purpose::OwnerExit
            | Purpose::RiskExit
            | Purpose::Protective => Err(purpose),
        }
    }
}

/// The mandate's own values for every key §4.3 constrains, which is what the chain compares against.
///
/// A key the mandate has no value for is [`PolicyValue::Absent`]: the research keys without a research
/// agent, and the two nullable maximums, where absence is itself the violation
/// ([`PolicyKey::absence_violates`]). `stagger_window_s` and `independent_approval_required` are not
/// mandate fields, so a mandate never states them.
///
/// `signal_model_types` is every model's type prefix. [`ModelId::parse`](crate::document::ModelId::parse)
/// admits only `fast.`, `llm.`, and `quant.` ids, so every id has one; were one missing, the whole id
/// would stand in and fail every set it is checked against rather than shrink this one.
pub fn values_of(mandate: &Mandate) -> Result<BTreeMap<PolicyKey, PolicyValue>, SpecError> {
    let risk = &mandate.risk;
    let behavior = &mandate.behavior;
    let autonomy = &mandate.autonomy;
    let models = &behavior.signal_models;
    let admitting = models.iter().find(|model| model.admits_instruments);
    let research = behavior.research.as_ref();
    let decimal = |value: &SchemaDec| PolicyValue::Decimal(value.clone());
    let maybe = |value: Option<&SchemaDec>| value.map_or(PolicyValue::Absent, decimal);
    let integer = |value: u32| PolicyValue::Integer(u64::from(value));
    let set = |items: Vec<&str>| PolicyValue::Set(items.into_iter().map(str::to_owned).collect());
    let auto = autonomy.default == AutonomyDecision::Auto
        || autonomy
            .rules
            .iter()
            .any(|rule| rule.then == AutonomyDecision::Auto);
    Ok(BTreeMap::from([
        (
            PolicyKey::AllocationUsd,
            decimal(&mandate.capital.allocation_usd),
        ),
        (
            PolicyKey::MaxLossFromAllocation,
            decimal(&mandate.capital.max_loss_from_allocation),
        ),
        (PolicyKey::MaxPositionUsd, decimal(&risk.max_position_usd)),
        (
            PolicyKey::MaxPositionFraction,
            decimal(&risk.max_position_fraction),
        ),
        (
            PolicyKey::MaxGrossExposureUsd,
            decimal(&risk.max_gross_exposure_usd),
        ),
        (PolicyKey::MaxOrderUsd, decimal(&risk.max_order_usd)),
        (PolicyKey::MaxOrdersPerDay, integer(risk.max_orders_per_day)),
        (PolicyKey::MaxDailyLoss, decimal(&risk.max_daily_loss)),
        (PolicyKey::MaxDrawdown, decimal(&risk.max_drawdown)),
        (PolicyKey::BreachConfirmS, integer(risk.breach_confirm_s)),
        (
            PolicyKey::MaxOutputAgeS,
            models
                .iter()
                .map(|model| model.max_output_age_s)
                .max()
                .map_or(PolicyValue::Absent, integer),
        ),
        (
            PolicyKey::ExitThreshold,
            decimal(&behavior.sizing.exit_threshold),
        ),
        (
            PolicyKey::StopDistanceMax,
            maybe(mandate.protection.stop_distance.as_ref()),
        ),
        (
            PolicyKey::ExitsOnlyAtMax,
            maybe(
                risk.drawdown_ladder
                    .iter()
                    .find(|rung| rung.action != LadderAction::ScaleSizes)
                    .map(|rung| &rung.at),
            ),
        ),
        (
            PolicyKey::TwoApproverAboveUsd,
            maybe(autonomy.approval.two_approver_above_usd.as_ref()),
        ),
        (
            PolicyKey::MaxInstruments,
            integer(mandate.universe.max_instruments),
        ),
        (
            PolicyKey::ResearchWeight,
            maybe(admitting.map(|model| &model.weight)),
        ),
        (
            PolicyKey::ResearchCostCapUsdPerDay,
            maybe(research.map(|research| &research.cost_cap_usd_per_day)),
        ),
        (
            PolicyKey::MaxRevisionsPerLineage,
            research.map_or(PolicyValue::Absent, |research| {
                integer(research.max_revisions_per_lineage)
            }),
        ),
        (
            PolicyKey::EntryThreshold,
            decimal(&behavior.sizing.entry_threshold),
        ),
        (
            PolicyKey::RebalanceBand,
            decimal(&behavior.sizing.rebalance_band),
        ),
        (PolicyKey::Hysteresis, decimal(&risk.hysteresis)),
        (
            PolicyKey::CadenceIntervalS,
            integer(behavior.cadence.interval_s),
        ),
        (
            PolicyKey::ApprovalTimeoutS,
            integer(autonomy.approval.timeout_s),
        ),
        (
            PolicyKey::ReentryCooldownS,
            integer(risk.reentry_cooldown_s),
        ),
        (PolicyKey::DailyBreachMinS, integer(risk.daily_breach_min_s)),
        (PolicyKey::ScaleLiftAfterS, integer(risk.scale_lift_after_s)),
        (
            PolicyKey::ResearchIntervalS,
            research.map_or(PolicyValue::Absent, |research| integer(research.interval_s)),
        ),
        (
            PolicyKey::LeveragedEtpsAllowed,
            PolicyValue::Flag(mandate.universe.leveraged_etps_enabled),
        ),
        (PolicyKey::AutoAllowed, PolicyValue::Flag(auto)),
        (
            PolicyKey::ResearchAgentAllowed,
            PolicyValue::Flag(admitting.is_some()),
        ),
        (
            PolicyKey::AdmissionAutoAllowed,
            PolicyValue::Flag(autonomy.admission == AutonomyDecision::Auto),
        ),
        (
            PolicyKey::ProtectionRequired,
            PolicyValue::Flag(mandate.protection.enabled),
        ),
        (
            PolicyKey::AssetClasses,
            set(mandate
                .universe
                .asset_classes
                .iter()
                .map(|class| class.as_str())
                .collect()),
        ),
        (
            PolicyKey::SignalModelTypes,
            set(models
                .iter()
                .map(|model| model.id.model_type().unwrap_or(model.id.as_str()))
                .collect()),
        ),
        (PolicyKey::GoalTypes, set(vec![mandate.goal.type_name()])),
        (
            PolicyKey::Channels,
            set(mandate
                .notifications
                .channels
                .iter()
                .map(|channel| channel_name(*channel))
                .collect()),
        ),
        (
            PolicyKey::Environments,
            set(vec![environment_name(mandate.environment)]),
        ),
    ]))
}

/// A channel as the schemas spell it.
fn channel_name(channel: Channel) -> &'static str {
    match channel {
        Channel::Email => "email",
        Channel::Phone => "phone",
        Channel::Slack => "slack",
        Channel::Sms => "sms",
        Channel::Telegram => "telegram",
        Channel::WebPush => "web_push",
    }
}

/// An environment as the schemas spell it.
fn environment_name(environment: Environment) -> &'static str {
    match environment {
        Environment::Paper => "paper",
        Environment::Live => "live",
    }
}

/// A numeric policy value as a decimal: a decimal as it is, an integer read exactly. `None` for a flag
/// or a set.
fn numeric(value: &PolicyValue) -> Option<SchemaDec> {
    match value {
        PolicyValue::Decimal(decimal) => Some(decimal.clone()),
        PolicyValue::Integer(integer) => {
            SchemaDec::parse(&integer.to_string(), DecGrammar::Decimal).ok()
        }
        _ => None,
    }
}

/// A value whose type is not the one the key takes: a caller's mistake, refused rather than read.
fn mismatch() -> SpecError {
    SpecError::InvalidInput {
        what: "a policy value of the wrong type for its key",
    }
}

/// Whether `child` breaks `parent` for `key` (§4.3's table). An absent child breaks nothing, except
/// for the two keys where no limit is looser than any limit.
fn violates(key: PolicyKey, child: &PolicyValue, parent: &PolicyValue) -> Result<bool, SpecError> {
    if *child == PolicyValue::Absent {
        return Ok(key.absence_violates());
    }
    Ok(match key.kind() {
        KeyKind::Maximum | KeyKind::Minimum => {
            let (Some(child), Some(parent)) = (numeric(child), numeric(parent)) else {
                return Err(mismatch());
            };
            let breaks = if key.kind() == KeyKind::Maximum {
                Ordering::Greater
            } else {
                Ordering::Less
            };
            child.cmp(&parent) == breaks
        }
        KeyKind::Permission => match (child, parent) {
            (PolicyValue::Flag(child), PolicyValue::Flag(parent)) => *child && !*parent,
            _ => return Err(mismatch()),
        },
        KeyKind::Requirement => match (child, parent) {
            (PolicyValue::Flag(child), PolicyValue::Flag(parent)) => *parent && !*child,
            _ => return Err(mismatch()),
        },
        KeyKind::Set => match (child, parent) {
            (PolicyValue::Set(child), PolicyValue::Set(parent)) => !child.is_subset(parent),
            _ => return Err(mismatch()),
        },
    })
}

/// The stricter of two stated values for `key`: the one the other does not break.
fn stricter<'a>(
    key: PolicyKey,
    first: &'a PolicyValue,
    second: &'a PolicyValue,
) -> Result<&'a PolicyValue, SpecError> {
    Ok(if violates(key, first, second)? {
        second
    } else {
        first
    })
}

/// The tighter of two stated values, a set meeting as the intersection. A set against anything else
/// is refused by [`stricter`].
fn tighten(
    key: PolicyKey,
    held: &PolicyValue,
    next: &PolicyValue,
) -> Result<PolicyValue, SpecError> {
    match (key.kind(), held, next) {
        (KeyKind::Set, PolicyValue::Set(held), PolicyValue::Set(next)) => {
            Ok(PolicyValue::Set(held.intersection(next).cloned().collect()))
        }
        _ => stricter(key, held, next).cloned(),
    }
}

/// Checks a mandate against a chain of levels, outermost first, and folds the chain into an overlay.
///
/// Every level, and then the mandate, is compared against every level above it; for each key a level
/// states, the violation names the **nearest** ancestor that states the key and is broken, and at most
/// one violation is reported per key and level. A level's `Absent` value states nothing. The overlay
/// is the tightest stated value per key over the levels, not the mandate.
///
/// A level named [`LevelName::Mandate`] in `levels` is refused with `invalid_input`: it would be
/// compared as an ancestor yet never folded into the overlay, so its ceiling would bind at save time
/// and vanish at runtime (#263 round 1).
pub fn check(mandate: &Mandate, levels: &[PolicyLevel]) -> Result<PolicyResult, SpecError> {
    if levels.iter().any(|level| level.name == LevelName::Mandate) {
        return Err(SpecError::InvalidInput {
            what: "a policy chain holds only ancestors; the mandate level is the document itself",
        });
    }
    let own = PolicyLevel {
        name: LevelName::Mandate,
        values: values_of(mandate)?,
    };
    let mut violations = Vec::new();
    let mut tightest: BTreeMap<PolicyKey, PolicyValue> = BTreeMap::new();
    for (index, child) in levels.iter().chain([&own]).enumerate() {
        let ancestors = levels.get(..index).unwrap_or_default();
        for (key, value) in &child.values {
            if *value == PolicyValue::Absent && child.name != LevelName::Mandate {
                continue;
            }
            for ancestor in ancestors.iter().rev() {
                let Some(limit) = ancestor.values.get(key) else {
                    continue;
                };
                if *limit == PolicyValue::Absent {
                    continue;
                }
                if violates(*key, value, limit)? {
                    violations.push(PolicyViolation {
                        key: *key,
                        level: child.name,
                        value: value.clone(),
                        limit_level: ancestor.name,
                        limit: limit.clone(),
                    });
                    break;
                }
            }
        }
        if child.name != LevelName::Mandate {
            for (key, value) in &child.values {
                if *value == PolicyValue::Absent {
                    continue;
                }
                let next = match tightest.get(key) {
                    Some(held) => tighten(*key, held, value)?,
                    None => value.clone(),
                };
                tightest.insert(*key, next);
            }
        }
    }
    Ok(PolicyResult {
        violations,
        overlay: PolicyOverlay { tightest },
    })
}

fn decimal_value(text: &str, grammar: DecGrammar) -> Result<PolicyValue, SpecError> {
    SchemaDec::parse(text, grammar)
        .map(PolicyValue::Decimal)
        .map_err(|_| SpecError::InvalidInput {
            what: "a policy decimal outside its grammar",
        })
}

fn names(items: &[&str]) -> PolicyValue {
    PolicyValue::Set(items.iter().map(|item| (*item).to_owned()).collect())
}

/// The platform base of §4.3: `max_loss_from_allocation` at most 0.5, `breach_confirm_s` at most 300,
/// `max_instruments` at most 20, `stagger_window_s` at least 900 (DEC-117, DEC-123).
pub fn platform_base() -> Result<PolicyLevel, SpecError> {
    Ok(PolicyLevel {
        name: LevelName::Platform,
        values: BTreeMap::from([
            (
                PolicyKey::MaxLossFromAllocation,
                decimal_value("0.5", DecGrammar::OpenFraction)?,
            ),
            (PolicyKey::BreachConfirmS, PolicyValue::Integer(300)),
            (PolicyKey::MaxInstruments, PolicyValue::Integer(20)),
            (PolicyKey::StaggerWindowS, PolicyValue::Integer(900)),
        ]),
    })
}

/// The retail profile of §4.3 (DEC-98), including the conservative placeholders that stand until
/// counsel answers: `approval_timeout_s` at least 120 and `max_loss_from_allocation` at most 0.2.
pub fn retail_profile() -> Result<PolicyLevel, SpecError> {
    Ok(PolicyLevel {
        name: LevelName::Platform,
        values: BTreeMap::from([
            (PolicyKey::AutoAllowed, PolicyValue::Flag(true)),
            (PolicyKey::SignalModelTypes, names(&["llm", "quant"])),
            (PolicyKey::LeveragedEtpsAllowed, PolicyValue::Flag(false)),
            (PolicyKey::ProtectionRequired, PolicyValue::Flag(true)),
            (PolicyKey::ApprovalTimeoutS, PolicyValue::Integer(120)),
            (
                PolicyKey::MaxLossFromAllocation,
                decimal_value("0.2", DecGrammar::OpenFraction)?,
            ),
            (PolicyKey::ResearchAgentAllowed, PolicyValue::Flag(false)),
            (PolicyKey::Environments, names(&["paper"])),
        ]),
    })
}

/// The internal research profile of §4.3 (DEC-103's thin slice): the research agent on, paper only,
/// every admission `ask`, at most three revisions per lineage.
pub fn internal_research_profile() -> Result<PolicyLevel, SpecError> {
    Ok(PolicyLevel {
        name: LevelName::Platform,
        values: BTreeMap::from([
            (PolicyKey::ResearchAgentAllowed, PolicyValue::Flag(true)),
            (PolicyKey::AdmissionAutoAllowed, PolicyValue::Flag(false)),
            (PolicyKey::MaxRevisionsPerLineage, PolicyValue::Integer(3)),
            (PolicyKey::Environments, names(&["paper"])),
        ]),
    })
}

#[cfg(test)]
mod tests;
