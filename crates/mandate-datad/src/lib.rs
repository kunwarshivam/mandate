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
//! The workspace data process in the local cell (data-plane spec §3.7, design §3.1): each listed
//! instrument's ([`series::Instrument`]) daily bars and minute bars, stored as
//! `mandate-marketdata`'s datasets, the one format the shell's
//! trust check admits and the model host's closes and the liquidity figures read (DEC-846 item 2,
//! DEC-470 item 4).
//!
//! - [`series`] lays the cell's recorded series onto the calendar: the last [`series::SESSIONS`]
//!   completed regular sessions, each close a function of its date alone, rising one
//!   [`series::STEP`] a session from an anchor, as the R0 rehearsal's bars rise (DEC-961 item 3),
//!   and stores them with [`mandate_marketdata::dataset::Store::put_day`], with the latest trading
//!   day's minute bars, which only `mandate_bars::trailing_minutes` reads (DEC-2385 item 6).
//! - [`observation`] puts the closes object the runtime's `ObservationRecorded` names, read back
//!   through `mandate_bars`'s trust check (DEC-2385 item 5).
//!
//! - [`iex`] is the hosted source (data-plane spec §3.8, DEC-2283): Alpaca's IEX bars of the
//!   workspace's registered instruments, fetched with the platform's data-only paper key, stored as
//!   immutable generations that `mandate_bars::generations` reads by the reader's clock.
//!
//! The recording holds nothing secret and reaches no host. The IEX source holds the platform data
//! key in memory only, reaches only the data host's bars and corporate-actions endpoints, and
//! reads the control stream without appending to it. Both write only the datasets and
//! content-addressed objects, whose put never replaces one. Neither reads a clock itself: the
//! binary hands it the host clock's reading.

pub mod iex;
pub mod observation;
pub mod series;

pub use series::{Anchor, Instrument, Published, SeriesError};
