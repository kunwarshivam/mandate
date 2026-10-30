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
//! The agent's chat ([DEC-2293](../../../docs/project/decisions/DEC-2293.md), agent harness spec
//! §15): the pinned chat entry and its tools ([`entry`]), and the turn loop over the model gateway
//! ([`engine`]).
//!
//! The model only proposes. Every tool is the caller's to serve: the workspace API reads the
//! agent's own records for the read tools, and acts for the action tools only through the routes
//! and records the web app uses, where the order builder, the gate and the autonomy rules decide
//! (`AGENTS.md` rules 1 and 4). Nothing here can reach a journal, a builder, a gate, or a broker:
//! the crate layering forbids it.

pub mod engine;
pub mod entry;

pub use engine::{End, Halt, Host, Model, RunSpec, Toolbox, run};
pub use entry::{ChatEntry, KINDS, menu};
