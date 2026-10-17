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
//! The object store behind a plain S3 endpoint, and the lock check that decides whether a store
//! is strong enough for [journal spec §6.2](../../../docs/specs/journal.md)'s write-once retention
//! (E21-40, [DEC-973](../../../docs/project/decisions/DEC-973.md)).
//!
//! §6.2 asks for object lock **in compliance mode, not governance mode**, with retain-until at
//! creation plus six years, for segments, manifests, artifacts, configuration objects, timestamp
//! tokens and verifier releases. The crate reaches storage only through a plain S3 endpoint
//! named in configuration, so a provider swap is a configuration change:
//!
//! - [`lock`] is the pure judgement: what a store reports about itself becomes an acceptance or a
//!   set of [`lock::Refusal`]s, for standard S3 Object Lock ([`lock::judge_s3`]) and for
//!   Cloudflare R2's bucket lock rules ([`lock::judge_r2`]), which it always refuses: a token that
//!   can edit the bucket's configuration can delete them (DEC-973 item 5, the written finding).
//! - [`sigv4`] signs requests, [`s3`] sends them to one bucket, and [`r2`] reads R2's rules
//!   through Cloudflare's API.
//! - [`check`] runs the start-up probe with the writer's own key, and only a passed check issues
//!   the [`check::LockCertificate`] that a [`check::LockedWriter`] needs, so the cold exporter and
//!   the artifact writer cannot write to a store weaker than §6.2.

pub mod check;
pub mod lock;
pub mod r2;
pub mod s3;
pub mod sigv4;
