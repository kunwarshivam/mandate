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
//! Broker connections (connections spec §3, §5.2, §8.1). This crate is pure: it holds the
//! connect flow's rules as types and functions, and every network call sits behind a trait an
//! adapter implements.
//!
//! **Which process may call what** (connections spec §5.2, DEC-690 item 1, DEC-691):
//!
//! - [`grant`] checks the granted scopes, in the connection's executor process.
//! - [`record`] is the connection record and the account-fingerprint uniqueness check, kept by the
//!   **connection manager, in the API process** (E7-11).
//! - [`hosts`] is the guard every outbound Alpaca request passes. The only request to the live
//!   host that can exist is [`hosts::LiveTokenRequest`], `POST /oauth/token` (DEC-821 item 2).
//!   Every other request is a [`hosts::PaperRequest`], which can address only the paper host.
//!
//! The `state` and authorization URL (API process), and the code exchange and vault (executor
//! process), follow in the next D2 slices. The permission checks (E7-12) are added beside these.

pub mod error;
pub mod grant;
pub mod hosts;
pub mod record;

pub use error::ConnectError;

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "ADR-0001 ES-09 excepts tests from the safety-critical denies"
)]
mod tests;
