//! What the readers return: data only, each record carrying the two instants of data-plane spec
//! §5.2 and nothing a reader could act on.

use std::collections::BTreeSet;

use mandate_canon::Digest;
use mandate_domain::{AssetClass, AssetId};
use mandate_num::{Price, Qty};
use mandate_time::UtcNanos;

use crate::error::ResearchRunError;

/// The longest id this crate accepts, in bytes. Ids reach the journal and the prompt, so an
/// unbounded one would let a reader or a model grow either without limit.
pub(crate) const MAX_ID_BYTES: usize = 128;

fn checked_id(text: &str, what: &'static str) -> Result<String, ResearchRunError> {
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ':' | '/');
    if text.is_empty() || text.len() > MAX_ID_BYTES || !text.chars().all(allowed) {
        return Err(ResearchRunError::InvalidId { what });
    }
    Ok(text.to_owned())
}

/// A retrieved item's id, unique within its source: what a thesis cites as evidence (§6.5).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemId(String);

impl ItemId {
    /// # Errors
    /// [`ResearchRunError::InvalidId`] for an empty, overlong, or non-ASCII-id id.
    pub fn new(id: &str) -> Result<Self, ResearchRunError> {
        checked_id(id, "item id").map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An allowlisted source's id (DEC-101), or one of the platform's own feeds.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceId(String);

impl SourceId {
    /// The platform's daily-bar feed: market data, which needs no allowlist entry.
    pub const BARS: &'static str = "platform.bars";
    /// The instrument snapshot: reference data, which needs no allowlist entry.
    pub const INSTRUMENTS: &'static str = "platform.instruments";
    /// The platform's quote feed: market data, which needs no allowlist entry (DEC-2255 item 6).
    pub const QUOTES: &'static str = "platform.quotes";

    /// # Errors
    /// [`ResearchRunError::InvalidId`] for an empty, overlong, or non-ASCII-id id.
    pub fn new(id: &str) -> Result<Self, ResearchRunError> {
        checked_id(id, "source id").map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A ticker as the instrument snapshot spells it: 1 to 16 of `A`–`Z`, `0`–`9`, `.`, `/`, `-`.
/// The model names instruments only by symbol, so this is the one grammar the resolution step
/// (`unknown_instrument`, §6.5) is run against.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Symbol(String);

impl Symbol {
    /// # Errors
    /// [`ResearchRunError::InvalidId`] outside the grammar.
    pub fn new(symbol: &str) -> Result<Self, ResearchRunError> {
        let allowed = |c: char| c.is_ascii_uppercase() || c.is_ascii_digit() || "./-".contains(c);
        if symbol.is_empty() || symbol.len() > 16 || !symbol.chars().all(allowed) {
            return Err(ResearchRunError::InvalidId { what: "symbol" });
        }
        Ok(Self(symbol.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Which read an item came from (§6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ItemKind {
    Bar,
    News,
    Filing,
    Instrument,
    Quote,
}

impl ItemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bar => "bar",
            Self::News => "news",
            Self::Filing => "filing",
            Self::Instrument => "instrument",
            Self::Quote => "quote",
        }
    }
}

/// One daily bar. Its knowledge time is the instant the bar was complete and known, which for a
/// backfilled bar is the session close, never the retrieval instant (data-plane spec §5.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bar {
    pub symbol: Symbol,
    pub knowledge_at: UtcNanos,
    pub open: Price,
    pub high: Price,
    pub low: Price,
    pub close: Price,
    pub volume: Qty,
}

/// The last quote at or before a run's cut-off (DEC-2255 item 4). A side with no order has no
/// price, since a price is always positive; a size is never negative.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quote {
    pub symbol: Symbol,
    pub knowledge_at: UtcNanos,
    pub bid: Option<Price>,
    pub bid_size: Qty,
    pub ask: Option<Price>,
    pub ask_size: Qty,
}

/// One news item from an allowlisted source. `published_at` is displayed only; every filter reads
/// `knowledge_at` (HI-13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewsItem {
    pub item_id: ItemId,
    pub source: SourceId,
    pub symbols: BTreeSet<Symbol>,
    pub knowledge_at: UtcNanos,
    pub published_at: UtcNanos,
    pub headline: String,
    pub summary: String,
    pub link: String,
    pub content_hash: Digest,
    /// Whether the item is licensed text, which is never sent until the inference spec's prompt
    /// record can keep it out of the artifact store (§6.2 precondition 3, HI-23).
    pub licensed: bool,
}

/// One filing entry. Public filings may be kept in full (data-plane spec §4.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filing {
    pub item_id: ItemId,
    pub source: SourceId,
    pub symbol: Symbol,
    pub form: String,
    pub knowledge_at: UtcNanos,
    pub filed_at: UtcNanos,
    pub excerpt: String,
    pub content_hash: Digest,
    pub licensed: bool,
}

/// One instrument's reference facts from the snapshot (§6.3's `instruments` read): what the
/// platform, never the model, says the instrument is (HI-18).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstrumentFact {
    pub asset_id: AssetId,
    pub symbol: Symbol,
    pub asset_class: AssetClass,
    pub eligible: bool,
    pub group: Option<String>,
    pub leveraged_or_inverse_etp: bool,
}

/// The source allowlist version in effect and its sources (DEC-101). A news or filing item from a
/// source off it is dropped at retrieval, whatever the reader returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allowlist {
    pub version: u32,
    pub sources: BTreeSet<SourceId>,
}
