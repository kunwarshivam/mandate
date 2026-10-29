#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]
//! The runtime's production ports, shared by every process that hosts the runtime's core
//! (DEC-968 item 6, DEC-2605 row 20): `mandate-shell`'s one-shot cycle and the cell's long-running
//! runtime, `mandate-daemon`, which may not depend on the shell. One copy of the glue between the
//! runtime's ports and the deterministic crates they call, so the two hosts cannot drift.
//!
//! It holds the decision-time contexts; the runtime's view of an admitted mandate
//! ([`runtime_view`]); the order builder's sizing ([`size`]), an owner's request clipped ([`requested`]), and
//! classification ([`classify`]); and
//! the gate's dry run: the runtime's [`mandate_runtime::Proposal`] mapped to `mandate-risk`'s
//! proposed order with the facts the proposal does not carry, and [`mandate_risk::evaluate`] over
//! the trusted context. Every fact comes from its caller; nothing here reads a clock, the journal
//! or the network.

mod context;
mod error;
mod gate;
mod mandate;
mod plan;
mod view;

pub use context::{AdvisoryGateContext, AdvisoryOrderFacts, BuilderContext, DecisionContext};
pub use error::PortError;
pub use gate::{evaluate, evaluate_deferring, proposed_order};
pub use mandate::gate_mandate;
pub use plan::{
    POLICY_OVERLAY, auto_not_allowed, classify, requested, size, thesis_exit,
    validate_order_execution,
};
pub use view::{ViewFacts, runtime_view};
