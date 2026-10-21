//! The trusted facts of one decision that neither the mandate document nor the runtime's proposal
//! carries, supplied by the host that assembled them.

use std::collections::BTreeMap;

use mandate_builder::{
    AccountSnapshot as BuilderAccountSnapshot, ActionContext, Market as BuilderMarket,
    RiskContext as BuilderRiskContext,
};
use mandate_canon::Digest;
use mandate_num::{Price, Usd};
use mandate_risk::{
    AccountSnapshot as GateAccountSnapshot, AgentSnapshot as GateAgentSnapshot, ConductState,
    GateConfig, GatePass, InstrumentSnapshot as GateInstrumentSnapshot,
    MarketSnapshot as GateMarketSnapshot, Origin, ProposedKind, RiskSnapshot,
    TimeInForce as GateTimeInForce, ValidatedMandate as GateMandate, WorkingUniverse,
};
use mandate_runtime::OrderExecution;
use mandate_time::UtcNanos;

/// Trusted facts for one decision that neither the mandate document nor runtime proposal contains.
#[derive(Debug, Clone)]
pub struct BuilderContext {
    pub account: BuilderAccountSnapshot,
    pub market: BuilderMarket,
    pub risk: BuilderRiskContext,
    pub action: ActionContext,
    /// Effective model content hashes from the trusted model registry, keyed by `(id, version)`.
    /// Runtime outputs do not carry this §8.1 identity, so absence refuses instead of copying the
    /// mandate's expected hash into an observation that never stated it.
    pub model_content_hashes: BTreeMap<(String, String), Digest>,
    pub execution: OrderExecution,
}

/// Proposed-order fields the runtime proposal does not carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvisoryOrderFacts {
    pub kind: ProposedKind,
    pub tif: GateTimeInForce,
    pub extended_hours: bool,
    pub origin: Origin,
    pub owner_confirmed_bid: Option<Price>,
    pub client_order_id: mandate_risk::ClientOrderId,
    pub fee_reservation: Usd,
}

/// Every advisory-gate fact other than the four order fields copied from the runtime proposal.
/// The account risk view (`risk`) and the agent's mode (`agent.mode`) are its host's to supply:
/// the gate reads them only from here.
#[derive(Debug, Clone)]
pub struct AdvisoryGateContext {
    pub now: UtcNanos,
    pub pass: GatePass,
    pub config: GateConfig,
    pub mandate: GateMandate,
    pub risk: RiskSnapshot,
    pub account: GateAccountSnapshot,
    pub agent: GateAgentSnapshot,
    pub instrument: GateInstrumentSnapshot,
    pub market: GateMarketSnapshot,
    pub conduct: ConductState,
    pub universe: WorkingUniverse,
    pub order: AdvisoryOrderFacts,
}

/// The typed facts used after admission for one deterministic decision.
#[derive(Debug, Clone)]
pub struct DecisionContext {
    pub builder: Option<BuilderContext>,
    pub gate: Option<AdvisoryGateContext>,
}
