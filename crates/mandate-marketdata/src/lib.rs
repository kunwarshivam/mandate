//! Alpaca historical market data (backlog E2-1): a client for the market-data host only, vendor
//! numbers read as raw JSON text and converted exactly or rejected, and idempotent Parquet
//! datasets with `Decimal128(38, s)` columns whose scales a manifest records (ADR-0001 ES-23,
//! DEC-89). [`inspect`] reports what a stored dataset covers and whether to trust it (E2-2),
//! with each gap's missing bar slots classified by the feed's [`venue`] hours and prices
//! split-adjusted by the corporate [`actions`] a download records (E2-4).
//!
//! The adapter layer (ES-02, layer 6): it does I/O and depends on the pure core, never the reverse.

pub mod actions;
pub mod alpaca;
pub mod client;
pub mod dataset;
pub mod download;
pub mod http;
pub mod inspect;
pub mod model;
pub mod number;
mod rate;
pub mod timestamp;
pub mod venue;
