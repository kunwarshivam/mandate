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
//! The workspace services API's contract ([workspace API spec](../../../docs/specs/workspace-api.md)
//! §3 to §5, story E10-10): what the web app, the CLI, and the Owlhead MCP server send and receive,
//! as typed Rust. Pure: no transport, no clock, no database. The server crate that serves it is
//! [DEC-680](../../../docs/project/decisions/DEC-680.md)'s; the readings this crate takes of the spec
//! are [DEC-681](../../../docs/project/decisions/DEC-681.md)'s.
//!
//! - [`envelope`] and [`responses`]: the envelope's shapes, and §5's command responses, closed.
//! - [`requests`]: the request bodies of §4.2, §4.5, §4.6, and §5.1 to §5.4, closed.
//! - [`problem`]: the RFC 9457 problem document of §3.5, with closed `code` and `effect`.
//! - [`wire`]: the scalar members (decimals as canonical strings, never JSON numbers, §3.1) and the
//!   one decoder every request body goes through.
//! - [`idempotency`]: the `Idempotency-Key` grammar and the event id derived from it (§3.4).

pub mod envelope;
pub mod idempotency;
pub mod problem;
pub mod requests;
pub mod responses;
pub mod wire;

/// The story every stub in this crate belongs to.
pub const STORY: &str = "E10-10";

/// What every stub of this tests PR returns until E10-10 implements it (DEC-77).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("E10-10 has not been implemented yet")]
pub struct Unimplemented;
