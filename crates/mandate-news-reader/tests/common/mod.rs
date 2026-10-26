#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test helpers: each test binary uses a subset, and a broken fixture should stop the test"
)]
//! Shared fixtures: a journal whose news stream is the real in-memory journal, so every record the
//! reader writes passes the journal's own rules, and whose other streams are the rows a test gives;
//! an artifact shelf; a recorded transport; and a poll-once executor.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_journal::{
    AppendOutcome, ArtifactError, ArtifactRef, ArtifactSource, ArtifactStore, Environment,
    StoredEvent, StreamId, StreamType,
};
use mandate_news_reader::{Config, Follow, NewsReader};
use mandate_sources::SourceError;
use mandate_sources::allowlist::{Allowlist, PermittedUrl};
use mandate_sources::fetch::{NewsAuth, RawResponse, Transport};
use mandate_store::{MemoryStore, Store};
use mandate_time::UtcNanos;

pub const WS: &str = "ws_1";
pub const AGENT: &str = "agent_a";
pub const ACCOUNT: &str = "01KDER00000000000000000001";
pub const SPY: &str = "b28f4066-5c6d-479b-a2af-85dc1a8f16fb";
pub const QQQ: &str = "eb82ffb0-efd6-4f4e-9307-8fead28e04f6";
pub const AAPL: &str = "11111111-1111-4111-8111-111111111111";
/// The checked-in allowlist, whose Alpaca news entry is in effect by the founder's approval.
pub const ALLOWLIST: &str = include_str!("../../../../config/sources/allowlist-v3.toml");

pub fn at(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).unwrap()
}

/// 2026-10-26T15:00:00Z, the tests' first step.
pub fn t0() -> UtcNanos {
    at("2026-10-26T15:00:00Z")
}

pub fn plus(t: UtcNanos, secs: i64) -> UtcNanos {
    UtcNanos::from_parts(t.secs() + secs, t.nanos()).unwrap()
}

pub fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    match future.as_mut().poll(&mut cx) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("a recorded fetch never waits"),
    }
}

/// A paper key, as `mandate vault put news-data-key` stores it.
pub const PAPER_KEY: &[u8] = b"PKTESTKEYID0001:topsecret0001";

pub fn paper_auth() -> NewsAuth {
    NewsAuth::from_stored(PAPER_KEY).unwrap()
}

/// The journal: the news stream is the real in-memory journal; every other stream answers the
/// rows a test set, or nothing when `others_down` is set.
#[derive(Clone, Default)]
pub struct Journal {
    pub memory: MemoryStore,
    pub others: Rc<RefCell<BTreeMap<String, Vec<StoredEvent>>>>,
    pub others_down: Rc<Cell<bool>>,
    /// Answers every append `Unavailable` while set.
    pub appends_down: Rc<Cell<bool>>,
    /// Answers every append `HeadMismatch` while set: a refusal that is not a fence.
    pub appends_refused: Rc<Cell<bool>>,
}

impl Store for Journal {
    fn take_ownership(&mut self, stream: &StreamId) -> Option<u64> {
        self.memory.take_ownership(stream)
    }

    fn rows(&self, stream: &StreamId) -> Option<Vec<StoredEvent>> {
        if stream.stream_type() == StreamType::News {
            return self.memory.rows(stream);
        }
        if self.others_down.get() {
            return None;
        }
        Some(
            self.others
                .borrow()
                .get(stream.as_str())
                .cloned()
                .unwrap_or_default(),
        )
    }

    fn append(
        &mut self,
        stream: &StreamId,
        expected_head: u64,
        epoch: u64,
        recorded_at: UtcNanos,
        drafts: &[Vec<u8>],
    ) -> AppendOutcome {
        if self.appends_down.get() {
            return AppendOutcome::Unavailable;
        }
        if self.appends_refused.get() {
            return AppendOutcome::HeadMismatch {
                actual_seq: 99,
                actual_hash: Digest::of(b"other"),
            };
        }
        self.memory
            .append(stream, expected_head, epoch, recorded_at, drafts)
    }
}

impl Journal {
    /// Appends a row of `event_type` with `payload` to `stream`, outside the journal's checks: the
    /// streams the reader only reads.
    pub fn push(&self, stream: &str, event_type: &str, payload: Value) {
        let mut others = self.others.borrow_mut();
        let rows = others.entry(stream.to_owned()).or_default();
        let seq = u64::try_from(rows.len()).unwrap() + 1;
        let body = mandate_canon::Value::Object(
            [
                ("event_type".to_owned(), Value::Str(event_type.to_owned())),
                ("payload".to_owned(), payload),
            ]
            .into_iter()
            .map(|(k, v)| (mandate_canon::Key::new(&k).unwrap(), v))
            .collect(),
        );
        rows.push(StoredEvent {
            stream_id: stream.to_owned(),
            seq,
            event_id: format!("e{seq}"),
            event_type: event_type.to_owned(),
            schema_version: 1,
            environment: "paper".to_owned(),
            recorded_at: "2026-10-26T14:00:00.000000000Z".to_owned(),
            prev_hash: Digest::of(b""),
            hash: Digest::of(b""),
            body: to_canonical(&body),
        });
    }

    /// The news stream's rows, as the journal stored them.
    pub fn news(&self) -> Vec<StoredEvent> {
        self.memory
            .rows(&StreamId::parse(&format!("news:{WS}")).unwrap())
            .unwrap()
    }
}

/// An artifact store shared with the test.
#[derive(Clone, Default)]
pub struct Shelf {
    pub objects: Rc<RefCell<BTreeMap<Digest, Vec<u8>>>>,
    pub down: Rc<Cell<bool>>,
}

impl ArtifactSource for Shelf {
    fn read_artifact(&self, reference: &ArtifactRef) -> Result<Vec<u8>, ArtifactError> {
        self.objects.borrow().read_artifact(reference)
    }
}

impl ArtifactStore for Shelf {
    fn put_artifact(&mut self, bytes: &[u8]) -> Result<ArtifactRef, ArtifactError> {
        if self.down.get() {
            return Err(ArtifactError::Unavailable);
        }
        self.objects.borrow_mut().put_artifact(bytes)
    }
}

/// A transport that answers from a queue of recorded responses, then `Transport` errors, and
/// records every URL it was handed, with whether credentials came along.
#[derive(Clone, Default)]
pub struct Recorded {
    pub answers: Rc<RefCell<Vec<RawResponse>>>,
    pub sent: Rc<RefCell<Vec<(String, bool)>>>,
}

impl Recorded {
    pub fn answering(bodies: &[Vec<u8>]) -> Self {
        let this = Self::default();
        for body in bodies {
            this.answer(body.clone());
        }
        this
    }

    pub fn answer(&self, body: Vec<u8>) {
        self.answers
            .borrow_mut()
            .push(RawResponse { status: 200, body });
    }

    pub fn sent(&self) -> Vec<String> {
        self.sent
            .borrow()
            .iter()
            .map(|(url, _)| url.clone())
            .collect()
    }
}

impl Transport for Recorded {
    async fn get(
        &self,
        url: &PermittedUrl,
        auth: Option<&NewsAuth>,
    ) -> Result<RawResponse, SourceError> {
        self.sent
            .borrow_mut()
            .push((url.as_str().to_owned(), auth.is_some()));
        let mut answers = self.answers.borrow_mut();
        if answers.is_empty() {
            return Err(SourceError::Transport);
        }
        Ok(answers.remove(0))
    }
}

pub fn config() -> Config {
    Config {
        workspace: WS.to_owned(),
        environment: Environment::Paper,
        allowlist: Allowlist::parse(ALLOWLIST).unwrap(),
        follows: vec![Follow {
            agent: AGENT.to_owned(),
            account: ACCOUNT.to_owned(),
        }],
        liveness: Duration::from_secs(10),
    }
}

pub type Reader = NewsReader<Journal, Shelf, Recorded>;

pub fn started(journal: &Journal, shelf: &Shelf, transport: &Recorded, now: UtcNanos) -> Reader {
    started_with(journal, shelf, transport, &config(), now)
}

pub fn started_with(
    journal: &Journal,
    shelf: &Shelf,
    transport: &Recorded,
    config: &Config,
    now: UtcNanos,
) -> Reader {
    NewsReader::start(
        journal.clone(),
        shelf.clone(),
        transport.clone(),
        config,
        Ok(paper_auth()),
        now,
    )
}

/// The URL's `symbols` query, as asked.
pub fn asked(url: &str) -> String {
    url.split("symbols=")
        .nth(1)
        .and_then(|rest| rest.split('&').next())
        .unwrap()
        .to_owned()
}

/// The news stream's `NewsItemStored` rows.
pub fn stored(journal: &Journal) -> Vec<StoredEvent> {
    journal
        .news()
        .into_iter()
        .filter(|row| row.event_type == "NewsItemStored")
        .collect()
}

/// The stored items' ids, in stream order.
pub fn ids(journal: &Journal) -> Vec<String> {
    stored(journal)
        .iter()
        .map(|row| member(row, "item_id").as_str().unwrap().to_owned())
        .collect()
}

pub fn object(members: &[(&str, Value)]) -> Value {
    Value::Object(
        members
            .iter()
            .map(|(k, v)| (mandate_canon::Key::new(k).unwrap(), v.clone()))
            .collect(),
    )
}

pub fn s(text: &str) -> Value {
    Value::Str(text.to_owned())
}

/// Registers `asset`'s instrument snapshot naming `symbol` on the control stream, its artifact on
/// the shelf; returns the artifact's reference.
pub fn register(journal: &Journal, shelf: &Shelf, asset: &str, symbol: &str) -> ArtifactRef {
    let snapshot = object(&[
        ("asset_class", s("us_equity")),
        ("etp", s("plain")),
        ("etp_classified_at", s("2026-10-01T00:00:00Z")),
        ("etp_source", s("nasdaq_trader_symbol_directory")),
        ("exchange", s("arca")),
        ("increment", s("whole")),
        ("instrument_id", s(asset)),
        ("symbol", s(symbol)),
    ]);
    let reference = shelf
        .clone()
        .put_artifact(&to_canonical(&snapshot))
        .unwrap();
    register_ref(journal, &reference);
    reference
}

/// A registration naming `reference`, whatever the shelf holds under it.
pub fn register_ref(journal: &Journal, reference: &ArtifactRef) {
    journal.push(
        &format!("ctl:{WS}"),
        "ConfigSnapshotRegistered",
        object(&[
            ("kind", s("instrument_snapshot")),
            ("content_hash", s(&reference.to_string())),
        ]),
    );
}

/// An admitted thesis of the followed agent on `asset`, expiring at `expires_at`.
pub fn thesis(journal: &Journal, thesis_id: &str, asset: &str, expires_at: &str) {
    thesis_of(journal, AGENT, thesis_id, asset, expires_at);
}

/// An admitted thesis of `agent` on `asset`, expiring at `expires_at`.
pub fn thesis_of(journal: &Journal, agent: &str, thesis_id: &str, asset: &str, expires_at: &str) {
    journal.push(
        &format!("agent:{WS}:{agent}"),
        "ThesisProposed",
        object(&[
            ("thesis_id", s(thesis_id)),
            ("lineage_id", s(thesis_id)),
            ("instrument_id", s(asset)),
            ("revision", Value::Int(mandate_canon::Int::new(0).unwrap())),
            ("expires_at", s(expires_at)),
            ("admitted", Value::Bool(true)),
            ("reason", Value::Null),
        ]),
    );
}

pub fn invalidated(journal: &Journal, thesis_id: &str) {
    journal.push(
        &format!("agent:{WS}:{AGENT}"),
        "ThesisInvalidated",
        object(&[("thesis_id", s(thesis_id))]),
    );
}

/// The agent's filled buy of `qty` of `asset` on its account stream.
pub fn bought(journal: &Journal, order: &str, asset: &str, qty: &str) {
    let stream = format!("acct:{WS}:{ACCOUNT}");
    let clock = "2026-10-26T14:30:00.000000000Z";
    journal.push(
        &stream,
        "OrderRequestRecorded",
        object(&[
            ("agent_id", s(AGENT)),
            ("purpose", s("open")),
            ("risk_clock", s(clock)),
        ]),
    );
    journal.push(
        &stream,
        "OrderSubmitted",
        object(&[
            ("client_order_id", s(order)),
            ("instrument_id", s(asset)),
            ("side", s("buy")),
        ]),
    );
    journal.push(
        &stream,
        "FillApplied",
        object(&[
            ("fill_id", s(&format!("f-{order}"))),
            ("client_order_id", s(order)),
            ("instrument_id", s(asset)),
            ("side", s("buy")),
            ("qty_gross", s(qty)),
            ("price", s("580.00")),
            ("risk_clock", s(clock)),
        ]),
    );
}

/// One news item as Alpaca's endpoint writes it.
pub fn item(id: u64, created: &str, updated: &str, symbols: &[&str]) -> String {
    let symbols: Vec<String> = symbols.iter().map(|s| format!("\"{s}\"")).collect();
    format!(
        r#"{{"id":{id},"headline":"Headline {id}","author":"Staff","created_at":"{created}","updated_at":"{updated}","summary":"Summary {id}.","content":"","url":"https://news.example/{id}","symbols":[{}],"source":"benzinga","images":[]}}"#,
        symbols.join(",")
    )
}

/// A page of `items`, with `next` as its continuation token.
pub fn page(items: &[String], next: Option<&str>) -> Vec<u8> {
    let next = next.map_or("null".to_owned(), |t| format!("\"{t}\""));
    format!(
        r#"{{"news":[{}],"next_page_token":{next}}}"#,
        items.join(",")
    )
    .into_bytes()
}

/// A stored row's payload member, as text.
pub fn member(row: &StoredEvent, name: &str) -> Value {
    parse(&row.body)
        .unwrap()
        .get("payload")
        .and_then(|p| p.get(name))
        .cloned()
        .unwrap_or(Value::Null)
}

/// A workspace whose agent holds a thesis on SPY, with SPY's ticker registered.
pub fn spy(journal: &Journal, shelf: &Shelf) {
    register(journal, shelf, SPY, "SPY");
    thesis(journal, "01THESISSPY", SPY, "2026-11-30T00:00:00Z");
}
