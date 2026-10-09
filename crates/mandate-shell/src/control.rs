//! The confirmed mandate version a paper run deploys, folded from the control stream's records,
//! read once, and the artifact store, never from a file (E19-11, slice D3, [DEC-505](../../../docs/project/decisions/DEC-505.md)).
//! The latest `AgentDeployed` for the agent with no `AgentStopped` after it names the version;
//! the stored document must re-hash to it; `ValidationContext::from_journal` folds the facts, so
//! the model registry is present and V-007 is checked (X-7). The connection is `paper` only when
//! the stream holds no fact about it (DEC-505 item 3). Two phases, as the brief orders them: every
//! check but V-002 before any credential is read, then V-002 on the equity the GET-only preflight
//! read. Only phase 2 yields a `ValidationContext`, so a caller cannot skip V-002.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::{Digest, Value};
use mandate_domain::Environment;
use mandate_journal::{ArtifactError, ArtifactRef, ArtifactSource, get_artifact};
use mandate_num::Usd;
use mandate_spec::context::{AgentId, ContextArgs, JournaledFact, Membership};
use mandate_spec::document::ModelId;
use mandate_spec::policy::{self, PolicyOverlay, PolicyViolation};
use mandate_spec::validate::{RegisteredModel, Violation, validate};
use mandate_spec::{Mandate, ValidationContext};
use mandate_time::{Date, UtcNanos};

/// One record of the workspace control stream as the run read it: its sequence number, type and
/// payload (journal spec §9.2). The journal has already verified the chain it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlRecord {
    pub seq: u64,
    pub event_type: String,
    pub payload: Value,
}

/// What the run supplies beside the stream (DEC-505 item 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunFacts {
    pub agent: AgentId,
    pub validation_date: Date,
    /// One user and one approver, the founder (DEC-505 item 3).
    pub membership: Membership,
}

/// Phase 1's answer: the version a paper run deploys, the stored document that re-hashes to it,
/// and what the fold found. Every V-rule but V-002 passed. It holds no usable context: only
/// [`Self::with_equity`] gives one. Both phases' fields are private, so nothing outside this module
/// can build, alter or recombine either phase's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmedVersion {
    version: Digest,
    mandate: Mandate,
    context: Folded,
    unpriced: ValidationContext,
}

/// What phase 1's fold found, to show before any credential is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folded {
    /// The V-007 registry, never `None` on this path (X-7).
    pub registry: Option<BTreeMap<ModelId, RegisteredModel>>,
    /// The connection's environment V-001 compared (DEC-505 item 3).
    pub connection_environment: Option<Environment>,
}

/// Phase 2's answer, the deployment input: the confirmed version and the context every V-rule,
/// V-002 on the preflight's equity included, passed in. Only [`ConfirmedVersion::with_equity`]
/// builds one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeploymentInput {
    version: Digest,
    mandate: Mandate,
    context: ValidationContext,
}

impl DeploymentInput {
    /// The deployed version's digest.
    pub fn version(&self) -> Digest {
        self.version
    }

    /// The stored document that re-hashes to [`Self::version`].
    pub fn mandate(&self) -> &Mandate {
        &self.mandate
    }

    /// The context every V-rule passed in, carrying the preflight's equity.
    pub fn context(&self) -> &ValidationContext {
        &self.context
    }
}

/// Why no deployment input could be built. Each but V-002's is a refusal before any credential
/// is read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DeploymentRefusal {
    /// The body of every stub in the tests PR (DEC-77).
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
    #[error("no AgentDeployed names the agent")]
    NotDeployed,
    #[error("the agent was stopped after its latest deployment")]
    Stopped,
    #[error("the deployed version's document is not in the artifact store")]
    DocumentMissing,
    #[error("the stored document does not re-hash to the deployed version")]
    DocumentCorrupt,
    #[error("the artifact store could not be read")]
    StoreUnavailable,
    #[error("the stored document is not a canonical mandate")]
    DocumentUnreadable,
    #[error("the deployed mandate's environment is live")]
    Live,
    #[error("a control-stream record could not be folded")]
    Fold,
    #[error("the confirmed version fails a V-rule")]
    Violations(BTreeSet<Violation>),
}

impl DeploymentRefusal {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unimplemented { .. } => "unimplemented",
            Self::NotDeployed => "not_deployed",
            Self::Stopped => "stopped",
            Self::DocumentMissing => "document_missing",
            Self::DocumentCorrupt => "document_corrupt",
            Self::StoreUnavailable => "store_unavailable",
            Self::DocumentUnreadable => "document_unreadable",
            Self::Live => "live",
            Self::Fold => "fold",
            Self::Violations(_) => "violations",
        }
    }
}

/// Phase 1: the confirmed version `run.agent` is deployed with, from `records` and the documents
/// in `store`, or why there is none. It needs no credential.
pub fn confirmed_version(
    records: &[ControlRecord],
    store: &dyn ArtifactSource,
    run: &RunFacts,
) -> Result<ConfirmedVersion, DeploymentRefusal> {
    let mut ordered: Vec<&ControlRecord> = records.iter().collect();
    ordered.sort_by_key(|record| record.seq);
    let ours = |record: &ControlRecord| {
        record.payload.get("agent_id").and_then(Value::as_str) == Some(run.agent.as_str())
    };
    let mut deployed = None;
    for record in &ordered {
        if ours(record) && record.event_type == "AgentDeployed" {
            let version = record
                .payload
                .get("mandate_version")
                .and_then(Value::as_str)
                .and_then(ArtifactRef::parse)
                .ok_or(DeploymentRefusal::Fold)?;
            deployed = Some(Some(version));
        } else if ours(record) && record.event_type == "AgentStopped" {
            deployed = deployed.map(|_| None);
        }
    }
    let version = match deployed {
        None => return Err(DeploymentRefusal::NotDeployed),
        Some(None) => return Err(DeploymentRefusal::Stopped),
        Some(Some(version)) => version,
    };
    let bytes = get_artifact(store, &version).map_err(|error| match error {
        ArtifactError::Missing => DeploymentRefusal::DocumentMissing,
        ArtifactError::Corrupt => DeploymentRefusal::DocumentCorrupt,
        ArtifactError::Unavailable => DeploymentRefusal::StoreUnavailable,
    })?;
    let value = mandate_canon::parse(&bytes).map_err(|_| DeploymentRefusal::DocumentUnreadable)?;
    let mandate = Mandate::parse(&value).map_err(|_| DeploymentRefusal::DocumentUnreadable)?;
    if mandate.environment == Environment::Live {
        return Err(DeploymentRefusal::Live);
    }
    let documents = |digest: &Digest| {
        get_artifact(store, &ArtifactRef::from_digest(*digest))
            .into_iter()
            .flat_map(|bytes| mandate_canon::parse(&bytes))
            .next()
    };
    let connection = &mandate.connection_id;
    let mut facts = Vec::new();
    let mut named = false;
    for record in &ordered {
        let fact = JournaledFact::from_record(
            &record.event_type,
            &record.payload,
            &documents,
            Some(connection),
        )
        .map_err(|_| DeploymentRefusal::Fold)?;
        if let Some(fact) = fact {
            named |= matches!(
                &fact,
                JournaledFact::ConnectionEstablished { connection_id, .. }
                    | JournaledFact::ConnectionRevoked { connection_id }
                    if connection_id == connection
            );
            facts.push(fact);
        }
    }
    let args = ContextArgs {
        agent: run.agent.clone(),
        connection_id: connection.clone(),
        validation_date: run.validation_date,
        membership: Some(run.membership),
        independent_approval_required: false,
        instrument_groups: BTreeMap::new(),
        eligibility_failures: BTreeSet::new(),
    };
    let mut context = ValidationContext::from_journal(&mandate, args, &facts)
        .map_err(|_| DeploymentRefusal::Fold)?;
    if !named {
        context.connection_environment = Some(Environment::Paper);
    }
    let mut violations = validate(&mandate, &context)
        .map_err(|_| DeploymentRefusal::Fold)?
        .violations;
    violations.remove(&Violation::V002);
    if !violations.is_empty() {
        return Err(DeploymentRefusal::Violations(violations));
    }
    let folded = Folded {
        registry: context.registry.clone(),
        connection_environment: context.connection_environment,
    };
    Ok(ConfirmedVersion {
        version: version.digest(),
        mandate,
        context: folded,
        unpriced: context,
    })
}

impl ConfirmedVersion {
    /// The deployed version's digest.
    pub fn version(&self) -> Digest {
        self.version
    }

    /// The stored document that re-hashes to [`Self::version`].
    pub fn mandate(&self) -> &Mandate {
        &self.mandate
    }

    /// What the fold found.
    pub fn context(&self) -> &Folded {
        &self.context
    }

    /// Phase 2: V-002 on the account equity the run's GET-only preflight read from the broker,
    /// which the returned context carries. The document must still re-hash to the version.
    pub fn with_equity(
        self,
        account_equity_usd: Usd,
    ) -> Result<DeploymentInput, DeploymentRefusal> {
        let rehashed = self
            .mandate
            .version()
            .map_err(|_| DeploymentRefusal::DocumentCorrupt)?;
        if rehashed.digest() != self.version {
            return Err(DeploymentRefusal::DocumentCorrupt);
        }
        let mut context = self.unpriced;
        context.account_equity_usd = account_equity_usd;
        let report = validate(&self.mandate, &context).map_err(|_| DeploymentRefusal::Fold)?;
        if !report.violations.is_empty() {
            return Err(DeploymentRefusal::Violations(report.violations));
        }
        Ok(DeploymentInput {
            version: self.version,
            mandate: self.mandate,
            context,
        })
    }
}

/// What DEC-505 item 1 narrows the registrations by: the pinned instrument and signal model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pinned {
    pub asset_id: String,
    pub symbol: String,
    pub model_id: String,
    pub model_version: String,
    pub content_hash: Digest,
}

/// One effective `ConfigSnapshotRegistered`: its `seq`, content hash, and re-hashed object bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registered {
    pub seq: u64,
    pub content_hash: Digest,
    pub bytes: Vec<u8>,
}

/// The listing exchanges DEC-523 item 3 admits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotExchange {
    Arca,
    Nasdaq,
}

/// The registered instrument snapshot as DEC-523 reads it, less the members with one value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstrumentSnapshot {
    pub instrument_id: String,
    pub symbol: String,
    pub exchange: SnapshotExchange,
    pub etp_classified_at: UtcNanos,
}

/// The effective registration of every configuration kind the run uses, and the snapshot read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Configuration {
    pub fee_config: Registered,
    pub trading_calendar: Registered,
    pub rule_set: Registered,
    pub instrument_snapshot: Registered,
    pub instrument: InstrumentSnapshot,
    pub model_version: Registered,
}

/// Why the registered configuration cannot be used. Each is a refusal before any credential is
/// read; a snapshot refusal names the DEC-523 member, never a value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigRefusal {
    /// The body of every stub in the tests PR (DEC-77).
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
    #[error("no effective registration of kind {kind}")]
    Unregistered { kind: &'static str },
    #[error("the {kind} object is not in the artifact store")]
    ObjectMissing { kind: &'static str },
    #[error("the stored {kind} object does not re-hash to its registration")]
    ObjectCorrupt { kind: &'static str },
    #[error("the artifact store could not be read")]
    StoreUnavailable,
    #[error("the {kind} object or its registration is not as the journal spec shapes it")]
    Malformed { kind: &'static str },
    #[error("the effective fee schedule is not yet effective on the trade date")]
    FeeNotYetEffective,
    #[error("the instrument snapshot lacks member {member}")]
    SnapshotMissingMember { member: &'static str },
    #[error("the instrument snapshot has a member DEC-523 does not list")]
    SnapshotExtraMember,
    #[error("the instrument snapshot's {member} is not a string")]
    SnapshotWrongType { member: &'static str },
    #[error("the instrument snapshot's {member} is outside DEC-523's value set")]
    SnapshotValue { member: &'static str },
    #[error("the model registry does not hold exactly one entry equal to the pinned registration")]
    RegistryMismatch,
}

impl ConfigRefusal {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unimplemented { .. } => "unimplemented",
            Self::Unregistered { .. } => "unregistered",
            Self::ObjectMissing { .. } => "object_missing",
            Self::ObjectCorrupt { .. } => "object_corrupt",
            Self::StoreUnavailable => "store_unavailable",
            Self::Malformed { .. } => "malformed",
            Self::FeeNotYetEffective => "fee_not_yet_effective",
            Self::SnapshotMissingMember { .. } => "snapshot_missing_member",
            Self::SnapshotExtraMember => "snapshot_extra_member",
            Self::SnapshotWrongType { .. } => "snapshot_wrong_type",
            Self::SnapshotValue { .. } => "snapshot_value",
            Self::RegistryMismatch => "registry_mismatch",
        }
    }
}

/// Each kind's effective registration in `records`: the latest by `seq`, a snapshot only if its
/// object names the pinned asset id, a model only if it registers the pinned triple (DEC-505 item
/// 1). A fee schedule not yet effective on `trade_date` is refused, never passed over. A candidate
/// snapshot whose object cannot be read, or names no asset id, refuses: it cannot be shown not to
/// name the pinned asset. The effective snapshot is read exactly as DEC-523 pins it, and a later
/// one that fails is refused, never passed over for an earlier one.
pub fn configuration(
    records: &[ControlRecord],
    store: &dyn ArtifactSource,
    pinned: &Pinned,
    trade_date: Date,
) -> Result<Configuration, ConfigRefusal> {
    let mut ordered: Vec<&ControlRecord> = records.iter().collect();
    ordered.sort_by_key(|record| record.seq);
    let mut latest: BTreeMap<&'static str, Registered> = BTreeMap::new();
    for record in ordered {
        if record.event_type != "ConfigSnapshotRegistered" {
            continue;
        }
        let payload = &record.payload;
        let text = |member: &str| payload.get(member).and_then(Value::as_str);
        let Some(kind) = KINDS.into_iter().find(|kind| text("kind") == Some(*kind)) else {
            continue;
        };
        let reference = text("content_hash")
            .and_then(ArtifactRef::parse)
            .ok_or(ConfigRefusal::Malformed { kind })?;
        if kind == "model_version"
            && (text("model_id") != Some(pinned.model_id.as_str())
                || text("model_version") != Some(pinned.model_version.as_str())
                || reference.digest() != pinned.content_hash)
        {
            continue;
        }
        let bytes = get_artifact(store, &reference).map_err(|error| match error {
            ArtifactError::Missing => ConfigRefusal::ObjectMissing { kind },
            ArtifactError::Corrupt => ConfigRefusal::ObjectCorrupt { kind },
            ArtifactError::Unavailable => ConfigRefusal::StoreUnavailable,
        })?;
        if kind == "instrument_snapshot" {
            let value =
                mandate_canon::parse(&bytes).map_err(|_| ConfigRefusal::Malformed { kind })?;
            let object = value.as_object().ok_or(ConfigRefusal::Malformed { kind })?;
            if snapshot_text(object, "instrument_id")? != pinned.asset_id {
                continue;
            }
        }
        let registered = Registered {
            seq: record.seq,
            content_hash: reference.digest(),
            bytes,
        };
        latest.insert(kind, registered);
    }
    let mut take = |kind: &'static str| {
        latest
            .remove(kind)
            .ok_or(ConfigRefusal::Unregistered { kind })
    };
    let fee_config = take("fee_config")?;
    let trading_calendar = take("trading_calendar")?;
    let rule_set = take("rule_set")?;
    let instrument_snapshot = take("instrument_snapshot")?;
    let model_version = take("model_version")?;
    let instrument = snapshot(&instrument_snapshot.bytes, pinned)?;
    let malformed = ConfigRefusal::Malformed { kind: "fee_config" };
    let fee = mandate_canon::parse(&fee_config.bytes).map_err(|_| malformed.clone())?;
    let effective_from = fee
        .get("effective_from")
        .and_then(Value::as_str)
        .ok_or(malformed.clone())
        .and_then(|text| Date::parse(text).map_err(|_| malformed))?;
    if effective_from > trade_date {
        return Err(ConfigRefusal::FeeNotYetEffective);
    }
    Ok(Configuration {
        fee_config,
        trading_calendar,
        rule_set,
        instrument_snapshot,
        instrument,
        model_version,
    })
}

const SNAPSHOT_MEMBERS: [&str; 8] = [
    "asset_class",
    "etp",
    "etp_classified_at",
    "etp_source",
    "exchange",
    "increment",
    "instrument_id",
    "symbol",
];

/// A DEC-523 member's text, or why there is none.
fn snapshot_text<'a>(
    object: &'a mandate_canon::Object,
    member: &'static str,
) -> Result<&'a str, ConfigRefusal> {
    object
        .get(member)
        .ok_or(ConfigRefusal::SnapshotMissingMember { member })?
        .as_str()
        .ok_or(ConfigRefusal::SnapshotWrongType { member })
}

/// The effective snapshot's `bytes` read exactly as DEC-523 pins them.
fn snapshot(bytes: &[u8], pinned: &Pinned) -> Result<InstrumentSnapshot, ConfigRefusal> {
    let malformed = ConfigRefusal::Malformed {
        kind: "instrument_snapshot",
    };
    let value = mandate_canon::parse(bytes).map_err(|_| malformed.clone())?;
    let object = value.as_object().ok_or(malformed)?;
    for member in SNAPSHOT_MEMBERS {
        snapshot_text(object, member)?;
    }
    if object.len() != SNAPSHOT_MEMBERS.len() {
        return Err(ConfigRefusal::SnapshotExtraMember);
    }
    let text = |member: &'static str| snapshot_text(object, member);
    let pinned_values = [
        ("asset_class", "us_equity"),
        ("etp", "plain"),
        ("etp_source", "nasdaq_trader_symbol_directory"),
        ("increment", "whole"),
        ("instrument_id", pinned.asset_id.as_str()),
        ("symbol", pinned.symbol.as_str()),
    ];
    for (member, expected) in pinned_values {
        if text(member)? != expected {
            return Err(ConfigRefusal::SnapshotValue { member });
        }
    }
    let exchange = text("exchange")?;
    let exchange = if exchange == "arca" {
        SnapshotExchange::Arca
    } else if exchange == "nasdaq" {
        SnapshotExchange::Nasdaq
    } else {
        return Err(ConfigRefusal::SnapshotValue { member: "exchange" });
    };
    let member = "etp_classified_at";
    let etp_classified_at = UtcNanos::parse_rfc3339(text(member)?)
        .map_err(|_| ConfigRefusal::SnapshotValue { member })?;
    Ok(InstrumentSnapshot {
        instrument_id: pinned.asset_id.clone(),
        symbol: pinned.symbol.clone(),
        exchange,
        etp_classified_at,
    })
}

const KINDS: [&str; 5] = [
    "fee_config",
    "trading_calendar",
    "rule_set",
    "instrument_snapshot",
    "model_version",
];

/// What governs a run beside its mandate (DEC-484, DEC-534): the effective `policy_set` and
/// `model_registry` registrations, §4.3's overlay folded from the policy's levels, and the
/// confirmed mandate's violations of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Governance {
    pub policy_set: Registered,
    pub model_registry: Registered,
    pub overlay: PolicyOverlay,
    /// Empty when the mandate conforms. Otherwise the run denies every action that adds risk and
    /// lets exits, protective orders and kill switches through (DEC-534).
    pub violations: Vec<PolicyViolation>,
}

/// The governance of the run that `records` and `store` define for `mandate`. Each kind's
/// effective registration is the latest by `seq` (DEC-505 item 1). The policy set is read
/// through `policy::parse_policy_set`. The registry is shaped as journal spec §9 says, and holds
/// exactly one entry for the pinned model, equal to its effective `model_version` registration:
/// triple, `params` and `admits_instruments` (DEC-484 item 5). A nonconforming mandate is no
/// refusal here: its violations are returned (DEC-534).
pub fn governance(
    records: &[ControlRecord],
    store: &dyn ArtifactSource,
    pinned: &Pinned,
    mandate: &Mandate,
) -> Result<Governance, ConfigRefusal> {
    let registration = latest(records, "model_version", |payload| {
        let text = |member: &str| payload.get(member).and_then(Value::as_str);
        text("model_id") == Some(pinned.model_id.as_str())
            && text("model_version") == Some(pinned.model_version.as_str())
            && text("content_hash")
                .and_then(ArtifactRef::parse)
                .is_some_and(|reference| reference.digest() == pinned.content_hash)
    })
    .ok_or(ConfigRefusal::Unregistered {
        kind: "model_version",
    })?;
    let (policy_set, policy_object) = effective(records, store, "policy_set")?;
    let (model_registry, registry_object) = effective(records, store, "model_registry")?;
    let malformed = ConfigRefusal::Malformed { kind: "policy_set" };
    let levels = policy::parse_policy_set(&policy_object).map_err(|_| malformed.clone())?;
    let checked = policy::check(mandate, &levels).map_err(|_| malformed)?;
    check_registry(&registry_object, &registration.payload, pinned)?;
    Ok(Governance {
        policy_set,
        model_registry,
        overlay: checked.overlay,
        violations: checked.violations,
    })
}

/// The latest record by `seq` of type `ConfigSnapshotRegistered` whose payload names `kind` and
/// satisfies `admits`.
fn latest<'a>(
    records: &'a [ControlRecord],
    kind: &str,
    admits: impl Fn(&Value) -> bool,
) -> Option<&'a ControlRecord> {
    records
        .iter()
        .filter(|record| {
            record.event_type == "ConfigSnapshotRegistered"
                && record.payload.get("kind").and_then(Value::as_str) == Some(kind)
                && admits(&record.payload)
        })
        .max_by_key(|record| record.seq)
}

/// The effective registration of `kind` and its parsed object. Only the latest registration is
/// read, so a later one that cannot be used refuses and an earlier one never stands in (DEC-505
/// item 1).
fn effective(
    records: &[ControlRecord],
    store: &dyn ArtifactSource,
    kind: &'static str,
) -> Result<(Registered, Value), ConfigRefusal> {
    let record = latest(records, kind, |_| true).ok_or(ConfigRefusal::Unregistered { kind })?;
    let malformed = ConfigRefusal::Malformed { kind };
    let reference = record
        .payload
        .get("content_hash")
        .and_then(Value::as_str)
        .and_then(ArtifactRef::parse)
        .ok_or(malformed.clone())?;
    let bytes = get_artifact(store, &reference).map_err(|error| match error {
        ArtifactError::Missing => ConfigRefusal::ObjectMissing { kind },
        ArtifactError::Corrupt => ConfigRefusal::ObjectCorrupt { kind },
        ArtifactError::Unavailable => ConfigRefusal::StoreUnavailable,
    })?;
    let object = mandate_canon::parse(&bytes).map_err(|_| malformed)?;
    let registered = Registered {
        seq: record.seq,
        content_hash: reference.digest(),
        bytes,
    };
    Ok((registered, object))
}

const ENTRY_MEMBERS: [&str; 5] = [
    "admits_instruments",
    "content_hash",
    "model_id",
    "model_version",
    "params",
];

/// Journal spec §9's `model_registry` shape, then DEC-484 item 5: the entry for the pinned model
/// equals the effective `model_version` registration's `payload` in every member.
fn check_registry(
    registry: &Value,
    registration: &Value,
    pinned: &Pinned,
) -> Result<(), ConfigRefusal> {
    let malformed = ConfigRefusal::Malformed {
        kind: "model_registry",
    };
    let object = registry.as_object().ok_or(malformed.clone())?;
    let models = registry.get("models").and_then(Value::as_array);
    let shaped = object.len() == 3
        && registry.get("kind").and_then(Value::as_str) == Some("model_registry")
        && registry
            .get("model_registry_version")
            .and_then(Value::as_int)
            == Some(1);
    let models = models.filter(|_| shaped).ok_or(malformed.clone())?;
    let mut ids: Vec<&str> = Vec::new();
    for entry in models {
        let members = entry.as_object().ok_or(malformed.clone())?;
        let text = |member: &str| entry.get(member).and_then(Value::as_str);
        let params = entry
            .get("params")
            .and_then(Value::as_array)
            .map(|items| items.iter().map(Value::as_str).collect::<Option<Vec<_>>>());
        let sorted = |items: &[&str]| items.iter().zip(items.iter().skip(1)).all(|(a, b)| a < b);
        let id = text("model_id").ok_or(malformed.clone())?;
        let in_shape = members.len() == ENTRY_MEMBERS.len()
            && ENTRY_MEMBERS
                .iter()
                .all(|member| members.contains_key(*member))
            && text("model_version").is_some()
            && text("content_hash").and_then(ArtifactRef::parse).is_some()
            && matches!(entry.get("admits_instruments"), Some(Value::Bool(_)))
            && params.flatten().is_some_and(|items| sorted(&items))
            && ids.last().is_none_or(|last| *last < id);
        if !in_shape {
            return Err(malformed);
        }
        ids.push(id);
    }
    let pinned_entry = models
        .iter()
        .find(|entry| entry.get("model_id").and_then(Value::as_str) == Some(&pinned.model_id));
    let equal = pinned_entry.is_some_and(|entry| {
        ENTRY_MEMBERS
            .iter()
            .all(|member| entry.get(member) == registration.get(member))
    });
    if equal {
        Ok(())
    } else {
        Err(ConfigRefusal::RegistryMismatch)
    }
}
