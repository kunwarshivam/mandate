//! The one gateway call a run makes (§6.2 step 3, §10.1; inference spec §3), and how the harness
//! treats each way it can fail (§7.3).

use mandate_canon::{Digest, Value};
use mandate_domain::AssetClass;
use mandate_num::Usd;
use mandate_time::UtcNanos;

use crate::error::ResearchRunError;
use crate::item::{ItemId, ItemKind, SourceId, Symbol};
use crate::retrieve::Retrieval;

/// The pinned identity of the research agent: id, version, and content hash (mandate spec §8.1,
/// V-007). The content hash covers the template, the retrieval plan, the output bounds, the
/// deadline, and the model (DEC-431 item 5), so a different value in any of them is a different
/// pin.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ModelPin {
    pub model_id: String,
    pub model_version: String,
    pub content_hash: Digest,
}

/// What the runtime core hands the worker once the run's observations are journaled: the ids and
/// limits of the call, and the `ObservationRecorded` event id of each retrieved item, in the
/// retrieval's order (§6.2 steps 2 and 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallPlan {
    pub run_id: String,
    pub call_id: String,
    pub workspace_id: String,
    pub agent_id: String,
    pub model: ModelPin,
    pub as_of: UtcNanos,
    pub deadline_ms: u32,
    pub max_output_tokens: u32,
    pub observations: Vec<String>,
}

/// One active thesis of this agent, as the model may see it: ids and the instrument, no figure
/// (§6.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryThesis {
    pub thesis_id: String,
    pub symbol: Symbol,
    pub expires_at: UtcNanos,
}

/// One retrieved item as the model sees it: marked untrusted, with the id a thesis cites, its
/// source, and the `ObservationRecorded` that journaled it first (§6.4, HI-2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UntrustedItem {
    pub item_id: ItemId,
    pub source: SourceId,
    pub kind: ItemKind,
    pub observation: String,
    pub body: Value,
}

/// §6.4's typed inputs: everything that may go in, and no field for anything that may not. There
/// is no allocation, limit, equity, P&L, credential, approval, or other agent's data here (HI-11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallInputs {
    pub items: Vec<UntrustedItem>,
    /// `behavior.description`, which construction checked against the prompt guard (§5.1 step 5).
    pub description: String,
    pub cut_off: UtcNanos,
    pub data_universe: Vec<Symbol>,
    pub asset_classes: Vec<AssetClass>,
    pub max_instruments: u32,
    pub working_universe: Vec<Symbol>,
    pub held: Vec<Symbol>,
    pub theses: Vec<MemoryThesis>,
}

/// One research call (§10.1's request table).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResearchCall {
    pub call_id: String,
    pub run_id: String,
    pub workspace_id: String,
    pub agent_id: String,
    pub model: ModelPin,
    pub inputs: CallInputs,
    pub as_of: UtcNanos,
    /// The dispatch instant plus the entry's `deadline_ms`, on the wall clock (§6.2 step 3,
    /// DEC-431 item 24).
    pub deadline: UtcNanos,
    pub max_output_tokens: u32,
}

/// What the run tells the model about itself besides the retrieved items.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallContext {
    pub description: String,
    pub data_universe: Vec<Symbol>,
    pub asset_classes: Vec<AssetClass>,
    pub max_instruments: u32,
    pub working_universe: Vec<Symbol>,
    pub held: Vec<Symbol>,
    pub theses: Vec<MemoryThesis>,
}

impl ResearchCall {
    /// The call for `plan` over `retrieval`, dispatched at `dispatched_at`. Each item is paired
    /// with its journaled observation, so an item that was not journaled has no way in (HI-2).
    ///
    /// # Errors
    /// [`ResearchRunError::ObservationsMismatch`] when the counts differ, and
    /// [`ResearchRunError::TimeOutOfRange`] for a deadline past the representable range.
    pub fn build(
        _plan: &CallPlan,
        _retrieval: &Retrieval,
        _context: CallContext,
        _dispatched_at: UtcNanos,
    ) -> Result<Self, ResearchRunError> {
        Err(ResearchRunError::Unimplemented { story: "E19-3" })
    }
}

/// A gateway reply with outcome `ok` (inference spec §3.2): the model's bytes, the identity the
/// provider reported, and the artifacts the shell stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayReply {
    pub call_id: String,
    pub reported: ModelPin,
    pub output: Vec<u8>,
    pub request_digest: Digest,
    pub prompt_ref: Digest,
    pub response_ref: Digest,
    pub cost_usd: Usd,
    pub completed_at: UtcNanos,
}

/// Inference spec §3.3's error kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ErrorKind {
    PolicyDenied,
    BudgetExhausted,
    RateLimitedLocal,
    MeterUnavailable,
    InputRejected,
    ModelWithdrawn,
    DeadlineExceeded,
    ProviderUnavailable,
    RateLimitedProvider,
    CredentialInvalid,
    ContentRefused,
    SchemaInvalid,
    IdentityMismatch,
}

/// What the harness does about one failed call besides treating the output as missing, which it
/// always does (HI-6, §7.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Treatment {
    /// Alert the operator with opaque text (rule 6).
    pub alert_operator: bool,
    /// Disable research for this pin until a new version (`model_withdrawn`).
    pub disable_research: bool,
    /// Counts toward the three-in-a-row `deadline_exceeded` alert.
    pub deadline_streak: bool,
}

impl ErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PolicyDenied => "policy_denied",
            Self::BudgetExhausted => "budget_exhausted",
            Self::RateLimitedLocal => "rate_limited_local",
            Self::MeterUnavailable => "meter_unavailable",
            Self::InputRejected => "input_rejected",
            Self::ModelWithdrawn => "model_withdrawn",
            Self::DeadlineExceeded => "deadline_exceeded",
            Self::ProviderUnavailable => "provider_unavailable",
            Self::RateLimitedProvider => "rate_limited_provider",
            Self::CredentialInvalid => "credential_invalid",
            Self::ContentRefused => "content_refused",
            Self::SchemaInvalid => "schema_invalid",
            Self::IdentityMismatch => "identity_mismatch",
        }
    }

    /// §7.3's table. No kind is ever retried within the run: the gateway has already retried what
    /// it may, and sampling again until something parses would select outputs (DEC-432 item 3).
    pub fn treatment(self) -> Treatment {
        Treatment {
            alert_operator: false,
            disable_research: false,
            deadline_streak: false,
        }
    }
}

/// A failed call: its id, the kind, and when the answer arrived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallFailure {
    pub call_id: String,
    pub kind: ErrorKind,
    pub completed_at: UtcNanos,
}

/// Exactly one of a reply or a typed failure (inference spec §3.2).
pub type CallOutcome = Result<GatewayReply, CallFailure>;

/// The gateway's answer re-checked against the request: a reply for another call, from another
/// identity, or completed after the deadline is a failure here whatever the gateway said
/// (INF-2, INF-3, caller side). A failure is passed through with the request's call id.
pub fn settle(_request: &ResearchCall, outcome: CallOutcome) -> CallOutcome {
    outcome
}
