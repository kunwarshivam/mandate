//! An item the reader stores (DEC-2278 item 3, DEC-2280 items 2 to 5; journal spec §9.37): each
//! item a page parses and the stream does not yet hold is stored as its typed record,
//! `RetrievedItem::record` of the whole parsed item, which for a licensed item holds no text, and
//! the page's new items are appended as one batch, `StreamOpened` first on an empty stream. Its id
//! is `<vendor id>:<vendor version>`, its tickers the vendor's, sorted and unique, and its
//! `knowledge_at` the latest of its receipt, publication, update, and the stream's last. An item
//! with no ticker is skipped and counted, never journaled. The raw response is dropped once the
//! page is parsed.
//!
//! A batch the journal cannot answer for is retried as the same bytes. A fenced append exits
//! `Fenced`; any other refused append, or an artifact store that cannot take a record, exits the
//! reader, and the next process folds the stream again.

use std::collections::BTreeSet;

use mandate_journal::{AppendOutcome, ArtifactStore};
use mandate_process::Input;
use mandate_research_run::{
    ItemId as RunItemId, NewsItem, RetrievedItem, SourceId as RunSourceId, Symbol as RunSymbol,
};
use mandate_sources::fetch::Transport;
use mandate_sources::record::Draft;
use mandate_sources::text::UntrustedText;
use mandate_store::Store;
use mandate_time::UtcNanos;

use crate::reader::{NEWS_SOURCE, NewsReader};
use crate::records::{Record, Stored};

/// A batch handed to the journal and not yet known to be committed: retried as the same bytes.
pub(crate) struct Pending {
    head: u64,
    stored: Vec<Stored>,
    drafts: Vec<Vec<u8>>,
}

impl<S: Store, A: ArtifactStore, T: Transport> NewsReader<S, A, T> {
    /// Stores each of `drafts` the stream does not hold, and appends their records as one batch.
    pub(crate) fn store_items(&mut self, now: UtcNanos, drafts: &[Draft]) {
        let mut last = self.last_known;
        let mut batch: Vec<Stored> = Vec::new();
        for draft in drafts {
            let item_id = format!("{}:{}", draft.vendor_id().as_str(), draft.vendor_version());
            if self.held.contains(&item_id) || batch.iter().any(|s| s.item_id == item_id) {
                self.counts.held = self.counts.held.saturating_add(1);
                continue;
            }
            let Some(symbols) = self.symbols(&item_id, draft) else {
                continue;
            };
            let knowledge_at = [
                Some(draft.knowledge_time()),
                Some(draft.published_at()),
                Some(draft.vendor_version()),
                last,
            ]
            .into_iter()
            .flatten()
            .max()
            .unwrap_or(now);
            let Some(record) = self.record(&item_id, draft, &symbols, knowledge_at) else {
                self.lifecycle.observe(now, Input::Failed);
                return;
            };
            let Ok(item_ref) = self.artifacts.put_artifact(&record) else {
                self.lifecycle.observe(now, Input::Failed);
                return;
            };
            last = Some(knowledge_at);
            batch.push(Stored {
                source: NEWS_SOURCE.to_owned(),
                allowlist_version: draft.allowlist_version().0,
                item_id,
                symbols: symbols.iter().map(|s| s.as_str().to_owned()).collect(),
                published_at: draft.published_at(),
                knowledge_at,
                item_ref,
                licensed: self.licensed,
            });
        }
        if batch.is_empty() {
            return;
        }
        self.write(now, batch);
    }

    /// The item's tickers as the record holds them, or `None` when it has none or one the record
    /// cannot hold, which is counted with its id.
    fn symbols(&mut self, item_id: &str, draft: &Draft) -> Option<BTreeSet<RunSymbol>> {
        let mut skip = |why: &'static str| {
            self.counts
                .skipped
                .entry(why)
                .or_default()
                .insert(item_id.to_owned());
        };
        if draft.tickers().is_empty() {
            skip("no_ticker");
            return None;
        }
        let symbols: Option<BTreeSet<RunSymbol>> = draft
            .tickers()
            .iter()
            .map(|ticker| RunSymbol::new(ticker.as_str()).ok())
            .collect();
        if symbols.is_none() {
            skip("ticker_unreadable");
        }
        symbols
    }

    /// The item's typed record: `RetrievedItem::record` of the whole parsed item, at its
    /// `knowledge_at`, which for a licensed item holds no text.
    fn record(
        &self,
        item_id: &str,
        draft: &Draft,
        symbols: &BTreeSet<RunSymbol>,
        knowledge_at: UtcNanos,
    ) -> Option<Vec<u8>> {
        let text = draft.text();
        let untrusted = |field: &Option<UntrustedText>| {
            field
                .as_ref()
                .map(|text| text.as_untrusted_str().to_owned())
                .unwrap_or_default()
        };
        let item = NewsItem {
            item_id: RunItemId::new(item_id).ok()?,
            source: RunSourceId::new(NEWS_SOURCE).ok()?,
            symbols: symbols.clone(),
            knowledge_at,
            published_at: draft.published_at(),
            headline: untrusted(&text.headline),
            summary: untrusted(&text.summary),
            link: untrusted(&text.link),
            content_hash: *draft.content().digest(),
            licensed: self.licensed,
        };
        RetrievedItem::News(item).record().ok()
    }

    /// Retries the pending batch; whether nothing is left pending.
    pub(crate) fn flush(&mut self, now: UtcNanos) -> bool {
        match self.pending.take() {
            Some(pending) => self.append(now, pending),
            None => true,
        }
    }

    /// Writes `stored` as one batch at the head, `StreamOpened` first on an empty stream.
    fn write(&mut self, now: UtcNanos, stored: Vec<Stored>) -> bool {
        let mut records: Vec<Record> = Vec::new();
        if self.head == 0 {
            records.push(Record::Opened);
        }
        records.extend(stored.iter().cloned().map(Record::Stored));
        let Some(drafts) = self.writer.drafts(self.head, now, &records) else {
            self.lifecycle.observe(now, Input::Failed);
            return false;
        };
        self.append(
            now,
            Pending {
                head: self.head,
                stored,
                drafts,
            },
        )
    }

    fn append(&mut self, now: UtcNanos, pending: Pending) -> bool {
        let Some(stream) = self.stream.clone() else {
            return false;
        };
        let outcome = self.store.append(
            &stream,
            pending.head,
            self.writer.epoch,
            now,
            &pending.drafts,
        );
        match outcome {
            AppendOutcome::Committed(rows) | AppendOutcome::AlreadyCommitted(rows) => {
                self.head = rows.last().map_or(self.head, |row| row.seq);
                for stored in pending.stored {
                    self.last_known = self.last_known.max(Some(stored.knowledge_at));
                    self.held.insert(stored.item_id);
                    self.counts.stored = self.counts.stored.saturating_add(1);
                }
                true
            }
            AppendOutcome::Unavailable | AppendOutcome::Ambiguous => {
                self.pending = Some(pending);
                false
            }
            AppendOutcome::Fenced { .. } => {
                self.lifecycle.observe(now, Input::Fenced);
                false
            }
            AppendOutcome::HeadMismatch { .. }
            | AppendOutcome::IdempotencyConflict { .. }
            | AppendOutcome::Invalid { .. } => {
                self.lifecycle.observe(now, Input::Failed);
                false
            }
        }
    }
}
