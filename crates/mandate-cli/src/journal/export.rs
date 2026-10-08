//! `mandate journal export`: one stream of the workspace's Postgres journal written as the segment
//! file of journal spec §6.2 that `mandate journal verify` reads, so a run's journal is verified,
//! with its artifact store, by the same checks as any export (first paper trade brief V0, X-11;
//! DEC-522).

use std::io::Write;
use std::path::PathBuf;

use clap::Args;
use mandate_canon::Digest;
use secrecy::SecretString;

use crate::control::ControlError;

#[derive(Debug, Args)]
pub struct ExportArgs {
    /// The stream to export, named as journal spec §2 names it (`acct:…`, `agent:…`, `ctl:…`).
    pub stream: String,
    /// The segment file to write. An existing file is refused, never overwritten.
    pub out: PathBuf,
    /// The workspace journal's Postgres DSN, without a password (set PGPASSWORD or ~/.pgpass).
    #[arg(long, value_name = "DSN")]
    pub journal: SecretString,
}

/// What an export wrote: every event of the stream, from `seq` 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exported {
    pub stream_id: String,
    pub last_seq: u64,
    pub last_hash: Digest,
}

/// Reads every event of `args.stream` from the journal, each re-checked on read (journal spec §11
/// checks 1 to 5), writes them to `args.out` as one §6.2 segment, and reports the span to `report`.
///
/// # Errors
/// A refusal whose message starts with its code: `export_stream_id_invalid` before the journal is
/// opened, `export_file_exists` before it is read, `export_stream_empty` for a stream with no
/// event, or the journal's own error; nothing is written on any of them, and no message names the
/// DSN.
pub fn export(args: &ExportArgs, report: &mut impl Write) -> anyhow::Result<Exported> {
    let _ = (args, report);
    Err(anyhow::Error::new(ControlError::Unimplemented {
        story: "E10-16",
    }))
}
