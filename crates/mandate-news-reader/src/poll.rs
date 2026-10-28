//! A round of requests (DEC-2278 item 2): the polled tickers, sorted, in chunks of at most the
//! fetcher's [`MAX_NEWS_SYMBOLS`], each chunk followed through the vendor's pages from one start,
//! at most [`MAX_PAGES`] pages a chunk, so an endless continuation token cannot pin a round. A
//! round with nothing to ask, or one a failure ended, is followed by a [`COOLDOWN_S`] pause, so an
//! idle or failing reader does not read every stream each second.

use std::collections::BTreeSet;

use mandate_sources::fetch::MAX_NEWS_SYMBOLS;
use mandate_sources::ingest::PageToken;
use mandate_sources::symbols::Symbol;
use mandate_time::UtcNanos;

/// How far back a round on an empty stream asks: the news read's 48 hours (DEC-1130 item 5,
/// agent harness spec §6.3).
pub const FIRST_LOOKBACK_S: i64 = 48 * 3_600;

/// The most pages one chunk of a round follows.
pub const MAX_PAGES: u32 = 20;

/// How long the reader waits after a round it could not start or that failed.
pub const COOLDOWN_S: i64 = 30;

/// Where a round asks from: the stream's last known time, or [`FIRST_LOOKBACK_S`] before `now` for
/// an empty stream. `None` only past the representable range.
pub fn round_start(last_known: Option<UtcNanos>, now: UtcNanos) -> Option<UtcNanos> {
    match last_known {
        Some(known) => Some(known),
        None => UtcNanos::from_parts(now.secs().checked_sub(FIRST_LOOKBACK_S)?, now.nanos()).ok(),
    }
}

/// When a pause begun at `now` ends. `None` only past the representable range, which pauses
/// nothing.
pub fn cooldown_until(now: UtcNanos) -> Option<UtcNanos> {
    UtcNanos::from_parts(now.secs().checked_add(COOLDOWN_S)?, now.nanos()).ok()
}

/// One round: its chunks of tickers, from one start, and where it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Round {
    chunks: Vec<Vec<Symbol>>,
    chunk: usize,
    start: UtcNanos,
    page: Option<PageToken>,
    pages: u32,
}

/// What a page leaves of its round.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct After {
    /// The round's next request, or `None` when the round is done.
    pub round: Option<Round>,
    /// Whether the chunk stopped at [`MAX_PAGES`] with a page still to follow.
    pub capped: bool,
}

impl Round {
    /// A round over `symbols` from `start`, or `None` when there is nothing to ask.
    pub fn new(symbols: BTreeSet<Symbol>, start: UtcNanos) -> Option<Self> {
        let symbols: Vec<Symbol> = symbols.into_iter().collect();
        let chunks: Vec<Vec<Symbol>> = symbols
            .chunks(MAX_NEWS_SYMBOLS)
            .map(<[Symbol]>::to_vec)
            .collect();
        if chunks.is_empty() {
            return None;
        }
        Some(Self {
            chunks,
            chunk: 0,
            start,
            page: None,
            pages: 0,
        })
    }

    /// The next request: its tickers, its start, and the page to ask for.
    pub fn request(&self) -> (&[Symbol], UtcNanos, Option<&PageToken>) {
        let symbols = self.chunks.get(self.chunk).map_or(&[][..], Vec::as_slice);
        (symbols, self.start, self.page.as_ref())
    }

    /// The round after a page whose continuation token is `next`: the same chunk's next page, or
    /// the next chunk's first, or none.
    pub fn after(mut self, next: Option<PageToken>) -> After {
        self.pages = self.pages.saturating_add(1);
        let capped = next.is_some() && self.pages >= MAX_PAGES;
        match next.filter(|_| !capped) {
            Some(token) => self.page = Some(token),
            None => {
                self.chunk = self.chunk.saturating_add(1);
                self.page = None;
                self.pages = 0;
            }
        }
        let round = (self.chunk < self.chunks.len()).then_some(self);
        After { round, capped }
    }
}
