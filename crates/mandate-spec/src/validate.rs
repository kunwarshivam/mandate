//! Semantic validation ([mandate spec §4.1](../../../docs/specs/mandate.md#41-v-rules), §4.2) and the
//! one way a validated mandate is made.
//!
//! [`validate`] returns **every** code the document breaks, sorted, because §4 says so: an author
//! fixes one round of findings, not one finding. `Err` is reserved for a document that cannot be
//! evaluated at all — a decimal no exact type can hold (DEC-128 item 4) — which is not a rule
//! violation and must never be reported as one.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::Value;
use mandate_domain::{AssetClass, AssetId, AutonomyDecision, Environment};
use mandate_num::{Fraction, NumError, Usd};
use mandate_time::{Date, UtcNanos};

use crate::change::{ChangeClass, classify};
use crate::condition::{
    Condition, ConditionField, ConditionValue, FieldKind, MAX_CONDITION_DEPTH, Operator,
};
use crate::document::{
    ConnectionId, Goal, LadderAction, Lifts, Mandate, MandateVersion, ModelId, Pointer,
    ProvenanceMap, ScaleAction, SignalModel, Source, pointer,
};
use crate::policy::{PolicyLevel, PolicyViolation, check, platform_base};
use crate::{DecGrammar, SchemaDec, SpecError};

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
    /// The `scale_sizes` factors' fractional digits sum past 12, so some set of active rungs would
    /// have a size factor the order builder cannot multiply by exactly (DEC-167).
    V040,
    /// A delegation's id repeats, its `lifts` names a source that is not an `ask`, or its window is
    /// empty or longer than 30 days (§6.5, DEC-181).
    V041,
    /// A version risk-increasing on any path but the delegations and the review date carries a
    /// delegation over from the previous version (§9.2 read with the delegations removed, DEC-353
    /// item 4).
    V042,
    /// A delegation's caps do not fit inside the envelope, or stand in for a second approver.
    V043,
    /// Tripwire ids are sorted and unique, and every threshold fits its metric and allocation
    /// (§6.7, DEC-352).
    V044,
    /// The workspace's policy requires independent approval and the workspace has fewer than two
    /// active users, so nothing the policy reserves for a second user could ever happen (DEC-411).
    /// Checked at validation and again when a version is applied ([`recheck_at_application`]).
    V047,
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
            Self::V040 => "V-040",
            Self::V041 => "V-041",
            Self::V042 => "V-042",
            Self::V043 => "V-043",
            Self::V044 => "V-044",
            Self::V047 => "V-047",
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
    /// The workspace's active members (V-020, V-047): a pending invitation or a deactivated account
    /// is not one. A caller with no count passes 1 or 0, never more, because a count that is absent or
    /// not known counts as one user (§4.1 V-047, rule 3); [`ContextArgs`](crate::context::ContextArgs)
    /// with no membership folds to 0.
    pub workspace_users: u32,
    pub approver_users: u32,
    /// The effective `independent_approval_required` (§4.3: once `true` at a level, every child is
    /// `true`), which V-047 reads. An absent policy key is `false`, as the hierarchy reads it.
    pub independent_approval_required: bool,
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
    /// The agent's current version, as the journal's latest applied version folds it: the document
    /// whose canonical hash (§9.1) this is (DEC-444 item 3). `None` when the platform holds no
    /// current version — a first deployment — and V-047 then refuses every version.
    ///
    /// At application the recheck reads this **again**, from the context folded at the moment of
    /// application, while `previous_version` is the version the draft was **validated against**,
    /// threaded from that validation and never re-derived from the version in force, which may
    /// have advanced past it: a version whose validated-against predecessor no longer equals this
    /// is refused by V-047, not classified against the newer one (DEC-444 item 3's fifth bullet).
    pub current_mandate_version: Option<MandateVersion>,
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

/// What V-031 and V-042 compare a new version against: the two fields that may never change, and
/// the previous version itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviousVersion {
    pub environment: Environment,
    pub connection_id: ConnectionId,
    /// The previous version's document, which V-042 classifies the new one against. `None` when the
    /// caller holds only the version's identity: V-042 then cannot tell a carried delegation from a
    /// new one, so it refuses every delegation the new version holds, failing closed (DEC-420).
    ///
    /// At validation this is the version in force; at application it is the version the draft was
    /// validated against, threaded from that validation — never the version in force at
    /// application, which may have advanced and would silently re-classify the draft against a
    /// newer document instead of refusing it (DEC-444 item 3's stale-predecessor rule).
    pub mandate: Option<Mandate>,
    /// The version's digest as the journal recorded it beside the document, so the pair is never
    /// assembled per request (DEC-444 item 3): V-047 matches it against
    /// [`ValidationContext::current_mandate_version`] and never re-hashes the document itself.
    /// `None` when the caller read no journal record, and V-047 then refuses.
    pub mandate_version: Option<MandateVersion>,
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
///
/// A mandate whose public fields no longer match the document it was parsed from is refused with
/// [`ParseError::Diverged`](crate::ParseError::Diverged): three rules read that document (V-009 the
/// order of the sets, V-015 an invalid date, V-020 a listed default's value), and a report must describe
/// the one mandate that would be hashed and enforced.
pub fn validate(
    mandate: &Mandate,
    context: &ValidationContext,
) -> Result<ValidationReport, SpecError> {
    let document = mandate.canonical()?;
    let worst_case = worst_case(mandate)?;
    let mut violations = BTreeSet::new();
    account_rules(mandate, context, &worst_case, &mut violations)?;
    document_rules(mandate, &document, &mut violations);
    condition_rules(mandate, &mut violations);
    delegation_rules(mandate, context, &mut violations)?;
    tripwire_rules(mandate)?;
    provenance_rules(mandate, &document, context, &mut violations);
    universe_rules(mandate, &mut violations);
    let rules = &mandate.autonomy.rules;
    let warnings = [
        (!context.eligibility_failures.is_empty(), Warning::W001),
        (
            worst_case
                .one_position_at_stop_usd
                .is_some_and(|loss| loss > worst_case.daily_loss_budget_usd),
            Warning::W002,
        ),
        (!mandate.protection.enabled, Warning::W003),
        (
            rules
                .iter()
                .rev()
                .skip(1)
                .any(|rule| rule.when.is_catch_all()),
            Warning::W005,
        ),
        (
            mandate.autonomy.admission == AutonomyDecision::Auto
                && admitting(mandate).next().is_some(),
            Warning::W006,
        ),
    ]
    .into_iter()
    .filter_map(|(fired, warning)| fired.then_some(warning))
    .collect();
    Ok(ValidationReport {
        violations,
        warnings,
        worst_case,
    })
}

/// V-044's tests-PR boundary. A non-empty list is never treated as valid before E6-13 implements
/// its sorted-id, count-threshold, cent, and allocation checks.
fn tripwire_rules(mandate: &Mandate) -> Result<(), SpecError> {
    if mandate.autonomy.tripwires.is_empty() {
        Ok(())
    } else {
        Err(SpecError::Unimplemented)
    }
}

/// The rules §4.1 checks again when a version is applied, against the facts at application: V-002,
/// atomically with the application, and V-047 (DEC-411, DEC-428). `context` is folded at the moment of
/// application, never the one the version was confirmed under, so a second user deactivated, or
/// another agent's allocation applied, between confirmation and application refuses it.
///
/// Returns the violated codes among those two and no others; an empty set lets the version apply. A
/// version that would fail any other rule never reached application, because it was refused at
/// validation, and the rules that read only the document cannot change in between.
pub fn recheck_at_application(
    mandate: &Mandate,
    context: &ValidationContext,
) -> Result<BTreeSet<Violation>, SpecError> {
    let allocation_path = "/capital/allocation_usd";
    let committed = context
        .other_allocations_usd
        .checked_add(usd(&mandate.capital.allocation_usd, allocation_path)?)
        .map_err(|cause| out_of_range(allocation_path, cause))?;
    let mut violations = BTreeSet::new();
    flag(
        &mut violations,
        committed > context.account_equity_usd,
        Violation::V002,
    );
    flag(
        &mut violations,
        lone_under_independent_approval(mandate, context)?,
        Violation::V047,
    );
    Ok(violations)
}

/// V-047's condition: the policy requires independent approval and the workspace has no second
/// active user, unless DEC-444's exemption applies — a version §9.2 classifies as risk-reducing
/// against the agent's current version. A count the caller did not know arrives as 0 or 1
/// (DEC-428 item 2), so it is here.
fn lone_under_independent_approval(
    mandate: &Mandate,
    context: &ValidationContext,
) -> Result<bool, SpecError> {
    if !(context.independent_approval_required && context.workspace_users < 2) {
        return Ok(false);
    }
    Ok(!exempted_reducing_version(mandate, context)?)
}

/// DEC-444 items 1 and 3: the exemption needs the agent's current version — the journal's
/// [`ValidationContext::current_mandate_version`] matched against the previous version's
/// journal-recorded digest, the pair never assembled per request — and §9.2's plain `classify`
/// calling the new version risk-reducing against that document. A digest that matches no current
/// version is the stale-predecessor refusal: at application the draft's validated-against
/// predecessor is no longer the version in force, and the version is refused rather than
/// classified against the newer one. Everything else is refused too: no previous version (a
/// deployment), identity only, a document the schema refuses, no current version, a neutral
/// version, and a version with any risk-increasing path (MC-V72 to MC-V77).
fn exempted_reducing_version(
    mandate: &Mandate,
    context: &ValidationContext,
) -> Result<bool, SpecError> {
    let Some(previous) = context.previous_version.as_ref() else {
        return Ok(false);
    };
    let (Some(previous_document), Some(previous_digest)) =
        (&previous.mandate, previous.mandate_version)
    else {
        return Ok(false);
    };
    let Some(current) = context.current_mandate_version else {
        return Ok(false);
    };
    if previous_digest.digest() != current.digest() {
        return Ok(false);
    }
    Ok(classify(previous_document, mandate)?.class == ChangeClass::RiskReducing)
}

/// The places of the size fraction the order builder multiplies its targets by (§8.3 step 2), which
/// V-040 bounds the scale factors' digits by: the product of any set of them has at most the sum of
/// their places. It is tighter than the 24 the risk state reports the factor at (§5.5, DEC-167).
const BUILDER_SIZE_FRACTION_PLACES: usize = 12;

/// The system fields, which carry no provenance rule (§7).
pub(crate) const SYSTEM_FIELDS: [&str; 2] = ["/mandate_schema_version", "/source_text_ref"];

/// True when `path` is `prefix` or lies under it, the JSON Pointer sense of "this entry is about that
/// field". The empty pointer is the whole document, so it covers everything.
pub(crate) fn covers(prefix: &str, path: &str) -> bool {
    path.strip_prefix(prefix)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

fn out_of_range(path: &str, cause: NumError) -> SpecError {
    SpecError::OutOfRange {
        path: Pointer::new(path),
        cause,
    }
}

fn usd(value: &SchemaDec, path: &str) -> Result<Usd, SpecError> {
    value.to_usd().map_err(|cause| out_of_range(path, cause))
}

/// `amount × fraction`, exact, or [`SpecError::OutOfRange`] naming the fraction's field.
fn times(amount: Usd, fraction: &SchemaDec, path: &str) -> Result<Usd, SpecError> {
    Fraction::parse(fraction.as_str())
        .and_then(|fraction| amount.times_fraction(fraction))
        .map_err(|cause| out_of_range(path, cause))
}

/// §4.2's four figures, exact.
fn worst_case(mandate: &Mandate) -> Result<WorstCase, SpecError> {
    let allocation = usd(&mandate.capital.allocation_usd, "/capital/allocation_usd")?;
    let risk = &mandate.risk;
    let position = usd(&risk.max_position_usd, "/risk/max_position_usd")?.min(times(
        allocation,
        &risk.max_position_fraction,
        "/risk/max_position_fraction",
    )?);
    let protection = &mandate.protection;
    let one_position_at_stop_usd = match (protection.enabled, &protection.stop_distance) {
        (true, Some(stop)) => {
            let at_stop = times(position, stop, "/protection/stop_distance")?;
            Some(match crypto_offset(mandate) {
                Some(offset) => {
                    let path = "/protection/crypto_stop_limit_offset";
                    at_stop
                        .checked_add(times(position, offset, path)?)
                        .map_err(|cause| out_of_range(path, cause))?
                }
                None => at_stop,
            })
        }
        _ => None,
    };
    Ok(WorstCase {
        one_position_at_stop_usd,
        daily_loss_budget_usd: times(allocation, &risk.max_daily_loss, "/risk/max_daily_loss")?,
        flatten_trigger_loss_usd: times(allocation, &risk.max_drawdown, "/risk/max_drawdown")?,
        lifetime_floor_loss_usd: times(
            allocation,
            &mandate.capital.max_loss_from_allocation,
            "/capital/max_loss_from_allocation",
        )?,
    })
}

/// The crypto stop-limit offset a worst case adds: set, and the universe admits crypto (§4.2 W-002).
fn crypto_offset(mandate: &Mandate) -> Option<&SchemaDec> {
    mandate
        .protection
        .crypto_stop_limit_offset
        .as_ref()
        .filter(|_| mandate.universe.asset_classes.contains(&AssetClass::Crypto))
}

fn flag(violations: &mut BTreeSet<Violation>, broken: bool, violation: Violation) {
    if broken {
        violations.insert(violation);
    }
}

/// The rules that read the account, the connection, and the workspace: V-001, V-002, V-005, V-006,
/// V-007, V-024, V-030, V-031, V-032, and V-047.
fn account_rules(
    m: &Mandate,
    ctx: &ValidationContext,
    worst_case: &WorstCase,
    out: &mut BTreeSet<Violation>,
) -> Result<(), SpecError> {
    let allocation_path = "/capital/allocation_usd";
    let committed = ctx
        .other_allocations_usd
        .checked_add(usd(&m.capital.allocation_usd, allocation_path)?)
        .map_err(|cause| out_of_range(allocation_path, cause))?;
    flag(
        out,
        ctx.connection_environment
            .is_some_and(|environment| environment != m.environment),
        Violation::V001,
    );
    flag(out, committed > ctx.account_equity_usd, Violation::V002);
    flag(
        out,
        lone_under_independent_approval(m, ctx)?,
        Violation::V047,
    );
    let universe = &m.universe;
    flag(
        out,
        universe.leveraged_etps_enabled
            && !universe
                .leveraged_etp_disclosure_version
                .is_some_and(|version| ctx.disclosures_accepted.contains(&version)),
        Violation::V005,
    );
    let claimed: BTreeSet<Claim<'_>> = ctx
        .claimed_by_other_agents
        .iter()
        .map(|asset| Claim::of(asset, ctx))
        .collect();
    flag(
        out,
        universe
            .pinned_instruments
            .iter()
            .any(|instrument| claimed.contains(&Claim::of(&instrument.asset_id, ctx))),
        Violation::V006,
    );
    if let Some(registry) = &ctx.registry {
        flag(
            out,
            !m.behavior
                .signal_models
                .iter()
                .all(|model| registered(model, registry)),
            Violation::V007,
        );
    }
    let approval = &m.autonomy.approval;
    flag(
        out,
        ctx.approver_users < 1
            || (approval.two_approver_above_usd.is_some() && ctx.approver_users < 2),
        Violation::V024,
    );
    flag(
        out,
        m.goal
            .end_date()
            .is_some_and(|end| *end < ctx.validation_date),
        Violation::V030,
    );
    flag(
        out,
        ctx.previous_version.as_ref().is_some_and(|previous| {
            previous.environment != m.environment || previous.connection_id != m.connection_id
        }),
        Violation::V031,
    );
    flag(
        out,
        ctx.connection_loss_carry_usd >= worst_case.lifetime_floor_loss_usd,
        Violation::V032,
    );
    Ok(())
}

/// What an instrument-group claim is on (trading spec §7.1): its group, or the instrument itself when
/// it has none. Two variants rather than one string, so a group whose id happens to spell an asset id
/// never claims that instrument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Claim<'a> {
    Group(&'a GroupId),
    Alone(&'a AssetId),
}

impl<'a> Claim<'a> {
    fn of(asset: &'a AssetId, ctx: &'a ValidationContext) -> Self {
        ctx.instrument_groups
            .get(asset)
            .map_or(Self::Alone(asset), Self::Group)
    }
}

/// V-007: the id, version, and content hash are registered together, and the parameter keys, in the
/// document's order, are exactly the declared ones in sorted order.
fn registered(model: &SignalModel, registry: &BTreeMap<ModelId, RegisteredModel>) -> bool {
    registry.get(&model.id).is_some_and(|entry| {
        entry.version == model.version
            && entry.content_hash == model.content_hash
            && model
                .params
                .iter()
                .map(|param| param.key.as_str())
                .eq(entry.params.iter().map(String::as_str))
    })
}

/// True when each item is strictly above the one before it: sorted and unique (V-009).
fn ascending<'a>(items: impl IntoIterator<Item = &'a str>) -> bool {
    let items: Vec<&str> = items.into_iter().collect();
    items.windows(2).all(|pair| match pair {
        [earlier, later] => earlier < later,
        _ => true,
    })
}

/// The strings of the array at `path` in the document as written. The typed sets are `BTreeSet`s,
/// which have already sorted what V-009 must see unsorted.
fn written<'a>(document: &'a Value, path: &str) -> Vec<&'a str> {
    pointer(document, path)
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_str)
        .collect()
}

/// The rules on the document's own shape: V-008 to V-016 and V-033.
fn document_rules(m: &Mandate, document: &Value, out: &mut BTreeSet<Violation>) {
    let protection = &m.protection;
    flag(
        out,
        if protection.enabled {
            m.universe.asset_classes.contains(&AssetClass::Crypto)
                && protection.crypto_stop_limit_offset.is_none()
        } else {
            protection.stop_distance.is_some()
                || protection.take_profit_distance.is_some()
                || protection.crypto_stop_limit_offset.is_some()
        },
        Violation::V008,
    );
    let models = &m.behavior.signal_models;
    let mut rule_ids = BTreeSet::new();
    let sorted = ascending(
        m.universe
            .pinned_instruments
            .iter()
            .map(|instrument| instrument.asset_id.as_str()),
    ) && ascending(models.iter().map(|model| model.id.as_str()))
        && models
            .iter()
            .all(|model| ascending(model.params.iter().map(|param| param.key.as_str())))
        && ascending(m.autonomy.approval.approvers.iter().map(|a| a.as_str()))
        && [
            "/behavior/cadence/event_sources",
            "/notifications/channels",
            "/universe/asset_classes",
        ]
        .iter()
        .all(|path| ascending(written(document, path)))
        && m.autonomy
            .rules
            .iter()
            .all(|rule| rule_ids.insert(&rule.id));
    flag(out, !sorted, Violation::V009);
    let risk = &m.risk;
    let ladder = &risk.drawdown_ladder;
    flag(
        out,
        !ladder.windows(2).all(|pair| match pair {
            [lower, upper] => lower.at < upper.at && lower.action <= upper.action,
            _ => true,
        }) || !ladder
            .iter()
            .all(|rung| (rung.action == LadderAction::ScaleSizes) == rung.factor.is_some()),
        Violation::V010,
    );
    let scale_places: usize = ladder
        .iter()
        .filter(|rung| rung.action == LadderAction::ScaleSizes)
        .filter_map(|rung| rung.factor.as_ref())
        .map(|factor| {
            factor
                .as_str()
                .split_once('.')
                .map_or(0, |(_, fraction)| fraction.len())
        })
        .sum();
    flag(
        out,
        scale_places > BUILDER_SIZE_FRACTION_PLACES,
        Violation::V040,
    );
    let flattens = ladder
        .iter()
        .filter(|rung| rung.action == LadderAction::FlattenAndPause)
        .count();
    flag(
        out,
        flattens != 1
            || !ladder.last().is_some_and(|rung| {
                rung.action == LadderAction::FlattenAndPause && rung.at == risk.max_drawdown
            }),
        Violation::V011,
    );
    flag(
        out,
        ladder
            .first()
            .is_some_and(|rung| risk.hysteresis >= rung.at),
        Violation::V012,
    );
    flag(
        out,
        !(risk.max_order_usd <= risk.max_position_usd
            && risk.max_position_usd <= risk.max_gross_exposure_usd
            && risk.max_gross_exposure_usd <= m.capital.allocation_usd),
        Violation::V013,
    );
    flag(
        out,
        m.capital.max_loss_from_allocation < risk.max_drawdown,
        Violation::V014,
    );
    flag(
        out,
        pointer(document, "/goal/end_date")
            .and_then(Value::as_str)
            .is_some_and(|text| Date::parse(text).is_err()),
        Violation::V015,
    );
    flag(
        out,
        m.notifications
            .quiet_hours
            .as_ref()
            .is_some_and(|quiet| quiet.start == quiet.end),
        Violation::V016,
    );
    flag(
        out,
        risk.scale_action == ScaleAction::TrimToTarget && matches!(m.goal, Goal::Accumulate { .. }),
        Violation::V033,
    );
}

/// The signal models that admit instruments: the research agent, of which V-036 allows one (§8.4).
fn admitting(m: &Mandate) -> impl Iterator<Item = &SignalModel> {
    m.behavior
        .signal_models
        .iter()
        .filter(|model| model.admits_instruments)
}

/// E17-1's field split (§2.3): V-003, V-034 to V-037, and V-039. Pinning the universe and running a
/// research agent are exclusive, and an `accumulate` goal is always pinned to its one instrument.
fn universe_rules(m: &Mandate, out: &mut BTreeSet<Violation>) {
    let universe = &m.universe;
    let pinned = &universe.pinned_instruments;
    let research = &m.behavior.research;
    if let Goal::Accumulate { instrument, .. } = &m.goal {
        flag(
            out,
            !(universe.pinned
                && pinned.iter().map(|entry| &entry.asset_id).eq([instrument])
                && research.is_none()),
            Violation::V003,
        );
    }
    flag(out, universe.pinned == pinned.is_empty(), Violation::V034);
    flag(
        out,
        u32::try_from(pinned.len()).map_or(true, |count| universe.max_instruments < count),
        Violation::V035,
    );
    let admitting: Vec<&SignalModel> = admitting(m).collect();
    flag(
        out,
        match admitting.as_slice() {
            [] => research.is_some(),
            [model] => model.id.model_type() != Some("llm") || research.is_none(),
            _ => true,
        },
        Violation::V036,
    );
    flag(
        out,
        universe.pinned && !admitting.is_empty(),
        Violation::V037,
    );
    flag(
        out,
        pinned
            .iter()
            .any(|entry| !class_allowed(entry.asset_class, &universe.asset_classes)),
        Violation::V039,
    );
}

/// V-017, V-018, and V-023, over every comparison of every rule.
fn condition_rules(m: &Mandate, out: &mut BTreeSet<Violation>) {
    let conditions = m.autonomy.rules.iter().map(|rule| &rule.when).chain(
        m.autonomy
            .delegations
            .iter()
            .map(|delegation| &delegation.when),
    );
    for when in conditions {
        for (comparison, depth) in when.comparisons() {
            flag(out, depth > MAX_CONDITION_DEPTH, Violation::V017);
            if let Condition::Compare { field, op, value } = comparison {
                flag(out, field.is_reserved(), Violation::V018);
                flag(out, !well_typed(*field, *op, value), Violation::V023);
            }
        }
    }
}

/// The longest a delegation may stand: 30 days (§6.5, V-041).
const DELEGATION_MAX_SPAN_S: i64 = 30 * 86_400;

/// V-041, V-042, and V-043 (§6.5, DEC-181, DEC-420).
///
/// V-042 classifies on its own basis: the delegations removed from both versions and the new
/// version's review date in both (DEC-273), so a version increasing only because of what its own
/// delegations lift carries them (DEC-353 item 4). With the previous document withheld, no
/// delegation can be shown new, so every one is refused (DEC-420 item 4).
fn delegation_rules(
    m: &Mandate,
    ctx: &ValidationContext,
    out: &mut BTreeSet<Violation>,
) -> Result<(), SpecError> {
    let autonomy = &m.autonomy;
    if autonomy.delegations.is_empty() {
        return Ok(());
    }
    let ids: BTreeSet<_> = autonomy.delegations.iter().map(|d| &d.id).collect();
    flag(
        out,
        ids.len() != autonomy.delegations.len(),
        Violation::V041,
    );
    let two_approver = autonomy.approval.two_approver_above_usd.as_ref();
    for d in &autonomy.delegations {
        let names_an_ask = match &d.lifts {
            Lifts::Default => autonomy.default == AutonomyDecision::Ask,
            Lifts::Rule(id) => autonomy
                .rules
                .iter()
                .any(|rule| &rule.id == id && rule.then == AutonomyDecision::Ask),
        };
        flag(
            out,
            !names_an_ask || !window_holds(d.starts_at, d.expires_at),
            Violation::V041,
        );
        let inside = d.max_order_usd <= m.risk.max_order_usd
            && d.max_order_usd <= d.max_total_usd
            && d.max_total_usd <= m.capital.allocation_usd
            && two_approver.is_none_or(|two| d.max_order_usd <= *two);
        flag(out, !inside, Violation::V043);
    }
    if let Some(previous) = &ctx.previous_version {
        let carried = match &previous.mandate {
            None => true,
            Some(before) => {
                before
                    .autonomy
                    .delegations
                    .iter()
                    .any(|d| ids.contains(&d.id))
                    && classify(
                        &without_delegations(before, m)?,
                        &without_delegations(m, m)?,
                    )?
                    .class
                        >= ChangeClass::RiskIncreasing
            }
        };
        flag(out, carried, Violation::V042);
    }
    Ok(())
}

/// `[starts, expires)` is a window, and at most 30 days long. An instant that is no instant is no
/// window (DEC-420 item 3).
fn window_holds(starts: Option<UtcNanos>, expires: Option<UtcNanos>) -> bool {
    match (starts, expires) {
        (Some(starts), Some(expires)) => {
            starts < expires
                && expires
                    .secs()
                    .checked_sub(starts.secs())
                    .is_some_and(|span| {
                        (span, expires.nanos()) <= (DELEGATION_MAX_SPAN_S, starts.nanos())
                    })
        }
        _ => false,
    }
}

/// `m` as V-042 classifies it: its document with the delegations removed and `review_of`'s review
/// date, or none, in place of its own, parsed again.
fn without_delegations(m: &Mandate, review_of: &Mandate) -> Result<Mandate, SpecError> {
    let review = review_of
        .canonical()?
        .get("autonomy")
        .and_then(|autonomy| autonomy.get("review_by"))
        .cloned();
    let mut document = m.canonical()?;
    if let Value::Object(top) = &mut document
        && let Some((_, Value::Object(autonomy))) =
            top.iter_mut().find(|(key, _)| key.as_str() == "autonomy")
    {
        let review_key = autonomy
            .keys()
            .find(|key| key.as_str() == "review_by")
            .cloned();
        autonomy.retain(|key, _| key.as_str() != "delegations" && key.as_str() != "review_by");
        if let Some(value) = review {
            let key = match review_key {
                Some(key) => key,
                None => {
                    mandate_canon::Key::new("review_by").map_err(|_| SpecError::InvalidInput {
                        what: "the review date's member name",
                    })?
                }
            };
            autonomy.insert(key, value);
        }
    }
    Ok(Mandate::parse(&document)?)
}

/// V-023 (§6.3). A decimal must also fit the exact type the order path compares it as, so a value the
/// autonomy walk would refuse mid-walk is refused here instead, before any rule is read (DEC-161).
fn well_typed(field: ConditionField, op: Operator, value: &ConditionValue) -> bool {
    match (field.kind(), value) {
        (FieldKind::Bool, ConditionValue::Bool(_)) => matches!(op, Operator::Eq | Operator::Ne),
        (FieldKind::Decimal, ConditionValue::Decimal(decimal)) => {
            !op.takes_list()
                && decimal.to_ratio().is_ok()
                && (!unit_bounded(field)
                    || SchemaDec::parse(decimal.as_str(), DecGrammar::Fraction).is_ok())
        }
        (FieldKind::Enum | FieldKind::Text, ConditionValue::Text(member)) => {
            matches!(op, Operator::Eq | Operator::Ne) && is_member(field, member)
        }
        (FieldKind::Enum | FieldKind::Text, ConditionValue::List(members)) => {
            op.takes_list()
                && !members.is_empty()
                && members.iter().all(|member| is_member(field, member))
        }
        _ => false,
    }
}

/// The fields §6.3 types "decimal in [0, 1]": `combined_score`, `drawdown`, and `thesis_confidence`
/// (DEC-161: [`ConditionField::is_unit_bounded`] omits the third, and V-023 reads §6.3's table).
fn unit_bounded(field: ConditionField) -> bool {
    field.is_unit_bounded() || field == ConditionField::ThesisConfidence
}

/// Whether `member` is a value §6.3 lists for an enum field. An instrument takes any string.
fn is_member(field: ConditionField, member: &str) -> bool {
    match field {
        ConditionField::Purpose => matches!(member, "open" | "increase"),
        ConditionField::Session => {
            matches!(member, "pre_market" | "regular" | "after_hours" | "crypto")
        }
        ConditionField::AssetClass => matches!(member, "us_equity" | "crypto"),
        _ => field.kind() == FieldKind::Text,
    }
}

/// V-020, V-022, and V-038, over the provenance map (§2.1, §7).
///
/// An entry speaks for its own path and everything under it, so an entry at `/autonomy` is about
/// `/autonomy/default` too. V-022 and V-038 therefore read an entry on the path, under it, or above
/// it: a proposal of the whole universe is a proposal of the pinned instruments (DEC-161).
fn provenance_rules(
    m: &Mandate,
    document: &Value,
    ctx: &ValidationContext,
    out: &mut BTreeSet<Violation>,
) {
    let listed = platform_defaultable();
    let entries = ctx.provenance.entries();
    for (path, provenance) in entries {
        let path = path.as_str();
        if SYSTEM_FIELDS.iter().any(|system| covers(system, path)) {
            continue;
        }
        let allowed = if provenance.source == Source::PlatformDefault {
            listed
                .iter()
                .find(|(field, _)| covers(field, path))
                .is_some_and(|(field, value)| {
                    let single_user =
                        *field != "/autonomy/approval/approvers" || ctx.workspace_users <= 1;
                    single_user
                        && value
                            .is_none_or(|value| rendered(pointer(document, path)) == Some(value))
                })
        } else {
            provenance.source.is_owner_sourced() && provenance.confirmed
        };
        flag(out, !allowed, Violation::V020);
        flag(
            out,
            provenance.source == Source::PlatformProposed
                && NEVER_PROPOSED
                    .iter()
                    .any(|never| covers(never, path) || covers(path, never)),
            Violation::V038,
        );
    }
    let autonomy = &m.autonomy;
    let autos = [
        (autonomy.default, "/autonomy/default".to_owned()),
        (autonomy.admission, "/autonomy/admission".to_owned()),
    ]
    .into_iter()
    .chain(
        autonomy
            .rules
            .iter()
            .enumerate()
            .map(|(index, rule)| (rule.then, format!("/autonomy/rules/{index}/then"))),
    )
    .filter(|(decision, _)| *decision == AutonomyDecision::Auto)
    .map(|(_, path)| path)
    .chain((0..autonomy.delegations.len()).map(|index| format!("/autonomy/delegations/{index}")));
    for auto in autos {
        flag(
            out,
            entries.iter().any(|(path, provenance)| {
                (covers(path.as_str(), &auto) || covers(&auto, path.as_str()))
                    && !(provenance.source == Source::UserEntered && provenance.confirmed)
            }),
            Violation::V022,
        );
    }
}

/// A scalar as §7's list writes it. The list fixes no value at `true`, so `true` renders as nothing and
/// matches no listed value, which is the answer a `true` there must get.
fn rendered(value: Option<&Value>) -> Option<&str> {
    match value? {
        Value::Str(text) => Some(text),
        Value::Bool(false) => Some("false"),
        Value::Null => Some("null"),
        _ => None,
    }
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
    /// The platform base (§4.3) is always the outermost level; `policies` are the levels below it, a
    /// profile, the organization, and the workspace. An empty slice therefore still checks the base, so
    /// no caller can skip policy by passing none (#263 round 1, reading 7).
    ///
    /// `Err(Rejected)` carries the whole report and every policy violation rather than one code, so a
    /// caller can show the author everything at once.
    pub fn new(
        mandate: Mandate,
        context: &ValidationContext,
        policies: &[PolicyLevel],
    ) -> Result<Self, Rejected> {
        let report = validate(&mandate, context)?;
        let chain: Vec<PolicyLevel> = [platform_base()?]
            .into_iter()
            .chain(policies.iter().cloned())
            .collect();
        let policy = check(&mandate, &chain)?.violations;
        if report.is_valid() && policy.is_empty() {
            Ok(Self { mandate })
        } else {
            Err(Rejected::Rules {
                report: Box::new(report),
                policy,
            })
        }
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
///
/// `None` exactly when there is no stop: protection is disabled, or its stop is null. The sum is exact
/// (a [`Ratio`](mandate_num::Ratio) holds both operands' places), and a value it cannot hold is
/// [`SpecError::OutOfRange`] naming the field, never a rounded distance (DEC-128 item 4).
///
/// Without the offset the result is the document's own stop, in its `open_fraction` grammar; with it,
/// the sum, in the `positive_decimal` grammar, because two open fractions can sum past one.
pub fn worst_case_stop_distance(mandate: &Mandate) -> Result<Option<SchemaDec>, SpecError> {
    let protection = &mandate.protection;
    let stop = match (protection.enabled, &protection.stop_distance) {
        (true, Some(stop)) => stop,
        _ => return Ok(None),
    };
    let Some(offset) = crypto_offset(mandate) else {
        return Ok(Some(stop.clone()));
    };
    let stop_path = "/protection/stop_distance";
    let offset_path = "/protection/crypto_stop_limit_offset";
    let sum = stop
        .to_ratio()
        .map_err(|cause| out_of_range(stop_path, cause))?
        .checked_add(
            offset
                .to_ratio()
                .map_err(|cause| out_of_range(offset_path, cause))?,
        )
        .map_err(|cause| out_of_range(offset_path, cause))?;
    SchemaDec::parse(&sum.to_string(), DecGrammar::PositiveDecimal)
        .map(Some)
        .map_err(|_| out_of_range(offset_path, NumError::NotCanonical))
}

#[cfg(test)]
pub(crate) mod tests;
