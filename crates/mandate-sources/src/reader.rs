//! The typed reads lane I2's research loop consumes (agent harness spec §6.3 and §10.2; data-plane
//! spec §4.6), through [`SourceReader`], and the E17-5 drift observation of each item.
//!
//! Every read takes its cut-off and the allowlist version from the harness, never from a model.
//! It returns only items from sources in effect in that version (DP-9) with knowledge time at or
//! before the cut-off (DP-1), at most the harness's caps of items and bytes, newest first, and says
//! how many items the cap dropped. A window or limit in the query can only narrow the caps.

use std::collections::BTreeSet;

use mandate_domain::AssetId;
use mandate_research::drift::{InputClass, InputObservation};
use mandate_research::{AllowlistVersion, SourceId};
use mandate_time::UtcNanos;

use crate::SourceError;
use crate::allowlist::{Allowlist, SourceKind};
use crate::record::{SourceRecord, TypedFields};
use crate::store::{ItemStore, StoreQuery};
use crate::text::UntrustedText;

/// `news`: 48 hours of knowledge time, 50 items, 2,000 bytes of text per item (§6.3).
pub const NEWS_WINDOW_S: u32 = 48 * 3_600;
pub const NEWS_LIMIT: u32 = 50;
pub const NEWS_ITEM_BYTES: usize = 2_000;
/// `filings`: 7 days of knowledge time, 20 entries, 4,000 bytes per excerpt (§6.3).
pub const FILINGS_WINDOW_S: u32 = 7 * 86_400;
pub const FILINGS_LIMIT: u32 = 20;
pub const FILING_EXCERPT_BYTES: usize = 4_000;

/// One read's scope, all of it from the harness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadQuery {
    /// The run's cut-off.
    pub as_of: UtcNanos,
    /// The allowlist version in effect; another version is refused.
    pub allowlist_version: AllowlistVersion,
    /// The data universe; an empty set reads nothing.
    pub instruments: BTreeSet<AssetId>,
    /// Narrows the read's window, never widens it.
    pub window_s: Option<u32>,
    /// Narrows the read's item cap, never widens it.
    pub limit: Option<u32>,
}

/// Whether licensed text goes out with a read. Until the inference spec's prompt record keeps
/// licensed text out of the artifact store, research reads none (agent harness spec §6.2's third
/// precondition, DEC-431 item 17; DEC-996 item 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LicensedText {
    Withheld,
    Included,
}

/// A news item as the model may see it: typed fields, and text only within the caps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewsItem {
    pub typed: TypedFields,
    pub licensed: bool,
    pub headline: Option<UntrustedText>,
    pub summary: Option<UntrustedText>,
    pub link: Option<UntrustedText>,
}

/// A filing entry as the model may see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilingEntry {
    pub typed: TypedFields,
    pub excerpt: Option<UntrustedText>,
}

/// One read's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadBatch<T> {
    pub items: Vec<T>,
    /// How many matching items the cap left out, oldest first (§6.3).
    pub dropped: u32,
}

/// The research loop's read of news and filings. Lane I2 builds against this trait; a recorded
/// fake is a [`StoreReader`] over stores filled from fixtures.
pub trait SourceReader {
    fn news(&self, query: &ReadQuery) -> Result<ReadBatch<NewsItem>, SourceError>;
    fn filings(&self, query: &ReadQuery) -> Result<ReadBatch<FilingEntry>, SourceError>;
}

/// [`SourceReader`] over a news store and a filings store.
#[derive(Debug, Clone, Copy)]
pub struct StoreReader<'a> {
    pub allowlist: &'a Allowlist,
    pub news: &'a ItemStore,
    pub filings: &'a ItemStore,
    pub licensed_text: LicensedText,
}

impl StoreReader<'_> {
    /// The as-of read of `store` under the harness caps, narrowed by the query.
    fn read<'s>(
        &'s self,
        store: &'s ItemStore,
        kind: SourceKind,
        query: &ReadQuery,
        caps: (u32, u32),
    ) -> Result<(Vec<&'s SourceRecord>, u32), SourceError> {
        if query.allowlist_version != self.allowlist.version() {
            return Err(SourceError::AllowlistVersionMismatch);
        }
        if store.kind() != kind {
            return Err(SourceError::WrongKind);
        }
        let (window_cap, limit_cap) = caps;
        let window = query.window_s.map_or(window_cap, |w| w.min(window_cap));
        let limit = query.limit.map_or(limit_cap, |l| l.min(limit_cap));
        let since = query
            .as_of
            .secs()
            .checked_sub(i64::from(window))
            .and_then(|secs| UtcNanos::from_parts(secs, query.as_of.nanos()).ok())
            .unwrap_or(UtcNanos::EPOCH);
        let sources: BTreeSet<SourceId> = self
            .allowlist
            .entries()
            .filter(|e| e.kind() == kind && self.allowlist.active(e.id()).is_some())
            .map(|e| e.id().clone())
            .collect();
        let answer = store.as_of(&StoreQuery {
            as_of: query.as_of,
            since,
            sources: &sources,
            instruments: &query.instruments,
            limit: usize::try_from(limit).map_err(|_| SourceError::Time)?,
        });
        let dropped = u32::try_from(answer.dropped).unwrap_or(u32::MAX);
        Ok((answer.records, dropped))
    }

    /// Whether `source`'s text goes out: never for a licensed source while it is withheld, and a
    /// source the allowlist does not describe counts as licensed.
    fn text_allowed(&self, source: &SourceId) -> (bool, bool) {
        let licensed = self.allowlist.active(source).is_none_or(|e| e.licensed());
        (
            licensed,
            !licensed || self.licensed_text == LicensedText::Included,
        )
    }
}

/// Cuts `raw` to what is left of `budget`, and takes its length from the budget.
fn spend(raw: Option<&UntrustedText>, budget: &mut usize) -> Option<UntrustedText> {
    let text = UntrustedText::bounded(raw?.as_untrusted_str(), *budget);
    *budget = budget.saturating_sub(text.len_bytes());
    Some(text)
}

impl SourceReader for StoreReader<'_> {
    fn news(&self, query: &ReadQuery) -> Result<ReadBatch<NewsItem>, SourceError> {
        let (records, dropped) = self.read(
            self.news,
            SourceKind::News,
            query,
            (NEWS_WINDOW_S, NEWS_LIMIT),
        )?;
        let items = records
            .into_iter()
            .map(|record| {
                let (licensed, shown) = self.text_allowed(&record.typed.source);
                let text = record.text();
                let mut budget = if shown { NEWS_ITEM_BYTES } else { 0 };
                let headline = spend(text.headline.as_ref(), &mut budget);
                let link = spend(text.link.as_ref(), &mut budget);
                let summary = spend(text.summary.as_ref(), &mut budget);
                let keep = |t: Option<UntrustedText>| t.filter(|_| shown);
                NewsItem {
                    typed: record.typed.clone(),
                    licensed,
                    headline: keep(headline),
                    summary: keep(summary),
                    link: keep(link),
                }
            })
            .collect();
        Ok(ReadBatch { items, dropped })
    }

    fn filings(&self, query: &ReadQuery) -> Result<ReadBatch<FilingEntry>, SourceError> {
        let (records, dropped) = self.read(
            self.filings,
            SourceKind::Filings,
            query,
            (FILINGS_WINDOW_S, FILINGS_LIMIT),
        )?;
        let items = records
            .into_iter()
            .map(|record| {
                let (_, shown) = self.text_allowed(&record.typed.source);
                let excerpt =
                    record.text().excerpt.as_ref().filter(|_| shown).map(|t| {
                        UntrustedText::bounded(t.as_untrusted_str(), FILING_EXCERPT_BYTES)
                    });
                FilingEntry {
                    typed: record.typed.clone(),
                    excerpt,
                }
            })
            .collect();
        Ok(ReadBatch { items, dropped })
    }
}

/// The typed per-item observation E17-5's drift detector folds, and the artifact the item's
/// `ObservationRecorded` names (data-plane spec §4.6, agent harness spec §6.3): the source, the
/// class, the knowledge time, the content hash, and the byte length, never text.
pub fn observation(typed: &TypedFields) -> InputObservation {
    InputObservation {
        source: typed.source.clone(),
        class: match typed.kind {
            SourceKind::News => InputClass::News,
            SourceKind::Filings => InputClass::Filing,
        },
        observed_at: typed.knowledge_time,
        content: typed.content.clone(),
        length_bytes: typed.length_bytes,
    }
}
