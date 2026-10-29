//! The news reader: one writer epoch on `news:{workspace_id}`, one vendor page a step, and each new
//! item stored and journaled before any run can read it (DEC-2278 items 1 to 3, DEC-2280; journal
//! spec §9.37's "What the writer owes").
//!
//! **Start.** With no follows or a malformed one, no key, a refused key, a vault that could not
//! answer, or a news source the allowlist does not have in effect, the reader takes no epoch, writes
//! nothing, fetches nothing, and stays not ready, with the reason's code
//! ([`NewsReader::not_ready`]). It never retries the key read. Otherwise it takes a newer epoch,
//! fencing every earlier reader, and folds its stream for the item ids it holds and the last
//! `knowledge_at`; a stored item whose body, `item_id` or `knowledge_at` it cannot read leaves it
//! not ready, `news_stream_unreadable`, fetching and writing nothing (DEC-2281 item 3).
//!
//! **A step.** A step asks the vendor for at most one page ([`crate::poll`]), so the allowlist's pace
//! is the only limit on requests. A round asks from 15 minutes before the last `knowledge_at`
//! ([`crate::poll::POLL_OVERLAP_S`], DEC-2290), or 48 hours back for an empty stream (DEC-1130
//! item 5), about the tickers of the followed agents' active theses and
//! held instruments ([`crate::watch`], [`crate::tickers`]). An instrument with no registered ticker
//! is not polled and is counted. A round with nothing to ask, a stream that cannot be read, or a
//! failed request pauses the reader for [`crate::poll::COOLDOWN_S`].
//!
//! **The receipt lead** (DEC-2289). A round whose receipt, the host clock's reading, runs more
//! than [`RECEIPT_LEAD_S`] past the step's trusted time asks and journals nothing, so no
//! `knowledge_at` is set past the trusted time by more than the lead, plus the sources' 1 s future
//! skew (`mandate_sources::ingest::FUTURE_SKEW_MS`) a vendor time may run past its receipt. It is counted, alerted once
//! an episode as [`RECEIPT_AHEAD`], and pauses the reader as a failed request does. A round within
//! the bound ends the episode. Every pause runs by the trusted time.
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

/// How far a round's receipt may run ahead of the trusted time before the round is refused
/// (DEC-2289).
pub const RECEIPT_LEAD_S: i64 = 60;

/// The alert a reader raises once an episode of receipts past [`RECEIPT_LEAD_S`] begins.
pub const RECEIPT_AHEAD: &str = "receipt_ahead";

/// A step's two readings of the time (DEC-2289): `now`, the host clock, which stamps a receipt,
/// and `trusted`, what the caller holds to be the time without that clock's word alone. The binary
/// takes the host clock at start plus a monotonic clock's elapsed time since.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clock {
    pub now: UtcNanos,
    pub trusted: UtcNanos,
}

impl Clock {
    /// The two readings agreeing at `now`.
    #[must_use]
    pub fn agreed(now: UtcNanos) -> Self {
        Self { now, trusted: now }
    }

    /// The trusted time `elapsed` after `start`, or `start` itself past the representable range,
    /// so a trusted time that cannot be formed is the earliest it can be and holds a round rather
    /// than admits one.
    #[must_use]
    pub fn trusted_after(start: UtcNanos, elapsed: Duration) -> UtcNanos {
        let nanos = u64::from(start.nanos()).saturating_add(u64::from(elapsed.subsec_nanos()));
        let carry = i64::from(nanos >= NANOS_PER_SEC);
        let secs = i64::try_from(elapsed.as_secs())
            .ok()
            .and_then(|secs| start.secs().checked_add(secs))
            .and_then(|secs| secs.checked_add(carry));
        secs.and_then(|secs| {
            let nanos = u32::try_from(nanos.checked_rem(NANOS_PER_SEC)?).ok()?;
            UtcNanos::from_parts(secs, nanos).ok()
        })
        .unwrap_or(start)
    }
}

const NANOS_PER_SEC: u64 = 1_000_000_000;

/// Whether `clock`'s receipt is at most [`RECEIPT_LEAD_S`] past its trusted time; a bound past
/// the representable range admits nothing.
fn within_lead(clock: Clock) -> bool {
    clock
        .trusted
        .secs()
        .checked_add(RECEIPT_LEAD_S)
        .and_then(|secs| UtcNanos::from_parts(secs, clock.trusted.nanos()).ok())
        .is_some_and(|bound| clock.now <= bound)
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
    /// Chunks of a round that stopped at the page cap with a page still to follow.
    pub page_caps: u64,
    /// Rounds refused because their receipt ran more than [`RECEIPT_LEAD_S`] ahead of the trusted
    /// time (DEC-2289).
    pub receipt_ahead: u64,
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
    /// The reader asks nothing before the trusted time reaches this, after a round it could not
    /// start, that failed, or that ran past the receipt lead: the trusted time, so a host clock
    /// that jumps ahead and back neither shortens nor lengthens a pause (DEC-2289).
    paused_until: Option<UtcNanos>,
    pub(crate) pending: Option<Pending>,
    pub(crate) lifecycle: Lifecycle,
    pub(crate) counts: Counts,
    /// The operator alerts raised and not yet taken, each a fixed code.
    alerts: Vec<&'static str>,
    /// Whether the last round judged was held past the receipt lead, already alerted.
    ahead: bool,
}

impl<S: Store, A: ArtifactStore, T: Transport> NewsReader<S, A, T> {
    /// Starts at `now` with the key read once, `key`. A refused key, a refused follow, or a source
    /// not in effect leaves the reader started and not ready, holding no epoch; a store that cannot
    /// answer exits it `Refused`. Otherwise it folds its stream, and only a stream it can read has it take a newer epoch and
    /// fold the stream again.
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
            alerts: Vec::new(),
            ahead: false,
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
        let Some(rows) = this.store.rows(&stream) else {
            this.lifecycle.observe(now, Input::Failed);
            return this;
        };
        if fold(&rows).is_none() {
            this.not_ready = Some("news_stream_unreadable");
            return this;
        }
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
        let Some(folded) = fold(&rows) else {
            this.not_ready = Some("news_stream_unreadable");
            return this;
        };
        this.head = folded.head;
        this.held = folded.held;
        this.last_known = folded.last_known;
        this.lifecycle.observe(
            now,
            Input::Replayed {
                position: this.head,
                head: this.head,
            },
        );
        this
    }

    /// One step at `clock`: a retried batch first, then, if the receipt is within the lead of the
    /// trusted time, at most one page asked and its new items stored. Nothing is done once the reader has exited, and nothing but
    /// a heartbeat while it is not ready or paused.
    pub async fn step(&mut self, clock: Clock) {
        let now = clock.now;
        if self.exited() {
            return;
        }
        self.lifecycle.observe(now, Input::Heartbeat);
        let paused = self.paused_until.is_some_and(|until| clock.trusted < until);
        if self.not_ready.is_some() || paused || !self.flush(now) {
            return;
        }
        if !within_lead(clock) {
            self.counts.receipt_ahead = self.counts.receipt_ahead.saturating_add(1);
            if !self.ahead {
                self.ahead = true;
                self.alerts.push(RECEIPT_AHEAD);
            }
            self.paused_until = cooldown_until(clock.trusted);
            return;
        }
        self.ahead = false;
        let Some(round) = self.round.take().or_else(|| self.next_round(now)) else {
            self.paused_until = cooldown_until(clock.trusted);
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
                self.paused_until = cooldown_until(clock.trusted);
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

    /// The operator alerts raised since the last take, each a fixed code, oldest first.
    pub fn take_alerts(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.alerts)
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
}

/// What a news stream's rows fold to: its head, the item ids it holds, and its last
/// `knowledge_at`.
struct Folded {
    head: u64,
    held: BTreeSet<String>,
    last_known: Option<UtcNanos>,
}

/// `rows` folded; `None` when a `NewsItemStored` row's body, `item_id` or `knowledge_at` cannot be
/// read, since the held ids or the last known time would then be short (DEC-2281 item 3). The
/// reader folds its stream once before it takes the writer epoch, so that a stream it cannot read
/// fences no earlier writer, and once after, from the rows the epoch now guards.
fn fold(rows: &[StoredEvent]) -> Option<Folded> {
    let mut folded = Folded {
        head: 0,
        held: BTreeSet::new(),
        last_known: None,
    };
    for row in rows {
        folded.head = row.seq;
        if row.event_type != "NewsItemStored" {
            continue;
        }
        let body = parse(&row.body).ok()?;
        let payload = body.get("payload")?;
        let member = |name: &str| payload.get(name).and_then(Value::as_str);
        let item_id = member("item_id")?;
        let known = UtcNanos::parse(member("knowledge_at")?).ok()?;
        folded.held.insert(item_id.to_owned());
        folded.last_known = folded.last_known.max(Some(known));
    }
    Some(folded)
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
