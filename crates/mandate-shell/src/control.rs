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
use mandate_spec::validate::{RegisteredModel, Violation, validate};
use mandate_spec::{Mandate, ValidationContext};
use mandate_time::Date;

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

/// The effective registration of every configuration kind the run uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Configuration {
    pub fee_config: Registered,
    pub trading_calendar: Registered,
    pub rule_set: Registered,
    pub instrument_snapshot: Registered,
    pub model_version: Registered,
}

/// Why the registered configuration cannot be used. Each is a refusal before any credential is
/// read.
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
        }
    }
}

/// Each kind's effective registration in `records`: the latest by `seq`, a snapshot only if its
/// object names the pinned asset id, a model only if it registers the pinned triple (DEC-505 item
/// 1). A fee schedule not yet effective on `trade_date` is refused, never passed over. A candidate
/// snapshot whose object cannot be read refuses: it cannot be shown not to name the pinned asset.
pub fn configuration(
    records: &[ControlRecord],
    store: &dyn ArtifactSource,
    pinned: &Pinned,
    trade_date: Date,
) -> Result<Configuration, ConfigRefusal> {
    let _ = (records, store, pinned, trade_date);
    Err(ConfigRefusal::Unimplemented { story: "E19-11" })
}
