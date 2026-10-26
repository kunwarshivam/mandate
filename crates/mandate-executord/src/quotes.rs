//! The executor's quote feed (E21-30, DEC-2359): the latest quote of each instrument whose exit
//! waits on a price ([`mandate_executor::ExecutorState::quote_wanted`]), handed to the core as an
//! `Input::Market`, so trading spec §5.6's ladder has a fresh, sane quote to price from.
//!
//! This is the interim DEC-2359 records: a REST read of Alpaca's market-data host, through the
//! connection's own transport and credential (DEC-168 item 2), until the workspace data service
//! pushes quotes over its host-local channel (data-plane spec §3.6, E21-44). The process reads
//! quotes only through [`QuoteFeed`], so that channel replaces [`AlpacaQuotes`] and nothing else.
//! [`AlpacaQuotes`] lives in the credential module, the one module that names Alpaca's clients
//! (DEC-2306 item 1, as DEC-2359 item 5 amends it).
//!
//! Every failure hands nothing (`AGENTS.md` rule 3): a quote that cannot be read, is staler than
//! the profile's bound, or is not a quote at all leaves the core where it was, falling back as
//! §5.6 and DEC-2358 say. So does a quote that reads but is not sane (trading spec §8.2): the core
//! never sees it, and an older sane quote ages out of its freshness bound within one exit step.
//! Quotes are process-local and never journaled; nothing here logs a symbol or a price.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use mandate_accounting::InstrumentId;
use mandate_executor::MarketObservation;
use mandate_num::Fraction;
use mandate_time::UtcNanos;

pub use crate::credential::AlpacaQuotes;

/// Reads one instrument's latest quote as the core observes it.
pub trait QuoteFeed {
    /// The instrument's latest sane, fresh quote at `now`.
    ///
    /// # Errors
    /// A code naming neither the instrument nor a price when there is no such quote to hand; the
    /// process then hands nothing.
    fn observe(
        &self,
        instrument: &InstrumentId,
        now: UtcNanos,
    ) -> Result<MarketObservation, &'static str>;

    /// The feed's name, as a mark records it (`MarkUpdated`'s `feed`, journal spec §9.30).
    fn feed(&self) -> &'static str;
}

/// A feed with no quotes: the fixture broker's, and any wiring that reads none.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoQuotes;

impl QuoteFeed for NoQuotes {
    fn observe(
        &self,
        _instrument: &InstrumentId,
        _now: UtcNanos,
    ) -> Result<MarketObservation, &'static str> {
        Err("no_quotes")
    }

    fn feed(&self) -> &'static str {
        "none"
    }
}

/// The data profile's bounds (trading spec §4.2, §8.2): how old a quote may be, and how wide its
/// spread, as a fraction of the bid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuoteBounds {
    pub max_age: Duration,
    pub max_spread: Fraction,
}

/// When each wanted instrument was last read, so each is read at most once per `every`: the
/// exit ladder's own step (`exit_step_s`), which is how fresh §5.6 needs the quote to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotePoll {
    every: Duration,
    read_at: BTreeMap<InstrumentId, UtcNanos>,
}

impl QuotePoll {
    pub fn new(every: Duration) -> Self {
        Self {
            every,
            read_at: BTreeMap::new(),
        }
    }

    /// The wanted instruments due a read at `now`, which it records as read: one never read, or
    /// last read `every` or more before `now`. An instrument no longer wanted is forgotten, so it
    /// is read at once when it is wanted again.
    ///
    /// # Errors
    /// Never: the result keeps the form its stub had (DEC-137).
    pub fn due(
        &mut self,
        wanted: &BTreeSet<InstrumentId>,
        now: UtcNanos,
    ) -> Result<Vec<InstrumentId>, &'static str> {
        self.read_at
            .retain(|instrument, _| wanted.contains(instrument));
        let every = i64::try_from(self.every.as_secs()).unwrap_or(i64::MAX);
        let due: Vec<InstrumentId> = wanted
            .iter()
            .filter(|instrument| {
                self.read_at
                    .get(*instrument)
                    .is_none_or(|last| now.secs().saturating_sub(last.secs()) >= every)
            })
            .cloned()
            .collect();
        for instrument in &due {
            self.read_at.insert(instrument.clone(), now);
        }
        Ok(due)
    }
}
