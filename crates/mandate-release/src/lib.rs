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
//! The release a process runs (E21-8, DEC-970; infrastructure design §7.1, OPS-15).
//!
//! A [`BuildDigest`] is the SHA-256 of an executable's bytes: the digest the release build's
//! `SHA256SUMS` lists for the same binary, so the `actor.build` a process journals names exactly
//! the artifact the reproducible build produced and the release signs. A process takes its own
//! with [`BuildDigest::running`] and names it with [`BuildDigest::reference`], the `sha256:<hex>`
//! content reference the envelope's `actor.build` member admits (journal spec §3).
//!
//! [`startup::check_running`] is the start-up signature check (E21-35, DEC-971, OPS-15): outside
//! dev and CI a process starts only when its executable's cosign signature verifies against a
//! release key compiled in; a wrong signature is refused in every tier.

pub mod digest;
pub mod startup;

pub use digest::{BuildDigest, ReleaseError};
pub use startup::{Admitted, Refused, ReleaseKey, Tier, Trust};
