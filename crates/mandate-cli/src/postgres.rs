//! The workspace's Postgres journal and artifact store behind [`ControlJournal`] (P0, X-12;
//! DEC-520). Every append is J0's artifact-aware one (DEC-510), and the commands D1, D2 and V0 add
//! flatten [`JournalArgs`].

use std::path::PathBuf;
use std::time::Duration;

use mandate_artifacts_fs::FsArtifactStore;
use mandate_journal::{AppendOutcome, Head, StoredEvent, StreamId};
use mandate_journal_pg::{AppendError, PgJournal};
use mandate_time::UtcNanos;
use secrecy::{ExposeSecret, SecretString};
use tokio::runtime::Runtime;

use crate::control::{ControlError, ControlJournal};

/// Where a control command reads and appends. The DSN carries no password: sqlx reads one from
/// `PGPASSWORD` or `~/.pgpass`. The DSN is a [`SecretString`], so `Debug` cannot print it, and it
/// is exposed only to open the journal; no error prints it (`AGENTS.md` rule 7; DEC-520).
#[derive(Clone, Debug, clap::Args)]
pub struct JournalArgs {
    /// The workspace journal's Postgres DSN, without a password (set PGPASSWORD or ~/.pgpass).
    #[arg(long, value_name = "DSN")]
    pub journal: SecretString,
    /// The root directory of the artifact store the journal's records name.
    #[arg(long, value_name = "DIR")]
    pub store: PathBuf,
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
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| ControlError::Journal("no runtime for the journal".to_owned()))?;
        let journal = {
            let _entered = runtime.enter();
            PgJournal::from_dsn(args.journal.expose_secret())
        }
        .map_err(|_| ControlError::Journal("the journal DSN does not parse".to_owned()))?;
        let store = FsArtifactStore::open(&args.store)
            .map_err(|e| ControlError::Journal(format!("the artifact store: {}", e.code())))?;
        Ok(Self {
            journal,
            store,
            runtime,
        })
    }
}

impl ControlJournal for PgControlJournal {
    fn rows(&self, stream: &StreamId) -> Result<Vec<StoredEvent>, ControlError> {
        let rows = self.runtime.block_on(self.journal.rows(stream));
        rows.map_err(|e| journal_error(e.code()))
    }

    fn head(&self, stream: &StreamId) -> Result<Head, ControlError> {
        let head = self.runtime.block_on(self.journal.head(stream));
        head.map_err(|e| journal_error(e.code()))
    }

    fn take_ownership(&mut self, stream: &StreamId) -> Result<u64, ControlError> {
        let epoch = self.runtime.block_on(self.journal.take_ownership(stream));
        epoch.map_err(|e| journal_error(e.code()))
    }

    fn append(
        &mut self,
        stream: &StreamId,
        expected_head: u64,
        writer_epoch: u64,
        recorded_at: UtcNanos,
        drafts: &[&[u8]],
    ) -> Result<AppendOutcome, ControlError> {
        let append = self.journal.append_with_config_artifacts(
            stream,
            expected_head,
            writer_epoch,
            recorded_at,
            drafts,
            &self.store,
        );
        self.runtime
            .block_on(append)
            .map_err(|AppendError::Integrity(e)| journal_error(e.code()))
    }

    fn wait(&mut self, delay: Duration) {
        std::thread::sleep(delay);
    }
}

/// A journal call that returned no answer, by its stable code (ADR-0001 ES-09), never the DSN: a
/// read's or an ownership change's `PgError` code (`unavailable` or `integrity`), or the journal
/// spec §11 check an append's stored bytes failed (DEC-520 item 1).
fn journal_error(code: &str) -> ControlError {
    ControlError::Journal(format!("the journal failed: {code}"))
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
