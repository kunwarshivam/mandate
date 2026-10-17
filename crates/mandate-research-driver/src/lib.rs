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
//! The shell's research driver ([agent harness spec](../../../docs/specs/agent-harness.md) §5.3,
//! §10.3; ADR-0005 §4; DEC-996 item 2; DEC-2204), as a library the shell wires into its run path
//! after R0 closes. It reads no clock and does no I/O: the shell hands it time, the journal, and
//! the worker.
//!
//! - **The trigger queue** ([`TriggerQueue`]): which run's trigger goes to the core next. One run
//!   in flight per agent, owner and explain runs ahead of a scheduled one, no start while paused or
//!   stopped, and any mode change cancelling the run in flight (agent harness spec v0.2 §6.9). The
//!   core decides a run's kind and tool menu; event triggers wait for E19-14.
//! - **The step machine** ([`Driver`]): each input to the core ([`RuntimeCore`] wraps
//!   `mandate_runtime::handle_research`), the step's drafts appended as one batch and folded back,
//!   and only then the worker's requests handed on. An unresolved append holds every other input
//!   until the same one is retried; a refused append halts research in the process.

mod driver;
mod trigger;

pub use driver::{
    AppendFailure, Core, Driver, DriverError, JournalWriter, RuntimeCore, Stepped, Worker,
};
pub use trigger::{Offered, QueueConfig, Trigger, TriggerQueue};
