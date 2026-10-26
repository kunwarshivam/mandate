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

mod ask;
mod connect_check;
mod connection;
mod door;
mod evidence;
mod grant;
mod harden;
mod lt10;
mod quote;
mod robinhood;
mod run_clock;
#[cfg(any(test, feature = "rh-sim"))]
mod sim_market;
mod snapshot;
mod wiring;

use std::ffi::OsString;
use std::path::PathBuf;

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use mandate_accounting::InstrumentId;
use mandate_alpaca::{
    AlpacaPaperHttp, Credentials, DataTransport, LatestQuote, Pause, TokioPause, TradingTransport,
};
use mandate_artifacts_fs::FsArtifactStore;
use mandate_canon::Value;
use mandate_domain::{AssetClass, Environment};
use mandate_journal::{
    AppendOutcome, ArtifactRef, ArtifactSource, ArtifactStore, StoredEvent, StreamId, get_artifact,
};
use mandate_journal_pg::PgJournal;
use mandate_mcp::{McpClient, Tools};
use mandate_modelhost::{DailyCloses, Evaluation, Pin, Refusal, Signal, evaluate};
use mandate_num::Usd;
use mandate_robinhood::AccountSnapshot;
use mandate_runtime::Observation;
use mandate_shell::adapters::Sources;
use mandate_shell::approvers::live_cli_confirm;
use mandate_shell::control::{
    Build as BuiltWith, ConfigRefusal, Confirmation, ConfirmedVersion, ControlRecord,
    DeploymentRefusal, DoorChecks, LiveAdmission, McpHost, Origin, Pinned, RunFacts,
    checked_before_credentials, configuration, confirmed_version,
};
use mandate_shell::host::refuse_configured_host;
use mandate_shell::paper::{
    Artifacts, BrokerFacts, DeploymentInput, GateQuote, PaperClock, PaperFacts, daily_closes,
    fresh_quote, liquidity_facts, load_contexts_with_clock, preflight, reconcile_context,
};
use mandate_shell::symbols::BrokerSymbols;
use mandate_shell::{
    Booked, Cause, Mark, ProductionCycle, Report, Setup, ShellError, Stage, Watch, production_cycle,
};
use mandate_shell::{close_window_bound, poll_interval};
use mandate_spec::Mandate;
use mandate_spec::context::{AgentId, Membership};
use mandate_time::{ExchangeCalendar, UtcNanos};

pub use ask::{BrokerAsk, QuoteRead};
pub use connect_check::{
    ACCOUNT_NUMBER_VAR, CheckArgs, CheckPorts, Checked, Observed, SeenContract, connect_check,
    observe, parse_check,
};
pub use connection::{
    Broker, Connection, ConnectionRefusal, Held, LapseRefusal, connection, lapse,
};
pub use door::{account_stream, chosen, lapse_read, unspent, unsupplied};
pub use evidence::{Evidence, EvidenceRefusal, evidence};
pub use grant::{Asked, GrantRefusal, Journaled, grant};
pub use harden::disable_core_dumps;
pub use lt10::{Lt10Refusal, Watched, lt10};
pub use quote::{QuoteRefusal, cross_check, fresh_ask};
pub use robinhood::{
    Build, Dial, EndpointRefusal, Linked, LoggedIn, Market, MarketUnread, REHEARSAL_ENDPOINT_VAR,
    REHEARSAL_HOST, RobinhoodEndpoint, SessionMarket, dial, endpoint, pinned_host, robinhood_reads,
};
pub use run_clock::{ReadClock, RunClock, run_clock};
#[cfg(feature = "rh-sim")]
pub use run_clock::{SIM_CLOCK_VAR, SimClock};
pub use snapshot::LapseSnapshot;
pub use wiring::{
    CollarMembers, WakeRead, answers, asked, broker_cross_check, collar_x, cross_check_the_opening,
    granted_opening, lt10_end, reported, robinhood_cross_check, session_cap, wait_for_the_grant,
    watch_may_end, watched,
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
    /// `--reconcile-only` (DEC-955 item 6): book the deployment's own unbooked fills to the
    /// journal and exit, before any market read, model or cycle. It never places.
    pub reconcile_only: bool,
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
    /// `earlier` is the account snapshot this run's live start already read through this session
    /// for the lapse, if any (DEC-959 item 1): the session stands on it while it is fresh and
    /// reads the account again otherwise.
    ///
    /// # Errors
    /// [`Cause::Absent`] when a read is not answered with the fact it asked for.
    fn preflight<P: Pause + Clone>(
        &self,
        artifacts: &Artifacts,
        pause: P,
        earlier: Option<&LapseSnapshot>,
    ) -> Result<BrokerFacts, Cause>;

    /// The production cycle over `sources` with this session's connector.
    ///
    /// # Errors
    /// [`Cause::Absent`] when the session cannot carry a cycle.
    fn cycle(&self, sources: Sources<()>, setup: Setup) -> Result<ProductionCycle, Cause>;

    /// A fresh IEX quote for the pinned instrument, read at an asked opening's wake so the grant
    /// is judged on a quote of its own wake, never on the preflight's (DEC-943 item 2): the
    /// preflight's own read, its age bound and its skew tolerance unchanged.
    ///
    /// # Errors
    /// [`Cause::Absent`] when no current, uncrossed IEX quote of the instrument is read.
    fn fresh_quote<P: Pause>(&self, artifacts: &Artifacts, pause: P) -> Result<LatestQuote, Cause>;

    /// Where the broker's own ask for the pinned instrument comes from: the quote an asked
    /// opening's cross-check compares with the independent IEX ask before it leaves (DEC-529
    /// item 12; DEC-940 item 4; DEC-945), taken before the cycle takes the session and asked at
    /// the check. [`BrokerAsk::Absent`] when this session reads none — and then the cross-check
    /// refuses the opening and nothing is sent (rule 3).
    fn broker_ask(&self, facts: &BrokerFacts) -> BrokerAsk;

    /// The agentic account's snapshot the live door's lapse counts (DEC-933 item 3, DEC-947
    /// item 1), read and sending nothing, or [`None`] when this session reads none — and then a
    /// live connection's lapse refuses `lapse_evidence_not_supplied` (DEC-919).
    ///
    /// # Errors
    /// [`Cause::Absent`] when the read is refused or unreadable.
    fn account_snapshot(&self) -> Result<Option<AccountSnapshot>, Cause>;
}

impl<T: TradingTransport + DataTransport + Clone + 'static> Session for T {
    /// An Alpaca paper session reads its own account: no live start reads one before it.
    fn preflight<P: Pause + Clone>(
        &self,
        artifacts: &Artifacts,
        pause: P,
        _earlier: Option<&LapseSnapshot>,
    ) -> Result<BrokerFacts, Cause> {
        preflight(artifacts, self.clone(), self.clone(), pause)
    }

    fn cycle(&self, sources: Sources<()>, setup: Setup) -> Result<ProductionCycle, Cause> {
        Ok(production_cycle(
            with_transport(sources, self.clone()),
            setup,
        ))
    }

    fn fresh_quote<P: Pause>(&self, artifacts: &Artifacts, pause: P) -> Result<LatestQuote, Cause> {
        fresh_quote(artifacts, self.clone(), pause)
    }

    /// Alpaca's own quote is the IEX quote its data client read at the preflight.
    fn broker_ask(&self, facts: &BrokerFacts) -> BrokerAsk {
        BrokerAsk::Quoted(facts.quote.ask)
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
        content_store,
        symbols,
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
        content_store,
        symbols,
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
    /// `--reconcile-only` ran: the deployment's own unbooked fills were booked and nothing was
    /// sent (DEC-955 item 6).
    Reconciled(Reconciled),
}

/// What a `--reconcile-only` run booked: how many fill records it journaled, and that a second
/// pass over the journal it wrote found nothing more owed (DEC-955 item 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reconciled {
    /// The fill records journaled, `FillApplied` and `LateFillApplied` together.
    pub fills: usize,
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
    /// The rehearsal build's run clock variable is set and is not an RFC 3339 instant (DEC-962
    /// item 1), before anything is read; the run never falls back to the system clock for it.
    #[cfg(any(test, feature = "rh-sim"))]
    #[error("the rehearsal's run clock is not an RFC 3339 instant")]
    SimClock,
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
    /// The connect check ran and a check failed; the failure is journaled as `event_id`
    /// (DEC-946). It names the check and its reason, never the account.
    #[error("the connection check failed: {check} {reason} (journaled as {event_id})")]
    ConnectCheck {
        event_id: String,
        check: &'static str,
        reason: &'static str,
    },
    /// The account stream could not be read or written; nothing further was sent.
    #[error("the journal could not be written")]
    Journal,
    #[error(transparent)]
    Shell(#[from] ShellError),
}

/// Parses the arguments after the program name: `--workspace`, `--agent`, `--account-ref`,
/// `--journal`, `--store` and `--bars`, each with a value; exactly one acknowledgement,
/// `--confirm-paper` or `--confirm-live` (DEC-937 item 3); `--place-one-order`, which needs
/// `--journal`; and `--reconcile-only` (DEC-955 item 6), which needs `--journal` and excludes
/// `--place-one-order`. Any other argument is unknown, so none names a mandate, a configuration, a model
/// output or a host.
///
/// # Errors
/// [`PaperError::Usage`] for a missing or doubled acknowledgement, a missing flag or value, or an
/// unknown argument.
pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Args, PaperError> {
    let mut values: [Option<String>; 6] = Default::default();
    let (mut paper, mut live, mut place_one_order) = (false, false, false);
    let mut reconcile_only = false;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let Some(at) = FLAGS.iter().position(|flag| *flag == arg) else {
            match arg.as_str() {
                "--confirm-paper" => paper = true,
                "--confirm-live" => live = true,
                "--place-one-order" => place_one_order = true,
                "--reconcile-only" => reconcile_only = true,
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
    if reconcile_only && journal.is_none() {
        return Err(usage("--reconcile-only needs --journal".to_owned()));
    }
    if reconcile_only && place_one_order {
        return Err(usage(
            "--reconcile-only and --place-one-order are exclusive".to_owned(),
        ));
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
        reconcile_only,
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

/// What an asked opening's cross-check reads (DEC-940 item 4; DEC-945 item 3): the pinned
/// instrument's asset class and 20-day median dollar volume, which choose the collar's own `x`,
/// where the broker's own ask is read at the check, the independent IEX quote the preflight
/// read, and the age bound the broker's read is held to.
struct Opening {
    asset_class: AssetClass,
    median_dollar_volume_20d: Usd,
    broker_ask: BrokerAsk,
    independent: LatestQuote,
    quote_max_age: Duration,
}

impl Opening {
    /// [`broker_cross_check`] over the registered collar's own members, at `pause`'s clock.
    fn cross_check<P: Pause + ?Sized>(
        &self,
        collar: &CollarMembers,
        pause: &P,
    ) -> Result<(), PaperError> {
        broker_cross_check(
            collar,
            self.asset_class,
            Some(self.median_dollar_volume_20d),
            &self.broker_ask,
            &self.independent,
            pause,
            self.quote_max_age,
        )
    }
}

/// One wake's fresh quote (DEC-943 item 2): `session`'s own IEX read through
/// [`Session::fresh_quote`], handed to the binding gate's `gate` so every binding request from
/// now on judges it, and returned as the wake's risk mark. A read that fails refuses as
/// untrusted market data, and the wait it stops sends nothing.
///
/// # Errors
/// [`PaperError::Shell`] at [`Stage::MarketData`] when no current, uncrossed IEX quote is read.
pub fn fresh_mark<S: Session, P: Pause>(
    session: &S,
    artifacts: &Artifacts,
    pause: P,
    gate: &GateQuote,
) -> Result<Mark, PaperError> {
    let quote = session
        .fresh_quote(artifacts, pause)
        .map_err(refused(Stage::MarketData))?;
    gate.hand(&quote);
    Ok(Mark::of_iex(&quote))
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
    let live = live_started(
        door.as_ref(),
        &records,
        &store,
        &mut facts,
        args,
        vars,
        THIS_BUILD,
        ports,
    )?;
    let (confirmed, early, lapse) = match live {
        Some(started) => (started.confirmed, Some(started.session), started.lapse),
        None => {
            if let Some((connection, stream)) = &door {
                lapse_read(connection, args.journal.as_deref(), stream, None)?;
            }
            let confirmed =
                confirmed_version(&records, &store, &facts).map_err(PaperError::Deployment)?;
            (confirmed, None, None)
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
    if args.reconcile_only && args.journal.is_none() {
        return Err(usage("--reconcile-only needs --journal".to_owned()));
    }
    let (connection, _) =
        door.ok_or_else(|| absent(Stage::Validate, "the connection the deployment names"))?;
    let live_scope = live_cli_confirm(&records, confirmed.mandate(), connection.environment);
    let session = match early {
        Some(session) => session,
        None => ports.connect(&connection, None)?,
    };
    let booking = Booking {
        session: &session,
        artifacts: &artifacts,
        deployed: &deployed,
        version: confirmed.version(),
        connection: connection.environment,
        args,
    };
    let booked = booking.book(clock.now())?;
    if args.reconcile_only {
        let again = booking.book(clock.now())?;
        if again.fills != 0 {
            return Err(absent(
                Stage::Reconcile,
                "a journal that owes no fill once booked",
            ));
        }
        return Ok(Outcome::Reconciled(Reconciled {
            fills: booked.fills,
        }));
    }
    let broker = session
        .preflight(&artifacts, clock.clone(), lapse.as_ref())
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
    let bytes = mandate_bars::closes_object(&daily.closes, instrument.asset_id.as_str())
        .map_err(|_| absent(Stage::Journal, "the observation's members"))?;
    let data_ref = store.put_artifact(&bytes).map_err(|_| PaperError::Store)?;
    let observation = Observation {
        source: SOURCE.to_owned(),
        instrument_id: Some(asset),
        as_of: output.as_of,
        data_ref: data_ref.digest(),
    };
    let agent = deployed.deployment().agent.clone();
    let at_preflight = Mark::of_iex(&broker.quote);
    let ticker = InstrumentId::new(&instrument.symbol)
        .map_err(|_| absent(Stage::Validate, "the pinned ticker"))?;
    let asset_id = InstrumentId::new(instrument.asset_id.as_str())
        .map_err(|_| absent(Stage::Validate, "the pinned asset id"))?;
    let opening = Opening {
        asset_class: instrument.asset_class,
        median_dollar_volume_20d: liquidity.median_dollar_volume_20d,
        broker_ask: session.broker_ask(&broker),
        independent: broker.quote.clone(),
        quote_max_age: artifacts.quote_max_age(),
    };
    let paper = PaperFacts { broker, liquidity };
    let binding_clock = Rc::new(PortClock(clock.clone()));
    let mut contexts = load_contexts_with_clock(
        &artifacts,
        &paper,
        now,
        &agent,
        connection.environment,
        binding_clock,
    )
    .map_err(refused(Stage::Validate))?;
    contexts.run.live_cli_confirm = live_scope;
    let contexts_quote = contexts.quote.clone();
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
        connection_environment: connection.environment,
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
        content_store: Some(Box::new(store.clone())),
        artifacts: Some(Arc::new(store)),
        symbols: BrokerSymbols::registered([(ticker, asset_id)]),
        transport: (),
    };
    let fresh = || fresh_mark(&session, &artifacts, watch.pause.clone(), &contexts_quote);
    let report = session
        .cycle(sources, setup)
        .map_err(refused(Stage::Reconcile))?
        .run_observed_asked_watched(
            observation,
            output,
            &watch,
            &at_preflight,
            |asked| {
                granted_opening(asked, &watch.pause, watch.interval, fresh, || {
                    opening.cross_check(&collar, &watch.pause)
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

/// The own-fill reconciliation a run performs before its preflight is judged (DEC-955): a
/// placing or `--reconcile-only` run on a paper connection books the deployment's own unbooked
/// fills to its journal, through a cycle whose executor context answers no binding request
/// ([`reconcile_context`]), before DEC-470 item 1's flat-account check, which is unchanged. A
/// dry run keeps nothing (DEC-157 item 6) and a live connection keeps its own lapse (DEC-529
/// item 3), so neither books.
struct Booking<'r, S> {
    session: &'r S,
    artifacts: &'r Artifacts,
    deployed: &'r DeploymentInput,
    version: mandate_canon::Digest,
    /// The environment of the connection the run chose; a live run books nothing.
    connection: Environment,
    args: &'r Args,
}

impl<S: Session> Booking<'_, S> {
    /// One own-fill pass at `now`, or nothing booked where the run books nothing.
    ///
    /// # Errors
    /// [`PaperError::Store`] when the store cannot be opened, and every [`PaperError::Shell`]
    /// stop of [`ProductionCycle::book_own_fills`].
    fn book(&self, now: UtcNanos) -> Result<Booked, PaperError> {
        let journal = self
            .args
            .journal
            .clone()
            .filter(|_| self.args.place_one_order || self.args.reconcile_only)
            .filter(|_| self.connection == Environment::Paper);
        let Some(journal) = journal else {
            return Ok(Booked::default());
        };
        let store = FsArtifactStore::open(&self.args.store).map_err(|_| PaperError::Store)?;
        let deployment = self.deployed.deployment();
        let context = reconcile_context(self.artifacts, &deployment.agent)
            .map_err(refused(Stage::Reconcile))?;
        let identity = self.artifacts.production_identity();
        let ticker = identity.symbol.clone();
        let asset_id = InstrumentId::new(identity.asset_id.as_str())
            .map_err(|_| absent(Stage::Validate, "the pinned asset id"))?;
        let sources = Sources {
            mandate: store.object_path(&ArtifactRef::from_digest(self.version)),
            journal: Some(journal),
            recorded_at: now,
            agent: deployment.agent.clone(),
            workspace: deployment.workspace.0.clone(),
            account_ref: self.deployed.account_ref().to_owned(),
            executor: Some(context),
            run: None,
            artifacts: Some(Arc::new(store)),
            content_store: None,
            symbols: BrokerSymbols::registered([(ticker, asset_id)]),
            transport: (),
        };
        let setup = Setup {
            deployment: deployment.clone(),
            account_ref: self.deployed.account_ref().to_owned(),
            now,
            place_one_order: false,
            new_cycle: false,
            connection_environment: self.connection,
        };
        let booked = self
            .session
            .cycle(sources, setup)
            .map_err(refused(Stage::Reconcile))?
            .book_own_fills()?;
        Ok(booked)
    }
}

/// Whether this binary was built with the runner's `live` feature, as the shell's admission
/// reads it (DEC-924 item 1, DEC-947 item 3).
pub const BUILT_WITH: BuiltWith = if cfg!(feature = "live") {
    BuiltWith::Live
} else {
    BuiltWith::Default
};

/// Which build a run is, as its start reads it: the endpoint and pin it dials
/// ([`Build`]) and whether the runner was compiled with `live` ([`BuiltWith`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Built {
    build: Build,
    with: BuiltWith,
}

/// This binary's build.
const THIS_BUILD: Built = Built {
    build: Build::CURRENT,
    with: BUILT_WITH,
};

/// A live run's start (DEC-947 item 2): for a live connection the door chose, its dial of
/// `built`'s endpoint from `vars`, its admission from the run's own facts, and [`live_start`],
/// answering the confirmed version and the session it connected. [`None`] for a paper connection
/// or none, which `run()` confirms itself, its session connected later, after the configuration
/// (LT-14).
///
/// # Errors
/// [`PaperError::Endpoint`] for a dial the build cannot make, and every [`PaperError`] of
/// [`live_start`].
#[allow(
    clippy::too_many_arguments,
    reason = "the run's own inputs, each read once; DEC-947 item 2"
)]
fn live_started<P: Ports>(
    door: Option<&(Connection, StreamId)>,
    records: &[ControlRecord],
    store: &dyn ArtifactSource,
    facts: &mut RunFacts,
    args: &Args,
    vars: &[(String, String)],
    built: Built,
    ports: &mut P,
) -> Result<Option<Started<P::Session>>, PaperError> {
    let Some((connection, stream)) = door else {
        return Ok(None);
    };
    if connection.environment != Environment::Live {
        return Ok(None);
    }
    let dial = dial(built.build, vars).map_err(PaperError::Endpoint)?;
    facts.live = live_admission(
        built.with,
        args.acknowledged,
        DoorChecks::NotPassed,
        Some(&dial.endpoint),
        pinned_host(built.build),
    );
    let journal = args.journal.as_deref();
    live_start(
        records, store, facts, connection, stream, journal, &dial, ports,
    )
    .map(Some)
}

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
/// 3. the agentic account's snapshot ([`Session::account_snapshot`]), the account reads alone,
///    with the run clock read just before it is sent, kept for the run's preflight (DEC-959
///    item 1);
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
) -> Result<Started<P::Session>, PaperError> {
    checked_before_credentials(records, store, facts).map_err(PaperError::Deployment)?;
    let session = ports.connect(connection, Some(dial))?;
    let sent = ports.pause().now();
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
    Ok(Started {
        confirmed,
        session,
        lapse: snapshot.map(|account| LapseSnapshot { account, sent }),
    })
}

/// What a live start hands the run (DEC-947 item 2, DEC-959 item 1): the version confirmed with
/// the door passed, the one session, and the account snapshot the lapse counted, read through
/// that session, with the run clock read just before it was sent, which the run hands to that
/// session's preflight and nowhere else.
pub(crate) struct Started<S> {
    pub(crate) confirmed: ConfirmedVersion,
    pub(crate) session: S,
    pub(crate) lapse: Option<LapseSnapshot>,
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

/// The binary's ports (DEC-846 item 1, DEC-937 item 1): the control stream read from the
/// Postgres journal at `journal`, the session of the broker the connection chose — the paper
/// credentials from the environment with the Alpaca paper client, or the Robinhood login only a
/// `live` build can make — and this build's [`RunClock`] (DEC-962). Without a journal there is no
/// control stream to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Production {
    pub journal: Option<String>,
    pub clock: RunClock,
}

impl Ports for Production {
    type Session = Connected;
    type Pause = RunClock;

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
                robinhood::connect(dial, self.clock).map(Connected::Robinhood)
            }
        }
    }

    /// [`disable_core_dumps`].
    fn harden(&mut self) -> Result<(), PaperError> {
        disable_core_dumps()
    }

    fn pause(&self) -> RunClock {
        self.clock
    }
}

/// The session [`Production::connect`] answers: the broker the connection record chose.
pub enum Connected<T = McpClient> {
    Alpaca(AlpacaPaperHttp),
    Robinhood(Linked<T>),
}

impl<T: Tools + 'static> Session for Connected<T> {
    fn preflight<P: Pause + Clone>(
        &self,
        artifacts: &Artifacts,
        pause: P,
        earlier: Option<&LapseSnapshot>,
    ) -> Result<BrokerFacts, Cause> {
        match self {
            Self::Alpaca(transport) => transport.preflight(artifacts, pause, earlier),
            Self::Robinhood(linked) => linked.preflight(artifacts, pause, earlier),
        }
    }

    fn cycle(&self, sources: Sources<()>, setup: Setup) -> Result<ProductionCycle, Cause> {
        match self {
            Self::Alpaca(transport) => transport.cycle(sources, setup),
            Self::Robinhood(linked) => linked.cycle(sources, setup),
        }
    }

    fn fresh_quote<P: Pause>(&self, artifacts: &Artifacts, pause: P) -> Result<LatestQuote, Cause> {
        match self {
            Self::Alpaca(transport) => transport.fresh_quote(artifacts, pause),
            Self::Robinhood(linked) => linked.fresh_quote(artifacts, pause),
        }
    }

    fn broker_ask(&self, facts: &BrokerFacts) -> BrokerAsk {
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

/// One verified row as the control-stream fold reads it: its `seq`, type and body's `payload`,
/// and the stream and environment it was journaled on (DEC-957).
fn control_record(row: &StoredEvent) -> Result<ControlRecord, PaperError> {
    let body = mandate_canon::parse(&row.body).map_err(|_| PaperError::Control)?;
    let payload = body.get("payload").ok_or(PaperError::Control)?.clone();
    let origin = Origin::of(&row.stream_id, &row.environment).ok_or(PaperError::Control)?;
    Ok(ControlRecord {
        seq: row.seq,
        event_type: row.event_type.clone(),
        payload,
        origin,
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
    let mut args = args.into_iter().peekable();
    let check = args.next_if(|first| first == CONNECT_CHECK).is_some();
    let vars: Vec<(String, String)> = vars
        .into_iter()
        .map(|(name, value)| {
            let name = name.to_string_lossy().into_owned();
            (name, value.to_string_lossy().into_owned())
        })
        .collect();
    let clock = run_clock(&vars)?;
    if check {
        let args = parse_check(args)?;
        let mut ports = ProductionCheck {
            journal: args.journal.clone(),
            clock,
        };
        let checked = connect_check(&args, &vars, &mut ports)?;
        return Ok(vec![format!("checked {}", checked.event_id)]);
    }
    let args = parse(args)?;
    let mut ports = Production {
        journal: args.journal.clone(),
        clock,
    };
    lines(&run(&args, &vars, &mut ports)?)
}

/// The first argument that selects the connect check (DEC-946) instead of a run.
pub const CONNECT_CHECK: &str = "--connect-check";

/// The connect check's ports in the binary (DEC-946): core dumps off, the founder's login at the
/// endpoint this build pins and the reads over its session ([`robinhood::login_from_env`],
/// [`Linked::observe`]), this build's [`RunClock`], and the account stream in the Postgres journal
/// at `journal`. No error names the DSN, the token or the account number (rule 7).
#[derive(Clone, PartialEq, Eq)]
pub struct ProductionCheck {
    pub journal: String,
    pub clock: RunClock,
}

/// The DSN may hold a password, so `Debug` never prints it (rule 7).
impl std::fmt::Debug for ProductionCheck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProductionCheck")
            .field("journal", &"<redacted>")
            .finish()
    }
}

impl ProductionCheck {
    /// `work` against the journal on a runtime of its own, any failure [`PaperError::Journal`].
    fn journal<T, F>(&self, work: impl FnOnce(PgJournal) -> F) -> Result<T, PaperError>
    where
        F: Future<Output = Result<T, PaperError>>,
    {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| PaperError::Journal)?;
        runtime.block_on(async {
            let journal = PgJournal::from_dsn(&self.journal).map_err(|_| PaperError::Journal)?;
            work(journal).await
        })
    }
}

impl CheckPorts for ProductionCheck {
    /// [`disable_core_dumps`].
    fn harden(&mut self) -> Result<(), PaperError> {
        disable_core_dumps()
    }

    fn observe(&mut self, account_number: &str) -> Result<Observed, PaperError> {
        Ok(robinhood::login_from_env()?.observe(account_number))
    }

    fn now(&self) -> UtcNanos {
        self.clock.now()
    }

    fn own(&mut self, stream: &StreamId) -> Result<(u64, u64), PaperError> {
        self.journal(|journal| async move {
            let epoch = journal
                .take_ownership(stream)
                .await
                .map_err(|_| PaperError::Journal)?;
            let head = journal
                .head(stream)
                .await
                .map_err(|_| PaperError::Journal)?;
            Ok((epoch, head.seq))
        })
    }

    fn append(
        &mut self,
        stream: &StreamId,
        head: u64,
        epoch: u64,
        at: UtcNanos,
        drafts: &[Vec<u8>],
    ) -> Result<AppendOutcome, PaperError> {
        let drafts: Vec<&[u8]> = drafts.iter().map(Vec::as_slice).collect();
        self.journal(|journal| async move {
            journal
                .append(stream, head, epoch, at, &drafts)
                .await
                .map_err(|_| PaperError::Journal)
        })
    }
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
        Outcome::Reconciled(Reconciled { fills }) => {
            let records = if *fills == 1 { "record" } else { "records" };
            vec![format!("booked {fills} fill {records}; nothing sent")]
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
