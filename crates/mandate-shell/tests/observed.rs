//! The model host's observation and output through the production cycle (E15-13, the first paper
//! trade brief's slice H3; DEC-503 item 6, journal spec §9.1, FT-6). SPY is deployed on the control
//! stream as `tests/registered.rs` deploys it, `mandate-modelhost` evaluates the pin on typed daily
//! closes, and the test plays the paper adapter: it stores the closes' bytes in the run's artifact
//! store and hands the cycle the observation, whose `data_ref` is their digest, with the host's
//! output. The broker is scripted; nothing here touches a network (ADR-0001 ES-19).

mod common;

use std::cell::Cell;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use mandate_alpaca::{Exchange, HttpRequest, Method, Response, TradingTransport, TransportError};
use mandate_canon::{Digest, Value, to_canonical};
use mandate_journal::{ArtifactSource, StoredEvent, TrustedStart, verify_events};
use mandate_modelhost::{DailyCloses, Evaluation, Pin, evaluate};
use mandate_num::Price;
use mandate_runtime::{ModelOutput, Observation};
use mandate_shell::adapters::{Sources, production};
use mandate_shell::paper::load_contexts;
use mandate_shell::stages::Stages;
use mandate_shell::{Report, Setup, ShellError, production_cycle, run_observed};
use mandate_spec::validate::RegisteredModel;
use mandate_time::{Date, ExchangeCalendar, UtcNanos};

use common::{AAPL, FEE, RULES, SNAPSHOT, SPY, Stream, json, model, spy_facts, spy_mandate};

const SOURCE: &str = "alpaca_iex_daily_bars";
/// Tuesday 13:00 in New York: Monday 2026-09-28 is the last completed session.
const TUESDAY: &str = "2026-09-29T17:00:00Z";
/// Wednesday 13:00 in New York: Tuesday 2026-09-29 is the last completed session.
const WEDNESDAY: &str = "2026-09-30T17:00:00Z";
const MONDAY_CLOSE: &str = "2026-09-28T20:00:00.000000000Z";
const TUESDAY_CLOSE: &str = "2026-09-29T20:00:00.000000000Z";
const WEDNESDAY_CLOSE: &str = "2026-09-30T20:00:00.000000000Z";

type Store = BTreeMap<Digest, Vec<u8>>;
type Ran = (Result<Report, ShellError>, Stages, usize);

/// The refusal of an observation whose data is not stored under its `data_ref` (H3).
const UNTRUSTED: &str = "market_data_untrusted";

/// The confirmed document as the file the run's mandate stage reads, written once per process so
/// parallel test threads never read it half written.
static MANDATE_FILE: OnceLock<PathBuf> = OnceLock::new();

fn at(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).unwrap()
}

/// SPY's mandate with its order cap raised to the allocation, so one share at 655.20 fits.
fn mandate() -> String {
    let mandate = spy_mandate(&model());
    mandate.replace(r#""max_order_usd": "300""#, r#""max_order_usd": "1000""#)
}

/// SPY's deployment on the control stream, its ETP classification dated within a week of the runs.
fn stream() -> Stream {
    let snapshot = SNAPSHOT.replace("2026-09-21", "2026-09-25");
    Stream::of(&mandate(), &snapshot, RULES, FEE, &model())
}

/// A fresh paper broker that accepts the one submission, counting each `POST /v2/orders`, and
/// finds no order by its client id, as `tests/tracer.rs`'s scripted broker does.
#[derive(Clone, Default)]
struct Broker {
    posts: Rc<Cell<usize>>,
}

impl TradingTransport for Broker {
    fn send(
        &self,
        request: &HttpRequest,
    ) -> impl Future<Output = Result<Response, TransportError>> {
        let path = request.path_and_query();
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tracer/alpaca");
        let fixture = |name: &str| fs::read(format!("{dir}/{name}")).unwrap();
        let (status, body) = if path.starts_with("/v2/orders:by_client_order_id") {
            (404, fixture("order-absent.json"))
        } else if path == "/v2/account" {
            (200, fixture("account.json"))
        } else if request.method() == Method::Post && path == "/v2/orders" {
            self.posts.set(self.posts.get() + 1);
            let sent = json(request.body().unwrap());
            let id = sent.get("client_order_id").and_then(Value::as_str).unwrap();
            let order = String::from_utf8(fixture("order.json")).unwrap();
            let order = order.replace("md-e144b97773a6f87c1978cc2831", id);
            let order = order.replace(AAPL, SPY).replace(r#""AAPL""#, r#""SPY""#);
            (200, order.into_bytes())
        } else if ["/v2/positions", "/v2/orders?", "/v2/account/activities?"]
            .iter()
            .any(|prefix| path.starts_with(prefix))
        {
            (200, b"[]".to_vec())
        } else {
            panic!("the scripted broker has no answer for {path}")
        };
        async move { Ok(Response { status, body }) }
    }
}

/// What the paper adapter hands the cycle: the observation, and the host's output on its closes.
#[derive(Clone)]
struct Observed {
    observation: Observation,
    output: ModelOutput,
    /// The closes' bytes, which the adapter stores under the observation's `data_ref`.
    bytes: Vec<u8>,
}

impl Observed {
    /// Twenty-five sessions of closes rising a dollar a day from `first_close`, ending on the last
    /// session completed at `now`, and the host's `Long` on them.
    fn at(now: &str, first_close: u32) -> Self {
        let calendar = ExchangeCalendar::us_equities().unwrap();
        let last = calendar.last_completed_regular_session(at(now)).unwrap();
        let mut days = Vec::new();
        let mut day = Date::parse("2026-08-03").unwrap();
        while Some(day) <= last {
            days.extend(calendar.is_trading_day(day).unwrap().then_some(day));
            day = day.next().unwrap();
        }
        let closes: Vec<(Date, Price)> = (first_close..)
            .zip(&days[days.len() - 25..])
            .map(|(close, day)| (*day, Price::parse(&format!("{close}.2")).unwrap()))
            .collect();
        let rows = closes.iter().map(|(d, p)| format!(r#"["{d}","{p}"]"#));
        let rows = rows.collect::<Vec<_>>().join(",");
        let bytes = format!(r#"{{"closes":[{rows}],"instrument_id":"{SPY}"}}"#);
        let document = mandate_spec::Mandate::parse(&json(&mandate())).unwrap();
        let model = document.behavior.signal_models[0].clone();
        let entry = RegisteredModel {
            version: model.version.clone(),
            content_hash: model.content_hash,
            params: ["fast_periods", "slow_periods"].map(str::to_owned).into(),
            admits_instruments: false,
        };
        let registry = BTreeMap::from([(model.id.clone(), entry)]);
        let instrument_id = mandate_accounting::InstrumentId::new(SPY).unwrap();
        let id = instrument_id.clone();
        let pin = Pin {
            model,
            instrument_id: id.clone(),
        };
        let closes = DailyCloses {
            instrument_id: id,
            closes,
        };
        let evaluated = evaluate(&pin, &registry, &calendar, &closes, at(now));
        let Ok(Evaluation::Long(output)) = evaluated else {
            panic!("rising closes give the host's Long at {now}: {evaluated:?}");
        };
        let observation = Observation {
            source: SOURCE.to_owned(),
            instrument_id: Some(instrument_id),
            as_of: output.as_of,
            data_ref: Digest::of(bytes.as_bytes()),
        };
        let (output, bytes) = (*output, bytes.into_bytes());
        Self {
            observation,
            output,
            bytes,
        }
    }

    fn parts(&self) -> (Observation, ModelOutput) {
        (self.observation.clone(), self.output.clone())
    }

    /// A store holding the closes under their digest, as the adapter leaves it.
    fn stored(&self) -> Store {
        BTreeMap::from([(Digest::of(&self.bytes), self.bytes.clone())])
    }
}

/// The production sources and setup for SPY's run at `now`, appending against `store` (none when
/// `None`), with the scripted broker's count of submissions.
fn sources(now: &str, store: Option<Store>) -> (Sources<Broker>, Setup, Rc<Cell<usize>>) {
    let day = Date::parse(&now[..10]).unwrap();
    let artifacts = stream().artifacts_on(day).unwrap();
    let deployed = artifacts.deployment("ws_spy".into(), "agent_spy".into(), "acct_spy".into());
    let deployed = deployed.unwrap();
    let agent = deployed.deployment().agent.clone();
    let facts = spy_facts(SPY, "SPY", Exchange::Arca, at(now));
    let contexts = load_contexts(&artifacts, &facts, at(now), &agent).unwrap();
    let file = MANDATE_FILE.get_or_init(|| {
        let path =
            std::env::temp_dir().join(format!("mandate-shell-h3-{}.json", std::process::id()));
        fs::write(&path, to_canonical(&json(&mandate()))).unwrap();
        path
    });
    let broker = Broker::default();
    let posts = Rc::clone(&broker.posts);
    let sources = Sources {
        mandate: file.clone(),
        dataset: PathBuf::from("the-observed-cycle-reads-no-dataset"),
        journal: None,
        recorded_at: at(now),
        agent,
        workspace: "ws_spy".to_owned(),
        account_ref: "acct_spy".to_owned(),
        executor: Some(contexts.executor),
        run: Some(contexts.run),
        artifacts: store.map(|store| Arc::new(store) as Arc<dyn ArtifactSource + Send + Sync>),
        transport: broker,
    };
    let setup = Setup {
        deployment: deployed.deployment().clone(),
        account_ref: "acct_spy".to_owned(),
        now: at(now),
        place_one_order: true,
        new_cycle: false,
    };
    (sources, setup, posts)
}

fn cycle(now: &str, store: Option<Store>) -> (Stages, Setup, Rc<Cell<usize>>) {
    let (sources, setup, posts) = sources(now, store);
    (production(sources), setup, posts)
}

/// One run at `now` over `observed`, appending against `store`.
fn run_at(now: &str, observed: &Observed, store: Option<Store>) -> Ran {
    let (mut stages, setup, posts) = cycle(now, store);
    let (observation, output) = observed.parts();
    let outcome = run_observed(&mut stages, &setup, observation, output);
    (outcome, stages, posts.get())
}

fn rows(stages: &Stages, stream: &str) -> Vec<StoredEvent> {
    stages.journal.read(stream).unwrap()
}

fn agent(stages: &Stages) -> Vec<StoredEvent> {
    rows(stages, "agent:ws_spy:agent_spy")
}

fn types(rows: &[StoredEvent]) -> Vec<&str> {
    rows.iter().map(|row| row.event_type.as_str()).collect()
}

/// The payload of the one `event_type` record in `rows`, failing when there is none or several.
fn only(rows: &[StoredEvent], event_type: &str) -> Value {
    let found: Vec<_> = rows.iter().filter(|r| r.event_type == event_type).collect();
    assert_eq!(found.len(), 1, "{event_type} in {:?}", types(rows));
    let body = mandate_canon::parse(&found[0].body).unwrap();
    body.get("payload").cloned().unwrap()
}

fn text<'v>(payload: &'v Value, member: &str) -> Option<&'v str> {
    payload.get(member).and_then(Value::as_str)
}

/// FT-6 and journal spec §9.1: the run journals the host's inputs as `ObservationRecorded` before
/// its output as `ModelOutputRecorded`, both before the decision and its intent, and the
/// observation names the stored closes by their digest. The agent stream verifies against the
/// store, and fails §11 check 6 without the closes, so `data_ref` is a checked artifact reference.
#[test]
#[ignore = "pending E15-13"]
fn the_observation_is_journaled_before_the_output_and_names_its_artifact() {
    let observed = Observed::at(TUESDAY, 631);
    let (outcome, stages, posts) = run_at(TUESDAY, &observed, Some(observed.stored()));
    assert_eq!((outcome.unwrap().submitted.len(), posts), (1, 1));
    let agent = agent(&stages);
    let kinds = types(&agent);
    let order = [
        "ObservationRecorded",
        "ModelOutputRecorded",
        "DecisionMade",
        "IntentProposed",
    ];
    let found: Vec<Option<usize>> = order
        .iter()
        .map(|kind| kinds.iter().position(|k| k == kind))
        .collect();
    assert!(found.iter().all(Option::is_some), "{kinds:?}");
    assert!(found.windows(2).all(|pair| pair[0] < pair[1]), "{kinds:?}");
    let observation = only(&agent, "ObservationRecorded");
    let digest = format!("sha256:{}", Digest::of(&observed.bytes));
    assert_eq!(text(&observation, "data_ref"), Some(digest.as_str()));
    assert_eq!(text(&observation, "instrument_id"), Some(SPY));
    assert_eq!(text(&observation, "source"), Some(SOURCE));
    assert_eq!(text(&observation, "as_of"), Some(MONDAY_CLOSE));
    let mut store = stream().store;
    let without = store.clone();
    store.extend(observed.stored());
    verify_events(&agent, TrustedStart::GENESIS, &store).unwrap();
    let missing = verify_events(&agent, TrustedStart::GENESIS, &without);
    assert!(missing.is_err(), "the closes are a checked reference");
}

/// Journal spec §5.1 and §11 check 6, FT-8: the closes are stored before the observation that
/// names them is appended. A run with no store, a store without them, or other bytes under their
/// digest journals neither record, decides nothing and sends nothing; the same run with them
/// stored places its one order through the paper adapter's door, `ProductionCycle::run_observed`
/// (DEC-503 item 2), which sends nothing without them. Each refusal is `market_data_untrusted`:
/// the observed data is not the run's, so nothing is appended for it.
#[test]
#[ignore = "pending E15-13"]
fn an_observation_whose_artifact_is_not_stored_stops_the_run_before_any_order() {
    let observed = Observed::at(TUESDAY, 631);
    let door = |store| {
        let (sources, setup, posts) = sources(TUESDAY, Some(store));
        let (observation, output) = observed.parts();
        let outcome = production_cycle(sources, setup).run_observed(observation, output);
        (outcome, posts.get())
    };
    let (placed, posts) = door(observed.stored());
    assert_eq!((placed.unwrap().submitted.len(), posts), (1, 1));
    let (refused, posts) = door(Store::new());
    let code = refused.as_ref().map_err(ShellError::code).err();
    assert_eq!((code, posts), (Some(UNTRUSTED), 0), "the door: {refused:?}");
    let other = BTreeMap::from([(Digest::of(&observed.bytes), b"other closes".to_vec())]);
    let cases = [None, Some(Store::new()), Some(other)];
    let names = ["no store", "not stored", "other bytes"];
    for (name, store) in names.into_iter().zip(cases) {
        let (outcome, stages, posts) = run_at(TUESDAY, &observed, store);
        let code = outcome.as_ref().map_err(ShellError::code).err();
        assert_eq!((code, posts), (Some(UNTRUSTED), 0), "{name}: {outcome:?}");
        let agent = types(&agent(&stages)).join(" ");
        let account = types(&rows(&stages, "acct:ws_spy:acct_spy")).join(" ");
        assert!(agent.starts_with("StreamOpened"), "{name}: {agent}");
        for kind in ["Observation", "ModelOutput", "Decision", "Intent"] {
            assert!(!agent.contains(kind), "{name}: {agent}");
        }
        for kind in ["Intent", "OrderRequest", "OrderSubmitted"] {
            assert!(!account.contains(kind), "{name}: {account}");
        }
    }
}

/// DEC-503 items 1 and 5, mandate spec §8.2: the output the cycle records and decides on is the
/// host's for the observation it was handed. Two runs on different days over different closes
/// record each one's own observation and the host's own `as_of` and `expires_at` for it, the last
/// close's end and that plus the pinned 86400 s, never the run clock or a value the shell holds.
#[test]
#[ignore = "pending E15-13"]
fn the_cycle_records_the_hosts_output_for_each_observation() {
    let pinned = format!("sha256:{}", Digest::of(model().as_bytes()));
    let cases = [
        (TUESDAY, 631, MONDAY_CLOSE, TUESDAY_CLOSE),
        (WEDNESDAY, 402, TUESDAY_CLOSE, WEDNESDAY_CLOSE),
    ];
    let mut refs = Vec::new();
    for (now, first, as_of, expires_at) in cases {
        let observed = Observed::at(now, first);
        let (outcome, stages, posts) = run_at(now, &observed, Some(observed.stored()));
        assert_eq!((outcome.unwrap().submitted.len(), posts), (1, 1), "{now}");
        let agent = agent(&stages);
        let output = only(&agent, "ModelOutputRecorded");
        let recorded =
            ["as_of", "expires_at", "instrument_id", "content_hash"].map(|m| text(&output, m));
        let expected = [as_of, expires_at, SPY, pinned.as_str()].map(Some);
        assert_eq!(recorded, expected, "{now}");
        let observation = only(&agent, "ObservationRecorded");
        let digest = format!("sha256:{}", Digest::of(&observed.bytes));
        let seen = ["as_of", "data_ref"].map(|m| text(&observation, m));
        assert_eq!(seen, [Some(as_of), Some(digest.as_str())], "{now}");
        refs.push(digest);
    }
    assert_ne!(refs[0], refs[1], "the two runs observed different closes");
}

/// Journal spec §8, FT-7 and FT-12: a restart over the run's journal replays the observation and
/// the output where they were written and journals neither again. The replayed stream still
/// carries one of each, the observation first, and the restart stops at the open cycle with no
/// second submission.
#[test]
#[ignore = "pending E15-13"]
fn the_order_holds_on_replay() {
    let observed = Observed::at(TUESDAY, 631);
    let (mut stages, setup, posts) = cycle(TUESDAY, Some(observed.stored()));
    let (observation, output) = observed.parts();
    let first = run_observed(&mut stages, &setup, observation.clone(), output.clone());
    assert_eq!(first.unwrap().submitted.len(), 1);
    let before = agent(&stages);
    let again = run_observed(&mut stages, &setup, observation, output);
    let code = again.as_ref().map_err(ShellError::code);
    assert_eq!(code.err(), Some("cycle_already_open"), "{again:?}");
    assert_eq!(posts.get(), 1);
    let replayed = agent(&stages);
    let kinds = types(&replayed);
    assert_eq!(replayed[..before.len()], before[..], "{kinds:?}");
    only(&replayed, "ObservationRecorded");
    only(&replayed, "ModelOutputRecorded");
    let position = |kind: &str| kinds.iter().position(|k| *k == kind);
    let (seen, output) = (
        position("ObservationRecorded"),
        position("ModelOutputRecorded"),
    );
    assert!(seen < output, "{kinds:?}");
}
