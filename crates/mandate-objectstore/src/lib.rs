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
//! tokens and verifier releases. This slice is the pure judgement: [`lock`] turns what a store
//! reports about itself into an acceptance or a set of [`lock::Refusal`]s, for the two shapes a
//! provider offers:
//!
//! - **Standard S3 Object Lock** ([`lock::judge_s3`]): the bucket's lock configuration and default
//!   rule, plus a probe object's own retention and what the writer's credential could do to it.
//! - **Cloudflare R2's bucket lock rules** ([`lock::judge_r2`]): prefix rules that a token able to
//!   edit the bucket's configuration can delete, with no compliance mode. The judgement refuses
//!   them for §6.2, which is the written finding of [DEC-973](../../../docs/project/decisions/DEC-973.md).
//!
//! The S3 client, the start-up probe that gathers the evidence, and the writer that only a passed
//! check can build are the next slices.

pub mod lock;
