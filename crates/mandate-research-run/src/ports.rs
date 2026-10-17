//! The worker's ports. Every reader takes `&self` and returns owned data: there is no method that
//! writes, sends, approves, or appends, so a reader can only read (HI-4). The shell implements them
//! over the data plane (lane I3 for news and filings) and the gateway (lane I1).

use std::collections::BTreeSet;

use mandate_time::UtcNanos;

use crate::call::{CallOutcome, ResearchCall};
use crate::item::{Bar, Filing, InstrumentFact, NewsItem, Symbol};

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
}

/// Filings from allowlisted sources (lane I3, E17-7).
pub trait FilingReader {
    /// # Errors
    /// [`ReadError`] when the store cannot answer.
    fn filings(&self, query: &TextQuery) -> Result<Vec<Filing>, ReadError>;
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

/// The model gateway (inference spec §3, lane I1). One call per run; the gateway renders the
/// prompt from the pinned template, reserves the cost, guards the prompt, calls, and validates the
/// output, and answers with exactly one of a reply or a typed failure. It holds the provider key;
/// the worker never sees one (HI-12).
pub trait ModelGateway {
    fn call(&self, request: &ResearchCall) -> CallOutcome;
}
