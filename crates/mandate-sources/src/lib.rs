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
//! The research agent's inputs beyond prices (backlog E17-7, with the news and filings halves of
//! E2-9 and E2-10 that it needs; [data-plane spec §4](../../../docs/specs/data-plane.md#4-news-filings-and-fundamentals);
//! DEC-996).
//!
//! - [`allowlist`]: the vetted-source allowlist, versioned configuration read from
//!   `config/sources/allowlist-v<N>.toml`, and the only way to build a [`allowlist::PermittedUrl`]
//!   (DP-9).
//! - [`ingest`]: Alpaca news pages and SEC EDGAR filings turned into records from structured
//!   fields only, with source, publish time, receive time, knowledge time, and content hash (§4.2).
//! - [`store`]: the news store and the filings store, append-only and point in time (DP-1, DP-2).
//! - [`reader`]: [`reader::SourceReader`], the bounded, typed read the harness research loop
//!   consumes (agent harness spec §6.3, §10.2), and the E17-5 drift observation of each item.
//! - [`fetch`]: the fetchers, which reach a source only through its allowlisted endpoints and a
//!   [`fetch::Transport`]; CI drives them on recorded fixtures, and the HTTPS transport lives in
//!   `mandate-sources-http`.
//!
//! **All text is untrusted** (DP-8). Headlines, summaries, links, and excerpts exist only as
//! [`text::UntrustedText`], which has no `Display`, no `Deref`, and no conversion into any other
//! type, and no typed field of a record is ever derived from text. The crate sits in a layer above
//! the order builder, the risk gate, and the executor (`xtask/layers.toml`), so none of them can
//! reach it.

pub mod allowlist;
pub mod error;
pub mod fetch;
pub mod ingest;
pub mod reader;
pub mod record;
pub mod store;
pub mod symbols;
pub mod text;

pub use error::SourceError;
