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
//! The role provisioner (DEC-2350 items 1 and 3; journal spec v0.77 §9.33; DEC-2630 item 7): the
//! one workspace process that may make a connection's Kubernetes-auth roles.
//!
//! It reads only the workspace control stream ([`plan`]). For each version-2 `ConnectionRequested`
//! with `credential` `oauth` that has no answer, it makes the connection's executor and token
//! exchange roles and journals `ConnectionRolesMade`, or `ConnectionRolesRefused` with an opaque
//! operator alert when the vault refuses or cannot answer in time. For each connection whose roles
//! were made and whose request has since closed without an establishment, or that was revoked, it
//! removes the token exchange's role and journals `ConnectionRolesRemoved`. It never removes an
//! executor's role: no step it plans does, and [`mandate_vault::provisioner::RoleProvisioner`] has
//! no operation that could.
//!
//! [`driver::Provisioner`] performs each step on the vault and then journals it ([`record`]), so a
//! crash between the two leaves a step the next pass, folding the stream again from its start,
//! performs and journals once: making a role that exists, or removing one that is gone, changes
//! nothing, and each record's `event_id` is derived from the step, so a second writer's record of
//! the same step is the same event.

pub mod driver;
pub mod plan;
pub mod record;
