//! Running the retrieval plan at a cut-off (§6.3), and the capture record of each item (§6.2 step
//! 2, data-plane spec §4.6).

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::{Digest, Int, Key, Object, Value, to_canonical};
use mandate_num::Price;
use mandate_time::UtcNanos;

use crate::error::ResearchRunError;
use crate::item::{
    Bar, Filing, InstrumentFact, ItemId, ItemKind, NewsItem, Quote, SourceId, Symbol,
};
use crate::plan::{RetrievalPlan, Scope};
use crate::ports::{Readers, TextQuery};

/// One item the run will show the model, exactly as it will be journaled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetrievedItem {
    Bar(Bar),
    News(NewsItem),
    Filing(Filing),
    Instrument(InstrumentFact),
    Quote(Quote),
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
    /// A reader's id in the namespace the platform derives its own ids in (`bar:`, `instrument:`),
    /// which would collide with a platform item's.
    ReservedId,
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
            Self::ReservedId => "reserved_id",
        }
    }
}

/// One recorded drop: the item's id, the reason, and the read it came from. Ordered by item id, so
/// the list is the same whatever order the readers answered in.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Drop {
    pub item: ItemId,
    pub reason: DropReason,
    pub kind: ItemKind,
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
            Self::Quote(_) => ItemKind::Quote,
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
        match self {
            Self::Bar(bar) => ItemId::new(&format!(
                "bar:{}:{}",
                bar.symbol.as_str(),
                bar.knowledge_at.secs()
            )),
            Self::Quote(quote) => ItemId::new(&format!(
                "quote:{}:{}",
                quote.symbol.as_str(),
                quote.knowledge_at.secs()
            )),
            Self::News(item) => Ok(item.item_id.clone()),
            Self::Filing(item) => Ok(item.item_id.clone()),
            Self::Instrument(fact) => {
                ItemId::new(&format!("instrument:{}", fact.asset_id.as_str()))
            }
        }
    }

    /// The item's source: the allowlisted one for news and filings, the platform's feed otherwise.
    ///
    /// # Errors
    /// [`ResearchRunError::InvalidId`] never, for the platform's two constant ids; the result type
    /// is kept so every id is built through the same check.
    pub fn source(&self) -> Result<SourceId, ResearchRunError> {
        match self {
            Self::Bar(_) => SourceId::new(SourceId::BARS),
            Self::Quote(_) => SourceId::new(SourceId::QUOTES),
            Self::News(item) => Ok(item.source.clone()),
            Self::Filing(item) => Ok(item.source.clone()),
            Self::Instrument(_) => SourceId::new(SourceId::INSTRUMENTS),
        }
    }

    /// The one instrument the item is about, where there is exactly one.
    pub fn symbol(&self) -> Option<&Symbol> {
        match self {
            Self::Bar(bar) => Some(&bar.symbol),
            Self::Quote(quote) => Some(&quote.symbol),
            Self::Filing(item) => Some(&item.symbol),
            Self::Instrument(fact) => Some(&fact.symbol),
            Self::News(item) if item.symbols.len() == 1 => item.symbols.first(),
            Self::News(_) => None,
        }
    }

    /// The instant the platform knew the item (data-plane spec §5.2). Reference data carries none,
    /// so an instrument fact reports the epoch, which no window ever reads.
    pub fn knowledge_at(&self) -> UtcNanos {
        match self {
            Self::Bar(bar) => bar.knowledge_at,
            Self::Quote(quote) => quote.knowledge_at,
            Self::News(item) => item.knowledge_at,
            Self::Filing(item) => item.knowledge_at,
            Self::Instrument(_) => UtcNanos::EPOCH,
        }
    }

    /// The text's length in bytes, which E17-5's detector folds; 0 for data that is not text.
    pub fn length_bytes(&self) -> u64 {
        let length = match self {
            Self::News(item) => item.headline.len().saturating_add(item.summary.len()),
            Self::Filing(item) => item.excerpt.len(),
            Self::Bar(_) | Self::Quote(_) | Self::Instrument(_) => 0,
        };
        u64::try_from(length).unwrap_or(u64::MAX)
    }

    /// The source's own hash of the content, for news and filings.
    pub fn content_hash(&self) -> Option<Digest> {
        match self {
            Self::News(item) => Some(item.content_hash),
            Self::Filing(item) => Some(item.content_hash),
            Self::Bar(_) | Self::Quote(_) | Self::Instrument(_) => None,
        }
    }

    /// The canonical JSON the shell stores as the item's artifact and the core journals by digest
    /// (journal spec §4). It holds the data itself for market data, reference data, and public
    /// filings, and never the text of a licensed item (§6.6).
    ///
    /// # Errors
    /// [`ResearchRunError`] when a member cannot be written canonically.
    pub fn record(&self) -> Result<Vec<u8>, ResearchRunError> {
        let item_id = self.item_id()?;
        let source = self.source()?;
        let mut members = vec![
            ("kind", text(self.kind().as_str())),
            ("item_id", text(item_id.as_str())),
            ("source", text(source.as_str())),
        ];
        match self {
            Self::Bar(bar) => members.extend([
                ("symbol", text(bar.symbol.as_str())),
                ("knowledge_at", text(&bar.knowledge_at.to_string())),
                ("open", text(&bar.open.to_string())),
                ("high", text(&bar.high.to_string())),
                ("low", text(&bar.low.to_string())),
                ("close", text(&bar.close.to_string())),
                ("volume", text(&bar.volume.to_string())),
            ]),
            Self::News(item) => {
                let licensed = |value: &str| {
                    if item.licensed {
                        Value::Null
                    } else {
                        text(value)
                    }
                };
                members.extend([
                    (
                        "symbols",
                        Value::Array(item.symbols.iter().map(|s| text(s.as_str())).collect()),
                    ),
                    ("knowledge_at", text(&item.knowledge_at.to_string())),
                    ("published_at", text(&item.published_at.to_string())),
                    ("headline", licensed(&item.headline)),
                    ("summary", licensed(&item.summary)),
                    ("link", licensed(&item.link)),
                    ("content_hash", digest(&item.content_hash)),
                    ("length_bytes", int(self.length_bytes())?),
                    ("licensed", Value::Bool(item.licensed)),
                ]);
            }
            Self::Filing(item) => members.extend([
                ("symbol", text(item.symbol.as_str())),
                ("form", text(&item.form)),
                ("knowledge_at", text(&item.knowledge_at.to_string())),
                ("filed_at", text(&item.filed_at.to_string())),
                (
                    "excerpt",
                    if item.licensed {
                        Value::Null
                    } else {
                        text(&item.excerpt)
                    },
                ),
                ("content_hash", digest(&item.content_hash)),
                ("length_bytes", int(self.length_bytes())?),
                ("licensed", Value::Bool(item.licensed)),
            ]),
            Self::Quote(quote) => {
                let price = |side: Option<Price>| {
                    side.map_or(Value::Null, |price| text(&price.to_string()))
                };
                members.extend([
                    ("symbol", text(quote.symbol.as_str())),
                    ("knowledge_at", text(&quote.knowledge_at.to_string())),
                    ("bid", price(quote.bid)),
                    ("bid_size", text(&quote.bid_size.to_string())),
                    ("ask", price(quote.ask)),
                    ("ask_size", text(&quote.ask_size.to_string())),
                ]);
            }
            Self::Instrument(fact) => members.extend([
                ("asset_id", text(fact.asset_id.as_str())),
                ("symbol", text(fact.symbol.as_str())),
                ("asset_class", text(fact.asset_class.as_str())),
                ("eligible", Value::Bool(fact.eligible)),
                ("group", fact.group.as_deref().map_or(Value::Null, text)),
                (
                    "leveraged_or_inverse_etp",
                    Value::Bool(fact.leveraged_or_inverse_etp),
                ),
            ]),
        }
        let mut object = Object::new();
        for (name, value) in members {
            let key = Key::new(name).map_err(|_| ResearchRunError::InvalidId { what: "member" })?;
            object.insert(key, value);
        }
        Ok(to_canonical(&Value::Object(object)))
    }

    /// The record's SHA-256, the observation's `data_ref`.
    ///
    /// # Errors
    /// As [`Self::record`].
    pub fn data_ref(&self) -> Result<Digest, ResearchRunError> {
        self.record().map(|bytes| Digest::of(&bytes))
    }
}

pub(crate) fn text(value: &str) -> Value {
    Value::Str(value.to_owned())
}

pub(crate) fn digest(value: &Digest) -> Value {
    Value::Str(format!("sha256:{value}"))
}

pub(crate) fn int(value: u64) -> Result<Value, ResearchRunError> {
    Int::new(value)
        .map(Value::Int)
        .ok_or(ResearchRunError::InvalidId { what: "length" })
}

/// `cut_off − window_s`, the oldest knowledge time a read keeps.
pub(crate) fn window_start(cut_off: UtcNanos, window_s: u32) -> Result<UtcNanos, ResearchRunError> {
    cut_off
        .secs()
        .checked_sub(i64::from(window_s))
        .and_then(|secs| UtcNanos::from_parts(secs, cut_off.nanos()).ok())
        .ok_or(ResearchRunError::TimeOutOfRange)
}

/// The longest prefix of `text` at most `budget` bytes long that ends on a character boundary.
pub(crate) fn cut(text: &str, budget: usize) -> &str {
    let end = (0..=budget.min(text.len()))
        .rev()
        .find(|end| text.is_char_boundary(*end))
        .unwrap_or(0);
    text.get(..end).unwrap_or_default()
}

/// The longest link a news item carries into the prompt, and the longest filing form name.
const LINK_BYTES: usize = 512;
const FORM_BYTES: usize = 32;

/// The id prefixes the platform derives its own items' ids under (`RetrievedItem::item_id`).
const RESERVED_PREFIXES: [&str; 3] = ["bar:", "instrument:", "quote:"];

pub(crate) fn reserved(id: &ItemId) -> bool {
    RESERVED_PREFIXES
        .iter()
        .any(|prefix| id.as_str().starts_with(prefix))
}

/// What every text read shares: a window, an allowlist, a scope, a cap, and a byte bound.
pub(crate) struct TextRead<'a> {
    pub(crate) scope: &'a Scope,
    pub(crate) symbols: &'a BTreeSet<Symbol>,
    pub(crate) window_start: UtcNanos,
    pub(crate) max_items: u32,
    pub(crate) kind: ItemKind,
}

impl TextRead<'_> {
    /// The first filter an item fails, in a fixed order, or `None` when it is kept.
    pub(crate) fn refusal(
        &self,
        knowledge_at: UtcNanos,
        source: &SourceId,
        licensed: bool,
        about: &BTreeSet<Symbol>,
    ) -> Option<DropReason> {
        if knowledge_at > self.scope.cut_off {
            return Some(DropReason::AfterCutOff);
        }
        if knowledge_at < self.window_start {
            return Some(DropReason::BeforeWindow);
        }
        if !self.scope.allowlist.sources.contains(source) {
            return Some(DropReason::NotAllowlisted);
        }
        if licensed {
            return Some(DropReason::Licensed);
        }
        if about.is_disjoint(self.symbols) {
            return Some(DropReason::OutsideScope);
        }
        None
    }

    /// Keeps the newest `max_items` of `kept`, dropping the oldest first.
    fn capped<T>(
        &self,
        mut kept: Vec<(UtcNanos, ItemId, T)>,
        drops: &mut Vec<Drop>,
    ) -> Vec<(UtcNanos, ItemId, T)> {
        kept.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
        let cap = usize::try_from(self.max_items).unwrap_or(usize::MAX);
        let over = kept.len().saturating_sub(cap);
        for (_, id, _) in kept.iter().take(over) {
            drops.push(Drop {
                item: id.clone(),
                reason: DropReason::OverCap,
                kind: self.kind,
            });
        }
        kept.split_off(over)
    }

    pub(crate) fn query(&self, symbols: &BTreeSet<Symbol>) -> TextQuery {
        TextQuery {
            workspace: self.scope.workspace.clone(),
            symbols: symbols.clone(),
            window_start: self.window_start,
            cut_off: self.scope.cut_off,
            allowlist_version: self.scope.allowlist.version,
            max_items: self.max_items,
        }
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
    plan: &RetrievalPlan,
    scope: &Scope,
    readers: Readers<'_>,
) -> Result<Retrieval, ResearchRunError> {
    let mut drops = Vec::new();
    let mut items = Vec::new();
    let mut covered: BTreeSet<Symbol> = scope.data_universe.clone();
    covered.extend(scope.held.iter().cloned());

    let mut instruments: BTreeMap<Symbol, InstrumentFact> = BTreeMap::new();
    for fact in readers
        .instruments
        .instruments(&covered)
        .unwrap_or_default()
    {
        let id = RetrievedItem::Instrument(fact.clone()).item_id()?;
        let reason = if !covered.contains(&fact.symbol) {
            Some(DropReason::OutsideScope)
        } else if instruments.contains_key(&fact.symbol) {
            Some(DropReason::Duplicate)
        } else {
            None
        };
        match reason {
            Some(reason) => drops.push(Drop {
                item: id,
                reason,
                kind: ItemKind::Instrument,
            }),
            None => {
                instruments.insert(fact.symbol.clone(), fact);
            }
        }
    }
    items.extend(instruments.into_values().map(RetrievedItem::Instrument));

    for symbol in &scope.data_universe {
        let answered = readers
            .bars
            .daily_bars(symbol, scope.cut_off, plan.bars_per_instrument)
            .unwrap_or_default();
        let mut by_time: BTreeMap<i64, Bar> = BTreeMap::new();
        for bar in answered {
            let id = RetrievedItem::Bar(bar.clone()).item_id()?;
            let reason = if bar.knowledge_at > scope.cut_off {
                Some(DropReason::AfterCutOff)
            } else if bar.symbol != *symbol {
                Some(DropReason::OutsideScope)
            } else if by_time.contains_key(&bar.knowledge_at.secs()) {
                Some(DropReason::Duplicate)
            } else {
                None
            };
            match reason {
                Some(reason) => drops.push(Drop {
                    item: id,
                    reason,
                    kind: ItemKind::Bar,
                }),
                None => {
                    by_time.insert(bar.knowledge_at.secs(), bar);
                }
            }
        }
        let cap = usize::try_from(plan.bars_per_instrument).unwrap_or(usize::MAX);
        let over = by_time.len().saturating_sub(cap);
        let mut kept = Vec::new();
        for (n, bar) in by_time.into_values().enumerate() {
            if n < over {
                drops.push(Drop {
                    item: RetrievedItem::Bar(bar).item_id()?,
                    reason: DropReason::OverCap,
                    kind: ItemKind::Bar,
                });
            } else {
                kept.push(RetrievedItem::Bar(bar));
            }
        }
        items.extend(kept);
    }

    let news_read = TextRead {
        scope,
        symbols: &scope.data_universe,
        window_start: window_start(scope.cut_off, plan.news_window_s)?,
        max_items: plan.news_max_items,
        kind: ItemKind::News,
    };
    let answered = readers
        .news
        .news(&news_read.query(&scope.data_universe))
        .unwrap_or_default();
    let mut seen = BTreeSet::new();
    let mut kept = Vec::new();
    for item in answered {
        let reason = news_read
            .refusal(
                item.knowledge_at,
                &item.source,
                item.licensed,
                &item.symbols,
            )
            .or_else(|| reserved(&item.item_id).then_some(DropReason::ReservedId))
            .or_else(|| {
                seen.contains(&item.item_id)
                    .then_some(DropReason::Duplicate)
            });
        match reason {
            Some(reason) => drops.push(Drop {
                item: item.item_id.clone(),
                reason,
                kind: ItemKind::News,
            }),
            None => {
                seen.insert(item.item_id.clone());
                kept.push((item.knowledge_at, item.item_id.clone(), item));
            }
        }
    }
    let budget = usize::try_from(plan.news_item_bytes).unwrap_or(usize::MAX);
    let mut news: Vec<(Symbol, UtcNanos, ItemId, NewsItem)> = Vec::new();
    for (knowledge_at, id, mut item) in news_read.capped(kept, &mut drops) {
        let over_budget = item.headline.len().saturating_add(item.summary.len()) > budget;
        if over_budget || item.link.len() > LINK_BYTES {
            let headline = cut(&item.headline, budget).to_owned();
            let left = budget.saturating_sub(headline.len());
            item.summary = cut(&item.summary, left).to_owned();
            item.headline = headline;
            item.link = cut(&item.link, LINK_BYTES).to_owned();
            drops.push(Drop {
                item: id.clone(),
                reason: DropReason::Trimmed,
                kind: ItemKind::News,
            });
        }
        let first = item
            .symbols
            .iter()
            .find(|symbol| scope.data_universe.contains(*symbol))
            .cloned()
            .ok_or(ResearchRunError::InvalidId { what: "symbol" })?;
        news.push((first, knowledge_at, id, item));
    }
    news.sort_by(|a, b| (&a.0, a.1, &a.2).cmp(&(&b.0, b.1, &b.2)));
    items.extend(
        news.into_iter()
            .map(|(_, _, _, item)| RetrievedItem::News(item)),
    );

    let filings_read = TextRead {
        scope,
        symbols: &covered,
        window_start: window_start(scope.cut_off, plan.filings_window_s)?,
        max_items: plan.filings_max_items,
        kind: ItemKind::Filing,
    };
    let answered = readers
        .filings
        .filings(&filings_read.query(&covered))
        .unwrap_or_default();
    let mut seen = BTreeSet::new();
    let mut kept = Vec::new();
    for item in answered {
        let reason = filings_read
            .refusal(
                item.knowledge_at,
                &item.source,
                item.licensed,
                &BTreeSet::from([item.symbol.clone()]),
            )
            .or_else(|| reserved(&item.item_id).then_some(DropReason::ReservedId))
            .or_else(|| {
                seen.contains(&item.item_id)
                    .then_some(DropReason::Duplicate)
            });
        match reason {
            Some(reason) => drops.push(Drop {
                item: item.item_id.clone(),
                reason,
                kind: ItemKind::Filing,
            }),
            None => {
                seen.insert(item.item_id.clone());
                kept.push((item.knowledge_at, item.item_id.clone(), item));
            }
        }
    }
    let budget = usize::try_from(plan.filing_excerpt_bytes).unwrap_or(usize::MAX);
    let mut filings: Vec<(Symbol, UtcNanos, ItemId, Filing)> = Vec::new();
    for (knowledge_at, id, mut item) in filings_read.capped(kept, &mut drops) {
        if item.excerpt.len() > budget || item.form.len() > FORM_BYTES {
            item.excerpt = cut(&item.excerpt, budget).to_owned();
            item.form = cut(&item.form, FORM_BYTES).to_owned();
            drops.push(Drop {
                item: id.clone(),
                reason: DropReason::Trimmed,
                kind: ItemKind::Filing,
            });
        }
        filings.push((item.symbol.clone(), knowledge_at, id, item));
    }
    filings.sort_by(|a, b| (&a.0, a.1, &a.2).cmp(&(&b.0, b.1, &b.2)));
    items.extend(
        filings
            .into_iter()
            .map(|(_, _, _, item)| RetrievedItem::Filing(item)),
    );

    drops.sort();
    Ok(Retrieval {
        cut_off: scope.cut_off,
        items,
        drops,
    })
}
