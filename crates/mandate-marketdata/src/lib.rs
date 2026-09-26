//! Alpaca historical market data (backlog E2-1): a client for the market-data host only, vendor
//! numbers read as raw JSON text and converted exactly or rejected, and idempotent Parquet
//! datasets with `Decimal128(38, s)` columns whose scales a manifest records (ADR-0001 ES-23,
//! DEC-89).
//!
//! The adapter layer (ES-02, layer 6): it does I/O and depends on the pure core, never the reverse.

pub mod alpaca;
pub mod client;
pub mod model;
pub mod number;
pub mod timestamp;
