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
//! The account executor as a long-running process (E21-30, DEC-2003; infrastructure design §3.1,
//! §3.4, §3.6; ADR-0001 ES-06, ES-20; DEC-17).
//!
//! An [`ExecutorProcess`] hosts one broker account's [`mandate_executor`] core under one writer
//! epoch on its account stream. The runtime is another process, so the journal is the only
//! channel between them (design §3.1): each pass reads the control stream for the agents that
//! trade on this account ([`follow::agents_of`]) and each agent's stream for the kill switches the
//! runtime copied there ([`follow::switches`]). A revocation never hides an agent's kill switch: an
//! agent deployed before it stays followed, its openings held back, and a connection's or the
//! workspace's kill-switch command is read from the control stream itself for an agent whose
//! connection is revoked at or after it ([`follow::Following::commands_for`], DEC-2343). In each
//! pass:
//! - an input whose append is in doubt is handed again first, as the same bytes, and nothing it
//!   asked of the broker leaves before it commits (journal spec §5.1, `AGENTS.md` rule 5);
//! - a reconciliation the core asked for is gathered as one snapshot; until it is clean the
//!   process is not ready (design §3.4);
//! - every kill switch not yet applied is applied, ready or not, as the core's own switch of that
//!   agent alone (ES-06, `AGENTS.md` rule 13), then the latest tick is handed.
//!
//! The agents' orders are the next slice (lane X, X1b).
//!
//! The core decides; this crate only reads, journals, and sends. Its broker is the [`Broker`]
//! port. In CI it is a fixture, never a broker host, and this crate names no host (X2). Paper
//! only: [`ExecutorProcess::start`] refuses any other environment before it takes the stream.

pub mod alpaca;
pub mod connect;
pub mod credential;
pub mod establish;
pub mod fixture;
pub mod follow;
pub mod launch;
pub mod lease;
pub mod oauth_check;
pub mod paper;
mod process;
pub mod serve;

pub use process::{
    Alert, Broker, Config, ExecutorProcess, Protections, Unplanned, VERSION, Wiring,
};
