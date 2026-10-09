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
//! **Which process may call what** (connections spec §5.2, DEC-690 item 1, DEC-691, DEC-821
//! items 2 and 4):
//!
//! - [`start`] belongs to the **API process**. It creates the single-use `state` and the
//!   authorization URL, and redeems the `state` at the callback. It names no token type, and
//!   the API process reaches no broker.
//! - [`exchange`] belongs to the **token-exchange process** only (DEC-821 item 2), which is not
//!   the executor: it holds no order client and no token lease. It exchanges the code through a
//!   [`exchange::TokenEndpoint`], checks the grant with [`grant::check_scope`] before the vault
//!   write, and stores the token in the [`vault::Vault`]. The [`exchange::ExchangedGrant`] it
//!   returns carries no secret and is what the permission checks (E7-12) take.
//! - [`grant::check_scope`] checks the granted scopes in the token-exchange process, on the token
//!   response and before the vault write.
//! - [`record`] is the connection record and the account-fingerprint uniqueness check, kept by the
//!   **connection manager, in the API process** (E7-11).
//! - [`checks`] runs the permission checks (E7-12), in the **connection's executor process**.
//! - [`hosts`] is the guard every outbound Alpaca request passes. The only request to the live
//!   host that can exist is [`hosts::LiveTokenRequest`], `POST /oauth/token` (DEC-821 item 2),
//!   and only the token-exchange process sends it. Every other request is a
//!   [`hosts::PaperRequest`], which can address only the paper host; the executor sends only
//!   those, and its egress contains no live host (DEC-821 item 4).
//! - [`revoke`] plans the platform-side revoke, ordinary or on a compromised credential, for the
//!   **API process**, which maps each planned effect to its journal event (DEC-694).
//! - [`manager`] plans the connection manager's start of a connect, in the **API process**: the
//!   pending record `ConnectionRequested` and the connect's step-up digest (DEC-694, DEC-699).

pub mod checks;
pub mod error;
pub mod exchange;
pub mod grant;
pub mod hosts;
pub mod manager;
pub mod record;
pub mod revoke;
pub mod start;
pub mod vault;

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
