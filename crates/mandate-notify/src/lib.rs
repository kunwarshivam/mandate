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
//! The closed notice: everything a push channel may carry out of the workspace deployment
//! ([notifications spec](../../../docs/specs/notifications.md) §3, §4.2, §4.3, backlog E8-9, DEC-438
//! items 1, 2, 10, DEC-710).
//!
//! **Nothing about trading can reach a provider** (`AGENTS.md` rule 6, NT-1). A [`Notification`]
//! holds a [`NoticeId`] and a [`TextKey`] and nothing else. A notice id is minted only from a
//! [`SecureRandom`] source and has no constructor from an event id, so no journal event id, and
//! with it no event's creation time, can become one (NT-4):
//!
//! ```compile_fail,E0308
//! let _ = mandate_notify::NoticeId::from("01J8ZNB0M000000000000000K1");
//! ```
//!
//! ```compile_fail,E0423
//! let _ = mandate_notify::NoticeId([0u8; 16]);
//! ```
//!
//! The one way back from text is [`NoticeId::parse`], to resolve a link: it takes exactly 32
//! lowercase hex digits, which no ULID is (DEC-702 item 2).
//!
//! **It reaches no stream.** The crate sits at layer 1 over `mandate-canon` and `mandate-time`, so
//! it cannot reach the journal or the control-stream writer (NT-3 at rung 1). It is pure: the
//! random source is passed in, and the dispatcher (E8-10) supplies the operating system's.
//!
//! **A provider sees no recipient id.** A send is addressed by an [`AddressHandle`], an opaque
//! [`Recipient`] and a [`PushChannel`], and keyed for the provider by an [`IdempotencyKey`], a
//! digest of the notice, recipient, and channel (NT-2, DEC-710 items 4, 5), through a [`Provider`]
//! that answers an [`Outcome`] (§5.2, DEC-712). [`canary`] is NT-1's scan (DEC-710 item 7).
//!
//! [`quiet_hours`], [`coalesce`], [`next_attempt`], and [`notice_keys`] are the dispatcher's pure
//! rules: quiet hours by class, the coalescing fold, the retry schedule, and one notice per cause
//! (E8-10 slice D1, DEC-701 item 1, DEC-725). Each takes its instants, and the zone's offsets,
//! from the caller.
//!
//! Every entry point returns a `Result`, so a later slice's stub reports
//! [`NotifyError::Unimplemented`] (DEC-77).

pub mod canary;
mod delivery;
mod kind;
mod payload;
mod provider;
mod quiet;
mod send;

pub use delivery::{Alert, Message, Pass, Read, Retry, coalesce, next_attempt, notice_keys};
pub use kind::{Class, NoticeKind, TextKey};
pub use payload::{NoticeId, Notification, Origin, SecureRandom, link, payload};
pub use provider::{FixtureProvider, Outcome, Provider, Reason, Sent};
pub use quiet::{Offset, QuietHours, QuietVerdict, quiet_hours};
pub use send::{AddressHandle, IdempotencyKey, PushChannel, Recipient, rendered};

/// Every way an entry point can refuse to answer. None of them is ever a reason to send more.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NotifyError {
    /// The body of every stub in the tests PR (DEC-77).
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
    /// The random source could not fill an id. No notice id is minted, and nothing falls back to a
    /// predictable one.
    #[error("the random source is unavailable")]
    EntropyUnavailable,
    /// Text that is not exactly 32 lowercase hex digits was offered as a notice id.
    #[error("a notice id is exactly 32 lowercase hex digits")]
    NotANoticeId,
    /// The configured origin is not `https://<lowercase host>[:<port>]` (DEC-710 item 3).
    #[error("the workspace app origin must be https://<lowercase host>[:<port>] and nothing else")]
    InvalidOrigin,
    /// Text that is not an opaque recipient id (1 to 64 characters of `[a-z0-9_]`, starting with a
    /// letter) was offered as one (DEC-710 item 4).
    #[error(
        "a recipient is an opaque id of 1 to 64 characters of [a-z0-9_], starting with a letter"
    )]
    NotARecipient,
    /// A fixed key could not be represented canonically.
    #[error("the {what} cannot be represented")]
    Unrepresentable { what: &'static str },
}
