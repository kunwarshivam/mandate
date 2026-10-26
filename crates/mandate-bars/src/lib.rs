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
//! The stored market bars a decision may rest on (DEC-2385): one trust rule for every reader, below
//! both the shell and the runtime process, so neither keeps its own copy.
//!
//! - [`trusted`]: the trust check on a stored daily-bar dataset and the bars and closes it admits.
//! - [`closes`]: the closes a model output rested on, as the artifact its observation names.
//! - [`minutes`]: the complete one-minute bars a trailing-volume figure rests on.

pub mod closes;
pub mod minutes;
pub mod trusted;

pub use closes::{MemberRefused, closes_object};
pub use minutes::trailing_minutes;
pub use trusted::{BarsError, trusted_daily_bars, trusted_daily_closes};
