//! Fixtures and doubles shared by the worker's tests: readers that answer from vectors, so a test
//! states only what a reader returns, and the research gateway over the recorded fake.

#![allow(dead_code, reason = "each test binary uses a different subset")]

use std::cell::Cell;
use std::collections::BTreeSet;

use mandate_canon::Digest;
use mandate_domain::{AssetClass, AssetId};
use mandate_gateway::fake::{Recorded, RecordedTransport};
use mandate_gateway::guard::GuardContext;
use mandate_gateway::meter::{Caps, DeploymentMeter, MemoryMeter, Meter};
use mandate_gateway::money::{PriceTable, TokenPrice, Usage, Usd6};
use mandate_gateway::policy::EndpointPolicy;
use mandate_gateway::provider::{
    Clock, Delivery, HttpRequest, ManualClock, NoKeys, ProviderKey, SecretError, SecretSource,
    SequentialIds, Transport,
};
use mandate_gateway::registry::{
    Endpoint, Identity, Locality, Registry, Reported, Route, Snapshot, Status,
};
use mandate_gateway::{Gateway, RateCaps, Settings};
use mandate_num::{Price, Qty};
use mandate_research_run::{
    Allowlist, Bar, BarReader, Filing, FilingEntry, FilingReader, FilingText, GatewayParts,
    InstrumentFact, InstrumentReader, ItemId, ModelPin, NewsItem, NewsReader, NewsText,
    NoHostedTransport, OutputBounds, Quote, ReadError, Readers, ResearchEntry, ResearchTransport,
    RetrievalPlan, Scope, Selection, SourceId, Symbol, TextQuery, recorded_reply, research_gateway,
};
use mandate_time::UtcNanos;

/// The cut-off every case reads at: 2026-10-16 20:00 UTC, a Friday after the close.
pub const CUT_OFF: i64 = 1_792_180_800;
pub const DAY: i64 = 86_400;
pub const NEWSWIRE: &str = "src.newswire";
pub const WIRE_TWO: &str = "src.wire_two";
pub const EDGAR: &str = "src.edgar";
pub const BLOG: &str = "src.anonymous_blog";
pub const AAPL_ID: &str = "7b4a1c2e-1111-4a2b-9c3d-000000000001";
pub const MSFT_ID: &str = "7b4a1c2e-2222-4a2b-9c3d-000000000002";

pub fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).unwrap_or_else(|e| panic!("{secs} is an instant: {e}"))
}

pub fn symbol(text: &str) -> Symbol {
    Symbol::new(text).unwrap_or_else(|e| panic!("`{text}` is a symbol: {e}"))
}

pub fn item(text: &str) -> ItemId {
    ItemId::new(text).unwrap_or_else(|e| panic!("`{text}` is an item id: {e}"))
}

pub fn source(text: &str) -> SourceId {
    SourceId::new(text).unwrap_or_else(|e| panic!("`{text}` is a source id: {e}"))
}

pub fn price(text: &str) -> Price {
    Price::parse(text).unwrap_or_else(|e| panic!("`{text}` is a price: {e}"))
}

pub fn qty(text: &str) -> Qty {
    Qty::parse(text).unwrap_or_else(|e| panic!("`{text}` is a quantity: {e}"))
}

pub fn bar(name: &str, knowledge_at: i64, close: &str) -> Bar {
    Bar {
        symbol: symbol(name),
        knowledge_at: at(knowledge_at),
        open: price(close),
        high: price(close),
        low: price(close),
        close: price(close),
        volume: qty("1000"),
    }
}

/// A daily bar for each of the `count` sessions ending the day before the cut-off, oldest first.
pub fn bars(name: &str, count: i64) -> Vec<Bar> {
    (0..count)
        .rev()
        .map(|back| bar(name, CUT_OFF - DAY * (back + 1), "100"))
        .collect()
}

pub fn news(id: &str, from: &str, about: &str, knowledge_at: i64) -> NewsItem {
    NewsItem {
        item_id: item(id),
        source: source(from),
        symbols: BTreeSet::from([symbol(about)]),
        knowledge_at: at(knowledge_at),
        published_at: at(knowledge_at),
        headline: format!("Headline {id}"),
        summary: format!("Summary of {id}."),
        link: format!("https://news.example/{id}"),
        content_hash: Digest::of(id.as_bytes()),
        licensed: false,
    }
}

pub fn filing(id: &str, about: &str, knowledge_at: i64) -> Filing {
    Filing {
        item_id: item(id),
        source: source(EDGAR),
        symbol: symbol(about),
        form: "8-K".to_owned(),
        knowledge_at: at(knowledge_at),
        filed_at: at(knowledge_at),
        excerpt: format!("Excerpt of {id}."),
        content_hash: Digest::of(id.as_bytes()),
        licensed: false,
    }
}

pub fn instrument(name: &str, id: &str) -> InstrumentFact {
    InstrumentFact {
        asset_id: AssetId::parse(id).unwrap_or_else(|e| panic!("`{id}` is an asset id: {e}")),
        symbol: symbol(name),
        asset_class: AssetClass::UsEquity,
        eligible: true,
        group: None,
        leveraged_or_inverse_etp: false,
    }
}

pub fn scope() -> Scope {
    Scope {
        workspace: "ws1".to_owned(),
        agent: "agent-a".to_owned(),
        cut_off: at(CUT_OFF),
        data_universe: BTreeSet::from([symbol("AAPL"), symbol("MSFT")]),
        held: BTreeSet::new(),
        allowlist: Allowlist {
            version: 3,
            sources: BTreeSet::from([source(NEWSWIRE), source(WIRE_TWO), source(EDGAR)]),
        },
    }
}

pub fn plan() -> RetrievalPlan {
    RetrievalPlan::v0_1()
}

/// One `filing_text` ask: the item, the cut-off, the allowlist version, the offset and the length.
pub type RangeAsk = (ItemId, UtcNanos, u32, u64, u32);

/// Readers that answer from vectors and count their calls. A reader that ignores the query's
/// window or cut-off is exactly what these are, so a case can show the worker filters again.
#[derive(Default)]
pub struct Fixture {
    pub bars: Vec<Bar>,
    pub news: Vec<NewsItem>,
    pub filings: Vec<Filing>,
    pub instruments: Vec<InstrumentFact>,
    pub failing: bool,
    pub calls: Cell<u32>,
    pub queries: std::cell::RefCell<Vec<TextQuery>>,
    pub terms: std::cell::RefCell<Vec<Vec<String>>>,
    pub texts: Vec<NewsText>,
    pub text_asks: std::cell::RefCell<Vec<(ItemId, UtcNanos, u32)>>,
    pub filing_entries: Vec<FilingEntry>,
    pub forms: std::cell::RefCell<Vec<Vec<String>>>,
    /// What `filing_text` answers; `None` reads as [`ReadError::Unavailable`].
    pub filing_text: Option<Result<FilingText, ReadError>>,
    pub range_asks: std::cell::RefCell<Vec<RangeAsk>>,
    /// What `last_quote` answers for each symbol asked, by name; a name absent answers none.
    pub quotes: std::collections::BTreeMap<String, Quote>,
    pub quote_asks: std::cell::RefCell<Vec<(Symbol, UtcNanos)>>,
}

impl Fixture {
    pub fn readers(&self) -> Readers<'_> {
        Readers {
            bars: self,
            news: self,
            filings: self,
            instruments: self,
        }
    }

    fn answer<T: Clone>(&self, rows: &[T]) -> Result<Vec<T>, ReadError> {
        self.calls.set(self.calls.get() + 1);
        if self.failing {
            return Err(ReadError::Unavailable);
        }
        Ok(rows.to_vec())
    }
}

impl BarReader for Fixture {
    fn daily_bars(
        &self,
        name: &Symbol,
        _cut_off: UtcNanos,
        _limit: u32,
    ) -> Result<Vec<Bar>, ReadError> {
        let rows: Vec<Bar> = self
            .bars
            .iter()
            .filter(|bar| bar.symbol == *name)
            .cloned()
            .collect();
        self.answer(&rows)
    }

    fn last_quote(&self, name: &Symbol, cut_off: UtcNanos) -> Result<Option<Quote>, ReadError> {
        self.quote_asks.borrow_mut().push((name.clone(), cut_off));
        let rows: Vec<Quote> = self
            .quotes
            .get(name.as_str())
            .cloned()
            .into_iter()
            .collect();
        Ok(self.answer(&rows)?.into_iter().next())
    }
}

impl NewsReader for Fixture {
    fn news(&self, query: &TextQuery) -> Result<Vec<NewsItem>, ReadError> {
        self.queries.borrow_mut().push(query.clone());
        self.answer(&self.news)
    }

    fn search(&self, query: &TextQuery, terms: &[String]) -> Result<Vec<NewsItem>, ReadError> {
        self.terms.borrow_mut().push(terms.to_vec());
        self.news(query)
    }

    fn news_text(
        &self,
        item: &ItemId,
        cut_off: UtcNanos,
        allowlist_version: u32,
    ) -> Result<NewsText, ReadError> {
        self.text_asks
            .borrow_mut()
            .push((item.clone(), cut_off, allowlist_version));
        let answered = self.answer(&self.texts)?;
        answered.into_iter().next().ok_or(ReadError::Unavailable)
    }
}

impl FilingReader for Fixture {
    fn filings(&self, query: &TextQuery) -> Result<Vec<Filing>, ReadError> {
        self.queries.borrow_mut().push(query.clone());
        self.answer(&self.filings)
    }

    fn search(
        &self,
        query: &TextQuery,
        forms: &[String],
        terms: &[String],
    ) -> Result<Vec<FilingEntry>, ReadError> {
        self.queries.borrow_mut().push(query.clone());
        self.forms.borrow_mut().push(forms.to_vec());
        self.terms.borrow_mut().push(terms.to_vec());
        self.answer(&self.filing_entries)
    }

    fn filing_text(
        &self,
        item: &ItemId,
        cut_off: UtcNanos,
        allowlist_version: u32,
        offset: u64,
        length: u32,
    ) -> Result<FilingText, ReadError> {
        self.range_asks.borrow_mut().push((
            item.clone(),
            cut_off,
            allowlist_version,
            offset,
            length,
        ));
        self.calls.set(self.calls.get() + 1);
        self.filing_text
            .clone()
            .unwrap_or(Err(ReadError::Unavailable))
    }
}

impl InstrumentReader for Fixture {
    fn instruments(&self, _symbols: &BTreeSet<Symbol>) -> Result<Vec<InstrumentFact>, ReadError> {
        self.answer(&self.instruments)
    }
}

/// The research entry's pinned model: one OpenRouter slug served by one upstream (DEC-997 item 2).
pub const SLUG: &str = "anthropic/claude-sonnet-4.5";
pub const UPSTREAM: &str = "anthropic";
pub const REPORTED_PROVIDER: &str = "Anthropic";
pub const HOST: &str = "openrouter.ai";
pub const WORKSPACE: &str = "ws1";
pub const AGENT: &str = "agent-a";

pub fn identity() -> Identity {
    Identity {
        provider: "anthropic".to_owned(),
        model_name: SLUG.to_owned(),
        snapshot: Snapshot::Dated("claude-sonnet-4-5-20250929".to_owned()),
        quantization: None,
    }
}

pub fn entry() -> ResearchEntry {
    ResearchEntry {
        model_id: "llm.research_agent".to_owned(),
        model_version: "0.1.0".to_owned(),
        identity: identity(),
        deadline_ms: 120_000,
        max_output_tokens: 4_096,
        seed: Some(7),
        plan: RetrievalPlan::v0_1(),
        bounds: OutputBounds::v0_1(),
        agentic: None,
    }
}

pub fn endpoint() -> Endpoint {
    Endpoint {
        id: "openrouter:anthropic".to_owned(),
        host: HOST.to_owned(),
        region: "us".to_owned(),
        locality: Locality::Hosted,
        route: Route {
            upstream: UPSTREAM.to_owned(),
            reported_provider: REPORTED_PROVIDER.to_owned(),
        },
        prices: PriceTable::new(
            TokenPrice::per_million(Usd6::parse("3").unwrap_or_else(|e| panic!("{e}"))),
            TokenPrice::per_million(Usd6::parse("15").unwrap_or_else(|e| panic!("{e}"))),
            TokenPrice::per_million(Usd6::parse("0.3").unwrap_or_else(|e| panic!("{e}"))),
            Digest::of(b"openrouter anthropic price table 1"),
        )
        .unwrap_or_else(|e| panic!("{e}")),
    }
}

/// The registry with the research entry registered, offered, and on its one endpoint.
pub fn registry() -> (Registry, ModelPin) {
    let mut registry = Registry::new();
    let content = entry().content();
    let pin = content.model_ref();
    registry
        .register(content)
        .unwrap_or_else(|e| panic!("the research entry registers: {e}"));
    let probe = Reported {
        model: Some(SLUG.to_owned()),
        provider: Some(REPORTED_PROVIDER.to_owned()),
    };
    registry
        .add_endpoint(&pin, endpoint(), &[probe])
        .unwrap_or_else(|e| panic!("{e}"));
    registry
        .set_status(&pin, Status::Offered)
        .unwrap_or_else(|e| panic!("{e}"));
    (registry, pin)
}

pub fn parts(caps: Caps) -> GatewayParts<MemoryMeter, ManualClock> {
    GatewayParts {
        settings: Settings {
            workspace_id: WORKSPACE.to_owned(),
            policy: EndpointPolicy::new(
                BTreeSet::from([Locality::Hosted]),
                Some(BTreeSet::from([UPSTREAM.to_owned()])),
                None,
            ),
            guard: GuardContext::default(),
            rate: RateCaps {
                per_workspace: 100,
                per_agent: 100,
            },
        },
        registry: registry().0,
        meter: Meter::new(
            MemoryMeter::new(),
            caps,
            DeploymentMeter::new(
                Box::new(MemoryMeter::new()),
                Usd6::dollars(1_000_000).unwrap_or(Usd6::ZERO),
                Box::new(SequentialIds::starting_at(1 << 100)),
            ),
        ),
        clock: ManualClock::at(at(CUT_OFF + 5)),
        ids: Box::new(SequentialIds::starting_at(1 << 80)),
    }
}

/// A key source holding one fixture key, never a real credential.
pub struct FixtureVault;

pub const FIXTURE_KEY: &str = "fixture-key-not-a-secret-0001";

impl SecretSource for FixtureVault {
    fn provider_key(&self, upstream: &str) -> Result<ProviderKey, SecretError> {
        if upstream == UPSTREAM {
            Ok(ProviderKey::new(FIXTURE_KEY.to_owned()))
        } else {
            Err(SecretError::Missing)
        }
    }
}

/// A stand-in for the cell's hosted transport: it replays like the recorded fake and records the
/// key it was handed, so a test can show which transport a call went through.
#[derive(Default)]
pub struct HostedDouble {
    pub inner: RecordedTransport,
    pub keys: Vec<String>,
}

impl Transport for HostedDouble {
    fn send(
        &mut self,
        request: &HttpRequest,
        key: Option<&ProviderKey>,
        deadline: UtcNanos,
        clock: &mut dyn Clock,
    ) -> Delivery {
        self.keys
            .extend(key.map(|key| key.expose_secret().to_owned()));
        self.inner.send(request, key, deadline, clock)
    }
}

pub type ResearchGateway = Gateway<ResearchTransport<NoHostedTransport>, MemoryMeter, ManualClock>;

/// The default gateway: the recorded fake, with `replies` scripted in order, each 30 s long.
pub fn recorded_gateway(replies: Vec<Vec<u8>>) -> (ResearchGateway, Selection) {
    let mut recorded = RecordedTransport::new();
    recorded.script(
        replies
            .into_iter()
            .map(|body| Recorded {
                latency_ms: 30_000,
                delivery: Delivery::Response {
                    status: 200,
                    body,
                    retry_after_ms: None,
                },
            })
            .collect(),
    );
    research_gateway(
        parts(budget()),
        Box::new(NoKeys),
        None::<NoHostedTransport>,
        recorded,
        UPSTREAM,
    )
}

pub fn usage() -> Usage {
    Usage {
        input: 2_000,
        output: 300,
        cached: 0,
    }
}

/// The pinned model's reply carrying `output`.
pub fn reply(output: &str) -> Vec<u8> {
    recorded_reply(output, SLUG, REPORTED_PROVIDER, usage())
}

/// A workspace's monthly model budget for the tests, a setting (DEC-2207 item 1): $50 here.
pub fn budget() -> Caps {
    Caps::new(Usd6::dollars(50).unwrap())
}
