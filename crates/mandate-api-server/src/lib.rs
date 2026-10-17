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
//! The workspace API server process ([workspace API spec](../../../docs/specs/workspace-api.md)
//! §3.11, story E21-31, [DEC-978](../../../docs/project/decisions/DEC-978.md)): HTTP over
//! `mandate-api`'s contract, behind the cell's one ingress.
//!
//! - [`config`]: the start checks. The process binds only loopback or its pod's own address, starts
//!   with no secret-named variable in its environment and no password in its database URL, and
//!   reads its session signing key from a credential file only its own user can read.
//! - [`lifecycle`]: readiness and the drain. A local predicate stands in for `mandate-process`
//!   (E21-28) behind [`lifecycle::Lifecycle`] until that crate lands.
//! - [`http`]: the router. `/livez` and `/readyz` read no data; every other path answers the one
//!   401 of API-1 until its route lands; CORS allows the one configured app origin (§3.3 item 5).
//! - [`writes`] and [`commands`]: pause, the kill switch, and an approval's Skip, recorded as the
//!   CLI records them on the workspace's control stream before the `202`, and replayed by key
//!   (DEC-1007).
//!
//! It never places, cancels, or sizes an order, and never calls a broker or a runtime (§1.4):
//! `xtask/layers.toml` keeps it from every crate that could.

pub mod clock;
pub mod commands;
pub mod config;
pub mod http;
pub mod journal;
pub mod keys;
pub mod lifecycle;
pub mod members;
pub mod reads;
pub mod routes;
pub mod session;
pub mod subject;
pub mod token;
pub mod writes;
