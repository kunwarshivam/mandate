//! Running a reply's reads and thinks ([agent harness spec](../../../docs/specs/agent-harness.md)
//! v0.2 §6.10 and §6.11; ADR-0005 slice 3; DEC-2203, DEC-2209): the worker's half of a dispatch.
//!
//! The core has already ordered the reply, screened every read against the run's frozen scope, and
//! applied the in-batch bounds ([`crate::agentic::RunState::on_reply`]). What it hands on is run
//! here, one call at a time in reply order, over the read-only ports: each read takes its scope from
//! the harness, every answer is filtered on knowledge time again (HI-13), a harness cap drops the
//! oldest items first and sets `truncated`, and a refused or failed call returns `{"error": code}`
//! and nothing else. A read queries the store and never causes a fetch (HI-28).
//!
//! Like the rest of the crate this is pure: no clock and no I/O of its own. `tool_timeout_s` is the
//! shell's to enforce on the wall clock; a reader past it answers [`crate::ReadError::Unavailable`].

use std::collections::BTreeSet;

use mandate_canon::{Key, Object, Value, to_canonical};
use mandate_num::Price;

use crate::agentic::{Code, LoopBounds, Tool, ToolCall, ToolCallId, ToolClass, ToolOutcome};
use crate::error::ResearchRunError;
use crate::item::{Bar, ItemId, ItemKind, NewsItem, Quote, Symbol};
use crate::plan::Scope;
use crate::ports::{FilingEntry, FilingText, NewsText, ReadError, Readers};
use crate::retrieve::{RetrievedItem, TextRead, cut, digest, int, reserved, text, window_start};

/// What a dispatch reads within: the run's frozen scope, its pinned bounds, and the result bytes the
/// run has left (`max_run_result_bytes` less every earlier read's result).
#[derive(Debug, Clone, Copy)]
pub struct DispatchScope<'a> {
    pub scope: &'a Scope,
    pub bounds: &'a LoopBounds,
    pub result_bytes_left: u32,
}

/// One call's result, as the worker hands it back and `ToolResultRecorded` journals it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dispatched {
    pub call: ToolCallId,
    pub outcome: ToolOutcome,
    pub code: Option<Code>,
    /// The result as the model is given it: canonical JSON, stored as the record's `content_ref`.
    pub content: Vec<u8>,
    pub truncated: bool,
    /// The result holds third-party text, so an action written after it names it as provenance.
    pub untrusted: bool,
    /// Every item the result holds, in the result's order, each as it will be observed.
    pub items: Vec<RetrievedItem>,
}

/// A dispatch's results, in the order of the calls, and the run's result bytes left after them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dispatch {
    pub results: Vec<Dispatched>,
    pub result_bytes_left: u32,
}

/// Runs `calls`, the reads and thinks of turn `turn`'s reply the core dispatched, in their order.
///
/// # Errors
/// [`ResearchRunError::LoopOrder`] for an action, which the core mediates and never dispatches;
/// [`ResearchRunError`] when a result cannot be written canonically.
pub fn dispatch(
    turn: u32,
    calls: &[ToolCall],
    within: DispatchScope<'_>,
    readers: Readers<'_>,
) -> Result<Dispatch, ResearchRunError> {
    if calls
        .iter()
        .any(|call| call.tool.class() == ToolClass::Action)
    {
        return Err(ResearchRunError::LoopOrder {
            what: "an action is mediated, never dispatched",
        });
    }
    let mut left = within.result_bytes_left;
    let mut results = Vec::with_capacity(calls.len());
    for call in calls {
        let id = ToolCallId {
            turn,
            index: call.index,
        };
        let result = match call.tool.class() {
            ToolClass::Action | ToolClass::Think => Dispatched {
                call: id,
                outcome: ToolOutcome::Ok,
                code: None,
                content: b"{}".to_vec(),
                truncated: false,
                untrusted: false,
                items: Vec::new(),
            },
            ToolClass::Read => {
                let budget = within.bounds.max_result_bytes.min(left);
                let result = match read(call, within.scope, readers)? {
                    Answer::Read(read) => read.fitted(id, budget)?,
                    Answer::Refused(code) => error(id, ToolOutcome::Refused, code),
                    Answer::Failed => error(id, ToolOutcome::Failed, Code::Unavailable),
                };
                let spent = u32::try_from(result.content.len()).unwrap_or(u32::MAX);
                left = left.saturating_sub(spent);
                result
            }
        };
        results.push(result);
    }
    Ok(Dispatch {
        results,
        result_bytes_left: left,
    })
}

/// What a read came to: what it found, a refusal, or nothing read.
enum Answer {
    Read(Read),
    Refused(Code),
    Failed,
}

/// What a read found, newest first, before the byte caps.
struct Read {
    items: Vec<RetrievedItem>,
    trimmed: bool,
    untrusted: bool,
    /// `news.get`'s stored text, shown in place of its one item's summary; the item observed stays
    /// the news item's own record (DEC-2213 item 6).
    stored: Option<String>,
    /// `filings.search`'s stored lengths, one per item in `items`' order, shown beside each filing
    /// and never observed (DEC-2250 item 5).
    stored_bytes: Vec<u64>,
    /// `filings.get`'s range: where its text starts in the filing's stored text, shown with
    /// `stored` in place of the excerpt (DEC-2252 item 3).
    range_offset: Option<u64>,
}

impl Read {
    /// The result with as many of the newest items as fit in `budget` bytes.
    fn fitted(self, call: ToolCallId, budget: u32) -> Result<Dispatched, ResearchRunError> {
        let budget = usize::try_from(budget).unwrap_or(usize::MAX);
        let mut kept = self.items.len();
        let mut truncated = self.trimmed;
        loop {
            let shown = self.items.get(..kept).unwrap_or_default();
            let content = render(
                shown,
                truncated,
                self.stored.as_deref(),
                &self.stored_bytes,
                self.range_offset,
            )?;
            if content.len() <= budget || kept == 0 {
                let mut items = self.items;
                items.truncate(kept);
                return Ok(Dispatched {
                    call,
                    outcome: ToolOutcome::Ok,
                    code: None,
                    content,
                    truncated,
                    untrusted: self.untrusted,
                    items,
                });
            }
            kept = kept.saturating_sub(1);
            truncated = true;
        }
    }
}

/// A refused or failed call's result: `{"error": code}` and nothing else (§6.10).
fn error(call: ToolCallId, outcome: ToolOutcome, code: Code) -> Dispatched {
    let content = format!("{{\"error\":\"{}\"}}", code.as_str()).into_bytes();
    Dispatched {
        call,
        outcome,
        code: Some(code),
        content,
        truncated: false,
        untrusted: false,
        items: Vec::new(),
    }
}

/// One read. Nothing is read for arguments outside the pinned schema, a reader that did not
/// answer, or a read this slice does not serve yet (DEC-2209 items 5 and 6), and a symbol outside
/// the run's data universe refuses the call whole before any reader is asked (item 10).
fn read(call: &ToolCall, scope: &Scope, readers: Readers<'_>) -> Result<Answer, ResearchRunError> {
    if call.tool == Tool::NewsGet {
        return news_get(call, scope, readers);
    }
    if call.tool == Tool::NewsSearch {
        return news_search(call, scope, readers);
    }
    if call.tool == Tool::FilingsSearch {
        return filings_search(call, scope, readers);
    }
    if call.tool == Tool::FilingsGet {
        return filings_get(call, scope, readers);
    }
    if call.tool == Tool::MarketQuote {
        return quotes(call, scope, readers);
    }
    let Ok(arguments) = mandate_canon::parse(call.arguments.as_bytes()) else {
        return Ok(Answer::Failed);
    };
    match call.tool {
        Tool::MarketBars => Ok(bars(&arguments, scope, readers)),
        _ => Ok(Answer::Failed),
    }
}

/// The most bytes of headline and summary one `news.search` item carries (§6.10).
const NEWS_ITEM_BYTES: usize = 2_000;

/// `news.search` (§6.10, DEC-2213).
fn news_search(
    call: &ToolCall,
    scope: &Scope,
    readers: Readers<'_>,
) -> Result<Answer, ResearchRunError> {
    let Ok(arguments) = mandate_canon::parse(call.arguments.as_bytes()) else {
        return Ok(Answer::Failed);
    };

    let parsed = (|| {
        let symbols = symbols(arguments.get("symbols")?, 5)?;
        let window_h = bounded(arguments.get("window_h")?, 1, 168)?;
        let terms = terms(arguments.get("terms")?)?;
        let limit = bounded(arguments.get("limit")?, 1, 20)?;
        Some((symbols, window_h, terms, limit))
    })();
    let Some((symbols, window_h, terms, limit)) = parsed else {
        return Ok(Answer::Failed);
    };
    if outside(&symbols, scope) {
        return Ok(Answer::Refused(Code::OutOfUniverse));
    }
    let read = TextRead {
        scope,
        symbols: &symbols,
        window_start: window_start(scope.cut_off, window_h.saturating_mul(3_600))?,
        max_items: limit,
        kind: ItemKind::News,
    };
    let Ok(answered) = readers.news.search(&read.query(&symbols), &terms) else {
        return Ok(Answer::Failed);
    };
    let mut seen = BTreeSet::new();
    let mut kept: Vec<NewsItem> = answered
        .into_iter()
        .filter(|item| {
            read.refusal(
                item.knowledge_at,
                &item.source,
                item.licensed,
                &item.symbols,
            )
            .is_none()
                && !reserved(&item.item_id)
                && seen.insert(item.item_id.clone())
        })
        .collect();
    kept.sort_by(|a, b| {
        b.knowledge_at
            .cmp(&a.knowledge_at)
            .then_with(|| a.item_id.cmp(&b.item_id))
    });
    kept.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    let mut trimmed = false;
    for item in &mut kept {
        if item.headline.len().saturating_add(item.summary.len()) > NEWS_ITEM_BYTES {
            let headline = cut(&item.headline, NEWS_ITEM_BYTES).to_owned();
            let left = NEWS_ITEM_BYTES.saturating_sub(headline.len());
            item.summary = cut(&item.summary, left).to_owned();
            item.headline = headline;
            trimmed = true;
        }
    }
    Ok(Answer::Read(Read {
        items: kept.into_iter().map(RetrievedItem::News).collect(),
        trimmed,
        untrusted: true,
        stored: None,
        stored_bytes: Vec::new(),
        range_offset: None,
    }))
}

fn symbols(value: &Value, most: usize) -> Option<BTreeSet<Symbol>> {
    let listed = value.as_array()?;
    if listed.is_empty() || listed.len() > most {
        return None;
    }
    listed.iter().map(symbol).collect()
}

/// §6.10's terms: at most five, each 1 to 32 bytes of ASCII letters, digits, spaces, hyphens, and
/// periods (DEC-2213 item 1).
fn terms(value: &Value) -> Option<Vec<String>> {
    let listed = value.as_array()?;
    if listed.len() > 5 {
        return None;
    }
    listed
        .iter()
        .map(|term| {
            let term = term.as_str()?;
            let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '.');
            ((1..=32).contains(&term.len()) && term.chars().all(allowed)).then(|| term.to_owned())
        })
        .collect()
}

/// The most bytes of stored text one `news.get` result carries (§6.10).
const NEWS_TEXT_BYTES: usize = 8_000;

/// `news.get` (§6.10, DEC-2213 items 6 to 8): one received item's stored text, read with the item
/// id, the run's cut-off, and its allowlist version and nothing else. An answer for another item,
/// one after the cut-off, one off the allowlist, or licensed text reads nothing (item 7).
fn news_get(
    call: &ToolCall,
    scope: &Scope,
    readers: Readers<'_>,
) -> Result<Answer, ResearchRunError> {
    let Ok(arguments) = mandate_canon::parse(call.arguments.as_bytes()) else {
        return Ok(Answer::Failed);
    };
    let Some(asked) = arguments
        .get("item")
        .and_then(Value::as_str)
        .and_then(|id| ItemId::new(id).ok())
    else {
        return Ok(Answer::Failed);
    };
    let Ok(answered) = readers
        .news
        .news_text(&asked, scope.cut_off, scope.allowlist.version)
    else {
        return Ok(Answer::Failed);
    };
    let NewsText { item, text } = answered;
    let leaks = item.item_id != asked
        || item.knowledge_at > scope.cut_off
        || !scope.allowlist.sources.contains(&item.source)
        || item.licensed;
    if leaks {
        return Ok(Answer::Failed);
    }
    let shown = cut(&text, NEWS_TEXT_BYTES);
    Ok(Answer::Read(Read {
        trimmed: shown.len() < text.len(),
        stored: Some(shown.to_owned()),
        items: vec![RetrievedItem::News(item)],
        untrusted: true,
        stored_bytes: Vec::new(),
        range_offset: None,
    }))
}

/// `filings.search` (§6.10, DEC-2250 items 1 to 5).
fn filings_search(
    call: &ToolCall,
    scope: &Scope,
    readers: Readers<'_>,
) -> Result<Answer, ResearchRunError> {
    let Ok(arguments) = mandate_canon::parse(call.arguments.as_bytes()) else {
        return Ok(Answer::Failed);
    };
    let parsed = (|| {
        let symbols = symbols(arguments.get("symbols")?, 5)?;
        let forms = forms(arguments.get("forms")?)?;
        let window_d = bounded(arguments.get("window_d")?, 1, 90)?;
        let terms = terms(arguments.get("terms")?)?;
        let limit = bounded(arguments.get("limit")?, 1, 10)?;
        Some((symbols, forms, window_d, terms, limit))
    })();
    let Some((symbols, forms, window_d, terms, limit)) = parsed else {
        return Ok(Answer::Failed);
    };
    if outside(&symbols, scope) {
        return Ok(Answer::Refused(Code::OutOfUniverse));
    }
    let read = TextRead {
        scope,
        symbols: &symbols,
        window_start: window_start(scope.cut_off, window_d.saturating_mul(86_400))?,
        max_items: limit,
        kind: ItemKind::Filing,
    };
    let Ok(answered) = readers
        .filings
        .search(&read.query(&symbols), &forms, &terms)
    else {
        return Ok(Answer::Failed);
    };
    let mut seen = BTreeSet::new();
    let mut kept: Vec<FilingEntry> = answered
        .into_iter()
        .filter(|entry| {
            let filing = &entry.filing;
            read.refusal(
                filing.knowledge_at,
                &filing.source,
                filing.licensed,
                &BTreeSet::from([filing.symbol.clone()]),
            )
            .is_none()
                && forms.contains(&filing.form)
                && !reserved(&filing.item_id)
                && seen.insert(filing.item_id.clone())
        })
        .collect();
    kept.sort_by(|a, b| {
        b.filing
            .knowledge_at
            .cmp(&a.filing.knowledge_at)
            .then_with(|| a.filing.item_id.cmp(&b.filing.item_id))
    });
    kept.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    let mut trimmed = false;
    for entry in &mut kept {
        if entry.filing.excerpt.len() > FILING_EXCERPT_BYTES {
            entry.filing.excerpt = cut(&entry.filing.excerpt, FILING_EXCERPT_BYTES).to_owned();
            trimmed = true;
        }
    }
    Ok(Answer::Read(Read {
        stored_bytes: kept.iter().map(|entry| entry.stored_bytes).collect(),
        range_offset: None,
        items: kept
            .into_iter()
            .map(|entry| RetrievedItem::Filing(entry.filing))
            .collect(),
        trimmed,
        untrusted: true,
        stored: None,
    }))
}

/// `filings.get` (§6.10, DEC-2250 items 6 to 9, DEC-2252 items 2 to 4).
fn filings_get(
    call: &ToolCall,
    scope: &Scope,
    readers: Readers<'_>,
) -> Result<Answer, ResearchRunError> {
    let Ok(arguments) = mandate_canon::parse(call.arguments.as_bytes()) else {
        return Ok(Answer::Failed);
    };
    let parsed = (|| {
        let asked = ItemId::new(arguments.get("item")?.as_str()?).ok()?;
        let Value::Int(offset) = arguments.get("offset")? else {
            return None;
        };
        let length = bounded(arguments.get("length")?, 1, 8_000)?;
        Some((asked, offset.get(), length))
    })();
    let Some((asked, offset, length)) = parsed else {
        return Ok(Answer::Failed);
    };
    let answered = match readers.filings.filing_text(
        &asked,
        scope.cut_off,
        scope.allowlist.version,
        offset,
        length,
    ) {
        Ok(answered) => answered,
        Err(ReadError::OutOfRange) => return Ok(Answer::Refused(Code::UnknownItem)),
        Err(_) => return Ok(Answer::Failed),
    };
    let FilingText {
        filing,
        stored_bytes,
        offset: answered_at,
        text,
    } = answered;
    let leaks = filing.item_id != asked
        || filing.knowledge_at > scope.cut_off
        || !scope.allowlist.sources.contains(&filing.source)
        || filing.licensed
        || answered_at != offset
        || text.len() > usize::try_from(length).unwrap_or(usize::MAX);
    if leaks {
        return Ok(Answer::Failed);
    }
    Ok(Answer::Read(Read {
        items: vec![RetrievedItem::Filing(filing)],
        trimmed: false,
        untrusted: true,
        stored: Some(text),
        stored_bytes: vec![stored_bytes],
        range_offset: Some(offset),
    }))
}

/// The forms `filings.search` may name, in their order (DEC-2250 item 1).
pub(crate) const FILING_FORMS: [&str; 6] = ["10-K", "10-K/A", "10-Q", "10-Q/A", "8-K", "8-K/A"];

/// The most bytes of excerpt one `filings.search` entry carries (§6.10).
const FILING_EXCERPT_BYTES: usize = 4_000;

/// §6.10's forms: at most five, each once and from [`FILING_FORMS`]; the forms the reader is asked
/// for, in the list's order, and the whole list when none is named (DEC-2250 items 1 and 2).
fn forms(value: &Value) -> Option<Vec<String>> {
    let listed = value.as_array()?;
    if listed.len() > 5 {
        return None;
    }
    let mut named = BTreeSet::new();
    for form in listed {
        let form = form.as_str()?;
        if !FILING_FORMS.contains(&form) || !named.insert(form) {
            return None;
        }
    }
    let asked = FILING_FORMS
        .iter()
        .filter(|form| named.is_empty() || named.contains(*form))
        .map(|form| (*form).to_owned())
        .collect();
    Some(asked)
}

/// `market.quote` (§6.10, DEC-2255).
fn quotes(
    call: &ToolCall,
    scope: &Scope,
    readers: Readers<'_>,
) -> Result<Answer, ResearchRunError> {
    let Ok(arguments) = mandate_canon::parse(call.arguments.as_bytes()) else {
        return Ok(Answer::Failed);
    };
    let Some(symbols) = arguments
        .get("symbols")
        .and_then(|listed| symbols(listed, 10))
    else {
        return Ok(Answer::Failed);
    };
    if outside(&symbols, scope) {
        return Ok(Answer::Refused(Code::OutOfUniverse));
    }
    let mut kept = Vec::with_capacity(symbols.len());
    for symbol in &symbols {
        match readers.bars.last_quote(symbol, scope.cut_off) {
            Ok(Some(quote)) => {
                if quote.symbol == *symbol && quote.knowledge_at <= scope.cut_off {
                    kept.push(RetrievedItem::Quote(quote));
                }
            }
            Ok(None) => {}
            Err(_) => return Ok(Answer::Failed),
        }
    }
    Ok(Answer::Read(Read {
        items: kept,
        trimmed: false,
        untrusted: false,
        stored: None,
        stored_bytes: Vec::new(),
        range_offset: None,
    }))
}

/// DEC-2255 item 5's structural sanity: both sides priced, and the bid at or below the ask.
fn sane(quote: &Quote) -> bool {
    matches!((quote.bid, quote.ask), (Some(bid), Some(ask)) if bid <= ask)
}

fn outside(symbols: &BTreeSet<Symbol>, scope: &Scope) -> bool {
    !symbols.is_subset(&scope.data_universe)
}

fn bars(arguments: &Value, scope: &Scope, readers: Readers<'_>) -> Answer {
    let parsed = arguments.get("symbol").and_then(symbol).zip(
        arguments
            .get("count")
            .and_then(|count| bounded(count, 1, 260)),
    );
    let Some((symbol, count)) = parsed else {
        return Answer::Failed;
    };
    if outside(&BTreeSet::from([symbol.clone()]), scope) {
        return Answer::Refused(Code::OutOfUniverse);
    }
    let Ok(answered) = readers.bars.daily_bars(&symbol, scope.cut_off, count) else {
        return Answer::Failed;
    };
    let mut seconds = BTreeSet::new();
    let mut kept: Vec<Bar> = answered
        .into_iter()
        .filter(|bar| bar.symbol == symbol && bar.knowledge_at <= scope.cut_off)
        .filter(|bar| seconds.insert(bar.knowledge_at.secs()))
        .collect();
    kept.sort_by_key(|bar| std::cmp::Reverse(bar.knowledge_at));
    kept.truncate(usize::try_from(count).unwrap_or(usize::MAX));
    Answer::Read(Read {
        items: kept.into_iter().map(RetrievedItem::Bar).collect(),
        trimmed: false,
        untrusted: false,
        stored: None,
        stored_bytes: Vec::new(),
        range_offset: None,
    })
}

fn bounded(value: &Value, low: u64, high: u64) -> Option<u32> {
    let Value::Int(number) = value else {
        return None;
    };
    let number = number.get();
    (low..=high)
        .contains(&number)
        .then(|| u32::try_from(number).ok())
        .flatten()
}

fn symbol(value: &Value) -> Option<Symbol> {
    Symbol::new(value.as_str()?).ok()
}

/// `{"items": [...], "truncated": bool}`, each item with its ids beside its fields, which are
/// wrapped as untrusted data whatever their source (§6.10, DEC-2209 item 4).
fn render(
    items: &[RetrievedItem],
    truncated: bool,
    stored: Option<&str>,
    stored_bytes: &[u64],
    range_offset: Option<u64>,
) -> Result<Vec<u8>, ResearchRunError> {
    let mut shown = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let (section, fields) = match item {
            RetrievedItem::Bar(bar) => (
                "untrusted",
                vec![
                    ("symbol", text(bar.symbol.as_str())),
                    ("knowledge_at", text(&bar.knowledge_at.to_string())),
                    ("open", text(&bar.open.to_string())),
                    ("high", text(&bar.high.to_string())),
                    ("low", text(&bar.low.to_string())),
                    ("close", text(&bar.close.to_string())),
                    ("volume", text(&bar.volume.to_string())),
                ],
            ),
            RetrievedItem::News(news) => (
                "untrusted",
                vec![
                    ("published_at", text(&news.published_at.to_string())),
                    ("headline", text(&news.headline)),
                    match stored {
                        Some(stored) => ("text", text(stored)),
                        None => ("summary", text(&news.summary)),
                    },
                    ("content_hash", digest(&news.content_hash)),
                    ("length_bytes", int(item.length_bytes())?),
                ],
            ),
            RetrievedItem::Filing(filing) => {
                let length = stored_bytes.get(index).ok_or(ResearchRunError::LoopOrder {
                    what: "a filing shown without its stored length",
                })?;
                let mut fields = vec![
                    ("form", text(&filing.form)),
                    ("filed_at", text(&filing.filed_at.to_string())),
                    ("content_hash", digest(&filing.content_hash)),
                    ("length_bytes", int(item.length_bytes())?),
                    ("stored_bytes", int(*length)?),
                ];
                match (stored, range_offset) {
                    (Some(range), Some(offset)) => {
                        fields.extend([("offset", int(offset)?), ("text", text(range))]);
                    }
                    (None, None) => fields.push(("excerpt", text(&filing.excerpt))),
                    _ => {
                        return Err(ResearchRunError::LoopOrder {
                            what: "a filing range shown without its offset or text",
                        });
                    }
                }
                ("untrusted", fields)
            }
            RetrievedItem::Quote(quote) => {
                let price = |side: Option<Price>| {
                    side.map_or(Value::Null, |price| text(&price.to_string()))
                };
                (
                    "untrusted",
                    vec![
                        ("symbol", text(quote.symbol.as_str())),
                        ("quoted_at", text(&quote.knowledge_at.to_string())),
                        ("bid", price(quote.bid)),
                        ("bid_size", text(&quote.bid_size.to_string())),
                        ("ask", price(quote.ask)),
                        ("ask_size", text(&quote.ask_size.to_string())),
                        ("sane", Value::Bool(sane(quote))),
                    ],
                )
            }
            RetrievedItem::Instrument(_) => {
                return Err(ResearchRunError::LoopOrder {
                    what: "an item no dispatched read returns",
                });
            }
        };
        shown.push(object(vec![
            ("item_id", text(item.item_id()?.as_str())),
            ("source", text(item.source()?.as_str())),
            (section, object(fields)?),
        ])?);
    }
    let result = object(vec![
        ("items", Value::Array(shown)),
        ("truncated", Value::Bool(truncated)),
    ])?;
    Ok(to_canonical(&result))
}

fn object(members: Vec<(&str, Value)>) -> Result<Value, ResearchRunError> {
    let mut object = Object::new();
    for (name, value) in members {
        let key = Key::new(name).map_err(|_| ResearchRunError::InvalidId { what: "member" })?;
        object.insert(key, value);
    }
    Ok(Value::Object(object))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use mandate_num::{Price, Qty};
    use mandate_time::UtcNanos;

    use super::*;
    use crate::item::{Allowlist, Bar, Filing, InstrumentFact, NewsItem};
    use crate::ports::{
        BarReader, FilingReader, InstrumentReader, NewsReader, ReadError, TextQuery,
    };

    /// Answers the two bars it holds, in its order, whatever it is asked.
    struct InOrder(Vec<Bar>);

    impl BarReader for InOrder {
        fn daily_bars(&self, _: &Symbol, _: UtcNanos, _: u32) -> Result<Vec<Bar>, ReadError> {
            Ok(self.0.clone())
        }
    }

    impl NewsReader for InOrder {
        fn news(&self, _: &TextQuery) -> Result<Vec<NewsItem>, ReadError> {
            Ok(Vec::new())
        }
    }

    impl FilingReader for InOrder {
        fn filings(&self, _: &TextQuery) -> Result<Vec<Filing>, ReadError> {
            Ok(Vec::new())
        }
    }

    impl InstrumentReader for InOrder {
        fn instruments(&self, _: &BTreeSet<Symbol>) -> Result<Vec<InstrumentFact>, ReadError> {
            Ok(Vec::new())
        }
    }

    const CUT_OFF: i64 = 1_792_180_800;

    fn bar(nanos: u32, close: &str) -> Result<Bar, ResearchRunError> {
        let price =
            Price::parse(close).map_err(|_| ResearchRunError::InvalidId { what: "price" })?;
        Ok(Bar {
            symbol: Symbol::new("AAPL")?,
            knowledge_at: UtcNanos::from_parts(CUT_OFF - 86_400, nanos)
                .map_err(|_| ResearchRunError::TimeOutOfRange)?,
            open: price,
            high: price,
            low: price,
            close: price,
            volume: Qty::parse("1000").map_err(|_| ResearchRunError::InvalidId { what: "qty" })?,
        })
    }

    /// The close of the one bar a dispatch keeps when the reader answers `answers` in that order.
    fn kept(answers: Vec<Bar>) -> Result<Vec<Price>, ResearchRunError> {
        let scope = Scope {
            workspace: "ws1".to_owned(),
            agent: "agent-a".to_owned(),
            cut_off: UtcNanos::from_parts(CUT_OFF, 0)
                .map_err(|_| ResearchRunError::TimeOutOfRange)?,
            data_universe: BTreeSet::from([Symbol::new("AAPL")?]),
            held: BTreeSet::new(),
            allowlist: Allowlist {
                version: 3,
                sources: BTreeSet::new(),
            },
        };
        let reader = InOrder(answers);
        let readers = Readers {
            bars: &reader,
            news: &reader,
            filings: &reader,
            instruments: &reader,
        };
        let bounds = LoopBounds::v0_2();
        let within = DispatchScope {
            scope: &scope,
            bounds: &bounds,
            result_bytes_left: 96_000,
        };
        let call = ToolCall {
            index: 0,
            tool: Tool::MarketBars,
            arguments: r#"{"count":5,"symbol":"AAPL"}"#.to_owned(),
        };
        let got = dispatch(1, &[call], within, readers)?;
        Ok(got
            .results
            .iter()
            .flat_map(|result| result.items.iter())
            .filter_map(|item| match item {
                RetrievedItem::Bar(bar) => Some(bar.close),
                RetrievedItem::News(_)
                | RetrievedItem::Filing(_)
                | RetrievedItem::Instrument(_)
                | RetrievedItem::Quote(_) => None,
            })
            .collect())
    }

    /// DEC-2209 item 12: two answers in one second share one id, and the first answered is kept,
    /// as `retrieve` keeps it, whichever of the two is later within the second.
    #[test]
    fn two_answers_in_one_second_keep_the_first_answered() -> Result<(), ResearchRunError> {
        let (earlier, later) = (bar(100, "100")?, bar(900, "101")?);
        assert_eq!(
            kept(vec![earlier.clone(), later.clone()])?,
            vec![earlier.close]
        );
        assert_eq!(kept(vec![later.clone(), earlier])?, vec![later.close]);
        Ok(())
    }
}
