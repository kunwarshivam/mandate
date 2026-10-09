#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]
//! The paper adapter: one run of the production cycle on the founder's Alpaca **paper** account
//! (E7-19 slice 5, the [first paper trade brief](../../../docs/project/tasks/first-paper-trade.md)'s
//! E1a, [DEC-846](../../../docs/project/decisions/DEC-846.md)).
//!
//! The only crate that sees both the model host and the shell (DEC-503 item 2). A run: the host
//! refusal, the control stream, E19-11's phase 1 and the artifacts (DEC-505), before any
//! credential; the credentials, the GET-only preflight and phase 2 (V-002); the trusted closes and
//! the host, where `Flat` or `Undecided` ends it; the closes stored, and only then the observation
//! and output handed to [`mandate_shell::ProductionCycle::run_observed`], the one door (FT-1,
//! FT-6). No product value is a constant here (FT-2).

use std::path::PathBuf;

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use mandate_accounting::InstrumentId;
use mandate_alpaca::{
    AlpacaPaperHttp, Credentials, DataTransport, Pause, TokioPause, TradingTransport,
};
use mandate_artifacts_fs::FsArtifactStore;
use mandate_canon::{Key, Object, Value, to_canonical};
use mandate_journal::{ArtifactRef, ArtifactStore, StoredEvent, StreamId};
use mandate_journal_pg::PgJournal;
use mandate_modelhost::{DailyCloses, Evaluation, Pin, Refusal, Signal, evaluate};
use mandate_num::Price;
use mandate_runtime::Observation;
use mandate_shell::adapters::Sources;
use mandate_shell::control::{
    ConfigRefusal, ControlRecord, DeploymentRefusal, Pinned, RunFacts, configuration,
    confirmed_version,
};
use mandate_shell::host::refuse_configured_host;
use mandate_shell::paper::{
    Artifacts, PaperClock, PaperFacts, daily_closes, liquidity_facts, load_contexts_with_clock,
    preflight,
};
use mandate_shell::{Cause, Report, Setup, ShellError, Stage, production_cycle};
use mandate_spec::context::{AgentId, Membership};
use mandate_time::{Date, ExchangeCalendar, UtcNanos};

/// What the founder asked for: opaque ids (TI-8), and no mandate, configuration, model output or
/// host (FT-3, FT-4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub workspace: String,
    pub agent: String,
    pub account_ref: String,
    /// The journal's DSN; only a placing run appends its cycle there (DEC-157 item 6). `None` is a
    /// scratch in-memory journal.
    pub journal: Option<String>,
    pub store: PathBuf,
    pub bars: PathBuf,
    pub place_one_order: bool,
}

/// The process's effects, the only things the binary supplies (ADR-0001 ES-05, ES-19).
pub trait Ports {
    type Transport: TradingTransport + DataTransport + Clone + 'static;
    type Pause: Pause + Clone + 'static;

    /// The workspace control stream's records, read once per run, or [`PaperError::Control`].
    fn control_stream(&mut self, workspace: &str) -> Result<Vec<ControlRecord>, PaperError>;

    /// The credentials and transport, or [`PaperError::Credentials`]; called at most once, and
    /// only after every check that needs no credential.
    fn connect(&mut self) -> Result<Self::Transport, PaperError>;

    /// The clock and the timer; its `now` is the run's one clock read and the binding gate's.
    fn pause(&self) -> Self::Pause;
}

/// How a run that refused nothing ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// `Flat` or `Undecided`: nothing stored, handed on or sent (DEC-157 item 4).
    NoOutput(Signal),
    /// The cycle ran: what it submitted, or the order it would have placed (boxed: it is large).
    Cycle(Box<Report>),
}

/// Why a run stopped. After any of them nothing further is sent.
#[derive(Debug, thiserror::Error)]
pub enum PaperError {
    /// The body of every stub in the tests PR (DEC-77).
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
    #[error("{0}")]
    Usage(String),
    #[error("the control stream could not be read")]
    Control,
    #[error("the paper credentials or client are unavailable")]
    Credentials,
    #[error("the artifact store could not be opened or written")]
    Store,
    #[error("no confirmed deployment: {0}")]
    Deployment(DeploymentRefusal),
    #[error("the registered configuration cannot be used: {0}")]
    Configuration(ConfigRefusal),
    #[error("the model host refused: {0}")]
    Model(Refusal),
    #[error(transparent)]
    Shell(ShellError),
}

/// Parses the arguments after the program name: `--workspace`, `--agent`, `--account-ref`,
/// `--journal`, `--store` and `--bars`, each with a value; the required `--confirm-paper`; and
/// `--place-one-order`, which needs `--journal`. Any other argument is unknown, so none names a
/// mandate, a configuration, a model output or a host.
///
/// # Errors
/// [`PaperError::Usage`] for a missing acknowledgement, flag or value, or an unknown argument.
pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Args, PaperError> {
    let mut values: [Option<String>; 6] = Default::default();
    let (mut confirmed, mut place_one_order) = (false, false);
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let Some(at) = FLAGS.iter().position(|flag| *flag == arg) else {
            match arg.as_str() {
                "--confirm-paper" => confirmed = true,
                "--place-one-order" => place_one_order = true,
                _ => return Err(usage("an argument is unknown".to_owned())),
            }
            continue;
        };
        let value = args.next().filter(|value| !value.starts_with("--"));
        let value = value.ok_or_else(|| usage(format!("{arg} needs a value")))?;
        let slot = values
            .get_mut(at)
            .ok_or_else(|| usage(format!("{arg} is unknown")))?;
        if slot.replace(value).is_some() {
            return Err(usage(format!("{arg} is given twice")));
        }
    }
    if !confirmed {
        return Err(usage("--confirm-paper is required".to_owned()));
    }
    let [workspace, agent, account_ref, journal, store, bars] = values;
    if place_one_order && journal.is_none() {
        return Err(usage("--place-one-order needs --journal".to_owned()));
    }
    let required = |value: Option<String>, flag: &str| {
        value.ok_or_else(|| usage(format!("{flag} is required")))
    };
    Ok(Args {
        workspace: required(workspace, "--workspace")?,
        agent: required(agent, "--agent")?,
        account_ref: required(account_ref, "--account-ref")?,
        journal,
        store: required(store, "--store")?.into(),
        bars: required(bars, "--bars")?.into(),
        place_one_order,
    })
}

/// The flags that take a value, in [`parse`]'s slot order.
const FLAGS: [&str; 6] = [
    "--workspace",
    "--agent",
    "--account-ref",
    "--journal",
    "--store",
    "--bars",
];

/// The observation's source: the stored IEX daily bars the closes were read from.
const SOURCE: &str = "alpaca_iex_daily_bars";

fn usage(message: String) -> PaperError {
    PaperError::Usage(message)
}

fn refused(stage: Stage) -> impl FnOnce(Cause) -> PaperError {
    move |cause| PaperError::Shell(ShellError::Refused { stage, cause })
}

fn absent(stage: Stage, what: &'static str) -> PaperError {
    refused(stage)(Cause::Absent { what })
}

/// The run's clock as the binding gate reads it, immediately before submission.
struct PortClock<P>(P);

impl<P: Pause> PaperClock for PortClock<P> {
    fn now(&self) -> Option<UtcNanos> {
        Some(self.0.now())
    }
}

/// One run, in the crate documentation's order; `vars` is the process environment.
///
/// # Errors
/// Every [`PaperError`] is a stop after which nothing further is sent.
pub fn run<P: Ports>(
    args: &Args,
    vars: &[(String, String)],
    ports: &mut P,
) -> Result<Outcome, PaperError> {
    refuse_configured_host(vars.iter().map(|(name, value)| (name, value)))
        .map_err(PaperError::Shell)?;
    let clock = ports.pause();
    let now = clock.now();
    let day = now.date();
    let records = ports.control_stream(&args.workspace)?;
    let mut store = FsArtifactStore::open(&args.store).map_err(|_| PaperError::Store)?;
    let facts = RunFacts {
        agent: AgentId::new(&args.agent),
        validation_date: day,
        membership: Membership {
            workspace_users: 1,
            approver_users: 1,
        },
    };
    let confirmed = confirmed_version(&records, &store, &facts).map_err(PaperError::Deployment)?;
    let mandate = confirmed.mandate();
    let ([instrument], [model]) = (
        mandate.universe.pinned_instruments.as_slice(),
        mandate.behavior.signal_models.as_slice(),
    ) else {
        return Err(PaperError::Shell(ShellError::UniverseNotPinned));
    };
    let (instrument, model) = (instrument.clone(), model.clone());
    let pinned = Pinned {
        asset_id: instrument.asset_id.as_str().to_owned(),
        symbol: instrument.symbol.clone(),
        model_id: model.id.as_str().to_owned(),
        model_version: model.version.clone(),
        content_hash: model.content_hash,
    };
    let config =
        configuration(&records, &store, &pinned, day).map_err(PaperError::Configuration)?;
    let artifacts =
        Artifacts::from_registered(&confirmed, &config).map_err(refused(Stage::Validate))?;
    let deployed = artifacts
        .deployment(
            args.workspace.clone(),
            args.agent.clone(),
            args.account_ref.clone(),
        )
        .map_err(refused(Stage::Validate))?;
    let transport = ports.connect()?;
    let broker = preflight(
        &artifacts,
        transport.clone(),
        transport.clone(),
        clock.clone(),
    )
    .map_err(refused(Stage::Reconcile))?;
    let input = confirmed
        .with_equity(broker.account.equity)
        .map_err(PaperError::Deployment)?;
    let symbol = artifacts.production_identity().symbol.clone();
    let liquidity = liquidity_facts(&symbol, &args.bars, &broker.minute_bars, now)
        .map_err(refused(Stage::MarketData))?;
    let closes = daily_closes(&symbol, &args.bars, now).map_err(refused(Stage::MarketData))?;
    let asset = InstrumentId::new(instrument.asset_id.as_str())
        .map_err(|_| absent(Stage::Validate, "the pinned asset id"))?;
    let registry = input.context().registry.clone();
    let registry = registry.ok_or_else(|| absent(Stage::Validate, "the model registry"))?;
    let calendar = ExchangeCalendar::us_equities()
        .map_err(|_| absent(Stage::MarketData, "the US equity calendar"))?;
    let pin = Pin {
        model,
        instrument_id: asset.clone(),
    };
    let daily = DailyCloses {
        instrument_id: asset.clone(),
        closes,
    };
    let output =
        match evaluate(&pin, &registry, &calendar, &daily, now).map_err(PaperError::Model)? {
            Evaluation::Long(output) => *output,
            Evaluation::NoOutput(signal) => return Ok(Outcome::NoOutput(signal)),
        };
    let bytes = closes_object(&daily.closes, instrument.asset_id.as_str())?;
    let data_ref = store.put_artifact(&bytes).map_err(|_| PaperError::Store)?;
    let observation = Observation {
        source: SOURCE.to_owned(),
        instrument_id: Some(asset),
        as_of: output.as_of,
        data_ref: data_ref.digest(),
    };
    let agent = deployed.deployment().agent.clone();
    let paper = PaperFacts { broker, liquidity };
    let binding_clock = Rc::new(PortClock(clock));
    let contexts = load_contexts_with_clock(&artifacts, &paper, now, &agent, binding_clock)
        .map_err(refused(Stage::Validate))?;
    let setup = Setup {
        deployment: deployed.deployment().clone(),
        account_ref: deployed.account_ref().to_owned(),
        now,
        place_one_order: args.place_one_order,
        new_cycle: false,
    };
    let sources = Sources {
        mandate: store.object_path(&ArtifactRef::from_digest(input.version())),
        dataset: args.bars.clone(),
        journal: args.journal.clone().filter(|_| args.place_one_order),
        recorded_at: now,
        agent,
        workspace: deployed.deployment().workspace.0.clone(),
        account_ref: deployed.account_ref().to_owned(),
        executor: Some(contexts.executor),
        run: Some(contexts.run),
        artifacts: Some(Arc::new(store)),
        transport,
    };
    let report = production_cycle(sources, setup)
        .run_observed(observation, output)
        .map_err(PaperError::Shell)?;
    Ok(Outcome::Cycle(Box::new(report)))
}

/// The closes the host read as DEC-846 item 3's canonical object: `[session, close]` pairs,
/// oldest first, and the pinned asset id.
fn closes_object(closes: &[(Date, Price)], asset_id: &str) -> Result<Vec<u8>, PaperError> {
    let pair = |(day, close): &(Date, Price)| {
        Value::Array(vec![
            Value::Str(day.to_string()),
            Value::Str(close.to_string()),
        ])
    };
    let mut object = Object::new();
    for (name, value) in [
        ("closes", Value::Array(closes.iter().map(pair).collect())),
        ("instrument_id", Value::Str(asset_id.to_owned())),
    ] {
        let key =
            Key::new(name).map_err(|_| absent(Stage::Journal, "the observation's members"))?;
        object.insert(key, value);
    }
    Ok(to_canonical(&Value::Object(object)))
}

/// The binary's ports (DEC-846 item 1): the control stream read from the Postgres journal at
/// `journal`, the paper credentials from the environment with the Alpaca paper client, and the
/// system clock. Without a journal there is no control stream to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Production {
    pub journal: Option<String>,
}

impl Ports for Production {
    type Transport = AlpacaPaperHttp;
    type Pause = SystemClock;

    /// `ctl:{workspace}`'s records from the journal, read once, or [`PaperError::Control`] when
    /// there is no journal or it cannot be read. The error never names the DSN (rule 7).
    fn control_stream(&mut self, workspace: &str) -> Result<Vec<ControlRecord>, PaperError> {
        let dsn = self.journal.as_deref().ok_or(PaperError::Control)?;
        let stream = StreamId::parse(&format!("ctl:{workspace}")).ok_or(PaperError::Control)?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| PaperError::Control)?;
        let rows = runtime.block_on(async {
            let journal = PgJournal::from_dsn(dsn).map_err(|_| PaperError::Control)?;
            journal.rows(&stream).await.map_err(|_| PaperError::Control)
        })?;
        rows.iter().map(control_record).collect()
    }

    /// `Credentials::from_env` and one [`AlpacaPaperHttp`], or [`PaperError::Credentials`]; the
    /// error never names a key.
    fn connect(&mut self) -> Result<AlpacaPaperHttp, PaperError> {
        let credentials = Credentials::from_env().map_err(|_| PaperError::Credentials)?;
        AlpacaPaperHttp::new(credentials).map_err(|_| PaperError::Credentials)
    }

    fn pause(&self) -> SystemClock {
        SystemClock
    }
}

/// One verified row as the control-stream fold reads it: its `seq`, type and body's `payload`.
fn control_record(row: &StoredEvent) -> Result<ControlRecord, PaperError> {
    let body = mandate_canon::parse(&row.body).map_err(|_| PaperError::Control)?;
    let payload = body.get("payload").ok_or(PaperError::Control)?.clone();
    Ok(ControlRecord {
        seq: row.seq,
        event_type: row.event_type.clone(),
        payload,
    })
}

/// The process's clock and timer: the system clock, read with nanoseconds, and the tokio timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemClock;

impl Pause for SystemClock {
    fn now(&self) -> UtcNanos {
        TokioPause.now()
    }

    fn pause(&self, duration: Duration) -> impl Future<Output = ()> {
        TokioPause.pause(duration)
    }
}

/// The binary's whole run over the process's arguments (after the program name) and environment:
/// [`parse`], [`run`] over [`Production`], and the lines to print: the order a dry run would place,
/// each submitted order, or the model's `Flat` or `Undecided`. The binary prints an error's message
/// alone on stderr and exits non-zero; no line or message names a DSN or a key (rule 7).
///
/// # Errors
/// Every [`PaperError`] of [`parse`] and [`run`].
pub fn process<A, V>(args: A, vars: V) -> Result<Vec<String>, PaperError>
where
    A: IntoIterator<Item = String>,
    V: IntoIterator<Item = (String, String)>,
{
    let args = parse(args)?;
    let vars: Vec<(String, String)> = vars.into_iter().collect();
    let mut ports = Production {
        journal: args.journal.clone(),
    };
    Ok(lines(&run(&args, &vars, &mut ports)?))
}

/// What a run that refused nothing prints, one line each: the order a dry run would place, each
/// submitted order, or the model's `Flat` or `Undecided`. Ids and the signal only, never a value
/// (rule 7).
#[must_use]
pub fn lines(outcome: &Outcome) -> Vec<String> {
    match outcome {
        Outcome::NoOutput(signal) => vec![format!("the model output {signal:?}; nothing sent")],
        Outcome::Cycle(report) => {
            let would_place = report.would_place.iter().map(|order| {
                format!(
                    "would place {} (nothing sent; pass --place-one-order)",
                    order.client_order_id.as_str()
                )
            });
            let submitted = report.submitted.iter().map(|id| format!("submitted {id}"));
            would_place.chain(submitted).collect()
        }
    }
}
