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
//! A stub token issuer for `dev` and `ci` cells only (story E21-52,
//! [DEC-2100](../../../docs/project/decisions/DEC-2100.md)), so a cell without the managed issuer
//! can open an API session through the server's own, unchanged verification path.
//!
//! - [`start`]: the start checks, in the order they run: a build without debug assertions, then
//!   the deployment tier, then the bind address, each before any key is made.
//! - [`issuer`]: the one P-256 key made at start and held in memory only, its key set, and the
//!   ES256 access tokens it signs.
//! - [`serve`]: the router, which answers only its own `Host` and sends no CORS header.
//! - [`alpaca`]: the fixture Alpaca OAuth provider, its authorization page and token endpoint, served
//!   only when started with its client ([DEC-2625](../../../docs/project/decisions/DEC-2625.md)).
//!
//! [`issuer`], [`serve`], and [`alpaca`] are compiled only with debug assertions: a build without them holds no
//! code that makes a key, signs a token, or serves a key set (DEC-2100 item 2). `xtask/layers.toml`
//! lets no crate depend on this one.

pub mod start;

#[cfg(debug_assertions)]
pub mod alpaca;

#[cfg(debug_assertions)]
pub mod issuer;

#[cfg(debug_assertions)]
pub mod serve;
