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
//! The local vault on the demo host (E10-13 V1, DEC-692). It holds the OAuth authorization codes,
//! the access tokens, and the platform's OAuth client secret, encrypted at rest. The full vault of
//! infrastructure §5 replaces it later.
//!
//! **Two processes, two Unix users, two keys** (connections spec §5.2, DEC-822 item 4):
//!
//! - The API process (`owlhead_api`) writes codes into `pending/` and may unlink a token by name.
//!   It holds only the pending key, and it can neither list nor read `tokens/`.
//! - The connection's executor (`owlhead_exec`) reads codes, writes and reads tokens, and reads
//!   the client secret. It holds both keys.
//!
//! Both keys arrive as systemd credentials (`LoadCredential=`) in `$CREDENTIALS_DIRECTORY`, never
//! as environment variables. [`startup::check`] refuses to start on any other arrangement.

pub mod error;
pub mod startup;

pub use error::VaultError;

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
