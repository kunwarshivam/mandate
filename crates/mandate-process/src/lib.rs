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
//! The lifecycle every long-running process shares (E21-28, DEC-970; infrastructure design §3.4,
//! §7.3).
//!
//! A [`Lifecycle`] folds what a process observes, each input at an injected time, into one
//! [`State`]: `starting → replaying → ready ⇄ not_ready → draining → exited`. It reads no clock
//! and draws no randomness, so a replay of the same inputs gives the same states. From the state
//! it answers what the process may do ([`Action`]): an opening only while ready, a risk exit once
//! the process holds its epoch and has replayed under it, and the kill switch whenever it is up,
//! never gated on readiness. It answers liveness against its bound and readiness, and abandons a
//! drain past [`DRAIN_BOUND`] rather than forcing it. [`crash_loop`] applies the crash-loop bound
//! to journaled restarts. [`probe`] turns a published [`Snapshot`] into fixed generic words, and
//! serves them over a Unix socket for `exec` probes (DEC-970 item 5): no listener on any network.

pub mod lifecycle;
pub mod probe;

pub use lifecycle::{
    Action, CRASH_LOOP_RESTARTS, CRASH_LOOP_WINDOW, DRAIN_BOUND, Exit, Input, Kind, Lifecycle,
    Snapshot, State, crash_loop,
};
