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
//! The model-gateway service's process (E21-62, DEC-2257, DEC-2265): `mandate-gateway`'s `Gateway`
//! over the HTTPS transport, the journaled meter and the system clock, serving its one workspace's
//! runtime one framed call at a time. It is the only live caller of a model provider.
//!
//! [`key`] holds the provider's key, read once from the vault at start, and [`start`] refuses a
//! live start whose meters cannot be read. [`serve`] is the serve loop: one thread per connection
//! under socket timeouts and a cap, one thread holding the gateway, and a log of codes alone.
//! [`cap`] reads each agent's daily research cap from the deployed mandate (DEC-2272), and
//! [`service`] holds each research call to it before the gateway reserves anything.

pub mod cap;
pub mod key;
pub mod serve;
pub mod service;
pub mod start;
