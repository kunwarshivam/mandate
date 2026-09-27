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
//! The process shell that binds the runtime, the executor, and the Alpaca paper connector
//! ([task brief](../../../docs/project/tasks/E7-7-tracer-bullet.md), backlog E7-7, DEC-138), and the
//! `mandate-tracer` binary that places **one** order on the owner's Alpaca **paper** account.
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
//! # State of this crate (DEC-77 stage 2)
//!
//! This is the tests PR: every production adapter in [`adapters`] is a stub whose answer is
//! [`Cause::Unimplemented`], so the tracer refuses to start at its first probe and never reaches a
//! broker. The effect runner, the mappings, the envelope and the host controls are the harness the
//! fail-closed suite tests, which is why they are real here (DEC-157 item 1).

pub mod adapters;
pub mod cli;
pub mod envelope;
mod error;
pub mod host;
pub mod map;
pub mod stages;
pub mod tracer;

pub use error::{Cause, ShellError};
pub use stages::Stage;
pub use tracer::{Report, Setup, run};
