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
//! The workspace data service (backlog E2-6, [data-plane spec](../../../docs/specs/data-plane.md)
//! v0.2 §3, DEC-433, DEC-994): one place per workspace that holds normalized market records, each
//! stamped with the vendor's time and its own receive time, in a bounded hot cache, so every
//! runtime and the executor read the same current prices.
//!
//! This slice holds quotes. A [`Quote`] is spec §3.1's record: exact decimals, UTC nanosecond
//! times, the instrument by its `asset_id`. [`HotCache`] keeps, per subscribed instrument, the
//! quote that may mark and the one that last did, and answers the one question the risk gate asks
//! of it: is there a fresh, sane quote for an opening now ([`HotCache::opening_quote`])?
//!
//! Every answer errs toward no quote, which denies openings and never touches an exit (`AGENTS.md`
//! rules 3 and 13, DP-4): a quote that is stale, stamped too far in the future, out of order, a
//! duplicate, crossed or too wide, or received before its subscription or the last connection
//! loss never marks, and no quote is fresh while the host clock is not known to be in tolerance.
//! The service decides only what it delivers; the gate's own checks stay the gate's.

mod cache;
mod quote;

pub use cache::{CacheFull, HotCache, Intake, Refused};
pub use quote::{Feed, Freshness, Quote};
