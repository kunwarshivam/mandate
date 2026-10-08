//! The deployment input from the workspace control stream (E19-11, [DEC-505](../../../docs/project/decisions/DEC-505.md),
//! the first paper trade brief's slice D3): the confirmed mandate version a paper run deploys,
//! folded from the stream's records and the artifact store, never from a mandate or configuration
//! file.
//!
//! [`confirmed_version`] reads the records the run read once (a registration committed during the
//! run applies from the next one), takes the latest `AgentDeployed` for the agent with no
//! `AgentStopped` after it, re-hashes the stored document its version names, and folds the
//! stream's facts with `ValidationContext::from_journal`. The model registry is therefore present
//! and V-007 is checked (X-7). Two facts come from their owners rather than the stream (DEC-505
//! item 3): the account equity V-002 reads is the run's broker read, and the connection's
//! environment is `paper` only when the stream holds no fact about the connection, so a
//! `ConnectionRevoked` still refuses. Every refusal comes before any credential is read.

use std::collections::BTreeSet;

use mandate_canon::{Digest, Value};
use mandate_journal::ArtifactSource;
use mandate_num::Usd;
use mandate_spec::context::{AgentId, Membership};
use mandate_spec::validate::Violation;
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
    /// The account equity the run's GET-only preflight read from the broker (V-002).
    pub account_equity_usd: Usd,
    /// One user and one approver, the founder (DEC-505 item 3).
    pub membership: Membership,
}

/// The version a paper run deploys: its digest, the stored document that re-hashes to it, and the
/// context every V-rule passed in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmedVersion {
    pub version: Digest,
    pub mandate: Mandate,
    pub context: ValidationContext,
}

/// Why no deployment input could be built. Each is a refusal before any credential is read.
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

/// The confirmed version `run.agent` is deployed with, from `records` and the documents in
/// `store`, or why there is none.
pub fn confirmed_version(
    records: &[ControlRecord],
    store: &dyn ArtifactSource,
    run: &RunFacts,
) -> Result<ConfirmedVersion, DeploymentRefusal> {
    let _ = (records, store, run);
    Err(DeploymentRefusal::Unimplemented { story: "E19-11" })
}
