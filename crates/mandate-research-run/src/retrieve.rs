//! Running the retrieval plan at a cut-off (§6.3), and the capture record of each item (§6.2 step
//! 2, data-plane spec §4.6).

use mandate_canon::Digest;
use mandate_time::UtcNanos;

use crate::error::ResearchRunError;
use crate::item::{Bar, Filing, InstrumentFact, ItemId, ItemKind, NewsItem, SourceId, Symbol};
use crate::plan::{RetrievalPlan, Scope};
use crate::ports::Readers;

/// One item the run will show the model, exactly as it will be journaled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetrievedItem {
    Bar(Bar),
    News(NewsItem),
    Filing(Filing),
    Instrument(InstrumentFact),
}

/// Why an item a reader returned is not in the retrieval. Every drop is recorded, so the artifact
/// says what the model was not shown and why (§6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DropReason {
    /// Knowledge time after the cut-off: look-ahead (HI-13, DP-1).
    AfterCutOff,
    /// Knowledge time before the read's window.
    BeforeWindow,
    /// A news or filing source off the allowlist version in effect (DEC-101).
    NotAllowlisted,
    /// Licensed text, which waits for §6.2 precondition 3 (HI-23).
    Licensed,
    /// About no instrument the read was scoped to.
    OutsideScope,
    /// An item id already kept from an earlier record of the same read.
    Duplicate,
    /// Past the read's cap; the oldest go first.
    OverCap,
    /// Kept, but its text was cut to the plan's byte cap.
    Trimmed,
}

impl DropReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AfterCutOff => "after_cut_off",
            Self::BeforeWindow => "before_window",
            Self::NotAllowlisted => "not_allowlisted",
            Self::Licensed => "licensed",
            Self::OutsideScope => "outside_scope",
            Self::Duplicate => "duplicate",
            Self::OverCap => "over_cap",
            Self::Trimmed => "trimmed",
        }
    }
}

/// One recorded drop: the item's id and kind, and the reason.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Drop {
    pub kind: ItemKind,
    pub item: ItemId,
    pub reason: DropReason,
}

/// What one run read. `items` is in a fixed order (instruments, then bars, news, and filings, each
/// by symbol, then knowledge time, then id), so the same reads give the same journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Retrieval {
    pub cut_off: UtcNanos,
    pub items: Vec<RetrievedItem>,
    pub drops: Vec<Drop>,
}

impl RetrievedItem {
    pub fn kind(&self) -> ItemKind {
        match self {
            Self::Bar(_) => ItemKind::Bar,
            Self::News(_) => ItemKind::News,
            Self::Filing(_) => ItemKind::Filing,
            Self::Instrument(_) => ItemKind::Instrument,
        }
    }

    /// The id a thesis cites: the reader's own for news and filings, and one derived from the
    /// record for platform data, so every item has exactly one.
    ///
    /// # Errors
    /// [`ResearchRunError::InvalidId`] when a derived id leaves the grammar.
    pub fn item_id(&self) -> Result<ItemId, ResearchRunError> {
        Err(ResearchRunError::Unimplemented { story: "E19-2" })
    }

    pub fn source(&self) -> Result<SourceId, ResearchRunError> {
        Err(ResearchRunError::Unimplemented { story: "E19-2" })
    }

    /// The one instrument the item is about, where there is exactly one.
    pub fn symbol(&self) -> Option<&Symbol> {
        None
    }

    pub fn knowledge_at(&self) -> UtcNanos {
        match self {
            Self::Bar(bar) => bar.knowledge_at,
            Self::News(item) => item.knowledge_at,
            Self::Filing(item) => item.knowledge_at,
            Self::Instrument(_) => UtcNanos::EPOCH,
        }
    }

    /// The canonical JSON the shell stores as the item's artifact and the core journals by digest
    /// (journal spec §4). It holds the data itself for market data, reference data, and public
    /// filings, and never the text of a licensed item (§6.6).
    ///
    /// # Errors
    /// [`ResearchRunError`] when a member cannot be written canonically.
    pub fn record(&self) -> Result<Vec<u8>, ResearchRunError> {
        Err(ResearchRunError::Unimplemented { story: "E19-2" })
    }

    /// The record's SHA-256, the observation's `data_ref`.
    ///
    /// # Errors
    /// As [`Self::record`].
    pub fn data_ref(&self) -> Result<Digest, ResearchRunError> {
        self.record().map(|bytes| Digest::of(&bytes))
    }
}

/// Runs `plan` at `scope.cut_off`.
///
/// Every reader's answer is filtered again here: knowledge time at or before the cut-off and inside
/// the read's window, sources on the allowlist, licensed text dropped, scope respected, duplicates
/// removed, then the cap applied with the oldest dropped first and text cut to its byte cap. A
/// failed read is an empty read. Nothing here can enlarge what a reader returned.
///
/// # Errors
/// [`ResearchRunError::TimeOutOfRange`] when a window starts before the representable range.
pub fn retrieve(
    _plan: &RetrievalPlan,
    _scope: &Scope,
    _readers: Readers<'_>,
) -> Result<Retrieval, ResearchRunError> {
    Err(ResearchRunError::Unimplemented { story: "E19-2" })
}
