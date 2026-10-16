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
//! refusal, the control stream, the live door the connection record and the confirmed mandate
//! choose (E7-26, [`chosen`](crate::chosen), [`account_stream`](crate::account_stream),
//! [`lapse_read`](crate::lapse_read), [`unsupplied`](crate::unsupplied), DEC-918, DEC-919, DEC-933) and E19-11's phase 1 and the artifacts (DEC-505),
//! before any credential; the credentials, the GET-only preflight and phase 2 (V-002); the trusted closes and
//! the host, where `Flat` or `Undecided` ends it; the closes stored, and only then the observation
//! and output handed to [`mandate_shell::ProductionCycle::run_observed_watched`], the one door
//! (FT-1, FT-6), which watches the one submission to the registered close window's start at the
//! registered rule set's interval (E1b, DEC-853 items 5 and 6, DEC-858). No product value is a
//! constant here (FT-2).

mod connection;
mod door;
mod evidence;
mod grant;
mod harden;
mod lt10;
mod quote;
mod robinhood;
mod wiring;

use std::ffi::OsString;
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
use mandate_domain::{AssetClass, Environment};
use mandate_journal::{
    ArtifactRef, ArtifactSource, ArtifactStore, StoredEvent, StreamId, get_artifact,
};
use mandate_journal_pg::PgJournal;
use mandate_modelhost::{DailyCloses, Evaluation, Pin, Refusal, Signal, evaluate};
use mandate_num::{Price, Usd};
use mandate_robinhood::AccountSnapshot;
use mandate_runtime::Observation;
use mandate_shell::adapters::Sources;
use mandate_shell::control::{
    Build as BuiltWith, ConfigRefusal, Confirmation, ConfirmedVersion, ControlRecord,
    DeploymentRefusal, DoorChecks, LiveAdmission, McpHost, Pinned, RunFacts,
    checked_before_credentials, configuration, confirmed_version,
};
use mandate_shell::host::refuse_configured_host;
use mandate_shell::paper::{
    Artifacts, BrokerFacts, PaperClock, PaperFacts, daily_closes, liquidity_facts,
    load_contexts_with_clock, preflight,
};
use mandate_shell::{
    Cause, ProductionCycle, Report, Setup, ShellError, Stage, Watch, production_cycle,
};
use mandate_shell::{close_window_bound, poll_interval};
use mandate_spec::Mandate;
use mandate_spec::context::{AgentId, Membership};
use mandate_time::{Date, ExchangeCalendar, UtcNanos};

pub use connection::{
    Broker, Connection, ConnectionRefusal, Held, LapseRefusal, connection, lapse,
};
pub use door::{account_stream, chosen, lapse_read, unspent, unsupplied};
pub use evidence::{Evidence, EvidenceRefusal, evidence};
pub use grant::{Asked, GrantRefusal, Journaled, grant};
pub use harden::disable_core_dumps;
pub use lt10::{Lt10Refusal, Watched, lt10};
pub use quote::{QuoteRefusal, cross_check};
pub use robinhood::{
    ACCOUNT_NUMBER_VAR, Build, Dial, EndpointRefusal, Linked, REHEARSAL_ENDPOINT_VAR,
    REHEARSAL_HOST, RobinhoodEndpoint, dial, endpoint, pinned_host,
};
pub use wiring::{
    CollarMembers, WakeRead, answers, asked, collar_x, cross_check_the_opening, granted_opening,
    lt10_end, reported, robinhood_cross_check, session_cap, wait_for_the_grant, watch_may_end,
    watched,
};

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
    /// The environment the founder acknowledged: `--confirm-paper` or `--confirm-live`, exactly
    /// one. The run refuses unless it is the confirmed mandate's own (LT-9, DEC-937 item 3).
    pub acknowledged: Environment,
}

/// The process's effects, the only things the binary supplies (ADR-0001 ES-05, ES-19).
/// Broker-neutral (G1a, DEC-937 item 1): the session [`Ports::connect`] answers is whichever
/// broker the connection record chose, and the run reads and trades only through [`Session`].
pub trait Ports {
    type Session: Session;
    type Pause: Pause + Clone + 'static;

    /// The workspace control stream's records, read once per run, or [`PaperError::Control`].
    fn control_stream(&mut self, workspace: &str) -> Result<Vec<ControlRecord>, PaperError>;

    /// The credentials and the session of the broker `connection` chose, or
    /// [`PaperError::Credentials`]; called at most once, only after every check that needs no
    /// credential, and for a live run only after [`Ports::harden`].
    fn connect(
        &mut self,
        connection: &Connection,
        dial: Option<&Dial>,
    ) -> Result<Self::Session, PaperError>;

    /// Turns the process's core dumps off (LT-9), or [`PaperError::Harden`]; called once, for a
    /// live run only, after its acknowledgement and before [`Ports::connect`].
    fn harden(&mut self) -> Result<(), PaperError>;

    /// The clock and the timer: its `now` dates the run's configuration, stamps the asset record
    /// and judges the quote at their reads, gives the run clock after the last broker read
    /// (DEC-935), and is the binding gate's.
    fn pause(&self) -> Self::Pause;
}

/// One connected broker, as the run reads and trades through it (G1a, DEC-937 item 1): the
/// GET-only preflight, and the production cycle over the broker's own connector. Every
/// [`TradingTransport`] and [`DataTransport`] is the Alpaca paper session it always was.
pub trait Session {
    /// The broker facts the run reads before any cycle, or the [`Cause`] it stopped at.
    ///
    /// # Errors
    /// [`Cause::Absent`] when a read is not answered with the fact it asked for.
    fn preflight<P: Pause + Clone>(
        &self,
        artifacts: &Artifacts,
        pause: P,
    ) -> Result<BrokerFacts, Cause>;

    /// The production cycle over `sources` with this session's connector.
    ///
    /// # Errors
    /// [`Cause::Absent`] when the session cannot carry a cycle.
    fn cycle(self, sources: Sources<()>, setup: Setup) -> Result<ProductionCycle, Cause>;

    /// The broker's own ask for the pinned instrument, the quote an asked opening's cross-check
    /// compares with the independent IEX ask before it leaves (DEC-529 item 12; DEC-940 item 4),
    /// or [`None`] when this session holds none — and then the cross-check refuses the opening
    /// and nothing is sent (rule 3).
    fn broker_ask(&self, facts: &BrokerFacts) -> Option<Price>;

    /// The agentic account's snapshot the live door's lapse counts (DEC-933 item 3, DEC-947
    /// item 1), read and sending nothing, or [`None`] when this session reads none — and then a
    /// live connection's lapse refuses `lapse_evidence_not_supplied` (DEC-919).
    ///
    /// # Errors
    /// [`Cause::Absent`] when the read is refused or unreadable.
    fn account_snapshot(&self) -> Result<Option<AccountSnapshot>, Cause>;
}

impl<T: TradingTransport + DataTransport + Clone + 'static> Session for T {
    fn preflight<P: Pause + Clone>(
        &self,
        artifacts: &Artifacts,
        pause: P,
    ) -> Result<BrokerFacts, Cause> {
        preflight(artifacts, self.clone(), self.clone(), pause)
    }

    fn cycle(self, sources: Sources<()>, setup: Setup) -> Result<ProductionCycle, Cause> {
        Ok(production_cycle(with_transport(sources, self), setup))
    }

    /// Alpaca's own quote is the IEX quote its data client read at the preflight.
    fn broker_ask(&self, facts: &BrokerFacts) -> Option<Price> {
        Some(facts.quote.ask)
    }

    /// An Alpaca paper session reads no Robinhood snapshot; a paper run's lapse makes no check.
    fn account_snapshot(&self) -> Result<Option<AccountSnapshot>, Cause> {
        Ok(None)
    }
}

/// `sources` carrying `transport`.
fn with_transport<T>(sources: Sources<()>, transport: T) -> Sources<T> {
    let Sources {
        mandate,
        journal,
        recorded_at,
        agent,
        workspace,
        account_ref,
        executor,
        run,
        artifacts,
        transport: (),
    } = sources;
    Sources {
        mandate,
        journal,
        recorded_at,
        agent,
        workspace,
        account_ref,
        executor,
        run,
        artifacts,
        transport,
    }
}

/// How a run that refused nothing ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// `Flat` or `Undecided`: nothing stored, handed on or sent (DEC-157 item 4).
    NoOutput(Signal),
    /// The cycle ran: what it submitted, or the order it would have placed (boxed: it is large),
    /// beside the exportable evidence its report names (DEC-914 item 6).
    Cycle(Box<Cycle>),
}

/// One finished cycle's report (DEC-914 item 6: the shell's `Report` cannot hold a
/// `mandate-paper` type, so the evidence the run named at its report attaches here, beside
/// it): the cycle's own report, and the streams the placing run's `journal export` reads,
/// named from the journal the run appended to — `None` when the run appended to no journal, so
/// nothing it holds exports (R0 step 4's dry run; [`EvidenceRefusal::JournalAbsent`] at the
/// report seam, which is a naming refused and never a run failed: the order lines print as
/// they did, and the refusal names no DSN, rule 7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cycle {
    /// The cycle's own report: what it submitted, or the order it would have placed.
    pub report: Report,
    /// The evidence a placing run names at its report; [`None`] when the run kept no journal.
    pub evidence: Option<Evidence>,
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
    /// A live run without `--confirm-live` (DEC-937 item 3), before any credential is read.
    #[error("the confirmed mandate is live; a live run needs --confirm-live")]
    LiveUnconfirmed,
    /// A paper run acknowledged as live: each environment needs its own acknowledgement.
    #[error("the confirmed mandate is paper; a paper run needs --confirm-paper")]
    PaperUnconfirmed,
    /// The process's core dumps could not be turned off, so a live run reads no credential
    /// (LT-9).
    #[error("core dumps could not be turned off")]
    Harden,
    /// The build has no Robinhood endpoint to dial (LT-1, DEC-922), before anything is sent.
    #[error(transparent)]
    Endpoint(EndpointRefusal),
    #[error("the artifact store could not be opened or written")]
    Store,
    #[error("no confirmed deployment: {0}")]
    Deployment(DeploymentRefusal),
    #[error("the registered configuration cannot be used: {0}")]
    Configuration(ConfigRefusal),
    #[error("the model host refused: {0}")]
    Model(Refusal),
    /// The connector choice from the connection record fails closed (G1a, DEC-529 item 3).
    #[error(transparent)]
    Connection(#[from] ConnectionRefusal),
    /// DEC-529 item 3's lapse: a live run refuses an account that is not flat or a spent
    /// connection record.
    #[error(transparent)]
    Lapse(#[from] LapseRefusal),
    /// The grant an `ask` opening waits for fails closed (G1b, LT-4).
    #[error(transparent)]
    Grant(#[from] GrantRefusal),
    /// The opening's quote cross-check fails closed (G1b, DEC-529 item 12).
    #[error(transparent)]
    CrossCheck(#[from] QuoteRefusal),
    /// The live watch's LT-10 end fails closed (G1b, LT-10).
    #[error(transparent)]
    Lt10(#[from] Lt10Refusal),
    /// The run's exportable evidence fails closed (G1b, the founder's run's step 6).
    #[error(transparent)]
    Evidence(#[from] EvidenceRefusal),
    #[error(transparent)]
    Shell(#[from] ShellError),
}

/// Parses the arguments after the program name: `--workspace`, `--agent`, `--account-ref`,
/// `--journal`, `--store` and `--bars`, each with a value; exactly one acknowledgement,
/// `--confirm-paper` or `--confirm-live` (DEC-937 item 3); and `--place-one-order`, which needs
/// `--journal`. Any other argument is unknown, so none names a mandate, a configuration, a model
/// output or a host.
///
/// # Errors
/// [`PaperError::Usage`] for a missing or doubled acknowledgement, a missing flag or value, or an
/// unknown argument.
pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Args, PaperError> {
    let mut values: [Option<String>; 6] = Default::default();
    let (mut paper, mut live, mut place_one_order) = (false, false, false);
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let Some(at) = FLAGS.iter().position(|flag| *flag == arg) else {
            match arg.as_str() {
                "--confirm-paper" => paper = true,
                "--confirm-live" => live = true,
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
    let acknowledged = match (paper, live) {
        (true, false) => Environment::Paper,
        (false, true) => Environment::Live,
        (false, false) => return Err(usage("--confirm-paper is required".to_owned())),
        (true, true) => {
            return Err(usage(
                "--confirm-paper and --confirm-live are exclusive".to_owned(),
            ));
        }
    };
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
        acknowledged,
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

/// What an asked opening's cross-check reads, taken at the preflight (DEC-940 item 4): the
/// pinned instrument's asset class and 20-day median dollar volume, which choose the collar's
/// own `x`, the broker's own ask, and the independent IEX ask.
struct Opening {
    asset_class: AssetClass,
    median_dollar_volume_20d: Usd,
    broker_ask: Option<Price>,
    independent_ask: Price,
}

impl Opening {
    /// [`cross_check_the_opening`] over the registered collar's own members.
    fn cross_check(&self, collar: &CollarMembers) -> Result<(), PaperError> {
        cross_check_the_opening(
            collar,
            self.asset_class,
            Some(self.median_dollar_volume_20d),
            self.broker_ask,
            Some(self.independent_ask),
        )
    }
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
/// The run clock everything after the preflight is judged and recorded at is read once, after the
/// preflight's last broker read, so every fact the broker answered is stamped at or before it and
/// each age bound is measured from it (DEC-935, #1355); the date the configuration is chosen for
/// is read before the control stream.
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
    let day = clock.now().date();
    let records = ports.control_stream(&args.workspace)?;
    let mut store = FsArtifactStore::open(&args.store).map_err(|_| PaperError::Store)?;
    let mut facts = RunFacts {
        agent: AgentId::new(&args.agent),
        validation_date: day,
        membership: Membership {
            workspace_users: 1,
            approver_users: 1,
        },
        live: LiveAdmission::refusing(),
    };
    let door = the_live_door(&records, &store, &facts.agent, args)?;
    if let Some((connection, _)) = &door {
        acknowledge(connection, args, ports)?;
    }
    let (confirmed, early) = match &door {
        Some((connection, stream)) if connection.environment == Environment::Live => {
            let dial = dial(Build::CURRENT, vars).map_err(PaperError::Endpoint)?;
            facts.live = live_admission(
                BUILT_WITH,
                args.acknowledged,
                DoorChecks::NotPassed,
                Some(&dial.endpoint),
                pinned_host(Build::CURRENT),
            );
            let journal = args.journal.as_deref();
            let (confirmed, session) = live_start(
                &records, &store, &facts, connection, stream, journal, &dial, ports,
            )?;
            (confirmed, Some(session))
        }
        Some((connection, stream)) => {
            lapse_read(connection, args.journal.as_deref(), stream, None)?;
            let confirmed =
                confirmed_version(&records, &store, &facts).map_err(PaperError::Deployment)?;
            (confirmed, None)
        }
        None => {
            let confirmed =
                confirmed_version(&records, &store, &facts).map_err(PaperError::Deployment)?;
            (confirmed, None)
        }
    };
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
    let session = match early {
        Some(session) => session,
        None => {
            let (connection, _) =
                door.ok_or_else(|| absent(Stage::Validate, "the connection the deployment names"))?;
            ports.connect(&connection, None)?
        }
    };
    let broker = session
        .preflight(&artifacts, clock.clone())
        .map_err(refused(Stage::Reconcile))?;
    let now = clock.now();
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
    let opening = Opening {
        asset_class: instrument.asset_class,
        median_dollar_volume_20d: liquidity.median_dollar_volume_20d,
        broker_ask: session.broker_ask(&broker),
        independent_ask: broker.quote.ask,
    };
    let paper = PaperFacts { broker, liquidity };
    let binding_clock = Rc::new(PortClock(clock.clone()));
    let contexts = load_contexts_with_clock(&artifacts, &paper, now, &agent, binding_clock)
        .map_err(refused(Stage::Validate))?;
    let registered = artifacts
        .production_configuration()
        .map_err(refused(Stage::Validate))?;
    let collar = CollarMembers {
        liquid_x: registered.gate.collar_liquid_x,
        other_x: registered.gate.collar_other_x,
        crypto_x: registered.gate.collar_crypto_x,
        liquid_threshold_usd: registered.gate.collar_liquid_threshold_usd,
    };
    let watch = Watch {
        pause: clock,
        bound: close_window_bound(now, registered.gate).map_err(refused(Stage::Validate))?,
        interval: poll_interval(&registered.executor).map_err(refused(Stage::Validate))?,
    };
    let setup = Setup {
        deployment: deployed.deployment().clone(),
        account_ref: deployed.account_ref().to_owned(),
        now,
        place_one_order: args.place_one_order,
        new_cycle: false,
    };
    let sources = Sources {
        mandate: store.object_path(&ArtifactRef::from_digest(input.version())),
        journal: args.journal.clone().filter(|_| args.place_one_order),
        recorded_at: now,
        agent,
        workspace: deployed.deployment().workspace.0.clone(),
        account_ref: deployed.account_ref().to_owned(),
        executor: Some(contexts.executor),
        run: Some(contexts.run),
        artifacts: Some(Arc::new(store)),
        transport: (),
    };
    let report = session
        .cycle(sources, setup)
        .map_err(refused(Stage::Reconcile))?
        .run_observed_asked_watched(
            observation,
            output,
            &watch,
            |asked| {
                granted_opening(asked, &watch.pause, watch.interval, || {
                    opening.cross_check(&collar)
                })
            },
            &lt10_end,
        )?;
    let evidence = wiring::reported(
        args.journal.as_deref(),
        args.place_one_order,
        &args.workspace,
        &args.agent,
        &args.account_ref,
    )
    .ok();
    Ok(Outcome::Cycle(Box::new(Cycle { report, evidence })))
}

/// Whether this binary was built with the runner's `live` feature, as the shell's admission
/// reads it (DEC-924 item 1, DEC-947 item 3).
pub const BUILT_WITH: BuiltWith = if cfg!(feature = "live") {
    BuiltWith::Live
} else {
    BuiltWith::Default
};

/// DEC-924 item 1's admission from the run's own facts, one each (DEC-934, DEC-947 item 3): the
/// build `built`, the acknowledgement `acknowledged` (only `--confirm-live` confirms), the door's
/// answer `door`, and the host of the endpoint the run dials, `dialed`, beside the host its build
/// pins, `pinned` ([`pinned_host`]). The shell's `admits` judges them; nothing here does.
pub fn live_admission(
    built: BuiltWith,
    acknowledged: Environment,
    door: DoorChecks,
    dialed: Option<&RobinhoodEndpoint>,
    pinned: Option<&'static str>,
) -> LiveAdmission {
    LiveAdmission {
        build: built,
        confirmation: match acknowledged {
            Environment::Live => Confirmation::ConfirmLive,
            Environment::Paper => Confirmation::Absent,
        },
        door,
        host: McpHost {
            dialed: dialed.map(|endpoint| endpoint.host.clone()),
            pinned,
        },
    }
}

/// A live run's start (DEC-947 items 1 and 2, the R0 rehearsal's R2 and R3), in the order no
/// credential is read before a check that needs none has passed, and nothing that can place is
/// built before the door has passed on real evidence:
///
/// 1. the shell's fold before credentials ([`checked_before_credentials`]): the walk, the
///    document, every V-rule but V-002, and the admission's build, `--confirm-live` and host;
/// 2. the one [`Ports::connect`], with the run's `dial`: the login and the MCP session;
/// 3. the agentic account's snapshot ([`Session::account_snapshot`]), the account reads alone;
/// 4. the door's lapse on that snapshot and the account stream's rows ([`lapse_read`]);
/// 5. the shell's full fold, with the door's condition now held (`DoorChecks::Passed`).
///
/// `facts` carry the admission as the run holds it before credentials, the door not passed.
/// Any refusal after the connect leaves the session unused: it read, and it sent nothing.
///
/// # Errors
/// [`PaperError::Deployment`] from either fold; [`PaperError::Credentials`] or
/// [`PaperError::Endpoint`] from the connect; [`PaperError::Shell`] at `Stage::Reconcile` for an
/// unread snapshot; any [`PaperError::Lapse`].
#[allow(
    clippy::too_many_arguments,
    reason = "the run's own inputs, each read once; DEC-947 item 2"
)]
fn live_start<P: Ports>(
    records: &[ControlRecord],
    store: &dyn ArtifactSource,
    facts: &RunFacts,
    connection: &Connection,
    stream: &StreamId,
    journal: Option<&str>,
    dial: &Dial,
    ports: &mut P,
) -> Result<(ConfirmedVersion, P::Session), PaperError> {
    checked_before_credentials(records, store, facts).map_err(PaperError::Deployment)?;
    let session = ports.connect(connection, Some(dial))?;
    let snapshot = session
        .account_snapshot()
        .map_err(refused(Stage::Reconcile))?;
    lapse_read(connection, journal, stream, snapshot.as_ref())?;
    let admitted = RunFacts {
        live: LiveAdmission {
            door: DoorChecks::Passed,
            ..facts.live.clone()
        },
        ..facts.clone()
    };
    let confirmed = confirmed_version(records, store, &admitted).map_err(PaperError::Deployment)?;
    Ok((confirmed, session))
}

/// The live door the run calls ahead of its own answers (E7-26, DEC-918 items 1 to 4; DEC-910
/// item 2: "These checks refuse before anything runs, ahead of the shell's own
/// `DeploymentRefusal::Live`, which stays for the shell's fold"): the connection choice
/// [`chosen`] and the account stream's naming [`account_stream`], in that order, over the records
/// the run's own single control-stream read holds and the deployment's own document, all before
/// the shell's phase-1 fold and every credential read (FT-3). The door engages where the run
/// holds the deployment's own document ([`deployed_document`]); where it holds none — no
/// deployment, a stopped agent, or a document the fold itself would refuse — the door answers
/// nothing, the shell's fold answers behind it with its own refusal unchanged, so the door never
/// answers where the shell refuses and performs no validation of its own (DEC-918 item 3's
/// bounds: only ever a refusal in front of the shell's).
///
/// The naming comes from the run's own names, ungated by `--place-one-order` (DEC-918 item 2).
/// DEC-529 item 3's lapse is the door's third check, made through [`lapse_read`] on the stream
/// named here: for a paper connection at once, where it makes no check and reads nothing (LT-14);
/// for a live one in [`live_start`], on the agentic account's own snapshot, which only the
/// Robinhood session reads, so after the fold's credential-free checks and the one connect
/// (DEC-933 item 3, DEC-947 items 1 and 2). A live connection whose session reads no snapshot
/// still refuses `lapse_evidence_not_supplied` (DEC-919).
///
/// # Errors
/// Any [`PaperError`] of the door's frames, before the shell's fold and before anything runs.
fn the_live_door(
    records: &[ControlRecord],
    store: &dyn ArtifactSource,
    agent: &AgentId,
    args: &Args,
) -> Result<Option<(Connection, StreamId)>, PaperError> {
    let Some(document) = deployed_document(records, store, agent) else {
        return Ok(None);
    };
    let connection = chosen(records, &document, &args.account_ref)?;
    let stream = account_stream(&args.workspace, &args.account_ref)?;
    Ok(Some((connection, stream)))
}

/// The founder's acknowledgement of the run's environment, checked as soon as the door has
/// chosen the connection and before any credential is read (DEC-937 item 3): a live connection
/// needs `--confirm-live`, then turns the process's core dumps off through [`Ports::harden`]
/// (LT-9); a paper connection needs `--confirm-paper` and is otherwise untouched.
///
/// # Errors
/// [`PaperError::LiveUnconfirmed`], [`PaperError::PaperUnconfirmed`], or the
/// [`PaperError::Harden`] of [`Ports::harden`].
pub fn acknowledge<P: Ports>(
    connection: &Connection,
    args: &Args,
    ports: &mut P,
) -> Result<(), PaperError> {
    match (connection.environment, args.acknowledged) {
        (Environment::Paper, Environment::Paper) => Ok(()),
        (Environment::Paper, Environment::Live) => Err(PaperError::PaperUnconfirmed),
        (Environment::Live, Environment::Paper) => Err(PaperError::LiveUnconfirmed),
        (Environment::Live, Environment::Live) => ports.harden(),
    }
}

/// The deployment's own mandate document, read from the same records and artifact store the
/// shell's phase-1 fold reads (DEC-918 item 3: "the run holds the same records and the same
/// artifact store phase-1 folds"): the fold's own walk — the records in `seq` order, the
/// agent's latest `AgentDeployed` behind no later `AgentStopped`, the version's checked read
/// ([`get_artifact`] re-hashes as the fold does) and the document's parse — and nothing else.
/// No V-rule and no live-environment stop: a live mandate is exactly the case whose exact
/// refusal the door owes ahead of the fold's `DeploymentRefusal::Live` (DEC-910 item 2), and no
/// validation of the door's own exists (DEC-918 item 3). Every failure falls through to
/// [`None`], where the shell's fold answers with its own refusal behind the door, so the
/// reading can only over-refuse (the DEC-176-safe side).
fn deployed_document(
    records: &[ControlRecord],
    store: &dyn ArtifactSource,
    agent: &AgentId,
) -> Option<Mandate> {
    let mut ordered: Vec<&ControlRecord> = records.iter().collect();
    ordered.sort_by_key(|record| record.seq);
    let ours = |record: &ControlRecord| {
        record.payload.get("agent_id").and_then(Value::as_str) == Some(agent.as_str())
    };
    let mut deployed: Option<Option<ArtifactRef>> = None;
    for record in &ordered {
        if !ours(record) {
            continue;
        }
        match record.event_type.as_str() {
            "AgentDeployed" => {
                let version = record
                    .payload
                    .get("mandate_version")
                    .and_then(Value::as_str)
                    .and_then(ArtifactRef::parse);
                deployed = Some(Some(version?));
            }
            "AgentStopped" => deployed = deployed.map(|_| None),
            _ => {}
        }
    }
    let version = match deployed {
        Some(Some(version)) => version,
        Some(None) | None => return None,
    };
    let bytes = get_artifact(store, &version).ok()?;
    let document = mandate_canon::parse(&bytes).ok()?;
    Mandate::parse(&document).ok()
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

/// The binary's ports (DEC-846 item 1, DEC-937 item 1): the control stream read from the
/// Postgres journal at `journal`, the session of the broker the connection chose — the paper
/// credentials from the environment with the Alpaca paper client, or the Robinhood login only a
/// `live` build can make — and the system clock. Without a journal there is no control stream to
/// read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Production {
    pub journal: Option<String>,
}

impl Ports for Production {
    type Session = Connected;
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

    /// For Alpaca, `Credentials::from_env` and one [`AlpacaPaperHttp`]; for Robinhood, the
    /// founder's login at the endpoint this build pins ([`robinhood::connect`]). Either fails as
    /// [`PaperError::Credentials`], and no error names a key or a token.
    fn connect(
        &mut self,
        connection: &Connection,
        dial: Option<&Dial>,
    ) -> Result<Connected, PaperError> {
        match connection.broker {
            Broker::Alpaca => {
                let credentials = Credentials::from_env().map_err(|_| PaperError::Credentials)?;
                AlpacaPaperHttp::new(credentials)
                    .map(Connected::Alpaca)
                    .map_err(|_| PaperError::Credentials)
            }
            Broker::Robinhood => {
                let dial = dial.ok_or(PaperError::Endpoint(EndpointRefusal::NotBuilt))?;
                robinhood::connect(dial).map(Connected::Robinhood)
            }
        }
    }

    /// [`disable_core_dumps`].
    fn harden(&mut self) -> Result<(), PaperError> {
        disable_core_dumps()
    }

    fn pause(&self) -> SystemClock {
        SystemClock
    }
}

/// The session [`Production::connect`] answers: the broker the connection record chose.
pub enum Connected {
    Alpaca(AlpacaPaperHttp),
    Robinhood(Linked),
}

impl Session for Connected {
    fn preflight<P: Pause + Clone>(
        &self,
        artifacts: &Artifacts,
        pause: P,
    ) -> Result<BrokerFacts, Cause> {
        match self {
            Self::Alpaca(transport) => transport.preflight(artifacts, pause),
            Self::Robinhood(linked) => linked.preflight(artifacts, pause),
        }
    }

    fn cycle(self, sources: Sources<()>, setup: Setup) -> Result<ProductionCycle, Cause> {
        match self {
            Self::Alpaca(transport) => transport.cycle(sources, setup),
            Self::Robinhood(linked) => linked.cycle(sources, setup),
        }
    }

    fn broker_ask(&self, facts: &BrokerFacts) -> Option<Price> {
        match self {
            Self::Alpaca(transport) => transport.broker_ask(facts),
            Self::Robinhood(linked) => linked.broker_ask(facts),
        }
    }

    fn account_snapshot(&self) -> Result<Option<AccountSnapshot>, Cause> {
        match self {
            Self::Alpaca(transport) => transport.account_snapshot(),
            Self::Robinhood(linked) => linked.account_snapshot(),
        }
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
/// each submitted order, or the model's `Flat` or `Undecided`. The binary prints an error's
/// [`stderr_line`] alone on stderr and exits non-zero; no line or message names a DSN or a key
/// (rule 7).
///
/// The environment is the process's own, as `std::env::vars_os` gives it, so a variable that is
/// not Unicode never panics the binary: each name and value is read lossily, so one the run does
/// not read changes nothing, and an Alpaca variable holding a URL is still refused by the host
/// check, naming the variable only.
///
/// # Errors
/// Every [`PaperError`] of [`parse`] and [`run`].
pub fn process<A, V>(args: A, vars: V) -> Result<Vec<String>, PaperError>
where
    A: IntoIterator<Item = String>,
    V: IntoIterator<Item = (OsString, OsString)>,
{
    let args = parse(args)?;
    let vars: Vec<(String, String)> = vars
        .into_iter()
        .map(|(name, value)| {
            let name = name.to_string_lossy().into_owned();
            (name, value.to_string_lossy().into_owned())
        })
        .collect();
    let mut ports = Production {
        journal: args.journal.clone(),
    };
    lines(&run(&args, &vars, &mut ports)?)
}

/// What a run that refused nothing prints, one line each: the order a dry run would place
/// (`would place <client_order_id> (nothing sent; pass --place-one-order)`), each submitted order
/// (`submitted <client_order_id>`), or the model's `Flat` or `Undecided` (`the model output
/// <signal>; nothing sent`). Ids and the signal only, never a value (rule 7).
///
/// # Errors
/// None: every outcome has its lines.
pub fn lines(outcome: &Outcome) -> Result<Vec<String>, PaperError> {
    Ok(match outcome {
        Outcome::NoOutput(signal) => vec![format!("the model output {signal:?}; nothing sent")],
        Outcome::Cycle(cycle) => {
            let would_place = cycle.report.would_place.iter().map(|order| {
                format!(
                    "would place {} (nothing sent; pass --place-one-order)",
                    order.client_order_id.as_str()
                )
            });
            let submitted = cycle
                .report
                .submitted
                .iter()
                .map(|id| format!("submitted {id}"));
            would_place.chain(submitted).collect()
        }
    })
}

/// The one line the binary prints on stderr for `error` before it exits non-zero: the stop's
/// message, except the cap's fail-closed stop, which prints its stable code `cancel_unconfirmed`
/// alone, the key-only alert DEC-858 item 5 asks for (DEC-877 item 1). No line names an order, a
/// DSN or a key (rules 6 and 7).
pub fn stderr_line(error: &PaperError) -> String {
    match error {
        PaperError::Shell(stop @ ShellError::CancelUnconfirmed) => stop.code().to_owned(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "test fixtures: a fixture that does not build fails the test"
)]
mod live_tests;
