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
//! The process shell that binds the runtime, the executor, and the Alpaca paper connector, and
//! from DEC-928 the Robinhood connector in [`robinhood`]
//! ([task brief](../../../docs/project/tasks/E7-7-tracer-bullet.md), backlog E7-7, DEC-138,
//! DEC-475 item 6). E7-7's one-shot `mandate-tracer` binary is deleted; `mandate-paper` is the one binary
//! that places an order on the owner's Alpaca **paper** account.
//!
//! # What lives here, and what never does
//!
//! The shell owns the process: the stage adapters, the effect runner that performs what
//! `mandate_runtime::handle` and `mandate_executor::handle` describe, the journal envelope, the
//! deterministic ids, and the host controls. It holds **no trading logic**: no sizing, no gating,
//! no pricing, no state machine, and no arithmetic on money or quantity (DEC-138 item 3). Every
//! decision is a crate's, reached through a [`stages`] trait.
//!
//! # Fail closed, by construction
//!
//! Every stage that can **add** risk maps every non-answer to "no order", and every stage that can
//! **reduce** risk maps every non-answer to "do not start" (`AGENTS.md` rules 3 and 13, TI-2 to
//! TI-4). The mappings in [`map`] are total functions whose only permitting arm reads the answer
//! itself, never an error, and the stage list is the [`Stage`] enum, over which the fail-closed
//! suite is built by an exhaustive match, so a new stage does not compile without its case.
//!
//! # State of this crate (DEC-77 stage 3, sliced by DEC-166)
//!
//! The adapters in [`adapters`] become real one slice at a time. The flatten and protection probes,
//! mandate validation, sizing, classification, advisory gate, journal, sink, executor, connector,
//! and reconciliation are live when their trusted inputs are injected. An absent run or executor
//! context refuses before deciding or reading the broker. The paper adapter supplies both from
//! [`paper`]'s one snapshot of broker GETs and stored data, and refuses before the run when any
//! fact in it is missing, stale, or ambiguous (DEC-466, DEC-470). The effect runner, mappings,
//! envelope, and host controls are the harness the fail-closed suite tests (DEC-157 item 1).

pub mod adapters;
pub use mandate_gate_inputs::{approvers, control};
pub mod envelope;
mod error;
pub mod host;
pub mod lt10;
pub mod map;
pub mod paper;
pub mod robinhood;
pub mod stages;
pub mod symbols;
pub mod tail;
pub mod tracer;
pub mod watch;

pub use error::{Cause, ShellError};
pub use lt10::watch_may_end;
pub use stages::Stage;
pub use tail::tailed;
pub use tracer::{
    AskedCycle, Booked, Mark, MayEnd, ProductionCycle, Report, Setup, Wake, run_observed,
    run_observed_asked_watched, run_observed_watched,
};
pub use watch::{Watch, close_window_bound, poll_interval};

/// Assembles one production cycle over a Robinhood session (DEC-928 item 2, DEC-947 item 4):
/// [`adapters::robinhood_stages`], so the cycle's connector and its startup reconciliation are
/// the bridge's, and the rest of its stages are the paper cycle's own.
pub fn robinhood_production_cycle<T>(
    sources: adapters::Sources<std::rc::Rc<robinhood::RobinhoodBridge<T>>>,
    setup: Setup,
) -> ProductionCycle
where
    T: mandate_robinhood::Tools + 'static,
{
    ProductionCycle::new(adapters::robinhood_stages(sources), setup)
}

/// Assembles one production paper cycle. Callers provide deployment inputs and the paper transport;
/// the returned cycle accepts model outputs but exposes none of its safety-critical stages.
pub fn production_cycle<T>(sources: adapters::Sources<T>, setup: Setup) -> ProductionCycle
where
    T: mandate_alpaca::TradingTransport + Clone + 'static,
{
    ProductionCycle::new(adapters::production(sources), setup)
}
