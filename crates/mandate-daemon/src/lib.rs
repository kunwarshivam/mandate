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
//! The agent runtime as a long-running process (E21-30 slice 1, DEC-1012; infrastructure design
//! §3.1, §3.4, §3.6; ADR-0001 ES-06, ES-20; DEC-17).
//!
//! A [`RuntimeProcess`] hosts one agent deployment's [`mandate_runtime`] core under one writer
//! epoch on its agent stream. Each pass of its loop reads the streams it follows from the journal,
//! the only channel ([`wake`] is a hint that only shortens a wait):
//! - first the workspace control stream, where the owner's kill switch is journaled before anything
//!   acts on it (design §3.6), handed whole each time it grows (DEC-914 item 2);
//! - then the scheduler's clock stream, whose latest tick is the runtime's risk clock.
//!
//! A clock stream that cannot be read holds no control row. The process's drafts are journaled
//! before anything else happens, and a batch whose append is in doubt is handed again as the same
//! input before any other, which the core requires (DEC-131 item 6): the runtime is not ready only
//! before its `Input::Started` commits and while such a batch is in doubt, and in both the core
//! takes nothing else, so no readiness gate of the process's own is needed here. The executor's
//! process, whose readiness also waits for reconciliation, is where a not-ready process hands the
//! kill switch and risk exits and nothing else (slice 2).
//!
//! This crate can never reach a broker: `xtask/layers.toml` keeps it from the shell, the executor,
//! the connectors, and the runner (`AGENTS.md` rule 12). Hosting a deployment's production ports
//! (the gate, the order plan, the flatten planner, and the configuration references) waits for the
//! post-R0 hook (DEC-968 item 6, DEC-1012 item 6); until then the binary runs only fixture wiring.

#[cfg(test)]
extern crate self as mandate_daemon;

pub mod bars;
pub mod cell;
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "ADR-0001 ES-09 excepts tests from the safety-critical denies"
)]
#[path = "../tests/cell_research/mod.rs"]
mod cell_research;
pub mod deployment;
pub mod fixture;
pub mod launch;
pub mod meter;
pub mod models;
pub mod production;
pub mod recorded;
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "ADR-0001 ES-09 excepts tests from the safety-critical denies"
)]
#[path = "../tests/research_rig/mod.rs"]
mod research_rig;
pub mod runtime;
pub mod source;
pub mod wake;
pub mod worker;

pub use runtime::{
    Alert, ArtifactSink, Config, FLATTEN_UNPLANNED, ModelsWiring, Refs, ResearchParts,
    ResearchSource, ResearchWiring, ResearchWorker, RuntimeProcess, Wiring,
};
pub use wake::Wake;
