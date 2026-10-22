//! The cell's recorded research bundle (DEC-2210 items 1 and 2): the research entry, its one
//! endpoint, the caps, the model's one recorded reply, and a recorded data plane whose times are
//! ages before each read's cut-off. Loaded once at start; anything missing refuses the load.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_domain::{AssetClass, AssetId};
use mandate_gateway::money::{PriceTable, TokenPrice, Usage, Usd6};
use mandate_gateway::registry::{Endpoint, Locality, Registry, Reported, Route, Status};
use mandate_gateway::schema::fits;
use mandate_num::{Price, Qty};
use mandate_research::{AllowlistVersion, SourceAllowlist, SourceId as AllowedSource};
use mandate_research_run::{
    Bar, BarReader, EntryError, Filing, FilingReader, InstrumentFact, InstrumentReader, ItemId,
    NewsItem, NewsReader, NewsText, ReadError, ResearchEntry, SourceId, Symbol, TextQuery,
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

/// The model's one recorded reply: the output text the fake replays, its latency, and its usage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedReply {
    pub output: String,
    pub latency_ms: u64,
    pub usage: Usage,
}

/// One scripted tool-call reply of an agentic run's turn (DEC-2266): each call's declared tool
/// name and canonical arguments, in reply order, its latency, and its usage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptedTurn {
    pub calls: Vec<(String, String)>,
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
    pub plane: RecordedPlane,
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

impl Bundle {
    /// The bundle in `dir`.
    ///
    /// # Errors
    /// [`BundleError`] when a file is missing or unreadable, or a member is missing or invalid.
    pub fn load(dir: &Path) -> Result<Self, BundleError> {
        let research = Doc::read(dir, RESEARCH)?;
        let entry = research.member("entry")?;
        let endpoint = research.member("endpoint")?;
        let caps = research.member("caps")?;
        let reply = research.member("reply")?;
        let pinned = ResearchEntry::recorded(entry.value).map_err(|error| match error {
            EntryError::Member(member) => entry.malformed(member),
        })?;
        let turns = research
            .value
            .get("turns")
            .map(|turns| scripted(turns, &pinned))
            .transpose()?
            .unwrap_or_default();
        let slug = pinned.identity.model_name.clone();
        let reported_provider = endpoint.text("reported_provider")?;
        let prices = PriceTable::new(
            endpoint.per_million("input_per_million")?,
            endpoint.per_million("output_per_million")?,
            endpoint.per_million("cached_per_million")?,
            Digest::of(endpoint.text("price_table")?.as_bytes()),
        )
        .map_err(|_| endpoint.malformed("cached_per_million"))?;
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
        Ok(Self {
            entry: pinned,
            endpoint: Endpoint {
                id: endpoint.text("id")?.to_owned(),
                host: endpoint.text("host")?.to_owned(),
                region: endpoint.text("region")?.to_owned(),
                locality: Locality::Hosted,
                route: Route {
                    upstream: endpoint.text("upstream")?.to_owned(),
                    reported_provider: reported_provider.to_owned(),
                },
                prices,
            },
            probe: Reported {
                model: Some(slug),
                provider: Some(reported_provider.to_owned()),
            },
            workspace_cap: caps.usd6("workspace_monthly_usd")?,
            deployment_cap: caps.usd6("deployment_usd")?,
            run_cap: caps
                .value
                .get("run_usd")
                .map(|_| caps.usd6("run_usd"))
                .transpose()?,
            reply: RecordedReply {
                output: reply.text("output")?.to_owned(),
                latency_ms: reply.int("latency_ms")?,
                usage: Usage {
                    input: reply.int("input_tokens")?,
                    output: reply.int("output_tokens")?,
                    cached: reply.int("cached_tokens")?,
                },
            },
            turns,
            plane,
            halted,
            allowlist,
        })
    }

    /// The gateway's registry: the entry, offered, on its one endpoint.
    ///
    /// # Errors
    /// [`BundleError::Unregistered`] when the registry refuses the entry or the endpoint.
    pub fn registry(&self) -> Result<Registry, BundleError> {
        let content = self.entry.content();
        let pin = content.model_ref();
        let mut registry = Registry::new();
        registry
            .register(content)
            .map_err(|_| BundleError::Unregistered)?;
        registry
            .add_endpoint(
                &pin,
                self.endpoint.clone(),
                std::slice::from_ref(&self.probe),
            )
            .map_err(|_| BundleError::Unregistered)?;
        registry
            .set_status(&pin, Status::Offered)
            .map_err(|_| BundleError::Unregistered)?;
        Ok(registry)
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

    fn per_million(&self, member: &'static str) -> Result<TokenPrice, BundleError> {
        self.usd6(member).map(TokenPrice::per_million)
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

/// `research.json`'s `turns` (DEC-2266): refused whole, `Malformed { member: "turns" }`, unless the
/// entry names the agentic loop and every turn is one its gateway would accept, so no bundle a
/// worker is built from scripts a call the entry does not declare.
fn scripted(turns: &Value, entry: &ResearchEntry) -> Result<Vec<ScriptedTurn>, BundleError> {
    let refused = BundleError::Malformed {
        file: RESEARCH,
        member: "turns",
    };
    let content = entry.content();
    let (Some(agentic), Some(listed)) = (content.agentic(), turns.as_array()) else {
        return Err(refused);
    };
    let bounds = agentic.loop_bounds;
    if u32::try_from(listed.len()).map_or(true, |n| n >= bounds.max_turns) {
        return Err(refused);
    }
    listed
        .iter()
        .map(|turn| {
            let count = |member: &str| turn.get(member).and_then(Value::as_int);
            let calls = turn.get("calls").and_then(Value::as_array)?;
            let fits_turn = u32::try_from(calls.len())
                .is_ok_and(|n| n > 0 && n <= bounds.max_tool_calls_per_turn);
            if !fits_turn {
                return None;
            }
            let calls = calls
                .iter()
                .map(|call| {
                    let name = call.get("name").and_then(Value::as_str)?;
                    let arguments = call.get("arguments")?;
                    let declared = agentic.tool(name)?;
                    fits(&declared.arguments, arguments).then(|| {
                        (
                            name.to_owned(),
                            String::from_utf8_lossy(&to_canonical(arguments)).into_owned(),
                        )
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            Some(ScriptedTurn {
                calls,
                latency_ms: count("latency_ms")?,
                usage: Usage {
                    input: count("input_tokens")?,
                    output: count("output_tokens")?,
                    cached: count("cached_tokens")?,
                },
            })
        })
        .collect::<Option<Vec<_>>>()
        .ok_or(refused)
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

fn news(row: &Doc<'_>) -> Result<RecordedNews, BundleError> {
    let symbols = row
        .value
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
        .collect::<Result<_, _>>()?;
    Ok(RecordedNews {
        item_id: ItemId::new(row.text("item_id")?).map_err(|_| row.malformed("item_id"))?,
        source: SourceId::new(row.text("source")?).map_err(|_| row.malformed("source"))?,
        symbols,
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
