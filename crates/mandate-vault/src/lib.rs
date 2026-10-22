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
//! The workspace vault behind one interface (E21-32; infrastructure §5.2, §5.6; DEC-434 item 14;
//! DEC-977). [`source::SecretSource`] is what a process holds: a session with the vault as one
//! [`policy::Identity`]. Two backends implement it and pass one conformance suite
//! (`src/tests/conformance.rs`):
//!
//! - [`local::LocalSource`], over the demo host's local vault (`mandate-vault-local`, DEC-692);
//! - [`openbao::OpenBaoSource`], over OpenBao's HTTP API, over TLS, with one AppRole and one
//!   generated policy per process ([`policy::openbao_policy`]), no wildcard.
//!
//! Every identity can do exactly what [`policy::allows`] says and nothing else: a runtime reads
//! nothing, an executor reads exactly its own connection's token, and a credential is write-only
//! for every other identity. An executor holds its token as a [`lease::Lease`];
//! [`lease::LeaseKeeper`] applies E21-9's renewal rule, so a vault outage stops no running
//! executor before its lease expires, and a revocation stops it at once.
//!
//! No value this crate holds reaches a log, an error, or `Debug` output, and no secret is ever an
//! environment variable: OpenBao's AppRole secret ids come in as values the caller read from a
//! mounted file.

pub mod credentials;
pub mod error;
pub mod fingerprint;
pub mod kube;
pub mod kubernetes;
pub mod lease;
pub mod local;
pub mod mounted;
pub mod openbao;
pub mod personal;
pub mod policy;
pub mod source;

pub use error::SourceError;

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
