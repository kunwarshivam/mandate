//! The news reader: one writer epoch on `news:{workspace_id}`, one vendor page a step, and each new
//! item stored and journaled before any run can read it (DEC-2278 items 1 to 3, DEC-2280; journal
//! spec §9.37's "What the writer owes").
//!
//! **Start.** With no key, a refused key, a vault that could not answer, or a news source the
//! allowlist does not have in effect, the reader takes no epoch, writes nothing, fetches nothing,
//! and stays not ready, with the reason's code ([`NewsReader::not_ready`]). It never retries the key
//! read. Otherwise it takes a newer epoch, fencing every earlier reader, and folds its stream for
//! the item ids it holds and the last `knowledge_at`.
//!
//! **A step.** A step asks the vendor for at most one page, so the allowlist's pace is the only
//! limit on requests. A round starts from the last `knowledge_at`, or 48 hours back for an empty
//! stream (DEC-1130 item 5), over the tickers of the followed agents' active theses and held
//! instruments ([`crate::watch`], [`crate::tickers`]), and follows the vendor's pages to the last.
//! An instrument with no registered ticker is not polled and is counted.
//!
//! **An item.** Each item the page parses and the stream does not yet hold is stored as its typed
//! record, `RetrievedItem::record` of the whole parsed item, which for a licensed item holds no
//! text, and the page's new items are appended as one batch, `StreamOpened` first on an empty
//! stream. Its id is `<vendor id>:<vendor version>`, its tickers the vendor's, sorted and unique,
//! and its `knowledge_at` the latest of its receipt, publication, update, and the stream's last
//! (DEC-2280 items 2, 3 and 5). An item with no ticker, or one the parser refused (more than 32
//! tickers, future-dated, malformed), is skipped and counted, never journaled. The raw response is
//! dropped once the page is parsed (DEC-2280 item 4).
//!
//! **Failure.** A batch the journal cannot answer for is retried as the same bytes. A fenced
//! append exits `Fenced`; any other refused append, or an artifact store that cannot take a
//! record, exits the reader not ready, and the next process folds the stream again.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use mandate_canon::{Value, parse};
use mandate_journal::{AppendOutcome, ArtifactStore, Environment, StoredEvent, StreamId};
use mandate_process::{Exit, Input, Kind, Lifecycle, State};
use mandate_research::SourceId;
use mandate_research_run::{
    ItemId as RunItemId, NewsItem, RetrievedItem, SourceId as RunSourceId, Symbol as RunSymbol,
};
use mandate_sources::SourceError;
use mandate_sources::allowlist::{Allowlist, SourceKind};
use mandate_sources::fetch::{Fetcher, NewsAuth, Transport};
use mandate_sources::ingest::{IngestContext, PageToken, alpaca_news_page};
use mandate_sources::record::{Capture, Draft};
use mandate_sources::symbols::{CikTable, Symbol, SymbolTable};
use mandate_store::Store;
use mandate_time::UtcNanos;

use crate::key::KeyRefusal;
use crate::records::{Record, Stored, Writer};
use crate::tickers::registered_tickers;
use crate::watch::followed;

/// How far back a round on an empty stream asks: the news read's 48 hours (DEC-1130 item 5,
/// agent harness spec §6.3).
pub const FIRST_LOOKBACK_S: i64 = 48 * 3_600;

/// The news source the reader fetches (allowlist version 3, DEC-2276).
pub const NEWS_SOURCE: &str = "alpaca.news";

/// One agent the reader follows: its id, and the broker account its executor trades, whose
/// account stream holds its positions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Follow {
    pub agent: String,
    pub account: String,
}

/// Each `--follow <agent>=<account>` in `args`, in order: an agent id and its account, neither
/// empty.
///
/// # Errors
/// `follow_malformed` for a `--follow` with no value, no `=`, or an empty side.
pub fn parse_follows(args: &[String]) -> Result<Vec<Follow>, &'static str> {
    args.iter()
        .enumerate()
        .filter(|(_, flag)| *flag == "--follow")
        .map(|(at, _)| {
            let (agent, account) = at
                .checked_add(1)
                .and_then(|next| args.get(next))
                .and_then(|value| value.split_once('='))
                .filter(|(agent, account)| !agent.is_empty() && !account.is_empty())
                .ok_or("follow_malformed")?;
            Ok(Follow {
                agent: agent.to_owned(),
                account: account.to_owned(),
            })
        })
        .collect()
}

/// What a reader runs.
#[derive(Debug, Clone)]
pub struct Config {
    pub workspace: String,
    pub environment: Environment,
    pub allowlist: Allowlist,
    pub follows: Vec<Follow>,
    /// The liveness bound of [`mandate_process`]: the loop handles an input at least this often.
    pub liveness: Duration,
}

/// What the reader skipped or could not read, never silently (DEC-2278 item 3, DEC-2280 item 1).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Counts {
    /// Items stored and journaled.
    pub stored: u64,
    /// Parsed items whose id the stream already held.
    pub held: u64,
    /// Each skipped item's id, by why: `no_ticker`, or a ticker the record cannot hold.
    pub skipped: BTreeMap<&'static str, BTreeSet<String>>,
    /// Items the parser refused, by its code (`too_many_tags`, `future_dated`, ...); a refused
    /// item has no id the parser could vouch for.
    pub refused: BTreeMap<&'static str, u64>,
    /// Followed instruments with no registered ticker, by asset id.
    pub unregistered: BTreeSet<String>,
    /// Snapshot registrations whose artifact named no asset.
    pub unreadable_snapshots: u64,
    /// Followed agents whose theses or positions could not be read.
    pub unreadable_follows: BTreeSet<String>,
    /// Requests the vendor or the transport failed, by code.
    pub fetch_failed: BTreeMap<&'static str, u64>,
}

/// A round of pages: the tickers asked, from when, and the next page's token.
#[derive(Debug, Clone)]
struct Round {
    symbols: Vec<Symbol>,
    start: UtcNanos,
    page: Option<PageToken>,
}

/// A batch handed to the journal and not yet known to be committed: retried as the same bytes.
struct Pending {
    head: u64,
    stored: Vec<Stored>,
    drafts: Vec<Vec<u8>>,
}

/// One workspace's news reader.
pub struct NewsReader<S, A, T> {
    store: S,
    artifacts: A,
    fetcher: Fetcher<T>,
    /// The news source and the parser's (empty) tagging tables; `None` only if either cannot be
    /// built, which leaves the reader exited `Refused`.
    source: Option<(SourceId, SymbolTable, CikTable)>,
    licensed: bool,
    auth: Option<NewsAuth>,
    not_ready: Option<&'static str>,
    writer: Writer,
    stream: Option<StreamId>,
    follows: Vec<Follow>,
    head: u64,
    held: BTreeSet<String>,
    last_known: Option<UtcNanos>,
    round: Option<Round>,
    pending: Option<Pending>,
    lifecycle: Lifecycle,
    counts: Counts,
}

impl<S: Store, A: ArtifactStore, T: Transport> NewsReader<S, A, T> {
    /// Starts at `now` with the key read once, `key`. A refused key or a source not in effect
    /// leaves the reader started and not ready, holding no epoch; a store that cannot answer
    /// exits it `Refused`. Otherwise it takes a newer epoch and folds its stream.
    pub fn start(
        store: S,
        artifacts: A,
        transport: T,
        config: &Config,
        key: Result<NewsAuth, KeyRefusal>,
        now: UtcNanos,
    ) -> Self {
        let writer = Writer {
            workspace: config.workspace.clone(),
            environment: config.environment,
            epoch: 0,
        };
        let stream = StreamId::parse(&writer.stream());
        let source = SourceId::new(NEWS_SOURCE).ok();
        let entry = source
            .as_ref()
            .and_then(|source| config.allowlist.active(source))
            .filter(|entry| entry.kind() == SourceKind::News);
        let licensed = entry.is_none_or(|entry| entry.licensed());
        let mut this = Self {
            store,
            artifacts,
            fetcher: Fetcher::new(config.allowlist.clone(), transport),
            source: None,
            licensed,
            auth: None,
            not_ready: None,
            writer,
            stream,
            follows: config.follows.clone(),
            head: 0,
            held: BTreeSet::new(),
            last_known: None,
            round: None,
            pending: None,
            lifecycle: Lifecycle::new(Kind::Writer, config.liveness, now),
            counts: Counts::default(),
        };
        let tables = SymbolTable::new(0, Vec::new())
            .ok()
            .zip(CikTable::new(0, Vec::new()).ok());
        this.source = source
            .zip(tables)
            .map(|(source, (symbols, ciks))| (source, symbols, ciks));
        if config.environment != Environment::Paper || this.source.is_none() {
            this.lifecycle.observe(now, Input::Failed);
            return this;
        }
        this.lifecycle.observe(now, Input::StartupPassed);
        match key {
            Err(refusal) => {
                this.not_ready = Some(refusal.code());
                return this;
            }
            Ok(auth) => this.auth = Some(auth),
        }
        if entry.is_none() {
            this.not_ready = Some("news_source_not_in_effect");
            return this;
        }
        let Some(stream) = this.stream.clone() else {
            this.lifecycle.observe(now, Input::Failed);
            return this;
        };
        let Some(epoch) = this.store.take_ownership(&stream) else {
            this.lifecycle.observe(now, Input::Failed);
            return this;
        };
        this.writer.epoch = epoch;
        this.lifecycle.observe(now, Input::EpochTaken);
        let Some(rows) = this.store.rows(&stream) else {
            this.lifecycle.observe(now, Input::Failed);
            return this;
        };
        this.replay(&rows);
        this.lifecycle.observe(
            now,
            Input::Replayed {
                position: this.head,
                head: this.head,
            },
        );
        this
    }

    /// One step at the host clock's reading `now`: a retried batch first, then at most one page
    /// asked and its new items stored. Nothing is done once the reader has exited, and nothing but
    /// a heartbeat while it is not ready.
    pub async fn step(&mut self, now: UtcNanos) {
        if self.exited() {
            return;
        }
        self.lifecycle.observe(now, Input::Heartbeat);
        if self.not_ready.is_some() || !self.flush(now) {
            return;
        }
        let Some(round) = self.round.take().or_else(|| self.next_round(now)) else {
            return;
        };
        let (Some(auth), Some((source, symbols, ciks))) =
            (self.auth.as_ref(), self.source.as_ref())
        else {
            return;
        };
        let body = match self
            .fetcher
            .alpaca_news(
                source,
                auth,
                &round.symbols,
                round.start,
                round.page.as_ref(),
                now,
            )
            .await
        {
            Ok(body) => body,
            Err(SourceError::TooSoon) => {
                self.round = Some(round);
                return;
            }
            Err(error) => {
                count(&mut self.counts.fetch_failed, error.code());
                return;
            }
        };
        let page = alpaca_news_page(
            &IngestContext {
                allowlist: self.fetcher.allowlist(),
                source,
                received_at: now,
                capture: Capture::Live,
                symbols,
                ciks,
            },
            &body,
        );
        drop(body);
        let page = match page {
            Ok(page) => page,
            Err(error) => {
                count(&mut self.counts.fetch_failed, error.code());
                return;
            }
        };
        for (_, error) in &page.refused {
            count(&mut self.counts.refused, error.code());
        }
        if let Some(next) = page.next_page_token {
            self.round = Some(Round {
                page: Some(next),
                ..round
            });
        }
        let mut drafts = page.drafts;
        drafts.sort_by_key(|draft| (draft.vendor_version(), draft.vendor_id().clone()));
        self.store_items(now, &drafts);
    }

    /// Drains at `now`: no new page; a batch in flight is tried once more, then the reader exits
    /// `Drained`, or `DrainIncomplete` if that batch is still unknown.
    pub fn drain(&mut self, now: UtcNanos) {
        if self.exited() {
            return;
        }
        self.lifecycle.observe(now, Input::DrainRequested);
        let last = if self.flush(now) {
            Input::Settled
        } else {
            Input::Failed
        };
        self.lifecycle.observe(now, last);
    }

    pub fn lifecycle(&self) -> &Lifecycle {
        &self.lifecycle
    }

    /// Why the reader is not ready, as the code it writes on its standard error, or `None`.
    pub fn not_ready(&self) -> Option<&'static str> {
        self.not_ready
    }

    pub fn counts(&self) -> &Counts {
        &self.counts
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    pub fn artifacts(&self) -> &A {
        &self.artifacts
    }

    pub fn transport(&self) -> &T {
        self.fetcher.transport()
    }

    /// The last `knowledge_at` the stream holds.
    pub fn last_known(&self) -> Option<UtcNanos> {
        self.last_known
    }

    fn exited(&self) -> bool {
        matches!(self.lifecycle.state(), State::Exited(_))
    }

    /// A new round at `now` over the followed instruments' tickers, or `None` when none has one.
    fn next_round(&mut self, now: UtcNanos) -> Option<Round> {
        let assets = self.followed_assets(now)?;
        let control = StreamId::parse(&format!("ctl:{}", self.writer.workspace))
            .and_then(|stream| self.store.rows(&stream))?;
        let tickers = registered_tickers(&control, &self.artifacts);
        self.counts.unreadable_snapshots = tickers.unreadable();
        let mut symbols = BTreeSet::new();
        for asset in assets {
            match tickers.ticker(&asset) {
                Some(symbol) => {
                    symbols.insert(symbol.clone());
                }
                None => {
                    self.counts.unregistered.insert(asset);
                }
            }
        }
        if symbols.is_empty() {
            return None;
        }
        let start = match self.last_known {
            Some(known) => known,
            None => {
                UtcNanos::from_parts(now.secs().checked_sub(FIRST_LOOKBACK_S)?, now.nanos()).ok()?
            }
        };
        Some(Round {
            symbols: symbols.into_iter().collect(),
            start,
            page: None,
        })
    }

    /// Every followed agent's active theses' and held instruments at `now`, or `None` when a
    /// stream cannot be read.
    fn followed_assets(&mut self, now: UtcNanos) -> Option<BTreeSet<String>> {
        let workspace = self.writer.workspace.clone();
        let mut assets = BTreeSet::new();
        for follow in self.follows.clone() {
            let agent_stream = StreamId::parse(&format!("agent:{workspace}:{}", follow.agent))?;
            let account_name = format!("acct:{workspace}:{}", follow.account);
            let account_stream = StreamId::parse(&account_name)?;
            let agent_rows = self.store.rows(&agent_stream)?;
            let account_rows = self.store.rows(&account_stream)?;
            let followed = followed(
                &follow.agent,
                &agent_rows,
                &account_name,
                &account_rows,
                now,
            );
            if followed.theses_unreadable || followed.held_unreadable {
                self.counts.unreadable_follows.insert(follow.agent.clone());
            }
            assets.extend(followed.assets);
        }
        Some(assets)
    }

    /// Stores each of `drafts` the stream does not hold, and appends their records as one batch.
    fn store_items(&mut self, now: UtcNanos, drafts: &[Draft]) {
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
        let untrusted = |field: &Option<mandate_sources::text::UntrustedText>| {
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
    fn flush(&mut self, now: UtcNanos) -> bool {
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

    /// The item ids the stream holds and its last `knowledge_at`, and its head.
    fn replay(&mut self, rows: &[StoredEvent]) {
        for row in rows {
            self.head = row.seq;
            if row.event_type != "NewsItemStored" {
                continue;
            }
            let body = parse(&row.body).ok();
            let payload = body.as_ref().and_then(|b| b.get("payload"));
            let member = |name: &str| payload.and_then(|p| p.get(name)).and_then(Value::as_str);
            if let Some(item_id) = member("item_id") {
                self.held.insert(item_id.to_owned());
            }
            let known = member("knowledge_at").and_then(|k| UtcNanos::parse(k).ok());
            self.last_known = self.last_known.max(known);
        }
    }
}

/// The exit a reader stopped with, if it has.
pub fn exit<S, A, T>(reader: &NewsReader<S, A, T>) -> Option<Exit> {
    match reader.lifecycle.state() {
        State::Exited(exit) => Some(exit),
        _ => None,
    }
}

fn count(counts: &mut BTreeMap<&'static str, u64>, code: &'static str) {
    let n = counts.entry(code).or_default();
    *n = n.saturating_add(1);
}
