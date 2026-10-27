//! The recorded data plane (DEC-2210 item 2, DEC-2211, DEC-2217 item 3): bars, news, filings and
//! instrument facts whose times are ages before each read's cut-off, read strictly from their JSON
//! rows. The cell's recorded bundle and the §4.3 gate's probe set read their corpus through it, in
//! the research runtime and in the model-gateway service alike, so both see one corpus the same way.

use std::collections::BTreeSet;

use mandate_canon::{Digest, Value};
use mandate_domain::{AssetClass, AssetId};
use mandate_num::{Price, Qty};
use mandate_time::UtcNanos;

use crate::{
    Bar, BarReader, Filing, FilingReader, InstrumentFact, InstrumentReader, ItemId, NewsItem,
    NewsReader, NewsText, ReadError, SourceId, Symbol, TextQuery,
};

/// The plane's files, by name, as their errors name them.
pub const BARS: &str = "bars.json";
pub const NEWS: &str = "news.json";
pub const FILINGS: &str = "filings.json";
pub const INSTRUMENTS: &str = "instruments.json";

/// Why a plane's rows did not read: the file, and the member missing or out of its range, or an
/// empty member when the file is not the array it must be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaneError {
    pub file: &'static str,
    pub member: &'static str,
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

/// One recorded filing, known `age_s` and filed `filed_age_s` seconds before the read's cut-off
/// (DEC-2211).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedFiling {
    pub item_id: ItemId,
    pub source: SourceId,
    pub symbol: Symbol,
    pub form: String,
    pub age_s: u32,
    pub filed_age_s: u32,
    pub excerpt: String,
    pub licensed: bool,
}

/// The recorded data plane. It stamps every item at its age before the cut-off of the read it
/// answers; `retrieve` filters the answer again. The cell's recording holds no filings (DEC-2210
/// item 2, DEC-2211).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedPlane {
    pub bars: Vec<RecordedBar>,
    pub news: Vec<RecordedNews>,
    pub filings: Vec<RecordedFiling>,
    pub instruments: Vec<InstrumentFact>,
    /// The source allowlist version the items were recorded under, the only one a get answers
    /// (data-plane spec §4.6).
    pub allowlist_version: u32,
}

impl RecordedPlane {
    /// The plane whose rows are `bars`, `news`, `filings` and `instruments`, each the parsed JSON
    /// array of its file, recorded under `allowlist_version`.
    ///
    /// # Errors
    /// [`PlaneError`] naming the first file and member that is missing or out of its range.
    pub fn read(
        bars: &Value,
        news: &Value,
        filings: &Value,
        instruments: &Value,
        allowlist_version: u32,
    ) -> Result<Self, PlaneError> {
        Ok(Self {
            bars: rows(BARS, bars)?
                .iter()
                .map(bar)
                .collect::<Result<_, _>>()?,
            news: rows(NEWS, news)?
                .iter()
                .map(news_row)
                .collect::<Result<_, _>>()?,
            filings: rows(FILINGS, filings)?
                .iter()
                .map(filing)
                .collect::<Result<_, _>>()?,
            instruments: rows(INSTRUMENTS, instruments)?
                .iter()
                .map(instrument)
                .collect::<Result<_, _>>()?,
            allowlist_version,
        })
    }
}

/// `age_s` seconds before `cut_off`, or `None` before the representable range.
fn before(cut_off: UtcNanos, age_s: u32) -> Option<UtcNanos> {
    let secs = cut_off.secs().checked_sub(i64::from(age_s))?;
    UtcNanos::from_parts(secs, cut_off.nanos()).ok()
}

impl BarReader for RecordedPlane {
    fn daily_bars(
        &self,
        symbol: &Symbol,
        cut_off: UtcNanos,
        _limit: u32,
    ) -> Result<Vec<Bar>, ReadError> {
        Ok(self
            .bars
            .iter()
            .filter(|bar| bar.symbol == *symbol)
            .filter_map(|bar| {
                Some(Bar {
                    symbol: bar.symbol.clone(),
                    knowledge_at: before(cut_off, bar.age_s)?,
                    open: bar.open,
                    high: bar.high,
                    low: bar.low,
                    close: bar.close,
                    volume: bar.volume,
                })
            })
            .collect())
    }
}

impl RecordedNews {
    /// The item as a read at `cut_off` gives it, at its age before the cut-off.
    fn at(&self, cut_off: UtcNanos) -> Option<NewsItem> {
        Some(NewsItem {
            item_id: self.item_id.clone(),
            source: self.source.clone(),
            symbols: self.symbols.clone(),
            knowledge_at: before(cut_off, self.age_s)?,
            published_at: before(cut_off, self.published_age_s)?,
            headline: self.headline.clone(),
            summary: self.summary.clone(),
            link: self.link.clone(),
            content_hash: Digest::of_parts(&[self.headline.as_bytes(), self.summary.as_bytes()]),
            licensed: self.licensed,
        })
    }

    /// Whether every one of `terms` is in the stored headline or the stored summary, a phrase inside
    /// one field (data-plane spec §4.6, DEC-2327 item 1).
    fn matches(&self, terms: &[String]) -> bool {
        terms
            .iter()
            .all(|term| holds_term(&self.headline, term) || holds_term(&self.summary, term))
    }
}

/// `term` in `text`, case-folded, as a whole word or phrase: bounded at each end by the text's edge
/// or by a character that is not an ASCII letter or digit (data-plane spec §4.6). It is matched
/// here, inside the plane, and sent nowhere (HI-28).
fn holds_term(text: &str, term: &str) -> bool {
    let text = text.to_ascii_lowercase();
    let term = term.to_ascii_lowercase();
    if term.is_empty() {
        return false;
    }
    let is_word = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric());
    text.match_indices(&term).any(|(start, found)| {
        let end = start.saturating_add(found.len());
        let before_ok = !is_word(text.get(..start).and_then(|t| t.chars().next_back()));
        let after_ok = !is_word(text.get(end..).and_then(|t| t.chars().next()));
        before_ok && after_ok
    })
}

impl NewsReader for RecordedPlane {
    fn news(&self, query: &TextQuery) -> Result<Vec<NewsItem>, ReadError> {
        Ok(self
            .news
            .iter()
            .filter_map(|item| item.at(query.cut_off))
            .collect())
    }

    fn search(&self, query: &TextQuery, terms: &[String]) -> Result<Vec<NewsItem>, ReadError> {
        Ok(self
            .news
            .iter()
            .filter(|item| item.matches(terms))
            .filter_map(|item| item.at(query.cut_off))
            .collect())
    }

    /// The recorded item's stored text, its headline and summary, at its age before `cut_off`, for
    /// the allowlist version the recording holds alone.
    fn news_text(
        &self,
        item: &ItemId,
        cut_off: UtcNanos,
        allowlist_version: u32,
    ) -> Result<NewsText, ReadError> {
        if allowlist_version != self.allowlist_version {
            return Err(ReadError::Unavailable);
        }
        let recorded = self
            .news
            .iter()
            .find(|recorded| recorded.item_id == *item)
            .ok_or(ReadError::Unavailable)?;
        let text = format!("{}\n\n{}", recorded.headline, recorded.summary);
        let item = recorded.at(cut_off).ok_or(ReadError::Unavailable)?;
        Ok(NewsText { item, text })
    }
}

impl FilingReader for RecordedPlane {
    fn filings(&self, query: &TextQuery) -> Result<Vec<Filing>, ReadError> {
        Ok(self
            .filings
            .iter()
            .filter_map(|item| {
                Some(Filing {
                    item_id: item.item_id.clone(),
                    source: item.source.clone(),
                    symbol: item.symbol.clone(),
                    form: item.form.clone(),
                    knowledge_at: before(query.cut_off, item.age_s)?,
                    filed_at: before(query.cut_off, item.filed_age_s)?,
                    excerpt: item.excerpt.clone(),
                    content_hash: Digest::of_parts(&[
                        item.form.as_bytes(),
                        item.excerpt.as_bytes(),
                    ]),
                    licensed: item.licensed,
                })
            })
            .collect())
    }
}

impl InstrumentReader for RecordedPlane {
    fn instruments(&self, symbols: &BTreeSet<Symbol>) -> Result<Vec<InstrumentFact>, ReadError> {
        Ok(self
            .instruments
            .iter()
            .filter(|fact| symbols.contains(&fact.symbol))
            .cloned()
            .collect())
    }
}

/// One row of a plane file, with the file it came from for errors.
struct Row<'a> {
    file: &'static str,
    value: &'a Value,
}

fn rows<'a>(file: &'static str, value: &'a Value) -> Result<Vec<Row<'a>>, PlaneError> {
    Ok(value
        .as_array()
        .ok_or(PlaneError { file, member: "" })?
        .iter()
        .map(|value| Row { file, value })
        .collect())
}

impl Row<'_> {
    fn malformed(&self, member: &'static str) -> PlaneError {
        PlaneError {
            file: self.file,
            member,
        }
    }

    fn text(&self, member: &'static str) -> Result<&str, PlaneError> {
        self.value
            .get(member)
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .ok_or_else(|| self.malformed(member))
    }

    fn small(&self, member: &'static str) -> Result<u32, PlaneError> {
        self.value
            .get(member)
            .and_then(Value::as_int)
            .and_then(|int| u32::try_from(int).ok())
            .ok_or_else(|| self.malformed(member))
    }

    fn flag(&self, member: &'static str) -> Result<bool, PlaneError> {
        match self.value.get(member) {
            Some(Value::Bool(flag)) => Ok(*flag),
            _ => Err(self.malformed(member)),
        }
    }

    fn price(&self, member: &'static str) -> Result<Price, PlaneError> {
        Price::parse(self.text(member)?).map_err(|_| self.malformed(member))
    }

    fn symbol(&self, member: &'static str) -> Result<Symbol, PlaneError> {
        Symbol::new(self.text(member)?).map_err(|_| self.malformed(member))
    }

    fn symbols(&self) -> Result<BTreeSet<Symbol>, PlaneError> {
        self.value
            .get("symbols")
            .and_then(Value::as_array)
            .ok_or_else(|| self.malformed("symbols"))?
            .iter()
            .map(|symbol| {
                symbol
                    .as_str()
                    .and_then(|text| Symbol::new(text).ok())
                    .ok_or_else(|| self.malformed("symbols"))
            })
            .collect()
    }
}

fn bar(row: &Row<'_>) -> Result<RecordedBar, PlaneError> {
    Ok(RecordedBar {
        symbol: row.symbol("symbol")?,
        age_s: row.small("age_s")?,
        open: row.price("open")?,
        high: row.price("high")?,
        low: row.price("low")?,
        close: row.price("close")?,
        volume: Qty::parse(row.text("volume")?).map_err(|_| row.malformed("volume"))?,
    })
}

fn filing(row: &Row<'_>) -> Result<RecordedFiling, PlaneError> {
    Ok(RecordedFiling {
        item_id: ItemId::new(row.text("item_id")?).map_err(|_| row.malformed("item_id"))?,
        source: SourceId::new(row.text("source")?).map_err(|_| row.malformed("source"))?,
        symbol: Symbol::new(row.text("symbol")?).map_err(|_| row.malformed("symbol"))?,
        form: row.text("form")?.to_owned(),
        age_s: row.small("age_s")?,
        filed_age_s: row.small("filed_age_s")?,
        excerpt: row.text("excerpt")?.to_owned(),
        licensed: row.flag("licensed")?,
    })
}

fn news_row(row: &Row<'_>) -> Result<RecordedNews, PlaneError> {
    Ok(RecordedNews {
        item_id: ItemId::new(row.text("item_id")?).map_err(|_| row.malformed("item_id"))?,
        source: SourceId::new(row.text("source")?).map_err(|_| row.malformed("source"))?,
        symbols: row.symbols()?,
        age_s: row.small("age_s")?,
        published_age_s: row.small("published_age_s")?,
        headline: row.text("headline")?.to_owned(),
        summary: row.text("summary")?.to_owned(),
        link: row.text("link")?.to_owned(),
        licensed: row.flag("licensed")?,
    })
}

fn instrument(row: &Row<'_>) -> Result<InstrumentFact, PlaneError> {
    Ok(InstrumentFact {
        asset_id: AssetId::parse(row.text("asset_id")?).map_err(|_| row.malformed("asset_id"))?,
        symbol: row.symbol("symbol")?,
        asset_class: AssetClass::parse(row.text("asset_class")?)
            .map_err(|_| row.malformed("asset_class"))?,
        eligible: row.flag("eligible")?,
        group: None,
        leveraged_or_inverse_etp: row.flag("leveraged_or_inverse_etp")?,
    })
}
