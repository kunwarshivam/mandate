//! The workspace's Postgres journal and artifact store behind [`ControlJournal`] (P0, X-12;
//! DEC-520). Every append is J0's artifact-aware one (DEC-510), and the commands D1, D2 and V0 add
//! flatten [`JournalArgs`].

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use mandate_artifacts_fs::FsArtifactStore;
use mandate_journal::{AppendOutcome, Head, StoredEvent, StreamId};
use mandate_journal_pg::PgJournal;
use mandate_time::UtcNanos;
use tokio::runtime::Runtime;

use crate::control::{ControlError, ControlJournal};

/// Where a control command reads and appends. The DSN carries no password: sqlx reads one from
/// `PGPASSWORD` or `~/.pgpass`, and neither [`Debug`](fmt::Debug) nor any error prints the DSN
/// (`AGENTS.md` rule 7; DEC-520).
#[derive(Clone, clap::Args)]
pub struct JournalArgs {
    /// The workspace journal's Postgres DSN, without a password (set PGPASSWORD or ~/.pgpass).
    #[arg(long, value_name = "DSN")]
    pub journal: String,
    /// The root directory of the artifact store the journal's records name.
    #[arg(long, value_name = "DIR")]
    pub store: PathBuf,
}

impl fmt::Debug for JournalArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JournalArgs")
            .field("journal", &"<not shown>")
            .field("store", &self.store)
            .finish()
    }
}

/// The workspace's Postgres journal and its artifact store as a [`ControlJournal`]. It owns a
/// current-thread runtime and blocks on each call, so it is used outside any other runtime.
pub struct PgControlJournal {
    journal: PgJournal,
    store: FsArtifactStore,
    runtime: Runtime,
}

impl PgControlJournal {
    /// Opens the journal at `args.journal` without connecting, and the store at `args.store`,
    /// creating its directories if they are missing.
    ///
    /// # Errors
    /// [`ControlError::Journal`] for a DSN that does not parse or a store that cannot be opened. No
    /// message names the DSN.
    pub fn open(args: &JournalArgs) -> Result<Self, ControlError> {
        let _ = args;
        Err(ControlError::Unimplemented { story: "E10-16" })
    }
}

impl ControlJournal for PgControlJournal {
    fn rows(&self, stream: &StreamId) -> Result<Vec<StoredEvent>, ControlError> {
        let _ = (stream, &self.journal, &self.runtime);
        Err(ControlError::Unimplemented { story: "E10-16" })
    }

    fn head(&self, stream: &StreamId) -> Result<Head, ControlError> {
        let _ = stream;
        Err(ControlError::Unimplemented { story: "E10-16" })
    }

    fn take_ownership(&mut self, stream: &StreamId) -> Result<u64, ControlError> {
        let _ = stream;
        Err(ControlError::Unimplemented { story: "E10-16" })
    }

    fn append(
        &mut self,
        stream: &StreamId,
        expected_head: u64,
        writer_epoch: u64,
        recorded_at: UtcNanos,
        drafts: &[&[u8]],
    ) -> Result<AppendOutcome, ControlError> {
        let _ = (stream, expected_head, writer_epoch);
        let _ = (recorded_at, drafts, &self.store);
        Err(ControlError::Unimplemented { story: "E10-16" })
    }

    fn wait(&mut self, delay: Duration) {
        std::thread::sleep(delay);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pause between attempts is slept, not skipped: a command that retried at once would
    /// fence the writer it collided with again (DEC-290).
    #[test]
    #[allow(
        clippy::disallowed_methods,
        reason = "the test measures the sleep; no core code reads the clock"
    )]
    fn waiting_sleeps_for_the_whole_delay() -> Result<(), Box<dyn std::error::Error>> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let journal =
            runtime.block_on(async { PgJournal::from_dsn("postgres://journal.invalid/j") })?;
        let store = FsArtifactStore::open(std::env::temp_dir().join("mandate-cli-wait"))?;
        let mut pg = PgControlJournal {
            journal,
            store,
            runtime,
        };
        let (delay, started) = (Duration::from_millis(30), std::time::Instant::now());
        pg.wait(delay);
        assert!(started.elapsed() >= delay, "{:?}", started.elapsed());
        Ok(())
    }
}
