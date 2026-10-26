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
//! The workspace news reader (E2-10; DEC-2278, DEC-2280; journal spec §9.37): the single writer of
//! `news:{workspace_id}`, which fetches Alpaca news with its own paper data key and stores each new
//! item before any run can read it.
//!
//! - [`key`] reads the data key once, as the vault's news reader, and refuses `no_key`,
//!   `vault_unavailable`, or the key's own refusal (DEC-2277 item 4, DEC-2280 item 6). The key is
//!   held only as `NewsAuth`, which never prints.
//! - [`tickers`] reads each asset id's ticker from the latest instrument snapshot the control stream
//!   registers (DEC-2280 item 1), and only those rows of it.
//! - [`watch`] folds the instruments to poll: the followed agents' active theses and held
//!   instruments, through the research core's and the gate context's own folds.
//! - [`records`] writes `StreamOpened` and `NewsItemStored` as drafts with deterministic event ids.
//! - [`reader`] runs them under one writer epoch with [`mandate_process`]'s lifecycle: it folds its
//!   stream at start, asks the vendor one page a step, stores each new item's typed record and
//!   appends its record, and stops on a refused append.
//!
//! It never reads a clock: the binary hands it the host clock's reading. It keeps no raw
//! response and no licensed text (DEC-2280 item 4), and nothing it writes is a risk input.

pub mod key;
pub mod reader;
pub mod records;
pub mod tickers;
pub mod watch;

pub use key::{KeyRefusal, read_key, stored_key};
pub use reader::{Config, Counts, Follow, NewsReader};
