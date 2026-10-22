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
//! The token-exchange process (connections spec §5.2 step 4, DEC-821 item 2, DEC-2629): the one
//! process that holds the platform's client secret and sends the live token request. No other
//! crate links it (`xtask/layers.toml`), so no other process can send that request.
//!
//! - [`form`] writes the request's body: exactly the six members, each once.
//! - [`response`] reads the answer: only a `200` JSON body, and only its three members.
//! - [`http`] sends the request: one `POST`, HTTPS only to the pinned URL, no redirect followed.
//! - [`startup`] reads the process's settings, refusing a token URL a release build or a tier
//!   other than `dev` or `ci` is given (DEC-2633 item 2).
//! - [`run`] runs the exchange after the start checks: the client secret first, then the take,
//!   the one request and the token write.
//! - `cell` (builds with debug assertions only) admits the token URL of the fixture provider a dev
//!   or CI cell runs (DEC-2625), never an Alpaca host. A release build contains none of it.

pub mod form;
pub mod http;
pub mod response;
pub mod run;
pub mod startup;

#[cfg(debug_assertions)]
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
mod tests;
