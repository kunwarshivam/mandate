//! The data process's IEX generations (data-plane spec §3.8, DEC-2283): which instruments a
//! workspace's generations hold, when each generation is due, and how one is written, against any
//! [`generation::BarsSource`], so the binary's fetch over the platform data key and the tests' fake
//! write alike.
//!
//! - [`instruments`]: the workspace's registered US-equity instruments, at most 50.
//! - [`schedule`]: when a daily or minute generation is due, and a workspace's own offsets.
//! - [`key`]: the platform data key, read once through the vault as the bars reader.
//! - [`pass`]: one pass at the clock, reporting counts and codes only.
//! - [`source`]: the [`generation::BarsSource`] over the batched fetch and its transport.
//! - [`generation`]: writing generations whole or not at all, `CONFIRM`, retention, and the closes
//!   object the runtime's observation names.

pub mod generation;
pub mod instruments;
pub mod key;
pub mod pass;
pub mod schedule;
pub mod source;
