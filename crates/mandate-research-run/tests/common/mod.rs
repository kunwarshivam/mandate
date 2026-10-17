//! Fixtures and doubles shared by the worker's tests: readers that answer from vectors, so a test
//! states only what a reader returns, and a recorded gateway.

#![allow(dead_code, reason = "each test binary uses a different subset")]

use std::cell::Cell;
use std::collections::BTreeSet;

use mandate_canon::Digest;
use mandate_domain::{AssetClass, AssetId};
use mandate_num::{Price, Qty};
use mandate_research_run::{
    Allowlist, Bar, BarReader, Filing, FilingReader, InstrumentFact, InstrumentReader, ItemId,
    NewsItem, NewsReader, ReadError, Readers, RetrievalPlan, Scope, SourceId, Symbol, TextQuery,
};
use mandate_time::UtcNanos;

/// The cut-off every case reads at: 2026-10-16 20:00 UTC, a Friday after the close.
pub const CUT_OFF: i64 = 1_792_180_800;
pub const DAY: i64 = 86_400;
pub const NEWSWIRE: &str = "src.newswire";
pub const WIRE_TWO: &str = "src.wire_two";
pub const EDGAR: &str = "src.edgar";
pub const BLOG: &str = "src.anonymous_blog";
pub const AAPL_ID: &str = "7b4a1c2e-1111-4a2b-9c3d-000000000001";
pub const MSFT_ID: &str = "7b4a1c2e-2222-4a2b-9c3d-000000000002";

pub fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).unwrap_or_else(|e| panic!("{secs} is an instant: {e}"))
}

pub fn symbol(text: &str) -> Symbol {
    Symbol::new(text).unwrap_or_else(|e| panic!("`{text}` is a symbol: {e}"))
}

pub fn item(text: &str) -> ItemId {
    ItemId::new(text).unwrap_or_else(|e| panic!("`{text}` is an item id: {e}"))
}

pub fn source(text: &str) -> SourceId {
    SourceId::new(text).unwrap_or_else(|e| panic!("`{text}` is a source id: {e}"))
}

pub fn price(text: &str) -> Price {
    Price::parse(text).unwrap_or_else(|e| panic!("`{text}` is a price: {e}"))
}

pub fn qty(text: &str) -> Qty {
    Qty::parse(text).unwrap_or_else(|e| panic!("`{text}` is a quantity: {e}"))
}

pub fn bar(name: &str, knowledge_at: i64, close: &str) -> Bar {
    Bar {
        symbol: symbol(name),
        knowledge_at: at(knowledge_at),
        open: price(close),
        high: price(close),
        low: price(close),
        close: price(close),
        volume: qty("1000"),
    }
}

/// A daily bar for each of the `count` sessions ending the day before the cut-off, oldest first.
pub fn bars(name: &str, count: i64) -> Vec<Bar> {
    (0..count)
        .rev()
        .map(|back| bar(name, CUT_OFF - DAY * (back + 1), "100"))
        .collect()
}

pub fn news(id: &str, from: &str, about: &str, knowledge_at: i64) -> NewsItem {
    NewsItem {
        item_id: item(id),
        source: source(from),
        symbols: BTreeSet::from([symbol(about)]),
        knowledge_at: at(knowledge_at),
        published_at: at(knowledge_at),
        headline: format!("Headline {id}"),
        summary: format!("Summary of {id}."),
        link: format!("https://news.example/{id}"),
        content_hash: Digest::of(id.as_bytes()),
        licensed: false,
    }
}

pub fn filing(id: &str, about: &str, knowledge_at: i64) -> Filing {
    Filing {
        item_id: item(id),
        source: source(EDGAR),
        symbol: symbol(about),
        form: "8-K".to_owned(),
        knowledge_at: at(knowledge_at),
        filed_at: at(knowledge_at),
        excerpt: format!("Excerpt of {id}."),
        content_hash: Digest::of(id.as_bytes()),
        licensed: false,
    }
}

pub fn instrument(name: &str, id: &str) -> InstrumentFact {
    InstrumentFact {
        asset_id: AssetId::parse(id).unwrap_or_else(|e| panic!("`{id}` is an asset id: {e}")),
        symbol: symbol(name),
        asset_class: AssetClass::UsEquity,
        eligible: true,
        group: None,
        leveraged_or_inverse_etp: false,
    }
}

pub fn scope() -> Scope {
    Scope {
        workspace: "ws1".to_owned(),
        agent: "agent-a".to_owned(),
        cut_off: at(CUT_OFF),
        data_universe: BTreeSet::from([symbol("AAPL"), symbol("MSFT")]),
        held: BTreeSet::new(),
        allowlist: Allowlist {
            version: 3,
            sources: BTreeSet::from([source(NEWSWIRE), source(WIRE_TWO), source(EDGAR)]),
        },
    }
}

pub fn plan() -> RetrievalPlan {
    RetrievalPlan::v0_1()
}

/// Readers that answer from vectors and count their calls. A reader that ignores the query's
/// window or cut-off is exactly what these are, so a case can show the worker filters again.
#[derive(Default)]
pub struct Fixture {
    pub bars: Vec<Bar>,
    pub news: Vec<NewsItem>,
    pub filings: Vec<Filing>,
    pub instruments: Vec<InstrumentFact>,
    pub failing: bool,
    pub calls: Cell<u32>,
    pub queries: std::cell::RefCell<Vec<TextQuery>>,
}

impl Fixture {
    pub fn readers(&self) -> Readers<'_> {
        Readers {
            bars: self,
            news: self,
            filings: self,
            instruments: self,
        }
    }

    fn answer<T: Clone>(&self, rows: &[T]) -> Result<Vec<T>, ReadError> {
        self.calls.set(self.calls.get() + 1);
        if self.failing {
            return Err(ReadError::Unavailable);
        }
        Ok(rows.to_vec())
    }
}

impl BarReader for Fixture {
    fn daily_bars(
        &self,
        name: &Symbol,
        _cut_off: UtcNanos,
        _limit: u32,
    ) -> Result<Vec<Bar>, ReadError> {
        let rows: Vec<Bar> = self
            .bars
            .iter()
            .filter(|bar| bar.symbol == *name)
            .cloned()
            .collect();
        self.answer(&rows)
    }
}

impl NewsReader for Fixture {
    fn news(&self, query: &TextQuery) -> Result<Vec<NewsItem>, ReadError> {
        self.queries.borrow_mut().push(query.clone());
        self.answer(&self.news)
    }
}

impl FilingReader for Fixture {
    fn filings(&self, query: &TextQuery) -> Result<Vec<Filing>, ReadError> {
        self.queries.borrow_mut().push(query.clone());
        self.answer(&self.filings)
    }
}

impl InstrumentReader for Fixture {
    fn instruments(&self, _symbols: &BTreeSet<Symbol>) -> Result<Vec<InstrumentFact>, ReadError> {
        self.answer(&self.instruments)
    }
}
