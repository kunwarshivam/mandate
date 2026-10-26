//! An asked opening granted after its preflight (DEC-943; DEC-940 item 6, gaps 1 and 3): the
//! runner's own grant wait ([`granted_opening`]) and watch end ([`lt10_end`]) over the shell's
//! production stages — the runtime, the executor, its paper binding gate and the validating
//! in-memory journal — against a scripted Alpaca paper broker whose IEX quote is stamped at the
//! clock's instant less an age, and a clock that moves only when the run pauses (ES-19). The
//! founder's grant is committed on the control stream by the clock itself, at the first pause
//! that reaches the grant's delay, exactly as the CLI commits one while the run waits.
//!
//! The sink is the production one: the granted intent carries the execution plan its request
//! bound under the content hash (DEC-942), so no double supplies one. The mandate stage and the
//! builder's two stages are the production ones too: the view resolves the mandate's `role:approver` to the workspace owner,
//! the user who recorded the mandate connection's `ConnectionEstablished` (DEC-953). Nothing here
//! reaches a host or a credential (`AGENTS.md` rule 8).

#[path = "../../mandate-shell/tests/common/mod.rs"]
mod shell_common;

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use mandate_alpaca::{
    BarsRequest, DataTransport, Exchange, HttpRequest, Method, Pause, QuoteRequest, Response,
    TradingTransport, TransportError,
};
use mandate_canon::{Digest, Value, to_canonical};
use mandate_journal::{AppendOutcome, ArtifactSource, StoredEvent};
use mandate_modelhost::{DailyCloses, Evaluation, Pin, evaluate};
use mandate_num::Price;
use mandate_paper::{GrantRefusal, PaperError, fresh_mark, granted_opening, lt10_end};
use mandate_runtime::{ModelOutput, Observation};
use mandate_shell::adapters::{Sources, StoreJournal, production};
use mandate_shell::paper::{PaperClock, load_contexts_with_clock};
use mandate_shell::stages::JournalWriter;
use mandate_shell::symbols::BrokerSymbols;
use mandate_shell::{Cause, Mark, Report, Setup, ShellError, Watch, run_observed_asked_watched};
use mandate_spec::validate::RegisteredModel;
use mandate_time::{Date, ExchangeCalendar, UtcNanos};
use proptest::prelude::*;

use shell_common::{AAPL, FEE, RULES, SNAPSHOT, SPY, Stream, json, model, spy_facts, spy_mandate};

/// Tuesday 2026-09-29 at 13:00 New York, a full session: the preflight's clock.
const PREFLIGHT: &str = "2026-09-29T17:00:00Z";
const NANOS_PER_S: i64 = 1_000_000_000;
/// The rule set's `iex_quote_max_age_s`.
const QUOTE_MAX_AGE_NS: i64 = 10 * NANOS_PER_S;
/// How old each fresh quote is at its read before the grant: half a second.
const CURRENT_NS: i64 = NANOS_PER_S / 2;
/// The workspace's control stream and the run's two streams.
const CONTROL: &str = "ctl:ws_spy";
const AGENT_STREAM: &str = "agent:ws_spy:agent_spy";
const ACCOUNT: &str = "acct:ws_spy:acct_spy";

fn at(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).unwrap()
}

/// SPY's mandate with its order cap raised to the allocation, as the shell's watch runs it, and
/// its routine rule asking for every opening, as `run.rs`'s asking scene deploys it.
fn mandate() -> String {
    spy_mandate(&model())
        .replace(r#""max_order_usd": "300""#, r#""max_order_usd": "1000""#)
        .replace(r#""then": "auto""#, r#""then": "ask""#)
}

/// The run's clock in nanoseconds, moved only by pauses. The first pause that reaches `grant_at`
/// commits the founder's grant of the run's ask on the control stream, then moves the broker's
/// quote to `after_grant`: its age and its bid and ask from then on.
#[derive(Clone)]
struct Clock {
    nanos: Rc<Cell<i64>>,
    grant_at: i64,
    granted: Rc<Cell<bool>>,
    journal: Shared,
    quote: Rc<RefCell<Quoted>>,
    after_grant: Quoted,
}

impl Pause for Clock {
    fn now(&self) -> UtcNanos {
        let nanos = self.nanos.get();
        UtcNanos::from_parts(
            nanos.div_euclid(NANOS_PER_S),
            u32::try_from(nanos.rem_euclid(NANOS_PER_S)).unwrap(),
        )
        .unwrap()
    }

    async fn pause(&self, duration: Duration) {
        let step = i64::try_from(duration.as_nanos()).unwrap();
        assert!(
            step > 0,
            "the run paused for no time: a loop that never ends"
        );
        let next = self.nanos.get() + step;
        assert!(
            next - at(PREFLIGHT).secs() * NANOS_PER_S < 3_600 * NANOS_PER_S,
            "the run paused past an hour: a loop that never ends"
        );
        self.nanos.set(next);
        if !self.granted.get() && next >= self.grant_at {
            self.granted.set(true);
            self.journal.grant(self.now());
            *self.quote.borrow_mut() = self.after_grant.clone();
        }
    }
}

/// The binding gate's clock: the run's own.
struct GateClock(Clock);

impl PaperClock for GateClock {
    fn now(&self) -> Option<UtcNanos> {
        Some(self.0.now())
    }
}

/// The broker's IEX quote: its age at each read and its bid and ask.
#[derive(Clone, Debug)]
struct Quoted {
    age_ns: i64,
    bid: &'static str,
    ask: &'static str,
}

/// The preflight's own quote, as [`spy_facts`] reads it, half a second old at every read.
const PREFLIGHT_QUOTE: Quoted = Quoted {
    age_ns: CURRENT_NS,
    bid: "655.1",
    ask: "655.2",
};

/// The founder's opaque user id: the grant's responder.
const FOUNDER: &str = "user_founder";

/// The `ConnectionEstablished` of the mandate's connection as the CLI records it, by `owner`: the
/// workspace owner, the one member every approver reference resolves to (DEC-953).
fn connection(owner: &str) -> String {
    format!(
        r#"{{"account_ref":"acct_spy","broker":"alpaca","connection_id":"conn_alpaca_paper_01","environment":"paper","scopes":["trading"],"user":"{owner}"}}"#
    )
}

/// The validating in-memory journal the run appends to, shared so the founder's CLI can commit
/// its grant on the control stream while the run waits, and the owner whose `ConnectionEstablished`
/// the control stream holds beside the grant, as the run's own control read holds it, the record
/// the runtime resolves its approvers from again at the wake (DEC-2384 item 6).
#[derive(Clone)]
struct Shared(Rc<RefCell<StoreJournal>>, Option<String>);

impl JournalWriter for Shared {
    fn take_ownership(&mut self, stream: &str) -> Result<u64, Cause> {
        self.0.borrow_mut().take_ownership(stream)
    }

    fn read(&self, stream: &str) -> Result<Vec<StoredEvent>, Cause> {
        self.0.borrow().read(stream)
    }

    fn append(
        &mut self,
        stream: &str,
        expected_head: u64,
        writer_epoch: u64,
        drafts: &[Vec<u8>],
    ) -> AppendOutcome {
        self.0
            .borrow_mut()
            .append(stream, expected_head, writer_epoch, drafts)
    }
}

impl Shared {
    fn rows(&self, stream: &str) -> Vec<StoredEvent> {
        self.0.borrow().read(stream).unwrap()
    }

    fn payloads(&self, stream: &str, event_type: &str) -> Vec<Value> {
        self.rows(stream)
            .iter()
            .filter(|row| row.event_type == event_type)
            .map(|row| {
                mandate_canon::parse(&row.body)
                    .unwrap()
                    .get("payload")
                    .unwrap()
                    .clone()
            })
            .collect()
    }

    /// The founder's `cli_confirm` grant of the run's one ask, submitted and authenticated at
    /// `now`, committed on the control stream as the CLI commits it (journal spec §9.7).
    fn grant(&self, now: UtcNanos) {
        let requests: Vec<StoredEvent> = self
            .rows(AGENT_STREAM)
            .into_iter()
            .filter(|row| row.event_type == "ApprovalRequested")
            .collect();
        let [request] = &requests[..] else {
            panic!("the run asked once before the grant: {}", requests.len());
        };
        let body = mandate_canon::parse(&request.body).unwrap();
        let hash = body.get("payload").unwrap().get("content_hash").unwrap();
        let hash = hash.as_str().unwrap();
        let secs = now.secs();
        let payload = format!(
            r#"{{"agent":"agent_spy","approval":"{approval}","verdict":"approved","content_hash":"{hash}","submitted_at":{secs},"step_up":{{"assertion_id":"founder-gesture-1","authenticated_at":{secs},"method":"cli_confirm"}},"responder":"user_founder","role":"approver"}}"#,
            approval = request.event_id,
        );
        let envelope = format!(
            r#"{{"envelope_version":1,"environment":"paper","event_id":"01K6C000000000000000000001","stream_id":"{CONTROL}","event_type":"ApprovalResponseSubmitted","schema_version":1,"event_time":"{now}","clock_source":"local","causation_id":null,"correlation_id":null,"actor":{{"kind":"user","id":"user_founder","version":"0.1.0","build":"sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"}},"config_refs":{{}},"payload":{payload},"artifact_refs":["{hash}"],"pii_refs":[]}}"#
        );
        let opening = r#"{"actor":{"build":"sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","id":"control_services","kind":"system","version":"0.1.0"},"artifact_refs":[],"causation_id":null,"clock_source":"local","config_refs":{},"correlation_id":null,"envelope_version":1,"environment":"paper","event_id":"01K6C000000000000000000000","event_time":"2026-09-29T16:00:00.000000000Z","event_type":"StreamOpened","payload":{"stream_type":"control","workspace_id":"ws_spy"},"pii_refs":[],"schema_version":1,"stream_id":"ctl:ws_spy"}"#;
        let established = self.1.as_ref().map(|owner| {
            let payload = format!(
                r#"{{"account_ref":"01K6C0000000000000000000A0","broker":"alpaca","connection_id":"conn_alpaca_paper_01","environment":"paper","margin_attestation":null,"scopes":["trading"],"step_up":{{"assertion_id":"cli-assertion-1","authenticated_at":"2026-09-29T16:00:01.000000000Z","method":"cli_confirm"}},"user":"{owner}"}}"#
            );
            format!(
                r#"{{"actor":{{"build":"sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","id":"{owner}","kind":"user","version":"0.1.0"}},"artifact_refs":[],"causation_id":"01K6C00000000000000000000R","clock_source":"local","config_refs":{{}},"correlation_id":null,"envelope_version":1,"environment":"paper","event_id":"01K6C00000000000000000000C","event_time":"2026-09-29T16:00:01.000000000Z","event_type":"ConnectionEstablished","payload":{payload},"pii_refs":[],"schema_version":2,"stream_id":"{CONTROL}"}}"#
            )
        });
        let drafts: Vec<Vec<u8>> = std::iter::once(opening)
            .chain(established.as_deref())
            .chain(std::iter::once(envelope.as_str()))
            .map(|text| to_canonical(&json(text)))
            .collect();
        let mut journal = self.0.borrow_mut();
        let epoch = journal.take_ownership(CONTROL).unwrap();
        let outcome = journal.append(CONTROL, 0, epoch, &drafts);
        assert!(
            matches!(outcome, AppendOutcome::Committed(_)),
            "the grant commits: {outcome:?}"
        );
    }
}

/// A paper broker that accepts every submission, reads the entry back filled, and answers every
/// IEX quote read with [`Quoted`]'s bid and ask, stamped its age before the clock's instant.
#[derive(Clone)]
struct Broker {
    clock: Rc<Cell<i64>>,
    quote: Rc<RefCell<Quoted>>,
    quote_reads: Rc<Cell<usize>>,
    posts: Rc<RefCell<Vec<Value>>>,
}

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../mandate-shell/tests/fixtures/tracer/alpaca"
);

impl Broker {
    fn order(&self, sent: &Value, status: &str, filled: &str) -> Vec<u8> {
        let order = fs::read_to_string(format!("{FIXTURES}/order.json")).unwrap();
        let field = |name: &str| sent.get(name).and_then(Value::as_str).unwrap().to_owned();
        let order = order.replace("md-e144b97773a6f87c1978cc2831", &field("client_order_id"));
        let order = order.replace(AAPL, SPY).replace(r#""AAPL""#, r#""SPY""#);
        let order = order.replace(r#""qty":"1""#, &format!(r#""qty":"{}""#, field("qty")));
        let order = order.replace(r#""side":"buy""#, &format!(r#""side":"{}""#, field("side")));
        let order = order.replace(
            r#""filled_qty":"0""#,
            &format!(r#""filled_qty":"{filled}""#),
        );
        order
            .replace(r#""status":"accepted""#, &format!(r#""status":"{status}""#))
            .into_bytes()
    }

    fn answer(&self, request: &HttpRequest) -> (u16, Vec<u8>) {
        let path = request.path_and_query();
        let fixture = |name: &str| fs::read(format!("{FIXTURES}/{name}")).unwrap();
        let entry = self.posts.borrow().first().cloned();
        if let Some(entry) = entry {
            let id = entry
                .get("client_order_id")
                .and_then(Value::as_str)
                .unwrap();
            if path.starts_with("/v2/orders:by_client_order_id") && path.ends_with(id) {
                let qty = entry.get("qty").and_then(Value::as_str).unwrap().to_owned();
                return (200, self.order(&entry, "filled", &qty));
            }
        }
        if path.starts_with("/v2/orders:by_client_order_id") {
            (404, fixture("order-absent.json"))
        } else if path == "/v2/account" {
            (200, fixture("account.json"))
        } else if request.method() == Method::Post && path == "/v2/orders" {
            let sent = json(request.body().unwrap());
            self.posts.borrow_mut().push(sent.clone());
            (200, self.order(&sent, "accepted", "0"))
        } else if ["/v2/positions", "/v2/orders?", "/v2/account/activities?"]
            .iter()
            .any(|prefix| path.starts_with(prefix))
        {
            (200, b"[]".to_vec())
        } else {
            panic!("the scripted broker has no answer for {path}")
        }
    }

    /// The opening orders the broker was sent: every buy.
    fn openings(&self) -> usize {
        let posts = self.posts.borrow();
        posts
            .iter()
            .filter(|post| post.get("side").and_then(Value::as_str) == Some("buy"))
            .count()
    }
}

impl TradingTransport for Broker {
    fn send(
        &self,
        request: &HttpRequest,
    ) -> impl Future<Output = Result<Response, TransportError>> {
        let (status, body) = self.answer(request);
        async move { Ok(Response { status, body }) }
    }
}

impl DataTransport for Broker {
    fn send(&self, _: &QuoteRequest) -> impl Future<Output = Result<Response, TransportError>> {
        self.quote_reads.set(self.quote_reads.get() + 1);
        let quote = self.quote.borrow().clone();
        let stamp = self.clock.get() - quote.age_ns;
        let stamp = UtcNanos::from_parts(
            stamp.div_euclid(NANOS_PER_S),
            u32::try_from(stamp.rem_euclid(NANOS_PER_S)).unwrap(),
        )
        .unwrap();
        let body = format!(
            r#"{{"quote":{{"ap":{ask},"as":1,"ax":"V","bp":{bid},"bs":2,"bx":"V","c":["R"],"t":"{stamp}","z":"C"}},"symbol":"SPY"}}"#,
            ask = quote.ask,
            bid = quote.bid,
        );
        async move {
            Ok(Response {
                status: 200,
                body: body.into_bytes(),
            })
        }
    }

    async fn send_bars(&self, _: &BarsRequest) -> Result<Response, TransportError> {
        panic!("the grant's wake reads no bars")
    }
}

/// The host's `Long` on 25 rising closes to `now`, and the store holding them (DEC-846 item 3).
fn observed(now: UtcNanos) -> (Observation, ModelOutput, BTreeMap<Digest, Vec<u8>>) {
    let calendar = ExchangeCalendar::us_equities().unwrap();
    let last = calendar.last_completed_regular_session(now).unwrap();
    let (mut days, mut day) = (Vec::new(), Date::parse("2026-08-03").unwrap());
    while Some(day) <= last {
        days.extend(calendar.is_trading_day(day).unwrap().then_some(day));
        day = day.next().unwrap();
    }
    let closes: Vec<(Date, Price)> = (631_u32..)
        .zip(&days[days.len() - 25..])
        .map(|(close, day)| (*day, Price::parse(&format!("{close}.2")).unwrap()))
        .collect();
    let rows: Vec<String> = closes
        .iter()
        .map(|(d, p)| format!(r#"["{d}","{p}"]"#))
        .collect();
    let bytes = format!(
        r#"{{"closes":[{}],"instrument_id":"{SPY}"}}"#,
        rows.join(",")
    );
    let document = mandate_spec::Mandate::parse(&json(&mandate())).unwrap();
    let model = document.behavior.signal_models[0].clone();
    let entry = RegisteredModel {
        version: model.version.clone(),
        content_hash: model.content_hash,
        params: ["fast_periods", "slow_periods"].map(str::to_owned).into(),
        admits_instruments: false,
    };
    let registry = BTreeMap::from([(model.id.clone(), entry)]);
    let id = mandate_accounting::InstrumentId::new(SPY).unwrap();
    let pin = Pin {
        model,
        instrument_id: id.clone(),
    };
    let daily = DailyCloses {
        instrument_id: id.clone(),
        closes,
    };
    let Ok(Evaluation::Long(output)) = evaluate(&pin, &registry, &calendar, &daily, now) else {
        panic!("rising closes are the host's Long at {now}");
    };
    let observation = Observation {
        source: "alpaca_iex_daily_bars".to_owned(),
        instrument_id: Some(id),
        as_of: output.as_of,
        data_ref: Digest::of(bytes.as_bytes()),
    };
    let store = BTreeMap::from([(Digest::of(bytes.as_bytes()), bytes.into_bytes())]);
    (observation, *output, store)
}

/// What one asked run did.
struct Granted {
    outcome: Result<Report, PaperError>,
    broker: Broker,
    journal: Shared,
    clock: Clock,
}

impl Granted {
    /// The re-validation's result and reason, when the runtime wrote one.
    fn revalidated(&self) -> Option<(String, Option<String>)> {
        let payloads = self.journal.payloads(AGENT_STREAM, "ApprovalRevalidated");
        let payload = payloads.first()?;
        let text = |name: &str| payload.get(name).and_then(Value::as_str).map(str::to_owned);
        Some((text("result")?, text("reason")))
    }

    /// The `MarkUpdated` prices the run journaled, in order.
    fn marks(&self) -> Vec<String> {
        self.journal
            .payloads(ACCOUNT, "MarkUpdated")
            .iter()
            .map(|payload| {
                payload
                    .get("price")
                    .and_then(Value::as_str)
                    .unwrap()
                    .to_owned()
            })
            .collect()
    }

    fn placed_once(&self) -> bool {
        matches!(&self.outcome, Ok(report) if report.submitted.len() == 1)
            && self.broker.openings() == 1
    }
}

static MANDATE_FILE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// One placing run whose cycle asks, at [`PREFLIGHT`], waiting at a five-second interval: the
/// founder grants it at the first pause `delay_s` seconds or more after the preflight, and from
/// then the broker's quote is `after_grant`. The wait is the runner's own: each wake reads a
/// fresh IEX quote through the run's own [`fresh_mark`], hands it to the binding gate, and journals its mark
/// before the runtime is handed the answers.
fn granted_after(delay_s: i64, after_grant: Quoted) -> Granted {
    granted_with_owner(Some(FOUNDER), delay_s, after_grant)
}

/// [`granted_with_owner`] by the founder, its journal putting each request's content object in
/// `content`, as the placing run's does (DEC-970 item 4).
fn granted_storing(content: &Contents) -> Granted {
    granted_into(Some(FOUNDER), 30, PREFLIGHT_QUOTE, Some(content.clone()))
}

/// [`granted_after`] on a stream whose mandate connection was recorded by `owner`, or by no one.
fn granted_with_owner(owner: Option<&str>, delay_s: i64, after_grant: Quoted) -> Granted {
    granted_into(owner, delay_s, after_grant, None)
}

/// A content store the test keeps a handle on after the journal takes its own (DEC-970 item 4).
#[derive(Clone, Default)]
struct Contents(Arc<std::sync::Mutex<BTreeMap<Digest, Vec<u8>>>>);

impl ArtifactSource for Contents {
    fn read_artifact(
        &self,
        reference: &mandate_journal::ArtifactRef,
    ) -> Result<Vec<u8>, mandate_journal::ArtifactError> {
        self.0.lock().unwrap().read_artifact(reference)
    }
}

impl mandate_journal::ArtifactStore for Contents {
    fn put_artifact(
        &mut self,
        bytes: &[u8],
    ) -> Result<mandate_journal::ArtifactRef, mandate_journal::ArtifactError> {
        self.0.lock().unwrap().put_artifact(bytes)
    }
}

/// [`granted_with_owner`], with the journal's content store `content`, or none.
fn granted_into(
    owner: Option<&str>,
    delay_s: i64,
    after_grant: Quoted,
    content: Option<Contents>,
) -> Granted {
    let now = at(PREFLIGHT);
    let mut stream = Stream::of(
        &mandate(),
        &SNAPSHOT.replace("2026-09-21", "2026-09-25"),
        RULES,
        FEE,
        &model(),
    );
    if let Some(owner) = owner {
        stream.record("ConnectionEstablished", &connection(owner));
    }
    let artifacts = stream
        .artifacts_on(Date::parse("2026-09-29").unwrap())
        .unwrap();
    let deployed = artifacts
        .deployment("ws_spy".into(), "agent_spy".into(), "acct_spy".into())
        .unwrap();
    let agent = deployed.deployment().agent.clone();
    let facts = spy_facts(SPY, "SPY", Exchange::Arca, now);
    let at_preflight = Mark::of_iex(&facts.broker.quote);
    let (observation, output, store) = observed(now);
    let store: Arc<dyn ArtifactSource + Send + Sync> = Arc::new(store);
    let journal = Shared(
        Rc::new(RefCell::new(
            StoreJournal::from_dsn(None, now)
                .with_artifacts(Some(Arc::clone(&store)))
                .with_content_store(content.map(|c| {
                    let boxed: Box<dyn mandate_journal::ArtifactStore + Send> = Box::new(c);
                    boxed
                })),
        )),
        owner.map(str::to_owned),
    );
    let nanos = Rc::new(Cell::new(now.secs() * NANOS_PER_S));
    let quote = Rc::new(RefCell::new(PREFLIGHT_QUOTE));
    let clock = Clock {
        nanos: Rc::clone(&nanos),
        grant_at: (now.secs() + delay_s) * NANOS_PER_S,
        granted: Rc::default(),
        journal: journal.clone(),
        quote: Rc::clone(&quote),
        after_grant,
    };
    let contexts = load_contexts_with_clock(
        &artifacts,
        &facts,
        now,
        &agent,
        mandate_domain::Environment::Paper,
        Rc::new(GateClock(clock.clone())),
    )
    .unwrap();
    let gate_quote = contexts.quote.clone();
    let file = MANDATE_FILE.get_or_init(|| {
        let path = std::env::temp_dir().join(format!(
            "mandate-paper-grant-marks-{}.json",
            std::process::id()
        ));
        fs::write(&path, to_canonical(&json(&mandate()))).unwrap();
        path
    });
    let broker = Broker {
        clock: nanos,
        quote,
        quote_reads: Rc::default(),
        posts: Rc::default(),
    };
    let mut stages = production(Sources {
        mandate: file.clone(),
        journal: None,
        recorded_at: now,
        agent,
        workspace: "ws_spy".to_owned(),
        account_ref: "acct_spy".to_owned(),
        executor: Some(contexts.executor),
        run: Some(contexts.run),
        artifacts: Some(store),
        content_store: None,
        symbols: BrokerSymbols::registered([(
            mandate_accounting::InstrumentId::new("SPY").unwrap(),
            mandate_accounting::InstrumentId::new(SPY).unwrap(),
        )]),
        transport: broker.clone(),
    });
    stages.journal = Box::new(journal.clone());
    let setup = Setup {
        deployment: deployed.deployment().clone(),
        account_ref: "acct_spy".to_owned(),
        now,
        place_one_order: true,
        new_cycle: false,
        connection_environment: mandate_domain::Environment::Paper,
    };
    let watch = Watch {
        pause: clock.clone(),
        bound: at("2026-09-29T19:50:00Z"),
        interval: Duration::from_secs(5),
    };
    let fresh = || fresh_mark(&broker, &artifacts, clock.clone(), &gate_quote);
    let outcome = run_observed_asked_watched(
        &mut stages,
        &setup,
        observation,
        output,
        &watch,
        &at_preflight,
        |asked| granted_opening(asked, &watch.pause, watch.interval, fresh, || Ok(())),
        &lt10_end,
    );
    Granted {
        outcome,
        broker,
        journal,
        clock,
    }
}

/// DEC-953: the founder's grant is admitted only when the founder is the workspace owner the
/// stream names. With the connection recorded by another user, or by no one, the view resolves no
/// approver the founder is, so admission refuses the grant `not_an_approver`, the wait holds to
/// the deadline, and nothing is sent.
#[test]
fn a_grant_by_anyone_but_the_workspace_owner_is_refused_with_nothing_sent() {
    for owner in [Some("user_other"), None] {
        let run = granted_with_owner(owner, 30, PREFLIGHT_QUOTE);
        assert!(run.outcome.is_err(), "{owner:?}: the run places nothing");
        assert_eq!(run.broker.openings(), 0, "{owner:?}: no opening sent");
        let copies = run.journal.payloads(AGENT_STREAM, "ApprovalResponded");
        let [copy] = &copies[..] else {
            panic!("{owner:?}: the runtime copies the one answer: {copies:?}");
        };
        let text = |name: &str| copy.get(name).and_then(Value::as_str);
        assert_eq!(text("result"), Some("refused"), "{owner:?}");
        assert_eq!(text("reason"), Some("not_an_approver"), "{owner:?}");
        assert_eq!(run.revalidated(), None, "{owner:?}: nothing re-validated");
    }
}

/// DEC-943 items 1 and 2 (DEC-940 item 6's gaps 1 and 3): a grant answered 30 seconds after the
/// preflight — three times the preflight quote's ten-second bound — is admitted and placed once.
/// The cycle's open journals the preflight quote's mark, which the request records as its
/// reference; the grant's wake reads a fresh IEX quote, journals its mark before the runtime
/// re-validates against it, and hands it to the binding gate, which judges it at its own clock.
#[test]
fn a_grant_30_s_after_the_preflight_is_admitted_and_placed_once() {
    let run = granted_after(30, PREFLIGHT_QUOTE);
    assert!(run.placed_once(), "{:?}", run.outcome.as_ref().err());
    assert_eq!(run.revalidated(), Some(("act".to_owned(), None)));
    let marks = run.marks();
    assert_eq!(
        marks.first().map(String::as_str),
        Some("655.1"),
        "the open's mark is the preflight quote's bid"
    );
    assert!(
        marks.len() >= 2 && marks.iter().all(|mark| mark == "655.1"),
        "the open's mark, then one per wake: {marks:?}"
    );
    let requests = run.journal.payloads(AGENT_STREAM, "ApprovalRequested");
    let reference = requests[0].get("reference_mark").unwrap();
    assert_eq!(
        reference.get("price").and_then(Value::as_str),
        Some("655.1"),
        "the request records the open's mark"
    );
    let granted_at = run.clock.grant_at / NANOS_PER_S;
    assert!(
        run.clock.now().secs() >= granted_at,
        "the run waited for the grant"
    );
    assert!(
        run.broker.quote_reads.get() >= 2,
        "a fresh quote per wake: {}",
        run.broker.quote_reads.get()
    );
}

/// DEC-943 item 2, rule 3: the grant's wake reads its fresh quote ten seconds and a nanosecond
/// old, so the read refuses before the runtime is handed the answer — no copy, no intent, and
/// nothing sent. The age bound is never relaxed for a grant.
#[test]
fn a_stale_fresh_read_at_the_grant_refuses_with_nothing_sent() {
    let stale = Quoted {
        age_ns: QUOTE_MAX_AGE_NS + 1,
        ..PREFLIGHT_QUOTE
    };
    let run = granted_after(30, stale);
    assert!(
        matches!(
            &run.outcome,
            Err(PaperError::Shell(ShellError::Refused {
                stage: mandate_shell::Stage::MarketData,
                cause: Cause::Absent { .. },
            }))
        ),
        "{:?}",
        run.outcome.as_ref().err()
    );
    assert_eq!(run.broker.openings(), 0, "nothing is sent");
    assert!(
        run.journal
            .payloads(AGENT_STREAM, "ApprovalResponded")
            .is_empty(),
        "the stale wake handed the runtime nothing"
    );
}

/// DEC-949 (DEC-940 item 7), rule 3: an ask nobody answers inside its 600-second timeout is closed
/// by the runtime itself. The wait ticks the runtime at the deadline, the runtime journals its
/// `ApprovalTimedOut` — exactly `{approval, on_timeout: skip}`, caused by the ask it ends (journal
/// spec §9.7, rule 135) — through the validating journal, the run ends denied, and nothing is
/// sent.
#[test]
fn an_unanswered_ask_is_closed_by_the_runtimes_own_timeout_with_nothing_sent() {
    let run = granted_after(700, PREFLIGHT_QUOTE);
    assert!(
        matches!(&run.outcome, Err(PaperError::Grant(GrantRefusal::TimedOut))),
        "{:?}",
        run.outcome.as_ref().err()
    );
    assert_eq!(run.broker.openings(), 0, "nothing is sent");
    assert_eq!(
        run.clock.now().secs(),
        at(PREFLIGHT).secs() + 600,
        "the run ends at the ask's deadline"
    );
    let rows = run.journal.rows(AGENT_STREAM);
    let request = rows
        .iter()
        .find(|row| row.event_type == "ApprovalRequested")
        .expect("the run asked");
    let timeouts: Vec<&StoredEvent> = rows
        .iter()
        .filter(|row| row.event_type == "ApprovalTimedOut")
        .collect();
    let [timeout] = &timeouts[..] else {
        panic!("the runtime closes the ask once: {}", timeouts.len());
    };
    let body = mandate_canon::parse(&timeout.body).unwrap();
    assert_eq!(
        body.get("payload").unwrap(),
        &json(&format!(
            r#"{{"approval":"{}","on_timeout":"skip"}}"#,
            request.event_id
        ))
    );
    assert_eq!(
        body.get("causation_id").and_then(Value::as_str),
        Some(request.event_id.as_str()),
        "the timeout is caused by the ask it ends"
    );
    let deadline = UtcNanos::from_parts(at(PREFLIGHT).secs() + 600, 0).unwrap();
    assert_eq!(
        body.get("event_time").and_then(Value::as_str),
        Some(deadline.to_string().as_str()),
        "the timeout is journaled at the deadline's wake"
    );
    assert!(
        run.journal
            .payloads(AGENT_STREAM, "ApprovalResponded")
            .is_empty(),
        "no answer reached the runtime"
    );
}

/// Mandate spec §6.4 check 12 (MI-22) with the run's own marks: the grant's wake reads a bid
/// more than 1% from the request's reference — 655.1 × 1.01 is 661.651 — so the re-validation
/// skips the grant as `drift`, the wait denies it, and nothing is sent.
#[test]
fn drift_beyond_the_mandates_band_still_skips_the_grant() {
    let drifted = Quoted {
        bid: "661.652",
        ask: "661.752",
        ..PREFLIGHT_QUOTE
    };
    let run = granted_after(30, drifted);
    assert!(
        matches!(&run.outcome, Err(PaperError::Grant(GrantRefusal::Denied))),
        "{:?}",
        run.outcome.as_ref().err()
    );
    assert_eq!(
        run.revalidated(),
        Some(("skip".to_owned(), Some("drift".to_owned())))
    );
    assert_eq!(run.marks().last().map(String::as_str), Some("661.652"));
    assert_eq!(run.broker.openings(), 0, "nothing is sent");
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 12, failure_persistence: None, ..ProptestConfig::default() })]

    /// DEC-943, over grant delays: a grant answered at any delay inside the ask's 600-second
    /// timeout, its wake reading a quote anywhere from fresh to exactly ten seconds old, is
    /// admitted and placed once; the same grant whose wake reads a quote past the bound, by a
    /// nanosecond to a minute, refuses with nothing sent.
    #[test]
    fn any_grant_delay_places_once_on_a_current_quote_and_never_on_a_stale_one(
        delay_s in 0_i64..=590,
        current_ns in 0_i64..=QUOTE_MAX_AGE_NS,
        stale_ns in QUOTE_MAX_AGE_NS + 1..=60 * NANOS_PER_S,
    ) {
        let current = granted_after(delay_s, Quoted { age_ns: current_ns, ..PREFLIGHT_QUOTE });
        prop_assert!(current.placed_once(), "{:?}", current.outcome.as_ref().err());
        let stale = granted_after(delay_s, Quoted { age_ns: stale_ns, ..PREFLIGHT_QUOTE });
        prop_assert!(stale.outcome.is_err());
        prop_assert_eq!(stale.broker.openings(), 0);
    }
}

/// Journal spec §3 and §11 check 6 (R0's R17, DEC-970 item 4): the `ApprovalRequested` lists its
/// `content_hash` in `artifact_refs`, so the placing run's journal puts the canonical `content` in
/// the store first, under exactly that reference, and `verify` finds it (before, nothing put it, and
/// the rehearsal's verify failed `artifact_missing`).
#[test]
fn the_requests_content_object_is_stored_under_its_content_hash() {
    let content = Contents::default();
    let run = granted_storing(&content);
    assert!(run.placed_once(), "{:?}", run.outcome.as_ref().err());
    let requests = run.journal.payloads(AGENT_STREAM, "ApprovalRequested");
    assert!(!requests.is_empty(), "the run asked");
    for request in &requests {
        let hash = request.get("content_hash").and_then(Value::as_str).unwrap();
        let reference = mandate_journal::ArtifactRef::parse(hash).unwrap();
        let stored = mandate_journal::get_artifact(&content, &reference)
            .unwrap_or_else(|e| panic!("the request's content object is stored: {e:?}"));
        assert_eq!(
            stored,
            to_canonical(request.get("content").unwrap()),
            "the object is the request's canonical content, which re-hashes to its content_hash"
        );
    }
    assert_eq!(
        content.0.lock().unwrap().len(),
        requests.len(),
        "one object per request and nothing else: no other record of the run carries inline content"
    );
}
