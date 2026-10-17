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
//! The global control plane's first slice ([control-plane design](../../../docs/design/control-plane.md)
//! §3.1, §3.2, §3.4; backlog E21-43 with E20-1, E20-2, E20-5; [DEC-984]): the directory, the
//! enrollment of a deployment, the routing of a client to its deployments, and fleet health from
//! heartbeats. Licensing, metering, the relay, distribution and the anchor witness are absent, not
//! stubbed.
//!
//! [`ControlPlane`] is the whole behaviour, over a [`Store`] it declares, with the clock and
//! randomness handed in, so it reads neither. It holds IDs, role names, enumerations, digests,
//! versions and timestamps only: no email, name, or identity-provider subject (E20-1, CP-1). It
//! has no message by which it could act on a deployment: a deployment calls it, never the
//! reverse (CP-7), and nothing it answers is an instruction (CP-4).
//!
//! [DEC-984]: ../../../docs/project/decisions/DEC-984.md

mod directory;
mod fleet;
mod request;

pub use directory::{
    ControlPlane, Deployment, DeploymentMode, Endpoint, Enrolled, Ident, Member, MemberState,
    MemoryStore, Refused, Route, Store, Workspace,
};
pub use fleet::{Component, FleetRow, Health, Heartbeat, Reachability, UNREACHABLE_AFTER_SECS};
pub use request::{MAX_SKEW_SECS, Signed, signed_request_bytes};
