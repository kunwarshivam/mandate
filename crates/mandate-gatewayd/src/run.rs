//! The binary's start (DEC-2326 items 1 to 3, DEC-2257 item 2, E21-62): what its arguments ask
//! for, and the recorded service built over the journaled meters, both checked before the listener
//! opens. `--recorded <research.json>` is the only mode it starts in: every test and the cell run
//! it, and it reaches no provider and reads no key. A live start (`--registry`) is refused
//! `live_not_built` until the founder names the live model and the budgets (DEC-2265 item 10,
//! DEC-79), so no argument, value or file can make this binary spend.

use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mandate_canon::{Digest, parse};
use mandate_gateway::fake::RecordedTransport;
use mandate_gateway::meter::{Caps, DeploymentMeter, Meter};
use mandate_gateway::openrouter::OpenRouter;
use mandate_gateway::provider::{Clock, IdSource, SequentialIds};
use mandate_gateway::wire::Reply;
use mandate_gateway::{CallRequest, Gateway};
use mandate_journal::{ArtifactSource, Environment};
use mandate_process::lifecycle::{Input, Lifecycle};
use mandate_process::probe::{Probe, Status};
use mandate_research_run::RecordedKey;
use mandate_research_run::recorded::RecordedModel;
use mandate_store::Store;
use mandate_store::meter::JournalMeter;
use mandate_time::UtcNanos;

use crate::config::Config;
use crate::recorded::{Replies, settings};
use crate::serve::Log;
use crate::service::{Control, Service};
use crate::start::meters_ready;

/// What the process was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Serve calls, as [`RunArgs`] says.
    Run(RunArgs),
    /// Answer one `exec` probe over the running process's socket.
    Probe { asked: Probe, socket: PathBuf },
}

/// What `run` was started with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunArgs {
    /// The recorded `research.json` the service replays.
    pub recorded: PathBuf,
    /// The address the runtime calls (DEC-2265 item 1).
    pub listen: SocketAddr,
    /// The probe socket's path.
    pub probe: PathBuf,
}

/// Why the arguments start nothing, by its code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ArgsError {
    #[error("neither run nor probe was asked for")]
    Usage,
    #[error("no probe socket is named")]
    ProbeSocketMissing,
    #[error("the probe asked for is neither livez nor readyz")]
    ProbeUnknown,
    #[error("no listen address is named, or it is not one")]
    ListenMissing,
    #[error("no recorded bundle is named")]
    RecordedMissing,
    #[error("a live start is not built until the live model and budgets are named")]
    LiveNotBuilt,
}

impl ArgsError {
    /// The code the process exits with.
    pub fn code(self) -> &'static str {
        match self {
            Self::Usage => "usage",
            Self::ProbeSocketMissing => "probe_socket_missing",
            Self::ProbeUnknown => "probe_unknown",
            Self::ListenMissing => "listen_missing",
            Self::RecordedMissing => "recorded_bundle_missing",
            Self::LiveNotBuilt => "live_not_built",
        }
    }
}

/// Reads the arguments after the program's name. Each flag takes the next argument as its value,
/// and a value that is empty or is itself a flag reads as missing; a flag written `--name=value`
/// is never read, so it refuses as missing too. Asking for a live start in any form refuses it,
/// whatever else is named.
///
/// # Errors
/// The first [`ArgsError`]: the live refusal, then the probe socket, then the subcommand's own.
pub fn command(args: &[String]) -> Result<Command, ArgsError> {
    let flag = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i.checked_add(1)?))
            .filter(|value| !value.is_empty() && !value.starts_with("--"))
            .cloned()
    };
    if args
        .iter()
        .any(|a| a == "--registry" || a.starts_with("--registry="))
    {
        return Err(ArgsError::LiveNotBuilt);
    }
    let socket = flag("--probe")
        .map(PathBuf::from)
        .ok_or(ArgsError::ProbeSocketMissing)?;
    match args.first().map(String::as_str) {
        Some("probe") => {
            let asked = args
                .get(1)
                .and_then(|word| Probe::parse(word))
                .ok_or(ArgsError::ProbeUnknown)?;
            Ok(Command::Probe { asked, socket })
        }
        Some("run") => {
            let listen = flag("--listen")
                .and_then(|text| text.parse().ok())
                .ok_or(ArgsError::ListenMissing)?;
            let recorded = flag("--recorded")
                .map(PathBuf::from)
                .ok_or(ArgsError::RecordedMissing)?;
            Ok(Command::Run(RunArgs {
                recorded,
                listen,
                probe: socket,
            }))
        }
        _ => Err(ArgsError::Usage),
    }
}

/// Why the recorded service did not start, by its code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum StartFailure {
    #[error("the recorded bundle cannot be read")]
    RecordedUnreadable,
    #[error("the recorded bundle is not a research recording")]
    RecordedMalformed,
    #[error("the recording's entry or endpoint does not register")]
    Unregistered,
    #[error("a meter stream cannot be read: {0}")]
    Meter(crate::start::StartError),
    #[error("a deployed agent's cap cannot be read: {0}")]
    Cap(crate::cap::CapError),
}

impl StartFailure {
    /// The code the process exits with.
    pub fn code(self) -> &'static str {
        match self {
            Self::RecordedUnreadable => "recorded_bundle_unreadable",
            Self::RecordedMalformed => "recorded_bundle_malformed",
            Self::Unregistered => "recorded_bundle_unregistered",
            Self::Meter(error) => error.code(),
            Self::Cap(error) => error.code(),
        }
    }
}

/// Reads the recording at `path`, strictly, once.
///
/// # Errors
/// [`StartFailure::RecordedUnreadable`] when the file cannot be read, and
/// [`StartFailure::RecordedMalformed`] when it is not strict JSON or lacks a member.
pub fn read_recorded(path: &Path) -> Result<RecordedModel, StartFailure> {
    let bytes = std::fs::read(path).map_err(|_| StartFailure::RecordedUnreadable)?;
    let value = parse(&bytes).map_err(|_| StartFailure::RecordedMalformed)?;
    RecordedModel::read(&value).map_err(|_| StartFailure::RecordedMalformed)
}

/// What the recorded service is built over besides its start-up values and its recording.
pub struct Parts<S, C> {
    /// The journal the workspace's meter stream is written through.
    pub workspace_store: S,
    /// The journal the deployment's meter stream is written through.
    pub deployment_store: S,
    /// The gateway's clock.
    pub clock: C,
    /// The instant each meter record is written at.
    pub now: Rc<dyn Fn() -> UtcNanos>,
    /// The ids of, in order: each call; the workspace meter stream's `StreamOpened`; the
    /// deployment meter stream's `StreamOpened`; and each copy the deployment meter writes of a
    /// workspace's record (DEC-2207 item 2). Each source must never repeat an id another gives.
    pub ids: [Box<dyn IdSource>; 4],
    /// The control stream each research call's deployed version is read from.
    pub control: Box<dyn Control>,
    /// The artifact store the deployed mandate is read from.
    pub artifacts: Box<dyn ArtifactSource>,
}

/// The service the binary serves in `--recorded` mode.
pub type RecordedService<S, C> = Service<RecordedTransport, JournalMeter<S>, C>;

/// The recorded service for `config`'s workspace and deployment, replaying `model`: the gateway
/// over the recorded transport and the recorded key, metering through the journaled meter streams
/// `meter:{workspace}` and `dmeter:{deployment}` under `config`'s monthly budgets, and holding
/// each research call to its agent's deployed daily cap. It starts only when both meter streams
/// read back and every deployed agent's cap reads.
///
/// # Errors
/// [`StartFailure`]: a recording that does not register, a meter stream that does not read, or a
/// deployed agent's cap that does not.
pub fn recorded_service<S: Store + 'static, C: Clock>(
    config: &Config,
    model: &RecordedModel,
    parts: Parts<S, C>,
) -> Result<RecordedService<S, C>, StartFailure> {
    let Parts {
        workspace_store,
        deployment_store,
        clock,
        now,
        ids:
            [
                call_ids,
                workspace_ids,
                deployment_stream_ids,
                deployment_ids,
            ],
        control,
        artifacts,
    } = parts;
    let registry = model.registry().map_err(|_| StartFailure::Unregistered)?;
    let workspace = JournalMeter::open(
        workspace_store,
        &config.workspace,
        Environment::Paper,
        Rc::clone(&now),
        workspace_ids,
    );
    let deployment = JournalMeter::open_deployment(
        deployment_store,
        &config.deployment,
        Environment::Paper,
        now,
        deployment_stream_ids,
    );
    meters_ready(&workspace, &deployment).map_err(StartFailure::Meter)?;
    let meter = Meter::new(
        workspace,
        Caps::new(config.workspace_monthly),
        DeploymentMeter::new(
            Box::new(deployment),
            config.deployment_monthly,
            deployment_ids,
        ),
    );
    let gateway = Gateway::new(
        settings(&config.workspace, model),
        registry,
        Box::new(OpenRouter),
        RecordedTransport::new(),
        Box::new(RecordedKey),
        meter,
        clock,
        call_ids,
    );
    let mut service = Service::new(
        &config.workspace,
        Environment::Paper,
        gateway,
        control,
        artifacts,
    );
    service.ready().map_err(StartFailure::Cap)?;
    Ok(service)
}

/// The recorded service's answer to `request`: the transport primed with the reply its turn
/// takes, then the service's answer, so each call is sent exactly its own reply.
pub fn answer_recorded<S: Store, C: Clock>(
    service: &mut RecordedService<S, C>,
    replies: &Replies,
    request: &CallRequest,
) -> Reply {
    replies.prime(service.transport_mut(), request);
    service.answer(request)
}

/// The ids a meter stream's writer opens `stream` with, its `StreamOpened` the one record it gives
/// an id of its own (DEC-2258 item 2): drawn from the stream's own name, as the cell's runtime
/// draws them, so no two streams open with one id and a restart reopens with the same one.
pub fn opened_ids(stream: &str) -> SequentialIds {
    let digest = Digest::of(stream.as_bytes());
    let (high, _) = digest.as_bytes().split_at(16);
    SequentialIds::starting_at(u128::from_be_bytes(high.try_into().unwrap_or([0; 16])))
}

/// The ids of one `lane` of records the service writes from its start at `at`: a ULID whose time
/// is `at`'s millisecond, and whose random part opens with `lane`, so the call ids and the
/// deployment meter's copies never meet, and a later start's ids sort after an earlier one's.
pub fn started_ids(at: UtcNanos, lane: u8) -> SequentialIds {
    let ms = u128::try_from(at.secs())
        .unwrap_or(0)
        .saturating_mul(1_000)
        .saturating_add(u128::from(at.nanos() / 1_000_000));
    let time = (ms & ((1 << 48) - 1)) << 80;
    SequentialIds::starting_at(time.saturating_add(u128::from(lane) << 72))
}

/// The host's clock: the one place the binary reads it (ES-05).
#[allow(
    clippy::disallowed_methods,
    reason = "the binary's edge reads the host clock, which the service takes as an input (ES-05)"
)]
pub fn host_now() -> UtcNanos {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = i64::try_from(since.as_secs()).unwrap_or(i64::MAX);
    UtcNanos::from_parts(secs, since.subsec_nanos()).unwrap_or(UtcNanos::EPOCH)
}

/// The service's log over `W`: each line, which holds only codes, written whole with its newline.
pub struct WriteLog<W>(Mutex<W>);

impl<W> WriteLog<W> {
    pub fn new(out: W) -> Self {
        Self(Mutex::new(out))
    }

    /// What was written, for a test to read.
    pub fn into_inner(self) -> W {
        self.0.into_inner().unwrap_or_else(PoisonError::into_inner)
    }
}

impl<W: Write + Send> Log for WriteLog<W> {
    fn line(&self, line: &str) {
        let mut out = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let _a_log_that_cannot_be_written_stops_nothing = writeln!(out, "{line}");
    }
}

/// Reports `lifecycle` live on `status` every `every`, at `now()`, while `serving` holds, on a
/// thread of its own, so the liveness probe fails within the lifecycle's bound of the serve loop
/// ending.
pub fn heartbeat(
    mut lifecycle: Lifecycle,
    status: Status,
    serving: Arc<AtomicBool>,
    every: Duration,
    now: fn() -> UtcNanos,
) {
    thread::spawn(move || {
        while serving.load(Ordering::SeqCst) {
            lifecycle.observe(now(), Input::Heartbeat);
            status.publish(lifecycle.snapshot());
            thread::sleep(every);
        }
    });
}
