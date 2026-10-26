//! The cell's recorded research bundle (DEC-2210 items 1 and 2): the research entry, its one
//! endpoint, the caps, the model's one recorded reply, and a recorded data plane whose times are
//! ages before each read's cut-off. Loaded once at start; anything missing refuses the load.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use mandate_canon::{Digest, Value, parse};
use mandate_domain::{AssetClass, AssetId};
use mandate_gateway::money::Usd6;
use mandate_gateway::registry::{Endpoint, Registry, Reported};
use mandate_num::{Price, Qty};
use mandate_research::{AllowlistVersion, SourceAllowlist, SourceId as AllowedSource};
use mandate_research_run::recorded::{self, RecordedError, RecordedModel};
pub use mandate_research_run::recorded::{RecordedReply, ScriptedTurn};
use mandate_research_run::{
    Bar, BarReader, Filing, FilingReader, InstrumentFact, InstrumentReader, ItemId, NewsItem,
    NewsReader, NewsText, ReadError, ResearchEntry, SourceId, Symbol, TextQuery,
};
use mandate_time::UtcNanos;

/// Why a bundle could not be loaded or registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BundleError {
    /// A file is missing or unreadable.
    Unreadable { file: &'static str },
    /// A file is not strict JSON, or a member is missing or out of its range. `member` names the
    /// member, or is empty when the file as a whole is not strict JSON or not the array it must be.
    Malformed {
        file: &'static str,
        member: &'static str,
    },
    /// The gateway's registry refused the entry or its endpoint.
    Unregistered,
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

/// One recorded feed item (agent harness spec §5.3, DEC-2392, DEC-2275): a news item that arrives
/// `arrives_s` seconds after its agent's `AgentDeployed`, known and published at its arrival. Its
/// times hang on the deployment, not on a read, so a restart offers the same item with the same
/// bytes and the core's one-item-one-run fold holds across it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedItem {
    pub item_id: ItemId,
    pub source: SourceId,
    pub symbols: BTreeSet<Symbol>,
    pub arrives_s: u32,
    pub headline: String,
    pub summary: String,
    pub link: String,
    pub licensed: bool,
}

impl FeedItem {
    /// The item as it arrives for an agent deployed at `deployed_at`, on the whole second, which is
    /// the observation's `as_of` exactly, or `None` past the representable range.
    pub fn arrived(&self, deployed_at: UtcNanos) -> Option<NewsItem> {
        let secs = deployed_at.secs().checked_add(i64::from(self.arrives_s))?;
        let at = UtcNanos::from_parts(secs, 0).ok()?;
        Some(NewsItem {
            item_id: self.item_id.clone(),
            source: self.source.clone(),
            symbols: self.symbols.clone(),
            knowledge_at: at,
            published_at: at,
            headline: self.headline.clone(),
            summary: self.summary.clone(),
            link: self.link.clone(),
            content_hash: Digest::of_parts(&[self.headline.as_bytes(), self.summary.as_bytes()]),
            licensed: self.licensed,
        })
    }
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

/// The cell's recorded research.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    pub entry: ResearchEntry,
    pub endpoint: Endpoint,
    /// What the endpoint reports it serves, which the registry probes it with.
    pub probe: Reported,
    pub workspace_cap: Usd6,
    pub deployment_cap: Usd6,
    /// The workspace's `research_run_cost_cap_usd`, recorded as `caps.run_usd`: an agentic run's
    /// cost ceiling, or `None` when the recording sets none (DEC-2268).
    pub run_cap: Option<Usd6>,
    pub reply: RecordedReply,
    /// The scripted replies of an agentic run's turns before its final one, turn 1 first; empty
    /// for a bundle with no `turns` (DEC-2266).
    pub turns: Vec<ScriptedTurn>,
    /// A `monitor` run's final reply, when the recording has one (agent harness spec §6.9).
    pub monitor_reply: Option<RecordedReply>,
    pub plane: RecordedPlane,
    /// The recorded feed, empty when the bundle holds no `feed.json` (DEC-2275).
    pub feed: Vec<FeedItem>,
    /// The recorded instruments halted at the bundle's as-of time (DEC-2214 item 1).
    pub halted: BTreeSet<AssetId>,
    /// The source allowlist the bundle's items were recorded under (DEC-2214 item 1).
    pub allowlist: SourceAllowlist,
}

const RESEARCH: &str = "research.json";
const BARS: &str = "bars.json";
const NEWS: &str = "news.json";
const FILINGS: &str = "filings.json";
const INSTRUMENTS: &str = "instruments.json";
const FEED: &str = "feed.json";

impl Bundle {
    /// The bundle in `dir`.
    ///
    /// # Errors
    /// [`BundleError`] when a file is missing or unreadable, or a member is missing or invalid.
    pub fn load(dir: &Path) -> Result<Self, BundleError> {
        let research = Doc::read(dir, RESEARCH)?;
        let model = RecordedModel::read(&research.value).map_err(|error| match error {
            RecordedError::Malformed(member) => BundleError::Malformed {
                file: RESEARCH,
                member,
            },
            RecordedError::Unregistered => BundleError::Unregistered,
        })?;
        let caps = research.member("caps")?;
        let allowlist = allowlist(&research)?;
        let plane = RecordedPlane {
            bars: Doc::read(dir, BARS)?
                .rows()?
                .iter()
                .map(bar)
                .collect::<Result<_, _>>()?,
            news: Doc::read(dir, NEWS)?
                .rows()?
                .iter()
                .map(news)
                .collect::<Result<_, _>>()?,
            filings: Doc::read(dir, FILINGS)?
                .rows()?
                .iter()
                .map(filing)
                .collect::<Result<_, _>>()?,
            instruments: Doc::read(dir, INSTRUMENTS)?
                .rows()?
                .iter()
                .map(instrument)
                .collect::<Result<_, _>>()?,
            allowlist_version: allowlist.version.0,
        };
        let halted = halts(&research, &plane.instruments)?;
        let present = dir
            .join(FEED)
            .try_exists()
            .map_err(|_| BundleError::Unreadable { file: FEED })?;
        let feed = if present {
            Doc::read(dir, FEED)?
                .rows()?
                .iter()
                .map(feed_item)
                .collect::<Result<_, _>>()?
        } else {
            Vec::new()
        };
        Ok(Self {
            entry: model.entry,
            endpoint: model.endpoint,
            probe: model.probe,
            workspace_cap: caps.usd6("workspace_monthly_usd")?,
            deployment_cap: caps.usd6("deployment_usd")?,
            run_cap: caps
                .value
                .get("run_usd")
                .map(|_| caps.usd6("run_usd"))
                .transpose()?,
            reply: model.reply,
            turns: model.turns,
            monitor_reply: model.monitor_reply,
            plane,
            feed,
            halted,
            allowlist,
        })
    }

    /// The gateway's registry: the entry, offered, on its one endpoint.
    ///
    /// # Errors
    /// [`BundleError::Unregistered`] when the registry refuses the entry or the endpoint.
    pub fn registry(&self) -> Result<Registry, BundleError> {
        recorded::registry(&self.entry, &self.endpoint, &self.probe)
            .map_err(|_| BundleError::Unregistered)
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

/// One JSON value of a bundle file, with the file it came from for errors.
struct Doc<'a> {
    file: &'static str,
    value: &'a Value,
}

/// A bundle file's parsed text.
struct Read {
    file: &'static str,
    value: Value,
}

impl Doc<'_> {
    fn read(dir: &Path, file: &'static str) -> Result<Read, BundleError> {
        let bytes = fs::read(dir.join(file)).map_err(|_| BundleError::Unreadable { file })?;
        let value = parse(&bytes).map_err(|_| BundleError::Malformed { file, member: "" })?;
        Ok(Read { file, value })
    }

    fn malformed(&self, member: &'static str) -> BundleError {
        BundleError::Malformed {
            file: self.file,
            member,
        }
    }

    fn text(&self, member: &'static str) -> Result<&str, BundleError> {
        self.value
            .get(member)
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .ok_or_else(|| self.malformed(member))
    }

    fn int(&self, member: &'static str) -> Result<u64, BundleError> {
        self.value
            .get(member)
            .and_then(Value::as_int)
            .ok_or_else(|| self.malformed(member))
    }

    fn small(&self, member: &'static str) -> Result<u32, BundleError> {
        u32::try_from(self.int(member)?).map_err(|_| self.malformed(member))
    }

    fn flag(&self, member: &'static str) -> Result<bool, BundleError> {
        match self.value.get(member) {
            Some(Value::Bool(flag)) => Ok(*flag),
            _ => Err(self.malformed(member)),
        }
    }

    fn usd6(&self, member: &'static str) -> Result<Usd6, BundleError> {
        Usd6::parse(self.text(member)?).map_err(|_| self.malformed(member))
    }

    fn price(&self, member: &'static str) -> Result<Price, BundleError> {
        Price::parse(self.text(member)?).map_err(|_| self.malformed(member))
    }

    fn symbol(&self, member: &'static str) -> Result<Symbol, BundleError> {
        Symbol::new(self.text(member)?).map_err(|_| self.malformed(member))
    }
}

impl Read {
    fn member(&self, member: &'static str) -> Result<Doc<'_>, BundleError> {
        let value = self.value.get(member).ok_or(BundleError::Malformed {
            file: self.file,
            member,
        })?;
        Ok(Doc {
            file: self.file,
            value,
        })
    }

    fn rows(&self) -> Result<Vec<Doc<'_>>, BundleError> {
        let rows = self.value.as_array().ok_or(BundleError::Malformed {
            file: self.file,
            member: "",
        })?;
        Ok(rows
            .iter()
            .map(|value| Doc {
                file: self.file,
                value,
            })
            .collect())
    }
}

/// The assets of the instruments `research.json`'s `halted_instruments` names by symbol, each one an
/// instrument the bundle records.
fn halts(
    research: &Read,
    instruments: &[InstrumentFact],
) -> Result<BTreeSet<AssetId>, BundleError> {
    let malformed = BundleError::Malformed {
        file: RESEARCH,
        member: "halted_instruments",
    };
    research
        .value
        .get("halted_instruments")
        .and_then(Value::as_array)
        .ok_or_else(|| malformed.clone())?
        .iter()
        .map(|symbol| {
            symbol
                .as_str()
                .and_then(|text| instruments.iter().find(|fact| fact.symbol.as_str() == text))
                .map(|fact| fact.asset_id.clone())
                .ok_or_else(|| malformed.clone())
        })
        .collect()
}

/// `research.json`'s `allowlist`: its version and its sources.
fn allowlist(research: &Read) -> Result<SourceAllowlist, BundleError> {
    let malformed = || BundleError::Malformed {
        file: RESEARCH,
        member: "allowlist",
    };
    let allowlist = research.value.get("allowlist").ok_or_else(malformed)?;
    let version = allowlist
        .get("version")
        .and_then(Value::as_int)
        .and_then(|version| u32::try_from(version).ok())
        .ok_or_else(malformed)?;
    let sources = allowlist
        .get("sources")
        .and_then(Value::as_array)
        .ok_or_else(malformed)?
        .iter()
        .map(|source| {
            source
                .as_str()
                .and_then(|text| AllowedSource::new(text).ok())
                .ok_or_else(malformed)
        })
        .collect::<Result<_, _>>()?;
    Ok(SourceAllowlist {
        version: AllowlistVersion(version),
        sources,
    })
}

fn bar(row: &Doc<'_>) -> Result<RecordedBar, BundleError> {
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

fn filing(row: &Doc<'_>) -> Result<RecordedFiling, BundleError> {
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

fn feed_item(row: &Doc<'_>) -> Result<FeedItem, BundleError> {
    Ok(FeedItem {
        item_id: ItemId::new(row.text("item_id")?).map_err(|_| row.malformed("item_id"))?,
        source: SourceId::new(row.text("source")?).map_err(|_| row.malformed("source"))?,
        symbols: symbols(row)?,
        arrives_s: row.small("arrives_s")?,
        headline: row.text("headline")?.to_owned(),
        summary: row.text("summary")?.to_owned(),
        link: row.text("link")?.to_owned(),
        licensed: row.flag("licensed")?,
    })
}

fn symbols(row: &Doc<'_>) -> Result<BTreeSet<Symbol>, BundleError> {
    row.value
        .get("symbols")
        .and_then(Value::as_array)
        .ok_or_else(|| row.malformed("symbols"))?
        .iter()
        .map(|symbol| {
            symbol
                .as_str()
                .and_then(|text| Symbol::new(text).ok())
                .ok_or_else(|| row.malformed("symbols"))
        })
        .collect()
}

fn news(row: &Doc<'_>) -> Result<RecordedNews, BundleError> {
    Ok(RecordedNews {
        item_id: ItemId::new(row.text("item_id")?).map_err(|_| row.malformed("item_id"))?,
        source: SourceId::new(row.text("source")?).map_err(|_| row.malformed("source"))?,
        symbols: symbols(row)?,
        age_s: row.small("age_s")?,
        published_age_s: row.small("published_age_s")?,
        headline: row.text("headline")?.to_owned(),
        summary: row.text("summary")?.to_owned(),
        link: row.text("link")?.to_owned(),
        licensed: row.flag("licensed")?,
    })
}

fn instrument(row: &Doc<'_>) -> Result<InstrumentFact, BundleError> {
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
