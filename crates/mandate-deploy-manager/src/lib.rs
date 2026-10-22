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
//! The deployment manager (E21-33, DEC-1170; infrastructure design §3.4, §3.6, §7.3): it keeps each
//! deployed agent's processes running, and leaves a crash-looping one down.
//!
//! The desired processes come only from the workspace control stream ([`control`]): a runtime for
//! each agent with a version-2 `AgentDeployed` and no `AgentStopped`, and an executor for each
//! account with such an agent, the account read from the deployment's connection's own
//! establishment on the stream (journal spec §9.24, DEC-1171). [`Manager`] folds those facts and
//! the manager's own journaled records, and from what the orchestrator reports ([`orchestrator`])
//! decides one step per process at a time: start, stop, or record a crash loop. [`Deployer`]
//! journals each step on the control stream (journal spec §9.21) and calls the orchestrator only
//! once the record is committed. It never stops a process for a kill switch or for readiness,
//! starts a process that is down at once when a kill switch covers it, never runs two instances of
//! one stream's writer, and never writes a mandate, an envelope, a deployment or a command.
//! It holds no broker credential and sends no order.

pub mod control;
pub mod driver;
pub mod fake;
pub mod kubernetes;
pub mod manager;
pub mod orchestrator;
pub mod record;

pub use control::{Cause, Fact, Process, Scope, Slot, StopCause};
pub use driver::{ControlJournal, Deployer, Halt, Identity};
pub use manager::{Decision, Manager, backoff};
pub use orchestrator::{Alerts, Effect, Instance, OperatorAlert, Orchestrator, Phase, Unavailable};
pub use record::Record;
