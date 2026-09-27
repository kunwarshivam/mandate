//! The injected collaborators. Every one is pure, so [`crate::handle`] stays deterministic.
//!
//! The binding gate is deliberately **not** here. A gate a caller can substitute is not
//! independent of agent logic (`AGENTS.md` rule 1; journal spec §2: "the risk gate is a pure
//! library it calls"), so `mandate-risk` is a crate-private dependency and stream I's injectable
//! `GateDryRun` — which can only narrow — is the advisory call, not this one
//! (task brief interpretation 4).

use std::future::Future;

use mandate_accounting::{AssetClass, InstrumentId};
use mandate_num::{Fraction, ShareIncrement};

use crate::types::{
    AgentId, BrokerOutcome, BrokerRequest, BrokerUnknown, EventId, ExecutorConfig, ExitTier,
    MandateVersion, Seq, WriterEpoch,
};

/// Deterministic event identity (ADR-0001 ES-06, ES-21, DEC-131 item 6). The id is derived from
/// `(epoch, head, ordinal)` rather than generated, so a retry after `Unavailable` or `Ambiguous`
/// re-derives it and the append answers `AlreadyCommitted` (journal spec §5.1). The epoch is in
/// the derivation so a fenced writer's retry cannot collide with the new writer's id at the same
/// head.
pub trait IdGen {
    fn event_id(&self, epoch: WriterEpoch, head: Seq, ordinal: u32) -> EventId;
}

/// The narrow read-only view of stream F's mandate that this crate needs.
///
/// `mandate-spec` is layer 3 and will own the full type; until it lands each layer-6 crate
/// declares the view it reads (stream I already does), and the two converge on F's type in the
/// implementation PR rather than one same-layer crate importing the other's.
pub trait MandateView {
    /// The agent's confirmed mandate version, which every gated draft carries as its `man`
    /// configuration reference (journal spec §9).
    fn version(&self, agent: &AgentId) -> Option<MandateVersion>;
    /// The fraction below the stop at which a crypto stop-limit's limit sits
    /// (trading-domain spec §5.4, DEC-36).
    fn crypto_stop_limit_offset(&self, agent: &AgentId) -> Option<Fraction>;
    /// Whether the agent's working universe holds the instrument at all.
    fn covers(&self, agent: &AgentId, instrument: &InstrumentId) -> bool;
}

/// Stream G's effective-dated instrument snapshot (`ins`), read-only.
pub trait InstrumentSnapshot {
    fn asset_class(&self, instrument: &InstrumentId) -> Option<AssetClass>;
    /// Whole or fractional shares. Only the whole-share part of a fractional position can be
    /// protected (trading-domain spec §5.4).
    fn increment(&self, instrument: &InstrumentId) -> Option<ShareIncrement>;
    /// Which of trading-domain spec §5.6's three tiers the instrument prices its exits from.
    fn exit_tier(&self, instrument: &InstrumentId) -> Option<ExitTier>;
}

/// The pure ports a step reads. Each is a function of its arguments, so `handle` stays
/// deterministic and a test injects fixed implementations.
pub struct Ports<'a> {
    pub ids: &'a dyn IdGen,
    pub mandates: &'a dyn MandateView,
    pub instruments: &'a dyn InstrumentSnapshot,
    /// The effective-dated parameters of trading-domain spec §5.4, §5.6, and §5.7. Configuration,
    /// not constants, so changing one is a configuration change with a content hash in
    /// `config_refs` (interpretation 22).
    pub config: &'a ExecutorConfig,
    /// The effective-dated fee configuration (`fee` in `config_refs`, trading-domain spec §6.2,
    /// §6.3), from which the executor computes paper's simulated regulatory fees with
    /// `mandate-accounting`'s own fee rules (§10, DEC-133).
    pub fees: &'a mandate_accounting::Config,
}

/// Why one broker round trip produced no broker fact.
///
/// Only [`Self::Unknown`] is handed to the executor, as `Input::Broker(Err(..))`: it means the
/// request may have reached the broker and nobody knows what it did, so recovery queries on the
/// client order id and never resubmits (task brief interpretation 10). The other two are **not**
/// unknown outcomes, and folding either as one would make the executor keep querying on an answer
/// it could never read, or on a request that never left the process. The shell stops the executor
/// and alerts on them instead (DEC-85: an uninterpreted input fails loudly, never a guess), and
/// neither is ever a rejection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ConnectorError {
    /// The outcome is unknown: a timeout, a dropped connection, or a 5xx after the request left.
    #[error(transparent)]
    Unknown(#[from] BrokerUnknown),
    /// The broker answered and the connector could not read the answer: a body that is not the
    /// wire type, a number that has been through a float, a status outside §5.7's table. `code`
    /// is the connector's own stable reason.
    #[error("the broker's answer could not be read ({code})")]
    Unreadable { code: &'static str },
    /// The request never left the process: its method and path are not an endpoint the connector
    /// allows, or the connector does not implement it yet. `code` is the connector's own reason.
    #[error("the request was not sent ({code})")]
    NotSent { code: &'static str },
}

impl ConnectorError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(self) -> &'static str {
        match self {
            Self::Unknown(_) => "unknown_outcome",
            Self::Unreadable { .. } => "unreadable",
            Self::NotSent { .. } => "not_sent",
        }
    }

    /// The unknown outcome to fold as `Input::Broker(Err(..))`, or `None` for a failure the shell
    /// must stop on rather than hand to the executor.
    pub fn as_unknown(self) -> Option<BrokerUnknown> {
        match self {
            Self::Unknown(unknown) => Some(unknown),
            Self::Unreadable { .. } | Self::NotSent { .. } => None,
        }
    }
}

/// One broker round trip. Implemented by `mandate-alpaca` and by the tests' fake connector.
///
/// The core never calls it: it only describes the request as an [`crate::Effect::Broker`], which
/// is what keeps `handle` pure and every HTTP type out of the core. An `Err` is **never** a
/// rejection: a rejection is an `Ok` answer the broker gave. [`ConnectorError::Unknown`] means
/// the outcome is unknown and recovery must query (task brief interpretation 10); the other two
/// variants are failures of this process that the shell stops on.
pub trait BrokerConnector {
    fn call(
        &mut self,
        request: &BrokerRequest,
    ) -> impl Future<Output = Result<BrokerOutcome, ConnectorError>>;
}
