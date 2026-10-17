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
//! The workspace scheduler (E21-29, DEC-1010; infrastructure design §3.1, §3.4; journal spec §2,
//! §5.4, §9.18): the single writer of `clock:{workspace_id}`.
//!
//! - [`tick`] picks each tick, the later of the previous one plus a second and the host clock's
//!   second, so the risk clock never stops, repeats, or runs backwards, and finds the first tick
//!   of each trading date.
//! - [`measure`] judges a measurement of the host clock against its time servers: the median by
//!   magnitude of the answering servers' offsets, out of tolerance when none answered, and when
//!   one is due.
//! - [`records`] writes the five §9.18 records as drafts with deterministic event ids.
//! - [`store`] is the journal port, over the in-memory journal and Postgres.
//! - [`scheduler`] runs them under one writer epoch, with [`mandate_process`]'s lifecycle: a
//!   `Fenced` append exits before anything more is written.
//! - [`sntp`] asks a time server over UDP, the one network call, with the host clock injected.
//!
//! The scheduler holds nothing secret. It never reads a clock itself: the binary hands it the host
//! clock's reading and its monotonic seconds.

pub mod measure;
pub mod records;
pub mod scheduler;
pub mod sntp;
pub mod store;
pub mod tick;

pub use measure::{Measurement, Sample, Trigger};
pub use scheduler::{Config, Scheduler, TimeServers};
pub use store::ClockStore;
