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
//! The deployment manager's process (E21-33 slice 2, DEC-1172): `mandate-deploy-manager`'s
//! `Deployer` over the Postgres journal and the Kubernetes API, with `mandate-process`'s lifecycle
//! and probes. The manager decides; this crate only wires its ports and folds each pass's outcome
//! into the lifecycle.

use std::collections::BTreeMap;
use std::path::Path;

use mandate_deploy_manager::{Alerts, ControlJournal, Halt, OperatorAlert, Process, Unavailable};
use mandate_journal::{AppendOutcome, MemoryJournal, StoredEvent, StreamId};
use mandate_journal_pg::PgJournal;
use mandate_process::Input;
use mandate_time::UtcNanos;
use serde_json::Value;

/// The workspace control stream over the Postgres journal. Workspace control services share the
/// stream, so each append takes a writer epoch for itself and writes at the head it reads then
/// (journal spec §5.1); the record's `event_id` makes a retry the same event.
pub struct PgControl {
    journal: PgJournal,
    stream: StreamId,
}

impl PgControl {
    /// The control stream `stream` of `journal`.
    pub fn new(journal: PgJournal, stream: StreamId) -> Self {
        Self { journal, stream }
    }
}

impl ControlJournal for PgControl {
    async fn rows_after(&mut self, seq: u64) -> Result<Vec<StoredEvent>, Unavailable> {
        let rows = self
            .journal
            .rows(&self.stream)
            .await
            .map_err(|_| Unavailable)?;
        Ok(rows.into_iter().filter(|r| r.seq > seq).collect())
    }

    async fn append(
        &mut self,
        draft: &[u8],
        recorded_at: UtcNanos,
    ) -> Result<AppendOutcome, Unavailable> {
        let epoch = self
            .journal
            .take_ownership(&self.stream)
            .await
            .map_err(|_| Unavailable)?;
        let head = self
            .journal
            .head(&self.stream)
            .await
            .map_err(|_| Unavailable)?;
        self.journal
            .append(&self.stream, head.seq, epoch, recorded_at, &[draft])
            .await
            .map_err(|_| Unavailable)
    }
}

/// The control stream in memory, for `run --fixture`: the same append protocol as [`PgControl`],
/// over a journal of the process's own, so the loop, its probes and its drain run without a
/// database.
#[derive(Debug)]
pub struct MemoryControl {
    journal: MemoryJournal,
    stream: StreamId,
}

impl MemoryControl {
    /// An empty control stream `stream`.
    pub fn new(stream: StreamId) -> Self {
        Self {
            journal: MemoryJournal::new(),
            stream,
        }
    }
}

impl ControlJournal for MemoryControl {
    async fn rows_after(&mut self, seq: u64) -> Result<Vec<StoredEvent>, Unavailable> {
        Ok(self
            .journal
            .rows(&self.stream)
            .iter()
            .filter(|r| r.seq > seq)
            .cloned()
            .collect())
    }

    async fn append(
        &mut self,
        draft: &[u8],
        recorded_at: UtcNanos,
    ) -> Result<AppendOutcome, Unavailable> {
        let epoch = self.journal.take_ownership(&self.stream);
        let head = self.journal.head(&self.stream).seq;
        Ok(self
            .journal
            .append(&self.stream, head, epoch, recorded_at, &[draft]))
    }
}

/// Operator alerts as one stderr line each: a stable code and its runbook, never content (design
/// §8.2). The alert path de-duplicates (DEC-988 item 6).
#[derive(Debug, Default)]
pub struct StderrAlerts {
    pub raised: Vec<OperatorAlert>,
}

impl Alerts for StderrAlerts {
    fn raise(&mut self, alert: OperatorAlert) {
        eprintln!("operator_alert {} {}", alert.code, alert.runbook);
        self.raised.push(alert);
    }
}

/// What one pass reports to the lifecycle. A pass that could not read, write, or list, or that
/// found the stream not yet opened, is a heartbeat, and the next pass starts again from the
/// journal; a record the journal refuses is a defect, so the process stops rather than retry it
/// blind.
pub fn input(pass: &Result<(), Halt>) -> Input {
    match pass {
        Ok(()) => Input::Heartbeat,
        Err(Halt::Refused { .. }) => Input::Failed,
        Err(Halt::Journal | Halt::Orchestrator | Halt::NotOpened) => Input::Heartbeat,
    }
}

/// Why the pod templates could not be read. None carries a file's content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateError {
    /// A template file exists but cannot be read, or is not a JSON object.
    Unreadable(Process),
}

impl TemplateError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Unreadable(Process::Runtime) => "runtime_template_unreadable",
            Self::Unreadable(Process::Executor) => "executor_template_unreadable",
            Self::Unreadable(Process::TokenExchange) => "token_exchange_template_unreadable",
        }
    }
}

/// The chart's pod template for each process, `<dir>/runtime.json`, `<dir>/executor.json` and
/// `<dir>/token-exchange.json`, each named for the chart's process (`Process::label`). A
/// process whose image the cell has not built has no file, so the manager journals its start and
/// the start fails until the image exists; one that is present must parse.
///
/// # Errors
/// [`TemplateError`].
pub fn templates(dir: &Path) -> Result<BTreeMap<Process, Value>, TemplateError> {
    let mut found = BTreeMap::new();
    for process in [Process::Runtime, Process::Executor, Process::TokenExchange] {
        let path = dir.join(format!("{}.json", process.label()));
        if !path.exists() {
            continue;
        }
        let text = std::fs::read(&path).map_err(|_| TemplateError::Unreadable(process))?;
        let template: Value =
            serde_json::from_slice(&text).map_err(|_| TemplateError::Unreadable(process))?;
        if !template.is_object() {
            return Err(TemplateError::Unreadable(process));
        }
        found.insert(process, template);
    }
    Ok(found)
}
