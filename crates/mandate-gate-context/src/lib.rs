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
//! The journaled facts an opening's builder and gate read (DEC-2378 S1): the mandate version in
//! force, the account's latest snapshot, and each instrument's latest risk mark, folded from
//! journal rows alone. The cell's runtime and the executor both fold through it, so the two cannot
//! disagree about a mark or a snapshot.
//!
//! **The risk clock is the only time.** [`Journaled::now`] is the latest `risk_clock` (or a
//! `ClockAdvanced`'s `clock`) on the account stream; nothing here reads a wall clock, so a replay
//! of the same rows gives the same facts (mandate spec §5.2).
//!
//! **It fails closed** (DEC-2378 item 3). [`Journaled::facts`] answers [`Held`] unless every fact
//! is journaled, current by the risk clock, and readable, and a row of a type it reads that does
//! not parse holds every later answer too. It is for openings only: an exit, a protective order
//! and a flatten never consult it, so a missing fact never holds risk reduction (`AGENTS.md`
//! rule 13).

use std::collections::BTreeMap;

use mandate_canon::Value;
use mandate_num::{Price, Usd};
use mandate_time::UtcNanos;

/// One journal row, as its store hands it over, in journal order.
#[derive(Debug, Clone, Copy)]
pub struct Row<'a> {
    pub stream: &'a str,
    pub event_type: &'a str,
    pub payload: &'a Value,
}

/// How old, in risk-clock seconds, a fact may grow before it no longer admits an opening.
///
/// Both count every second of the risk clock. For an equity's mark that is tighter than mandate
/// spec §5.2's regular-session seconds, so it can only hold more (DEC-2378 item 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Freshness {
    pub mark_max_age_s: u32,
    pub snapshot_max_age_s: u32,
}

/// The account as its latest `AccountSnapshotRecorded` states it (journal spec §9.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountFacts {
    pub status: String,
    pub crypto_status: String,
    pub trading_blocked: bool,
    pub account_blocked: bool,
    pub trade_suspended_by_user: bool,
    pub multiplier: u64,
    pub equity: Usd,
    pub cash: Usd,
    pub buying_power: Usd,
    pub non_marginable_buying_power: Usd,
    pub at: UtcNanos,
}

/// An instrument's latest sane risk mark from a counted session (journal spec §9.30).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mark {
    pub bid: Price,
    pub at: UtcNanos,
}

/// Every fact an opening needs, each current at `now`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    pub now: UtcNanos,
    pub mandate_version: String,
    pub account: AccountFacts,
    pub mark: Mark,
}

/// Why an opening is held: the first fact, in this order, that is not journaled, current, or
/// readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Held {
    #[error("a journal row the fold reads did not parse")]
    Unreadable,
    #[error("no risk clock on the account stream")]
    NoClock,
    #[error("no mandate version in force for the agent")]
    NoMandate,
    #[error("no account snapshot")]
    NoSnapshot,
    #[error("the account snapshot is stale")]
    StaleSnapshot,
    #[error("no sane mark for the instrument")]
    NoMark,
    #[error("the instrument's latest mark is not sane")]
    InsaneMark,
    #[error("the instrument's mark is stale")]
    StaleMark,
}

/// A row of a type the fold reads that does not parse, naming the event type and the member.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{event_type} is unreadable at {member}")]
pub struct Unreadable {
    pub event_type: String,
    pub member: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Latest {
    Sane(Mark),
    NotSane,
}

/// The fold over one account's journal rows for one agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Journaled {
    account_stream: String,
    agent: String,
    now: Option<UtcNanos>,
    mandate: Option<String>,
    account: Option<AccountFacts>,
    marks: BTreeMap<String, Latest>,
    unreadable: bool,
}

impl Journaled {
    pub fn new(account_stream: &str, agent: &str) -> Self {
        Self {
            account_stream: account_stream.to_owned(),
            agent: agent.to_owned(),
            now: None,
            mandate: None,
            account: None,
            marks: BTreeMap::new(),
            unreadable: false,
        }
    }

    /// Folds one row. A row of a type the fold reads that does not parse is refused and holds
    /// every later [`Journaled::facts`], since it may have changed a fact.
    pub fn fold(&mut self, row: Row<'_>) -> Result<(), Unreadable> {
        let _ = row;
        Ok(())
    }

    /// The latest risk clock on the account stream.
    pub fn now(&self) -> Option<UtcNanos> {
        self.now
    }

    /// The facts an opening in `instrument` reads, or why it is held.
    pub fn facts(&self, instrument: &str, fresh: Freshness) -> Result<Facts, Held> {
        let _ = (instrument, fresh);
        Err(Held::Unreadable)
    }
}
