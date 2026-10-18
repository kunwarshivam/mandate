//! The cell's recorded research bundle (DEC-2210 items 1 and 2): the research entry, its one
//! endpoint, the caps, the model's one recorded reply, and a recorded data plane whose times are
//! ages before each read's cut-off. Loaded once at start; anything missing refuses the load.

use std::collections::BTreeSet;
use std::path::Path;

use mandate_gateway::money::{Usage, Usd6};
use mandate_gateway::registry::{Endpoint, Registry, Reported};
use mandate_num::{Price, Qty};
use mandate_research_run::{
    Bar, BarReader, Filing, FilingReader, InstrumentFact, InstrumentReader, ItemId, NewsItem,
    NewsReader, ReadError, ResearchEntry, SourceId, Symbol, TextQuery,
};
use mandate_time::UtcNanos;

/// Why a bundle could not be loaded or registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BundleError {
    /// DEC-2210's loader is not implemented yet (E21-54).
    Unimplemented,
    /// A file is missing or unreadable.
    Unreadable { file: &'static str },
    /// A file is not strict JSON, or a member is missing or out of its range.
    Malformed {
        file: &'static str,
        member: &'static str,
    },
    /// The gateway's registry refused the entry or its endpoint.
    Unregistered,
}

impl BundleError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unimplemented => "unimplemented",
            Self::Unreadable { .. } => "research_bundle_unreadable",
            Self::Malformed { .. } => "research_bundle_malformed",
            Self::Unregistered => "research_bundle_unregistered",
        }
    }
}

/// The model's one recorded reply: the output text the fake replays, its latency, and its usage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedReply {
    pub output: String,
    pub latency_ms: u64,
    pub usage: Usage,
}

/// One recorded daily bar, known `age_s` seconds before the read's cut-off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedBar {
    pub symbol: Symbol,
    pub age_s: u32,
    pub open: Price,
    pub high: Price,
    pub low: Price,
    pub close: Price,
    pub volume: Qty,
}

/// One recorded news item, known `age_s` and published `published_age_s` seconds before the
/// read's cut-off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedNews {
    pub item_id: ItemId,
    pub source: SourceId,
    pub symbols: BTreeSet<Symbol>,
    pub age_s: u32,
    pub published_age_s: u32,
    pub headline: String,
    pub summary: String,
    pub link: String,
    pub licensed: bool,
}

/// The recorded data plane. It returns no filings, and stamps every item at its age before the
/// cut-off of the read it answers; `retrieve` filters the answer again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedPlane {
    pub bars: Vec<RecordedBar>,
    pub news: Vec<RecordedNews>,
    pub instruments: Vec<InstrumentFact>,
}

/// The cell's recorded research.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    pub entry: ResearchEntry,
    pub endpoint: Endpoint,
    /// What the endpoint reports it serves, which the registry probes it with.
    pub probe: Reported,
    pub workspace_cap: Usd6,
    pub deployment_cap: Usd6,
    pub reply: RecordedReply,
    pub plane: RecordedPlane,
}

impl Bundle {
    /// The bundle in `dir`.
    ///
    /// # Errors
    /// [`BundleError`] when a file is missing or unreadable, or a member is missing or invalid.
    pub fn load(dir: &Path) -> Result<Self, BundleError> {
        let _ = dir;
        Err(BundleError::Unimplemented)
    }

    /// The gateway's registry: the entry, offered, on its one endpoint.
    ///
    /// # Errors
    /// [`BundleError::Unregistered`] when the registry refuses the entry or the endpoint.
    pub fn registry(&self) -> Result<Registry, BundleError> {
        Err(BundleError::Unimplemented)
    }
}

impl BarReader for RecordedPlane {
    #[expect(
        clippy::todo,
        reason = "a reader's error has no stub variant, so its stub is todo!(), the other form DEC-137 names"
    )]
    fn daily_bars(
        &self,
        _symbol: &Symbol,
        _cut_off: UtcNanos,
        _limit: u32,
    ) -> Result<Vec<Bar>, ReadError> {
        todo!()
    }
}

impl NewsReader for RecordedPlane {
    #[expect(
        clippy::todo,
        reason = "a reader's error has no stub variant, so its stub is todo!(), the other form DEC-137 names"
    )]
    fn news(&self, _query: &TextQuery) -> Result<Vec<NewsItem>, ReadError> {
        todo!()
    }
}

impl FilingReader for RecordedPlane {
    #[expect(
        clippy::todo,
        reason = "a reader's error has no stub variant, so its stub is todo!(), the other form DEC-137 names"
    )]
    fn filings(&self, _query: &TextQuery) -> Result<Vec<Filing>, ReadError> {
        todo!()
    }
}

impl InstrumentReader for RecordedPlane {
    #[expect(
        clippy::todo,
        reason = "a reader's error has no stub variant, so its stub is todo!(), the other form DEC-137 names"
    )]
    fn instruments(&self, _symbols: &BTreeSet<Symbol>) -> Result<Vec<InstrumentFact>, ReadError> {
        todo!()
    }
}
