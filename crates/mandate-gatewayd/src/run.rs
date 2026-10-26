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
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mandate_gateway::CallRequest;
use mandate_gateway::fake::RecordedTransport;
use mandate_gateway::provider::{Clock, IdSource, SequentialIds};
use mandate_gateway::wire::Reply;
use mandate_journal::ArtifactSource;
use mandate_process::lifecycle::Lifecycle;
use mandate_process::probe::{Probe, Status};
use mandate_research_run::recorded::RecordedModel;
use mandate_store::Store;
use mandate_store::meter::JournalMeter;
use mandate_time::UtcNanos;

use crate::config::Config;
use crate::recorded::Replies;
use crate::serve::Log;
use crate::service::{Control, Service};

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
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub fn code(self) -> &'static str {
        let _ = self;
        todo!()
    }
}

/// Reads the arguments after the program's name. Each flag takes the next argument as its value,
/// and a value that is empty or is itself a flag reads as missing; a flag written `--name=value`
/// is never read, so it refuses as missing too. Asking for a live start in any form refuses it,
/// whatever else is named.
///
/// # Errors
/// The first [`ArgsError`]: the live refusal, then the probe socket, then the subcommand's own.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn command(args: &[String]) -> Result<Command, ArgsError> {
    let _ = args;
    todo!()
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
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub fn code(self) -> &'static str {
        let _ = self;
        todo!()
    }
}

/// Reads the recording at `path`, strictly, once.
///
/// # Errors
/// [`StartFailure::RecordedUnreadable`] when the file cannot be read, and
/// [`StartFailure::RecordedMalformed`] when it is not strict JSON or lacks a member.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn read_recorded(path: &Path) -> Result<RecordedModel, StartFailure> {
    let _ = path;
    todo!()
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
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn recorded_service<S: Store + 'static, C: Clock>(
    config: &Config,
    model: &RecordedModel,
    parts: Parts<S, C>,
) -> Result<RecordedService<S, C>, StartFailure> {
    let _ = (config, model, parts);
    todo!()
}

/// The recorded service's answer to `request`: the transport primed with the reply its turn
/// takes, then the service's answer, so each call is sent exactly its own reply.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn answer_recorded<S: Store, C: Clock>(
    service: &mut RecordedService<S, C>,
    replies: &Replies,
    request: &CallRequest,
) -> Reply {
    let _ = (service, replies, request);
    todo!()
}

/// The ids a meter stream's writer opens `stream` with, its `StreamOpened` the one record it gives
/// an id of its own (DEC-2258 item 2): drawn from the stream's own name, as the cell's runtime
/// draws them, so no two streams open with one id and a restart reopens with the same one.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn opened_ids(stream: &str) -> SequentialIds {
    let _ = stream;
    todo!()
}

/// The ids of one `lane` of records the service writes from its start at `at`: a ULID whose time
/// is `at`'s millisecond, and whose random part opens with `lane`, so the call ids and the
/// deployment meter's copies never meet, and a later start's ids sort after an earlier one's.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn started_ids(at: UtcNanos, lane: u8) -> SequentialIds {
    let _ = (at, lane);
    todo!()
}

/// The host's clock: the one place the binary reads it (ES-05).
#[allow(
    clippy::disallowed_methods,
    reason = "the binary's edge reads the host clock, which the service takes as an input (ES-05)"
)]
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn host_now() -> UtcNanos {
    todo!()
}

/// The service's log over `W`: each line, which holds only codes, written whole with its newline.
pub struct WriteLog<W>(Mutex<W>);

impl<W> WriteLog<W> {
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub fn new(out: W) -> Self {
        let _ = out;
        todo!()
    }
    /// What was written, for a test to read.
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub fn into_inner(self) -> W {
        let _ = self;
        todo!()
    }
}

impl<W: Write + Send> Log for WriteLog<W> {
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    fn line(&self, line: &str) {
        let _ = (self, line);
        todo!()
    }
}

/// Reports `lifecycle` live on `status` every `every`, at `now()`, while `serving` holds, on a
/// thread of its own, so the liveness probe fails within the lifecycle's bound of the serve loop
/// ending.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn heartbeat(
    lifecycle: Lifecycle,
    status: Status,
    serving: Arc<AtomicBool>,
    every: Duration,
    now: fn() -> UtcNanos,
) {
    let _ = (lifecycle, status, serving, every, now);
    todo!()
}
