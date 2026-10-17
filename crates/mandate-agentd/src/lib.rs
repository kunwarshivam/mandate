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
//! The long-running agent process, first slice (E21-2, DEC-968; infrastructure design §3.4, §7.3,
//! §11).
//!
//! A [`Supervisor`] hosts one deployment's runtime and executor, reached through the [`Host`]
//! seam, on a schedule. It holds a [`Lease`] on the deployment's streams, the journal's writer
//! epochs (journal spec §5.1), so two processes can never act for the same deployment: every cycle
//! asks the lease first, and the lease hands the cycle its own epoch rather than taking a newer
//! one. On start it runs the host's reconciliation before any cycle; on drain it starts no new
//! cycle, lets the host settle exits and protective orders, and releases the lease. [`health`]
//! answers liveness and readiness on loopback with generic text only (`AGENTS.md` rule 6).
//! [`Instrumented`] counts the host's calls by outcome in `mandate-telemetry`'s metrics, after each
//! call returns, so nothing the host journals or sends depends on telemetry (E21-3, OPS-11).
//!
//! Paper only: a supervisor for any other environment refuses before it takes a lease, and this
//! crate can never reach the one crate that may declare the `live` feature (`xtask/layers.toml`).

pub mod health;
pub mod lease;
pub mod memory;
pub mod supervisor;
pub mod telemetry;

pub use lease::{Epochs, FENCED, Lease};
pub use memory::MemoryStore;
pub use supervisor::{Config, Host, IdleHost, Phase, Status, Stop, Supervisor};
pub use telemetry::Instrumented;
