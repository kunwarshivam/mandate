//! The policy overlay in the production cycle (mandate spec §6.2 step 5c and §4.3, DEC-534 item 2,
//! DEC-536; `AGENTS.md` rules 2, 3 and 13). SPY is deployed on the control stream as
//! `tests/observed.rs` deploys it, the governance is the real fold of a registered `policy_set` and
//! `model_registry` (`control::governance`), and the run is `production`'s, driven through the paper
//! adapter's door with the model host's output. While the confirmed version is nonconforming, an
//! opening or an increase is denied, named `policy_overlay` only when the overlay changed the
//! decision, and an exit is still decided `auto` by the built-in risk-reducing step and handed on.
//! A run with no governance, or under a policy the mandate conforms to, is the run without one.
//! These pins replace the ones `tests/tracer.rs` held until #1295 and #1296 deleted it. The broker
//! is scripted; nothing here touches a network (ADR-0001 ES-19).

mod common;

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use mandate_accounting::Side;
use mandate_alpaca::{Exchange, HttpRequest, Method, Response, TradingTransport, TransportError};
use mandate_canon::{Digest, Value, to_canonical};
use mandate_domain::Purpose as BuilderPurpose;
use mandate_journal::{ArtifactSource, StoredEvent, TrustedStart, verify_events};
use mandate_modelhost::{DailyCloses, Evaluation, Pin, evaluate};
use mandate_num::{Price, Qty};
use mandate_runtime::{
    Autonomy, Classified, ExitOrigin, MandateView, ModelOutput, Observation, Proposal, Purpose,
    SignalInputs,
};
use mandate_shell::adapters::{BuilderPlan, RunContext, Sources, production};
use mandate_shell::control::{ControlRecord, Governance, Pinned, governance};
use mandate_shell::paper::load_contexts;
use mandate_shell::stages::{Classifier, Sizing, Stages};
use mandate_shell::symbols::BrokerSymbols;
use mandate_shell::{Cause, Report, Setup, ShellError, Stage, run_observed};
use mandate_spec::Mandate;
use mandate_spec::validate::RegisteredModel;
use mandate_time::{Date, ExchangeCalendar, UtcNanos};

use common::{AAPL, FEE, RULES, SNAPSHOT, SPY, Stream, json, model, reference, spy_facts};

/// Tuesday 13:00 in New York: Monday 2026-09-28 is the last completed session.
const TUESDAY: &str = "2026-09-29T17:00:00Z";
const SOURCE: &str = "alpaca_iex_daily_bars";
const AGENT_STREAM: &str = "agent:ws_spy:agent_spy";
const ACCOUNT_STREAM: &str = "acct:ws_spy:acct_spy";
/// A workspace level the E7-7 mandate exceeds twice: its `routine` rule says `auto`, and its daily
/// loss limit is above one percent. The `deny` variant below exceeds both, the `ask` one only the
/// second.
const NONCONFORMING: &str = r#""auto_allowed":false,"max_daily_loss":"0.01""#;
/// A workspace level every variant conforms to.
const CONFORMING: &str = r#""max_order_usd":"1000000""#;
/// The rule the `deny` variant puts first: an opening is denied by the rules themselves.
const NO_OPENS: &str =
    r#"{"id":"no_opens","then":"deny","when":{"field":"purpose","op":"in","value":["open"]}}"#;

type Store = BTreeMap<Digest, Vec<u8>>;

fn at(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).unwrap()
}

/// Which autonomy rules the SPY mandate carries.
#[derive(Clone, Copy, Debug)]
enum Rules {
    /// The E7-7 mandate's own: `routine` makes an opening and an increase `auto`.
    Auto,
    /// `routine` says `ask` instead.
    Ask,
    /// `no_opens` denies an opening first; `routine` still makes an increase `auto`.
    Deny,
}

/// SPY's mandate under `rules`, its order cap raised to the allocation so one share at 655.20 fits.
fn mandate(rules: Rules) -> String {
    let mandate = common::spy_mandate(&model());
    let mandate = mandate.replace(r#""max_order_usd": "300""#, r#""max_order_usd": "1000""#);
    match rules {
        Rules::Auto => mandate,
        Rules::Ask => mandate.replace(r#""then": "auto""#, r#""then": "ask""#),
        Rules::Deny => mandate.replace(r#""rules": ["#, &format!(r#""rules": [{NO_OPENS},"#)),
    }
}

/// `mandate`'s deployment on the control stream, its ETP classification dated within a week.
fn stream(mandate: &str) -> Stream {
    let snapshot = SNAPSHOT.replace("2026-09-21", "2026-09-25");
    Stream::of(mandate, &snapshot, RULES, FEE, &model())
}

/// What governs a run beside its mandate, and the registered objects the journal checks its
/// version-2 refs against.
#[derive(Clone)]
struct Governed {
    governance: Governance,
    objects: Store,
}

/// The governance `control::governance` folds for `mandate` once a policy set holding one
/// workspace level of `values`, and a registry with the pinned model's one entry, are registered
/// after its deployment.
fn governed(mandate: &str, values: &str) -> Governed {
    let mut stream = stream(mandate);
    let content = model();
    let level =
        format!(r#"{{"level":"workspace","policy_schema_version":1,"values":{{{values}}}}}"#);
    let policy = format!(r#"{{"kind":"policy_set","levels":[{level}],"policy_set_version":1}}"#);
    let entry = format!(
        r#"{{"admits_instruments":false,"content_hash":"{}","model_id":"quant.ma_crossover","model_version":"1.0.0","params":["fast_periods","slow_periods"]}}"#,
        reference(content.as_bytes())
    );
    let registry =
        format!(r#"{{"kind":"model_registry","model_registry_version":1,"models":[{entry}]}}"#);
    let mut objects = Store::new();
    for (kind, object) in [("policy_set", &policy), ("model_registry", &registry)] {
        let bytes = to_canonical(&json(object));
        assert_eq!(bytes, object.as_bytes(), "{kind} is written canonical");
        let hash = stream.put(object);
        objects.insert(Digest::of(&bytes), bytes);
        let payload = format!(
            r#"{{"admits_instruments":null,"content_hash":"{hash}","kind":"{kind}","model_id":null,"model_version":null,"params":[]}}"#
        );
        stream.record("ConfigSnapshotRegistered", &payload);
    }
    let pinned = Pinned {
        asset_id: SPY.to_owned(),
        symbol: "SPY".to_owned(),
        model_id: "quant.ma_crossover".to_owned(),
        model_version: "1.0.0".to_owned(),
        content_hash: Digest::of(content.as_bytes()),
    };
    let document = Mandate::parse(&json(mandate)).unwrap();
    let records: &[ControlRecord] = &stream.records;
    let governance = governance(records, &stream.store, &pinned, &document).unwrap();
    Governed {
        governance,
        objects,
    }
}

/// `mandate` under the workspace level it exceeds: nonconforming, which no read refuses.
fn nonconforming(mandate: &str) -> Governed {
    let governed = governed(mandate, NONCONFORMING);
    let violations = &governed.governance.violations;
    assert!(!violations.is_empty(), "the mandate conforms");
    governed
}

/// `mandate` under a looser workspace level: governed, and conforming.
fn conforming(mandate: &str) -> Governed {
    let governed = governed(mandate, CONFORMING);
    let violations = &governed.governance.violations;
    assert!(violations.is_empty(), "{violations:?}");
    governed
}

/// A fresh paper broker that accepts the one submission, counting each `POST /v2/orders`, and
/// finds no order by its client id.
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

/// What the paper adapter hands the cycle: the observation, the host's output on its closes, and
/// the closes' bytes, stored under the observation's `data_ref`.
struct Observed {
    observation: Observation,
    output: ModelOutput,
    bytes: Vec<u8>,
}

/// Twenty-five sessions of closes rising a dollar a day, ending on the last session completed at
/// [`TUESDAY`], and the host's `Long` on them under `mandate`'s pin.
fn observed(mandate: &str) -> Observed {
    let calendar = ExchangeCalendar::us_equities().unwrap();
    let last = calendar
        .last_completed_regular_session(at(TUESDAY))
        .unwrap();
    let mut days = Vec::new();
    let mut day = Date::parse("2026-08-03").unwrap();
    while Some(day) <= last {
        days.extend(calendar.is_trading_day(day).unwrap().then_some(day));
        day = day.next().unwrap();
    }
    let closes: Vec<(Date, Price)> = (631..)
        .zip(&days[days.len() - 25..])
        .map(|(close, day)| (*day, Price::parse(&format!("{close}.2")).unwrap()))
        .collect();
    let rows = closes.iter().map(|(d, p)| format!(r#"["{d}","{p}"]"#));
    let rows = rows.collect::<Vec<_>>().join(",");
    let bytes = format!(r#"{{"closes":[{rows}],"instrument_id":"{SPY}"}}"#);
    let document = Mandate::parse(&json(mandate)).unwrap();
    let model = document.behavior.signal_models[0].clone();
    let entry = RegisteredModel {
        version: model.version.clone(),
        content_hash: model.content_hash,
        params: ["fast_periods", "slow_periods"].map(str::to_owned).into(),
        admits_instruments: false,
    };
    let registry = BTreeMap::from([(model.id.clone(), entry)]);
    let instrument_id = mandate_accounting::InstrumentId::new(SPY).unwrap();
    let pin = Pin {
        model,
        instrument_id: instrument_id.clone(),
    };
    let closes = DailyCloses {
        instrument_id: instrument_id.clone(),
        closes,
    };
    let evaluated = evaluate(&pin, &registry, &calendar, &closes, at(TUESDAY));
    let Ok(Evaluation::Long(output)) = evaluated else {
        panic!("rising closes give the host's Long: {evaluated:?}");
    };
    let observation = Observation {
        source: SOURCE.to_owned(),
        instrument_id: Some(instrument_id),
        as_of: output.as_of,
        data_ref: Digest::of(bytes.as_bytes()),
    };
    Observed {
        observation,
        output: *output,
        bytes: bytes.into_bytes(),
    }
}

/// Each mandate file this process writes gets its own name, so no test thread reads another's
/// file half written.
static FILES: AtomicUsize = AtomicUsize::new(0);

fn mandate_file(mandate: &str) -> PathBuf {
    let n = FILES.fetch_add(1, Ordering::Relaxed);
    let name = format!("mandate-shell-overlay-{}-{n}.json", std::process::id());
    let path = std::env::temp_dir().join(name);
    fs::write(&path, to_canonical(&json(mandate))).unwrap();
    path
}

/// What one run proposes, as the builder sized it or retargeted to another purpose.
#[derive(Clone, Copy, Debug)]
enum Proposing {
    /// The opening the builder sizes on the flat account.
    Opening,
    /// That proposal turned into `purpose` on `side`, with the run's classification facts naming
    /// the same purpose and the advisory gate holding the one share it adds to or sells.
    Retargeted(BuilderPurpose, Purpose, Side),
}

const INCREASE: Proposing =
    Proposing::Retargeted(BuilderPurpose::Increase, Purpose::Increase, Side::Buy);
const EXIT: Proposing = Proposing::Retargeted(
    BuilderPurpose::DiscretionaryExit,
    Purpose::DiscretionaryExit,
    Side::Sell,
);

/// The run's own builder, with each proposal it sizes turned into `purpose` on `side`, so a run
/// reaches the decision for an increase or an exit the flat paper account cannot size itself.
struct Retarget {
    inner: Box<dyn Sizing>,
    side: Side,
    purpose: Purpose,
}

impl Sizing for Retarget {
    fn size(&self, view: &MandateView, inputs: &SignalInputs) -> Result<Option<Proposal>, Cause> {
        let exit_origin = (!self.purpose.adds_risk()).then_some(ExitOrigin::Signal);
        Ok(self.inner.size(view, inputs)?.map(|proposal| Proposal {
            side: self.side,
            purpose: self.purpose,
            exit_origin,
            ..proposal
        }))
    }
}

/// The run's own classifier, keeping the view and proposal it was asked about.
struct Capture {
    inner: Box<dyn Classifier>,
    asked: Rc<RefCell<Option<(MandateView, Proposal)>>>,
}

impl Classifier for Capture {
    fn classify(&self, view: &MandateView, proposal: &Proposal) -> Result<Classified, Cause> {
        *self.asked.borrow_mut() = Some((view.clone(), proposal.clone()));
        self.inner.classify(view, proposal)
    }
}

/// One production run at [`TUESDAY`] and what it left behind.
struct Ran {
    outcome: Result<Report, ShellError>,
    stages: Stages,
    posts: usize,
    /// The mandate file, the run context and the classification question the run asked.
    file: PathBuf,
    context: RunContext,
    asked: Option<(MandateView, Proposal)>,
}

/// SPY's production run under `mandate` and `governed` (none: ungoverned), proposing `proposing`.
/// The journal appends against a store holding the observed closes and the governance's objects.
fn run(mandate: &str, governed: Option<&Governed>, proposing: Proposing) -> Ran {
    let day = Date::parse(&TUESDAY[..10]).unwrap();
    let artifacts = stream(mandate).artifacts_on(day).unwrap();
    let deployed = artifacts.deployment("ws_spy".into(), "agent_spy".into(), "acct_spy".into());
    let deployed = deployed.unwrap();
    let agent = deployed.deployment().agent.clone();
    let facts = spy_facts(SPY, "SPY", Exchange::Arca, at(TUESDAY));
    let contexts = load_contexts(
        &artifacts,
        &facts,
        at(TUESDAY),
        &agent,
        mandate_domain::Environment::Paper,
    )
    .unwrap();
    let mut context = contexts.run;
    context.governance = governed.map(|governed| governed.governance.clone());
    if let Proposing::Retargeted(purpose, _, _) = proposing {
        let decision = context.decision.as_mut().unwrap();
        decision.builder.as_mut().unwrap().action.purpose = purpose;
        let gate = decision.gate.as_mut().unwrap();
        let held = gate.instrument.instrument.clone();
        gate.agent.positions.insert(held, Qty::parse("1").unwrap());
    }
    let observed = observed(mandate);
    let mut store = BTreeMap::from([(Digest::of(&observed.bytes), observed.bytes.clone())]);
    if let Some(governed) = governed {
        store.extend(governed.objects.clone());
    }
    let file = mandate_file(mandate);
    let broker = Broker::default();
    let posts = Rc::clone(&broker.posts);
    let sources = Sources {
        mandate: file.clone(),
        journal: None,
        recorded_at: at(TUESDAY),
        agent,
        workspace: "ws_spy".to_owned(),
        account_ref: "acct_spy".to_owned(),
        executor: Some(contexts.executor),
        run: Some(context.clone()),
        artifacts: Some(Arc::new(store) as Arc<dyn ArtifactSource + Send + Sync>),
        content_store: None,
        symbols: BrokerSymbols::default(),
        transport: broker,
    };
    let setup = Setup {
        deployment: deployed.deployment().clone(),
        account_ref: "acct_spy".to_owned(),
        now: at(TUESDAY),
        place_one_order: true,
        new_cycle: false,
        connection_environment: mandate_domain::Environment::Paper,
    };
    let mut stages = production(sources);
    if let Proposing::Retargeted(_, purpose, side) = proposing {
        let inner = std::mem::replace(&mut stages.sizing, Box::new(Unsized));
        stages.sizing = Box::new(Retarget {
            inner,
            side,
            purpose,
        });
    }
    let asked = Rc::new(RefCell::new(None));
    let inner = std::mem::replace(&mut stages.classifier, Box::new(Unasked));
    stages.classifier = Box::new(Capture {
        inner,
        asked: Rc::clone(&asked),
    });
    let outcome = run_observed(&mut stages, &setup, observed.observation, observed.output);
    let asked = asked.borrow().clone();
    Ran {
        outcome,
        stages,
        posts: posts.get(),
        file,
        context,
        asked,
    }
}

/// The placeholder sizing that stands in only while [`run`] wraps the run's own.
struct Unsized;

impl Sizing for Unsized {
    fn size(&self, _view: &MandateView, _inputs: &SignalInputs) -> Result<Option<Proposal>, Cause> {
        panic!("the placeholder sizing is never asked")
    }
}

/// The placeholder classifier that stands in only while [`run`] wraps the run's own.
struct Unasked;

impl Classifier for Unasked {
    fn classify(&self, _view: &MandateView, _proposal: &Proposal) -> Result<Classified, Cause> {
        panic!("the placeholder classifier is never asked")
    }
}

fn rows(stages: &Stages, stream: &str) -> Vec<StoredEvent> {
    stages.journal.read(stream).unwrap()
}

fn count(rows: &[StoredEvent], event_type: &str) -> usize {
    rows.iter()
        .filter(|row| row.event_type == event_type)
        .count()
}

/// The run's one `DecisionMade`: its schema version and its `autonomy`, `decided_by` and
/// `purpose`, as journaled.
fn decision(ran: &Ran) -> (u64, [Option<String>; 3]) {
    let agent = rows(&ran.stages, AGENT_STREAM);
    let found: Vec<&StoredEvent> = agent
        .iter()
        .filter(|row| row.event_type == "DecisionMade")
        .collect();
    assert_eq!(found.len(), 1, "one decision: {:?}", ran.outcome);
    let body = mandate_canon::parse(&found[0].body).unwrap();
    let payload = body.get("payload").unwrap();
    let field = |name| payload.get(name).and_then(Value::as_str).map(str::to_owned);
    let version = found[0].schema_version;
    (version, ["autonomy", "decided_by", "purpose"].map(field))
}

fn fields(autonomy: &str, decided_by: &str, purpose: &str) -> [Option<String>; 3] {
    [autonomy, decided_by, purpose].map(|text| Some(text.to_owned()))
}

/// Mandate spec §6.2 step 5c and DEC-536 item 2: the run stopped at the classification as a deny,
/// recorded one version-2 `deny` of `purpose` labelled `decided_by`, bound to the effective policy
/// set and registry, proposed no intent and asked no approval, put nothing past reconciliation on
/// the account stream, and sent the broker nothing. Both streams verify against the store.
fn assert_denied(ran: &Ran, mandate: &str, governed: &Governed, purpose: &str, decided_by: &str) {
    let refused = matches!(
        &ran.outcome,
        Err(ShellError::Refused {
            stage: Stage::Classify,
            cause: Cause::NotAuto { autonomy: "deny" },
        })
    );
    assert!(refused, "{purpose}: {:?}", ran.outcome);
    assert_eq!(ran.posts, 0, "{purpose}: the broker saw an order");
    let (version, found) = decision(ran);
    assert_eq!(version, 2, "{purpose}");
    assert_eq!(found, fields("deny", decided_by, purpose));
    let agent = rows(&ran.stages, AGENT_STREAM);
    let decided = agent.iter().find(|row| row.event_type == "DecisionMade");
    let body = mandate_canon::parse(&decided.unwrap().body).unwrap();
    let refs = body.get("config_refs").unwrap();
    let governance = &governed.governance;
    for (field, digest) in [
        ("policy_set", governance.policy_set.content_hash),
        ("model_registry", governance.model_registry.content_hash),
    ] {
        let expected = format!("sha256:{digest}");
        let found = refs.get(field).and_then(Value::as_str);
        assert_eq!(found, Some(expected.as_str()), "{purpose}: {field}");
    }
    for event_type in ["IntentProposed", "ApprovalRequested"] {
        assert_eq!(count(&agent, event_type), 0, "{purpose}: {event_type}");
    }
    let account = rows(&ran.stages, ACCOUNT_STREAM);
    let types: Vec<&str> = account.iter().map(|row| row.event_type.as_str()).collect();
    let reconciled = [
        "StreamOpened",
        "AccountStateObserved",
        "AccountSnapshotRecorded",
        "ReconciliationRun",
    ];
    assert_eq!(types, reconciled, "{purpose}");
    let mut store = stream(mandate).store;
    store.extend(governed.objects.clone());
    let closes = observed(mandate).bytes;
    store.insert(Digest::of(&closes), closes);
    for stream in [AGENT_STREAM, ACCOUNT_STREAM] {
        let rows = rows(&ran.stages, stream);
        let verified = verify_events(&rows, TrustedStart::GENESIS, &store);
        assert!(verified.is_ok(), "{purpose}: {stream}: {verified:?}");
    }
}

/// `context`'s classification, under the mandate at `file`, of `proposal` re-labelled `purpose`,
/// with the classification facts naming the same purpose: the run's own classifier asked directly.
fn classify_as(
    file: &Path,
    context: &RunContext,
    (view, proposal): &(MandateView, Proposal),
    (builder, runtime): (BuilderPurpose, Purpose),
    governance: Option<Governance>,
) -> Result<Classified, String> {
    let mut context = context.clone();
    context.governance = governance;
    let decision = context.decision.as_mut().unwrap();
    decision.builder.as_mut().unwrap().action.purpose = builder;
    let plan = BuilderPlan {
        mandate: file.to_path_buf(),
        context: Some(Rc::new(context)),
    };
    let proposal = Proposal {
        purpose: runtime,
        ..proposal.clone()
    };
    plan.classify(view, &proposal)
        .map_err(|cause| format!("{cause}: {cause:?}"))
}

fn classified(autonomy: Autonomy, decided_by: &str) -> Classified {
    Classified {
        autonomy,
        decided_by: Some(decided_by.to_owned()),
    }
}

/// Mandate spec §6.2 steps 3 and 5c, DEC-534 item 2, `AGENTS.md` rules 2 and 13: every exit is
/// `auto`, decided by the built-in risk-reducing step, whatever governs the run: a nonconforming
/// mandate whose overlay forbids `auto`, a conforming governance, or none. The discretionary sell,
/// the risk exit, the protective order and the owner exit are each asked of the run's own
/// classifier, over the run's own view and proposal, under every autonomy-rule variant, the
/// `deny` one included.
#[test]
fn an_exit_is_auto_while_the_mandate_is_nonconforming() {
    let exits = [
        (
            BuilderPurpose::DiscretionaryExit,
            Purpose::DiscretionaryExit,
        ),
        (BuilderPurpose::RiskExit, Purpose::RiskExit),
        (BuilderPurpose::Protective, Purpose::Protective),
        (BuilderPurpose::OwnerExit, Purpose::OwnerExit),
    ];
    let auto = classified(Autonomy::Auto, "builtin_risk_reducing");
    for rules in [Rules::Auto, Rules::Ask, Rules::Deny] {
        let mandate = mandate(rules);
        let nonconforming = nonconforming(&mandate).governance;
        assert!(!nonconforming.overlay.auto_allowed(), "{rules:?}");
        let ran = run(&mandate, None, Proposing::Opening);
        let asked = ran.asked.clone().unwrap();
        let governances = [
            ("nonconforming", Some(nonconforming)),
            ("conforming", Some(conforming(&mandate).governance)),
            ("ungoverned", None),
        ];
        for (name, governance) in governances {
            for exit in exits {
                let answer = classify_as(&ran.file, &ran.context, &asked, exit, governance.clone());
                assert_eq!(answer, Ok(auto.clone()), "{rules:?}, {name}, {exit:?}");
            }
        }
    }
}

/// `AGENTS.md` rules 3 and 13, DEC-534 item 2: a discretionary exit goes through the whole agent
/// side of a run under a nonconforming mandate, a conforming governance, and none at all. It is
/// decided `auto` by the built-in risk-reducing step, journaled at the governed version, proposed,
/// and handed to the account stream, whose binding gate then decides it on the account's own fold.
#[test]
fn an_exit_is_decided_and_handed_under_a_nonconforming_or_ungoverned_run() {
    let mandate = mandate(Rules::Auto);
    let nonconforming = nonconforming(&mandate);
    let conforming = conforming(&mandate);
    let cases = [
        ("nonconforming", Some(&nonconforming), 2),
        ("conforming", Some(&conforming), 2),
        ("ungoverned", None, 1),
    ];
    for (name, governed, version) in cases {
        let ran = run(&mandate, governed, EXIT);
        let refused_at_classify = matches!(
            &ran.outcome,
            Err(ShellError::Refused {
                stage: Stage::Classify,
                ..
            })
        );
        assert!(!refused_at_classify, "{name}: {:?}", ran.outcome);
        let found = decision(&ran);
        let expected = fields("auto", "builtin_risk_reducing", "discretionary_exit");
        assert_eq!(found, (version, expected), "{name}");
        let agent = rows(&ran.stages, AGENT_STREAM);
        assert_eq!(count(&agent, "IntentProposed"), 1, "{name}");
        let account = rows(&ran.stages, ACCOUNT_STREAM);
        assert_eq!(count(&account, "IntentReceived"), 1, "{name}");
    }
}

/// DEC-534 and `AGENTS.md` rules 1 and 2: an opening under a confirmed mandate that the effective
/// policy set makes nonconforming places nothing. The run reaches the decision, since the model
/// output is journaled, and then no intent is proposed or received, no order is recorded or
/// submitted, the broker sees no order, and the run reports none. The same run ungoverned, or under
/// a policy the mandate conforms to, places its one order.
#[test]
fn a_nonconforming_mandate_places_no_opening_order() {
    let mandate = mandate(Rules::Auto);
    let ran = run(&mandate, Some(&nonconforming(&mandate)), Proposing::Opening);
    let agent = rows(&ran.stages, AGENT_STREAM);
    assert_eq!(count(&agent, "ModelOutputRecorded"), 1, "{:?}", ran.outcome);
    assert!(ran.outcome.is_err(), "placed: {:?}", ran.outcome);
    assert_eq!(ran.posts, 0, "the broker saw an order");
    assert_eq!(count(&agent, "IntentProposed"), 0);
    let account = rows(&ran.stages, ACCOUNT_STREAM);
    for event_type in ["IntentReceived", "OrderRequestRecorded", "OrderSubmitted"] {
        assert_eq!(count(&account, event_type), 0, "{event_type}");
    }
    let conforming = conforming(&mandate);
    for (name, governed) in [("ungoverned", None), ("conforming", Some(&conforming))] {
        let ran = run(&mandate, governed, Proposing::Opening);
        let placed = ran.outcome.as_ref().map(|report| report.submitted.len());
        assert_eq!(
            (placed.ok(), ran.posts),
            (Some(1), 1),
            "{name}: {:?}",
            ran.outcome
        );
    }
}

/// Mandate spec §6.2 step 5c, DEC-534 item 2, DEC-536: an opening under a nonconforming mandate is
/// denied by the overlay whatever the rules decided first: the E7-7 mandate's `auto` rule and the
/// `ask` variant's `ask` rule both become `deny`, named `policy_overlay`. The run records the deny
/// and places nothing. Ungoverned, or under a conforming policy, the same rules decide as they
/// would alone: `auto` by `rule:routine`, and `ask` by `rule:routine`.
#[test]
fn an_opening_under_a_nonconforming_mandate_is_denied_by_the_policy_overlay() {
    for (rules, alone) in [(Rules::Auto, "auto"), (Rules::Ask, "ask")] {
        let mandate = mandate(rules);
        let governed = nonconforming(&mandate);
        let ran = run(&mandate, Some(&governed), Proposing::Opening);
        assert_denied(&ran, &mandate, &governed, "open", "policy_overlay");
        let conforming = conforming(&mandate);
        for (name, governed, version) in [
            ("ungoverned", None, 1),
            ("conforming", Some(&conforming), 2),
        ] {
            let ran = run(&mandate, governed, Proposing::Opening);
            let expected = fields(alone, "rule:routine", "open");
            assert_eq!(decision(&ran), (version, expected), "{rules:?}, {name}");
        }
    }
}

/// DEC-534 item 2: an increase is denied the same way, named `policy_overlay`, and ungoverned or
/// under a conforming policy it is the rules' `auto`. No paper account sizes an increase, since the
/// assembly refuses a held position, so the run's own sizing is retargeted to one.
#[test]
fn an_increase_under_a_nonconforming_mandate_is_denied_by_the_policy_overlay() {
    let mandate = mandate(Rules::Auto);
    let governed = nonconforming(&mandate);
    let ran = run(&mandate, Some(&governed), INCREASE);
    assert_denied(&ran, &mandate, &governed, "increase", "policy_overlay");
    let conforming = conforming(&mandate);
    for (name, governed, version) in [
        ("ungoverned", None, 1),
        ("conforming", Some(&conforming), 2),
    ] {
        let ran = run(&mandate, governed, INCREASE);
        let expected = fields("auto", "rule:routine", "increase");
        assert_eq!(decision(&ran), (version, expected), "{name}");
    }
}

/// Mandate spec §6.2 step 5c and DEC-536 item 2: the overlay names itself only when it changed the
/// decision. Under the `deny` variant, nonconforming because its daily loss limit exceeds the
/// policy's, an opening that its first rule `no_opens` already denies stays `deny` labelled
/// `rule:no_opens`, never `policy_overlay`, and the run journals that label and places nothing. An
/// increase, which `routine` makes `auto`, is the overlay's own deny, so the same mandate shows the
/// label follows the step that decided.
#[test]
fn a_deny_the_rules_decided_under_a_nonconforming_mandate_keeps_the_rule_label() {
    let mandate = mandate(Rules::Deny);
    let governed = nonconforming(&mandate);
    let ran = run(&mandate, Some(&governed), Proposing::Opening);
    assert_denied(&ran, &mandate, &governed, "open", "rule:no_opens");
    let ran = run(&mandate, Some(&governed), INCREASE);
    assert_denied(&ran, &mandate, &governed, "increase", "policy_overlay");
    let ran = run(&mandate, None, Proposing::Opening);
    let expected = fields("deny", "rule:no_opens", "open");
    assert_eq!(decision(&ran), (1, expected));
}

/// The classifier answers only for the action its facts describe, and refuses first, before any
/// overlay could relabel it: a proposal differing from the run's classification facts in exactly
/// one of instrument, asset class, purpose, combined score, quantity or limit is refused as
/// `Absent`, governed or not, while the proposal as asked is classified.
#[test]
fn classification_refuses_each_mismatched_action_fact() {
    let mandate = mandate(Rules::Auto);
    let ran = run(&mandate, None, Proposing::Opening);
    let (view, asked) = ran.asked.clone().unwrap();
    let opening = (BuilderPurpose::Open, Purpose::Open);
    let other_qty = if asked.qty == Qty::parse("2").unwrap() {
        "3"
    } else {
        "2"
    };
    let mismatched = [
        (
            "instrument",
            Proposal {
                instrument: mandate_accounting::InstrumentId::new(AAPL).unwrap(),
                ..asked.clone()
            },
            opening,
        ),
        (
            "asset class",
            Proposal {
                asset_class: mandate_accounting::AssetClass::Crypto,
                ..asked.clone()
            },
            opening,
        ),
        (
            "purpose",
            asked.clone(),
            (BuilderPurpose::Open, Purpose::Increase),
        ),
        (
            "combined score",
            Proposal {
                combined_score: Value::Str("0.5".to_owned()),
                ..asked.clone()
            },
            opening,
        ),
        (
            "quantity",
            Proposal {
                qty: Qty::parse(other_qty).unwrap(),
                ..asked.clone()
            },
            opening,
        ),
        (
            "limit",
            Proposal {
                limit: Price::parse("1.01").unwrap(),
                ..asked.clone()
            },
            opening,
        ),
    ];
    let governances = [
        ("ungoverned", None),
        ("nonconforming", Some(nonconforming(&mandate).governance)),
    ];
    for (name, governance) in governances {
        let pair = (view.clone(), asked.clone());
        let answer = classify_as(&ran.file, &ran.context, &pair, opening, governance.clone());
        assert!(answer.is_ok(), "{name}: the proposal as asked: {answer:?}");
        for (fact, proposal, purposes) in &mismatched {
            let pair = (view.clone(), proposal.clone());
            let answer = classify_as(
                &ran.file,
                &ran.context,
                &pair,
                *purposes,
                governance.clone(),
            );
            let refused = answer.as_ref().err();
            let expected = "classification facts for the proposed action";
            let named = refused.is_some_and(|cause| cause.contains(expected));
            assert!(named, "{name}: {fact}: {answer:?}");
        }
    }
}
