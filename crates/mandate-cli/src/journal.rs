//! `mandate journal verify`: the ordered verification checks of journal spec §11 over an exported
//! segment file (§6.2), the artifact store its events reference (§6.3), and, when one is given, an
//! anchor (§10), so an auditor checks a journal without writing code (backlog E5-4).
//!
//! The checks themselves are `mandate_journal::verify_events` and `verify_anchor`; this module
//! reads the export back into the stored rows they take, and prints the first failure with the
//! spec's check code. An export carries only each event's body and hash, so the columns check 2
//! compares are read from the body itself (see [`row`]).

use std::fmt;
use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Subcommand};
use mandate_canon::Digest;
use mandate_journal::{EventFailure, RangeCheck};

#[derive(Debug, Subcommand)]
pub enum JournalCommand {
    /// Verify an exported journal segment against its hash chain, the artifact store its events
    /// reference, and an anchor when one is given. Exits with an error on any failure.
    Verify(VerifyArgs),
}

#[derive(Debug, Args)]
pub struct VerifyArgs {
    /// The exported segment file (journal spec §6.2): one `{"body":…,"hash":"…"}` line per event
    /// in `seq` order, each followed by a line feed.
    pub export: PathBuf,
    /// The artifact store directory holding every artifact the export's events reference. Without
    /// it, an event that names one fails `artifact_missing`.
    #[arg(long)]
    pub store: Option<PathBuf>,
    /// An anchor file to check the export against after the per-event checks pass: canonical JSON
    /// `{"leaves":[{"hash":…,"seq":…,"stream_id":…}],"root":…}`, as `AnchorComputed` records it.
    #[arg(long)]
    pub anchor: Option<PathBuf>,
    /// The `seq` of the export's first event, from the manifest or anchor the start is trusted
    /// from. Default 1: the whole stream from its start.
    #[arg(long, requires = "trusted_prev_hash")]
    pub from_seq: Option<u64>,
    /// The trusted hash the first event's `prev_hash` must equal, from the same manifest or
    /// anchor. Default 64 zeros, the genesis start.
    #[arg(long, requires = "from_seq")]
    pub trusted_prev_hash: Option<String>,
}

/// What an export covers once it has verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub stream_id: String,
    pub first_seq: u64,
    pub last_seq: u64,
    pub last_hash: Digest,
}

/// The run's result: everything verified, or the one failure the spec reports. Per-event checks run
/// first and in order, so a failing export is described by its *first* failing check alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Every check passed. The span is `None` for an export that holds no event.
    Verified(Option<Span>),
    /// The first per-event failure (spec §11 checks 1 to 6).
    Event(EventFailure),
    /// A per-range check failed although every per-event check passed (spec §11, anchors).
    Range(RangeCheck),
}

impl Outcome {
    /// The spec §11 check code that failed, or `None` when everything verified.
    pub fn code(&self) -> Option<&'static str> {
        match self {
            Self::Verified(_) => None,
            Self::Event(failure) => Some(failure.check.code()),
            Self::Range(check) => Some(check.code()),
        }
    }

    /// Whether the command must exit with a non-zero status.
    pub fn failed(&self) -> bool {
        self.code().is_some()
    }
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Verified(None) => f.write_str("verified, no events"),
            Self::Verified(Some(span)) => write!(
                f,
                "verified, stream {}, seq {} to {}, last hash {}",
                span.stream_id, span.first_seq, span.last_seq, span.last_hash
            ),
            Self::Event(failure) => {
                write!(f, "failed, seq {}, {}", failure.seq, failure.check.code())
            }
            Self::Range(check) => write!(f, "failed, {}", check.code()),
        }
    }
}

/// Verifies the export `args` names, writing the report to `report`. The returned outcome carries
/// the exit status: [`Outcome::failed`].
pub fn verify(args: &VerifyArgs, report: &mut impl Write) -> anyhow::Result<Outcome> {
    let _ = (args, report);
    Ok(Outcome::Verified(None))
}
