//! The news reader: one writer epoch on `news:{workspace_id}`, one vendor page a step, and each new
//! item stored and journaled before any run can read it (DEC-2278 items 1 to 3, DEC-2280; journal
//! spec §9.37's "What the writer owes").
//!
//! **Start.** With no follows or a malformed one, no key, a refused key, a vault that could not
//! answer, or a news source the allowlist does not have in effect, the reader takes no epoch, writes
//! nothing, fetches nothing, and stays not ready, with the reason's code
//! ([`NewsReader::not_ready`]). It never retries the key read. Otherwise it takes a newer epoch,
//! fencing every earlier reader, and folds its stream for the item ids it holds and the last
//! `knowledge_at`.
//!
//! **A step.** A step asks the vendor for at most one page ([`crate::poll`]), so the allowlist's pace
//! is the only limit on requests. A round asks from the last `knowledge_at`, or 48 hours back for
//! an empty stream (DEC-1130 item 5), about the tickers of the followed agents' active theses and
//! held instruments ([`crate::watch`], [`crate::tickers`]). An instrument with no registered ticker
//! is not polled and is counted. A round with nothing to ask, a stream that cannot be read, or a
//! failed request pauses the reader for [`crate::poll::COOLDOWN_S`].
//!
//! **An item** is stored and journaled as [`crate::items`] says.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use mandate_canon::{Value, parse};
use mandate_journal::{ArtifactStore, Environment, StoredEvent, StreamId};
use mandate_process::{Exit, Input, Kind, Lifecycle, State};
use mandate_research::SourceId;
use mandate_sources::SourceError;
use mandate_sources::allowlist::{Allowlist, SourceKind};
use mandate_sources::fetch::{Fetcher, NewsAuth, Transport};
use mandate_sources::ingest::{IngestContext, alpaca_news_page};
use mandate_sources::record::Capture;
use mandate_sources::symbols::{CikTable, SymbolTable};
use mandate_store::Store;
use mandate_time::UtcNanos;

use crate::follows::{self, Follow, Followed};
use crate::items::Pending;
use crate::key::KeyRefusal;
use crate::poll::{Round, cooldown_until, round_start};
use crate::records::Writer;
use crate::tickers::registered_tickers;
use crate::watch::followed;

/// The news source the reader fetches (allowlist version 3, DEC-2276).
pub const NEWS_SOURCE: &str = "alpaca.news";

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
    /// Chunks of a round that stopped at the page cap with a page still to follow.
    pub page_caps: u64,
}

/// One workspace's news reader.
pub struct NewsReader<S, A, T> {
    pub(crate) store: S,
    pub(crate) artifacts: A,
    fetcher: Fetcher<T>,
    /// The news source and the parser's (empty) tagging tables; `None` only if either cannot be
    /// built, which leaves the reader exited `Refused`.
    source: Option<(SourceId, SymbolTable, CikTable)>,
    pub(crate) licensed: bool,
    auth: Option<NewsAuth>,
    not_ready: Option<&'static str>,
    pub(crate) writer: Writer,
    pub(crate) stream: Option<StreamId>,
    follows: Vec<Followed>,
    pub(crate) head: u64,
    pub(crate) held: BTreeSet<String>,
    pub(crate) last_known: Option<UtcNanos>,
    round: Option<Round>,
    /// The reader asks nothing before this, after a round it could not start or that failed.
    paused_until: Option<UtcNanos>,
    pub(crate) pending: Option<Pending>,
    pub(crate) lifecycle: Lifecycle,
    pub(crate) counts: Counts,
}

impl<S: Store, A: ArtifactStore, T: Transport> NewsReader<S, A, T> {
    /// Starts at `now` with the key read once, `key`. A refused key, a refused follow, or a source
    /// not in effect leaves the reader started and not ready, holding no epoch; a store that cannot
    /// answer exits it `Refused`. Otherwise it takes a newer epoch and folds its stream.
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
        let tables = SymbolTable::new(0, Vec::new())
            .ok()
            .zip(CikTable::new(0, Vec::new()).ok());
        let mut this = Self {
            store,
            artifacts,
            fetcher: Fetcher::new(config.allowlist.clone(), transport),
            source: source
                .zip(tables)
                .map(|(source, (symbols, ciks))| (source, symbols, ciks)),
            licensed,
            auth: None,
            not_ready: None,
            writer,
            stream,
            follows: Vec::new(),
            head: 0,
            held: BTreeSet::new(),
            last_known: None,
            round: None,
            paused_until: None,
            pending: None,
            lifecycle: Lifecycle::new(Kind::Writer, config.liveness, now),
            counts: Counts::default(),
        };
        if config.environment != Environment::Paper || this.source.is_none() {
            this.lifecycle.observe(now, Input::Failed);
            return this;
        }
        this.lifecycle.observe(now, Input::StartupPassed);
        let refused = match (follows::check(&config.workspace, &config.follows), key) {
            (Err(code), _) => Some(code),
            (Ok(_), Err(refusal)) => Some(refusal.code()),
            (Ok(follows), Ok(auth)) => {
                this.auth = Some(auth);
                this.follows = follows;
                entry.is_none().then_some("news_source_not_in_effect")
            }
        };
        if refused.is_some() {
            this.not_ready = refused;
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
    /// a heartbeat while it is not ready or paused.
    pub async fn step(&mut self, now: UtcNanos) {
        if self.exited() {
            return;
        }
        self.lifecycle.observe(now, Input::Heartbeat);
        let paused = self.paused_until.is_some_and(|until| now < until);
        if self.not_ready.is_some() || paused || !self.flush(now) {
            return;
        }
        let Some(round) = self.round.take().or_else(|| self.next_round(now)) else {
            self.paused_until = cooldown_until(now);
            return;
        };
        let (Some(auth), Some((source, symbols, ciks))) =
            (self.auth.as_ref(), self.source.as_ref())
        else {
            return;
        };
        let (tickers, start, page) = round.request();
        let fetched = self
            .fetcher
            .alpaca_news(source, auth, tickers, start, page, now)
            .await;
        let page = fetched.and_then(|body| {
            alpaca_news_page(
                &IngestContext {
                    allowlist: self.fetcher.allowlist(),
                    source,
                    received_at: now,
                    capture: Capture::Live,
                    symbols,
                    ciks,
                },
                &body,
            )
        });
        let page = match page {
            Ok(page) => page,
            Err(SourceError::TooSoon) => {
                self.round = Some(round);
                return;
            }
            Err(error) => {
                count(&mut self.counts.fetch_failed, error.code());
                self.paused_until = cooldown_until(now);
                return;
            }
        };
        for (_, error) in &page.refused {
            count(&mut self.counts.refused, error.code());
        }
        let after = round.after(page.next_page_token);
        if after.capped {
            self.counts.page_caps = self.counts.page_caps.saturating_add(1);
        }
        self.round = after.round;
        let mut drafts = page.drafts;
        if drafts.is_empty() {
            return;
        }
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

    /// A new round at `now` over the followed instruments' tickers, or `None` when none has one or
    /// a stream cannot be read.
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
        Round::new(symbols, round_start(self.last_known, now)?)
    }

    /// Every followed agent's active theses' and held instruments at `now`, or `None` when a
    /// stream cannot be read.
    fn followed_assets(&mut self, now: UtcNanos) -> Option<BTreeSet<String>> {
        let mut assets = BTreeSet::new();
        for follow in &self.follows {
            let agent_rows = self.store.rows(&follow.agent_stream)?;
            let account_rows = self.store.rows(&follow.account_stream)?;
            let watched = followed(
                &follow.agent,
                &agent_rows,
                follow.account_stream.as_str(),
                &account_rows,
                now,
            );
            if watched.theses_unreadable || watched.held_unreadable {
                self.counts.unreadable_follows.insert(follow.agent.clone());
            }
            assets.extend(watched.assets);
        }
        Some(assets)
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

pub(crate) fn count(counts: &mut BTreeMap<&'static str, u64>, code: &'static str) {
    let n = counts.entry(code).or_default();
    *n = n.saturating_add(1);
}
