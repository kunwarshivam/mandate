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
//! item before any run can read it. It lands in parts (#2758's carve): its credential, what a
//! round asks the vendor, and what it follows.
//!
//! - [`key`] reads the data key once, as the vault's news reader, and refuses `no_key`,
//!   `vault_unavailable`, or the key's own refusal (`not_paper` for a key id that is not a paper
//!   `PK` key, `news_key_malformed`; DEC-2277 item 4, DEC-2280 item 6). The key is held only as
//!   `NewsAuth`, which never prints.
//! - [`tickers`] reads each asset id's ticker from the latest instrument snapshot the control stream
//!   registers (DEC-2280 item 1), and only those rows of it.
//! - [`follows`] names the agents the reader follows, each checked at start (DEC-2281 item 1).
//! - [`watch`] folds the instruments to poll: the followed agents' active theses and held
//!   instruments, through the research core's and the gate context's own folds.
//! - [`poll`] runs a round: the tickers in chunks the fetcher takes, a page cap per chunk, and a
//!   pause after an idle or failed round.
//!
//! It never reads a clock, keeps no raw response and no licensed text (DEC-2280 item 4), and
//! nothing it writes is a risk input.

pub mod follows;
pub mod key;
pub mod poll;
pub mod tickers;
pub mod watch;

pub use follows::{Follow, parse_follows};
pub use key::{KeyRefusal, read_key, stored_key};
