//! The IEX source over Postgres (DEC-2283 item 3): the control stream read through the journal with
//! the process's read-only role, and [`serve`], which builds the worker's parts over it, the bars
//! transport and the store, and runs [`worker::run`]. The binary hands it only what it reads at its
//! edge: the DSN, the key, the clock and the pause.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use mandate_artifacts_fs::FsArtifactStore;
use mandate_journal::{StoredEvent, StreamId, TrustedStart};
use mandate_journal_pg::PgJournal;
use mandate_marketdata::client::{Client, TokioPause};
use mandate_marketdata::http::{AlpacaDataHttp, Credentials};
use mandate_time::ExchangeCalendar;

use crate::iex::generation::Writer;
use crate::iex::pass::Pass;
use crate::iex::progress::Progress;
use crate::iex::schedule::Jitter;
use crate::iex::source::ClientSource;
use crate::iex::worker::{self, ControlRows, Edge, Parts};

/// The control stream `stream` over `journal`, each read driven to completion on `runtime`.
pub struct PgRows<'a> {
    pub runtime: &'a tokio::runtime::Runtime,
    pub journal: &'a PgJournal,
    pub stream: &'a StreamId,
}

impl ControlRows for PgRows<'_> {
    fn rows_from(&self, start: TrustedStart) -> Result<Vec<StoredEvent>, &'static str> {
        self.runtime
            .block_on(self.journal.rows_from(self.stream, start))
            .map_err(|e| e.code())
    }
}

/// Which workspace the source serves and where it writes.
#[derive(Debug, Clone)]
pub struct Hosted {
    pub workspace: String,
    pub store: PathBuf,
    pub artifacts: PathBuf,
}

/// Builds the worker's parts and runs it until `terminate` or a refused key. Whatever happens, the
/// worker ends marked stopped in `progress`, so the probe loop fails a process whose worker is
/// gone.
///
/// # Errors
/// A code when a part cannot be built: the workspace, the calendar, the runtime, the transport, the
/// journal, the artifact store or the writer.
pub fn serve(
    hosted: &Hosted,
    dsn: &str,
    key: Credentials,
    terminate: &AtomicBool,
    progress: &Progress,
    edge: Edge<'_>,
) -> Result<(), &'static str> {
    let built = (|| {
        let stream =
            StreamId::parse(&format!("ctl:{}", hosted.workspace)).ok_or("workspace_invalid")?;
        let calendar = ExchangeCalendar::us_equities().map_err(|e| e.code())?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "runtime_unavailable")?;
        let transport = AlpacaDataHttp::bars_only(key).map_err(|e| e.code())?;
        let source = ClientSource::new(Client::new(transport, TokioPause))?;
        let journal = runtime
            .block_on(async { PgJournal::single_writer(dsn) })
            .map_err(|e| e.code())?;
        let artifacts = FsArtifactStore::open_existing(&hosted.artifacts)
            .map_err(|_| "artifacts_unavailable")?;
        let writer =
            Writer::new(hosted.store.clone(), hosted.artifacts.clone()).map_err(|e| e.code())?;
        Ok((
            stream, calendar, runtime, source, journal, artifacts, writer,
        ))
    })();
    let (stream, calendar, runtime, source, journal, artifacts, writer) = match built {
        Ok(built) => built,
        Err(code) => {
            progress.stopped();
            return Err(code);
        }
    };
    let control = PgRows {
        runtime: &runtime,
        journal: &journal,
        stream: &stream,
    };
    let parts = Parts {
        source: &source,
        control: &control,
        artifacts: &artifacts,
        calendar: &calendar,
        pass: Pass::new(writer, Jitter::of(&hosted.workspace)),
    };
    worker::run(parts, terminate, progress, edge);
    Ok(())
}
