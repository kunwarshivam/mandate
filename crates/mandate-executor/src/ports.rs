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
}

/// One broker round trip. Implemented by `mandate-alpaca` and by the tests' fake connector.
///
/// The core never calls it: it only describes the request as an [`crate::Effect::Broker`], which
/// is what keeps `handle` pure and every HTTP type out of the core. An `Err` is **not** a
/// rejection: it means the outcome is unknown and recovery must query
/// (task brief interpretation 10).
pub trait BrokerConnector {
    fn call(
        &mut self,
        request: &BrokerRequest,
    ) -> impl Future<Output = Result<BrokerOutcome, BrokerUnknown>>;
}
