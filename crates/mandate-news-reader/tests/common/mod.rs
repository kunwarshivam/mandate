#![allow(
    dead_code,
    unused_imports,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test helpers: each test binary uses a subset, and a broken fixture should stop the test"
)]
//! Shared fixtures for the news reader's tests.

pub mod fixtures;
pub use fixtures::*;
