//! `mandate journal export`: one stream of the workspace's Postgres journal written as the segment
//! file of journal spec §6.2 that `mandate journal verify` reads, so a run's journal is verified,
//! with its artifact store, by the same checks as any export (first paper trade brief V0, X-11;
//! DEC-522).

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};
use clap::Args;
use mandate_canon::Digest;
use mandate_journal::{StreamId, export_segment};
use secrecy::SecretString;

use super::{Refusal, refuse};
use crate::postgres::read_stream;

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
    let stream = StreamId::parse(&args.stream).ok_or_else(|| {
        refuse(
            Refusal::ExportStreamId,
            format_args!("`{}` is not a stream_id of journal spec §2", args.stream),
        )
    })?;
    if args.out.exists() {
        return Err(refuse(
            Refusal::ExportExists,
            format_args!(
                "{} exists, and an export never replaces a file",
                args.out.display()
            ),
        ));
    }
    let rows = read_stream(&args.journal, &stream).map_err(|e| anyhow!("{e}"))?;
    let Some(last) = rows.last() else {
        return Err(refuse(
            Refusal::ExportEmpty,
            format_args!("{} holds no event", stream.as_str()),
        ));
    };
    let temp = temp_path(&args.out);
    let mut file = create_temp(&temp)?;
    let published = file
        .write_all(&export_segment(&rows))
        .and_then(|()| file.sync_all())
        .with_context(|| format!("writing {}", temp.display()))
        .and_then(|()| publish(&temp, &args.out));
    fs::remove_file(&temp).ok();
    published?;
    let exported = Exported {
        stream_id: stream.as_str().to_owned(),
        last_seq: last.seq,
        last_hash: last.hash,
    };
    writeln!(
        report,
        "exported: stream {}, seq 1 to {}, last hash {}, to {}",
        exported.stream_id,
        exported.last_seq,
        exported.last_hash,
        args.out.display()
    )
    .context("writing the report")?;
    Ok(exported)
}

/// `<out>.tmp`, where the segment is written whole before it is published under `<out>`.
fn temp_path(out: &Path) -> PathBuf {
    let mut name = out.as_os_str().to_owned();
    name.push(".tmp");
    PathBuf::from(name)
}

/// A new `temp`. One that exists is refused and left alone, so another export's file is never
/// written over or removed; the caller removes the `temp` this creates once it is published or has
/// failed.
fn create_temp(temp: &Path) -> anyhow::Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp)
        .with_context(|| format!("creating {}", temp.display()))
}

/// Publishes the whole `temp` under `out` by a hard link, which never replaces a file, unlike a
/// rename on Unix, and then flushes the directory. So `out` holds the whole segment or does not
/// exist, and a file that appeared at `out` after the first check makes the link fail with the
/// system's "file exists" error instead of being overwritten (DEC-522 item 3).
fn publish(temp: &Path, out: &Path) -> anyhow::Result<()> {
    fs::hard_link(temp, out)
        .with_context(|| format!("publishing {}, never over an existing file", out.display()))?;
    let out = std::path::absolute(out).with_context(|| format!("locating {}", out.display()))?;
    let dir = out.parent().unwrap_or(&out);
    File::open(dir)
        .and_then(|d| d.sync_all())
        .with_context(|| format!("flushing {}", dir.display()))
}
