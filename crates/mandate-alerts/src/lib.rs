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
//! The safety alerts of the [infrastructure design](../../../docs/design/infrastructure.md) §8.2,
//! raised from journal events and from signals, with opaque payloads (backlog E21-4, OPS-10,
//! DEC-988).
//!
//! **One kind per row.** [`AlertKind`] is closed: every row of §8.2 is a kind, with its reason
//! code, severity, and runbook, and a row naming two conditions of different severity is two
//! kinds. [`from_event`] raises the rows whose source is a journal record, read from the stored
//! record's payload; [`from_signal`] raises the rest from a [`Signal`], which holds only counts,
//! durations, and flags.
//!
//! **De-duplication is the caller's.** One cause can raise several alerts: a workspace kill switch
//! is journaled on every stream it touches, and every opening held for an unattributed leg is its
//! own gate decision. The cell's alert path (E21-39) groups them; nothing here drops one.
//!
//! **Nothing about trading leaves** (`AGENTS.md` rule 6, OPS-10). An [`Alert`] holds an id
//! minted from a [`mandate_notify::SecureRandom`] source and a kind, and no field for its cause,
//! so neither the record that raised it nor its stream, instrument, amount, or mandate can reach
//! [`payload`]. The payload follows `mandate-notify`'s rules: an opaque id and fixed text.
//!
//! **No paging vendor** (DEC-434 item 20, H6 stays the founder's). A [`Sink`] takes the
//! payload's bytes; [`CaptureSink`] keeps them. The cell's alert path is E21-39.
//!
//! The crate is pure: it reads no clock, no network, and no randomness of its own, and it sits
//! above the journal only for the stored record's type, so it cannot append to any stream.

mod event;
mod kind;
mod payload;
mod signal;

pub use event::from_event;
pub use kind::{AlertKind, Severity};
pub use payload::{Alert, CaptureSink, Sink, payload, rendered, send};
pub use signal::{
    APPEND_P99_LIMIT_US, COLD_EXPORT_LAG_LIMIT_S, STALE_FEEDS_AT_SCALE, Signal, from_signal,
};

/// Every way an entry point can refuse. None is ever a reason to act on trading.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AlertError {
    /// The body of every stub before its implementation (DEC-77, DEC-921).
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
    /// A stored record's body did not parse or held no payload object.
    #[error("the stored record is unreadable")]
    Unreadable,
    /// The random source could not fill an alert id.
    #[error("the random source is unavailable")]
    Entropy,
    /// A fixed key or id could not be encoded.
    #[error("the payload could not be encoded")]
    Unrepresentable,
    /// The sink could not take the payload.
    #[error("the sink could not take the alert")]
    Undelivered,
}
