//! The worker's ports. Every reader takes `&self` and returns owned data: there is no method that
//! writes, sends, approves, or appends, so a reader can only read (HI-4). The shell implements them
//! over the data plane (lane I3 for news and filings). The model gateway is `mandate-gateway`'s
//! [`mandate_gateway::Gateway`] itself, reached through [`crate::call`].

use std::collections::BTreeSet;

use mandate_time::UtcNanos;

use crate::item::{Bar, Filing, InstrumentFact, ItemId, NewsItem, Symbol};

/// Why a read returned nothing. A failed read is an empty read: the run goes on with what it has,
/// and the model is never told why (`AGENTS.md` rule 3).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReadError {
    #[error("the data plane is unavailable")]
    Unavailable,
    #[error("the read was refused: {reason}")]
    Refused { reason: String },
}

/// A point-in-time query over allowlisted text (data-plane spec §4.6): the window ends at the
/// cut-off and the scope is the harness's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextQuery {
    pub workspace: String,
    pub symbols: BTreeSet<Symbol>,
    pub window_start: UtcNanos,
    pub cut_off: UtcNanos,
    pub allowlist_version: u32,
    pub max_items: u32,
}

/// One news item with the text the store holds for it, as `news.get` reads it (§6.10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewsText {
    pub item: NewsItem,
    pub text: String,
}

/// One filing as `filings.search` reads it: its record, and the byte length of the text the store
/// holds for it, so the model can ask `filings.get` for a range that starts inside it (§6.10,
/// DEC-2250 item 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilingEntry {
    pub filing: Filing,
    pub stored_bytes: u64,
}

/// Daily bars (`mandate-marketdata`'s, through the shell's adapter).
pub trait BarReader {
    /// # Errors
    /// [`ReadError`] when the store cannot answer.
    fn daily_bars(
        &self,
        symbol: &Symbol,
        cut_off: UtcNanos,
        limit: u32,
    ) -> Result<Vec<Bar>, ReadError>;
}

/// News from allowlisted sources (lane I3, E17-7).
pub trait NewsReader {
    /// # Errors
    /// [`ReadError`] when the store cannot answer.
    fn news(&self, query: &TextQuery) -> Result<Vec<NewsItem>, ReadError>;

    /// `news.search`'s read: the query, narrowed by the model's search terms, each matched inside
    /// the data service and never sent to a source (§6.10, data-plane spec §4.6, HI-28). A reader
    /// that cannot match terms keeps this default and answers [`ReadError::Unavailable`], which
    /// reads nothing (rule 3).
    ///
    /// # Errors
    /// [`ReadError`] when the store cannot answer.
    fn search(&self, query: &TextQuery, terms: &[String]) -> Result<Vec<NewsItem>, ReadError> {
        let _ = (query, terms);
        Err(ReadError::Unavailable)
    }

    /// The stored text of one item the run already received, as of `cut_off` and from a source on
    /// the allowlist version (data-plane spec §4.6's `items`). A reader that stores no text keeps
    /// this default and answers [`ReadError::Unavailable`], which reads nothing (rule 3).
    ///
    /// # Errors
    /// [`ReadError`] when the store cannot answer.
    fn news_text(
        &self,
        item: &ItemId,
        cut_off: UtcNanos,
        allowlist_version: u32,
    ) -> Result<NewsText, ReadError> {
        let _ = (item, cut_off, allowlist_version);
        Err(ReadError::Unavailable)
    }
}

/// Filings from allowlisted sources (lane I3, E17-7).
pub trait FilingReader {
    /// # Errors
    /// [`ReadError`] when the store cannot answer.
    fn filings(&self, query: &TextQuery) -> Result<Vec<Filing>, ReadError>;

    /// `filings.search`'s read: the query, narrowed by the forms and the model's search terms, each
    /// matched inside the data service and never sent to a source (§6.10, data-plane spec §4.6,
    /// HI-28). `forms` is never empty: a call that names none passes the whole pinned list
    /// (DEC-2250 items 1 and 2). A reader that cannot match keeps this default and answers
    /// [`ReadError::Unavailable`], which reads nothing (rule 3).
    ///
    /// # Errors
    /// [`ReadError`] when the store cannot answer.
    fn search(
        &self,
        query: &TextQuery,
        forms: &[String],
        terms: &[String],
    ) -> Result<Vec<FilingEntry>, ReadError> {
        let _ = (query, forms, terms);
        Err(ReadError::Unavailable)
    }
}

/// The instrument snapshot by symbol.
pub trait InstrumentReader {
    /// # Errors
    /// [`ReadError`] when the snapshot cannot answer.
    fn instruments(&self, symbols: &BTreeSet<Symbol>) -> Result<Vec<InstrumentFact>, ReadError>;
}

/// The four readers a run uses, borrowed for one retrieval.
#[derive(Clone, Copy)]
pub struct Readers<'a> {
    pub bars: &'a dyn BarReader,
    pub news: &'a dyn NewsReader,
    pub filings: &'a dyn FilingReader,
    pub instruments: &'a dyn InstrumentReader,
}
