//! `mandate journal verify`: the ordered verification checks of journal spec §11 over an exported
//! segment file (§6.2), the artifact store its events reference (§6.3), and, when one is given, an
//! anchor (§10), so an auditor checks a journal without writing code (backlog E5-4).
//!
//! The checks themselves are `mandate_journal::verify_events` and `verify_anchor`; this module
//! reads the export back into the stored rows they take, and prints the first failure with the
//! spec's check code. An export carries only each event's body and hash, so the columns check 2
//! compares are read from the body itself (see [`row`]). The cold-store half, `verify-cold` over a
//! directory of segments and manifests, is [`cold`] (E5-8).

use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};
use clap::{Args, Subcommand};
use mandate_artifacts_fs::FsArtifactStore;
use mandate_canon::{Digest, Value, parse};
use mandate_journal::{
    Anchor, AnchorLeaf, ArtifactError, ArtifactRef, ArtifactSource, ControlVerifyError, EventCheck,
    EventFailure, HeldAnchor, RangeCheck, StoredEvent, StreamId, StreamType, TrustedStart,
    verify_agent_stream_anchored, verify_anchor, verify_anchor_self, verify_break_glass_causes,
    verify_events,
};

pub mod cold;
pub mod export;

#[derive(Debug, Subcommand)]
pub enum JournalCommand {
    /// Verify an exported journal segment against its hash chain, the artifact store its events
    /// reference, and an anchor when one is given. Exits with an error on any failure.
    Verify(VerifyArgs),
    /// Verify a cold-store export: a directory of segment files and their manifests, walked in
    /// order from a trusted start, then an anchor and its timestamp token when they are given.
    /// Exits with an error on any failure, and on a token until its signature can be checked.
    VerifyCold(cold::VerifyColdArgs),
    /// Write one stream of the workspace's Postgres journal as a segment file `verify` reads.
    /// Refuses an existing file and a stream with no event.
    Export(export::ExportArgs),
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
    /// A range check of the stream's type failed, at an event (spec §11, DEC-782).
    Stream(StreamFailure),
}

/// A §11 range check of a stream type that failed at the event `seq` (DEC-782 item 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamFailure {
    /// The event the check reports at (journal spec rule 111).
    pub seq: u64,
    /// The §11 code of the check that failed.
    pub code: &'static str,
}

/// §11's range checks of `stream`'s type over `rows`, entered at `start`: the failure at the lowest
/// `seq`, ties going to §11's listing order (DEC-782 items 1 and 2). Other types run none. The CLI
/// cannot read the chain before the range, so the agent checks get no hold anchor and a tail range
/// fails closed at its first version-2 copy (DEC-782 item 4). A check the library has not built is
/// an error, never a pass (DEC-77).
pub(crate) fn stream_checks(
    stream: &StreamId,
    rows: &[StoredEvent],
    start: TrustedStart,
) -> anyhow::Result<Option<StreamFailure>> {
    match stream.stream_type() {
        StreamType::Control => control_checks(rows, start),
        StreamType::Agent => Ok(agent_checks(rows, start)),
        _ => Ok(None),
    }
}

/// The agent stream's `intent_action_mismatch`, `mode_event_mismatch` and `held_mismatch`, which
/// the library reports at the lowest `seq` in §11's order, with no hold anchor (DEC-782 item 4).
fn agent_checks(rows: &[StoredEvent], start: TrustedStart) -> Option<StreamFailure> {
    verify_agent_stream_anchored(rows, start, HeldAnchor::Unknown)
        .err()
        .map(|failure| StreamFailure {
            seq: failure.seq,
            code: failure.check.code(),
        })
}

/// The control stream's `anchor_self_mismatch` and `break_glass_cause_mismatch`: the failure at
/// the lowest `seq`, a tie going to `anchor_self_mismatch`, which §11 lists first.
fn control_checks(
    rows: &[StoredEvent],
    start: TrustedStart,
) -> anyhow::Result<Option<StreamFailure>> {
    let mut failures = Vec::new();
    for answer in [
        verify_anchor_self(rows),
        verify_break_glass_causes(rows, start),
    ] {
        match answer {
            Ok(()) => {}
            Err(ControlVerifyError::Mismatch(f)) => failures.push(StreamFailure {
                seq: f.seq,
                code: f.check.code(),
            }),
            Err(ControlVerifyError::Unimplemented { story }) => {
                return Err(anyhow!(
                    "{story}: a control-stream range check of journal spec §11 is not built, so \
                     the range cannot be verified"
                ));
            }
        }
    }
    Ok(failures.into_iter().min_by_key(|f| f.seq))
}

impl Outcome {
    /// The spec §11 check code that failed, or `None` when everything verified.
    pub fn code(&self) -> Option<&'static str> {
        match self {
            Self::Verified(_) => None,
            Self::Event(failure) => Some(failure.check.code()),
            Self::Range(check) => Some(check.code()),
            Self::Stream(failure) => Some(failure.code),
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
            Self::Stream(failure) => write!(f, "failed, seq {}, {}", failure.seq, failure.code),
        }
    }
}

/// Why an input was refused before any check of spec §11 could run. `verify` reports these through
/// `anyhow` with the code first, so an auditor and a script read the same stable word
/// (ADR-0001 ES-09), as the artifact errors of check 6 already do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// `--from-seq` and `--trusted-prev-hash` are unpaired, out of range, or not 64 lowercase hex.
    TrustedStart,
    /// `--store` is not a directory, or could not be opened, so nothing was read from it.
    ArtifactStore,
    /// `--anchor` is not an anchor of journal spec §10.
    Anchor,
    /// The anchor names no leaf for the stream the export holds, so it vouches for nothing here.
    AnchorStream,
    /// The export's lines do not all carry one `stream_id`; a segment is one stream's range (§6.2).
    ExportStreams,
    /// The export's `stream_id` is not one of journal spec §2.
    ExportStreamId,
    /// `journal export` was given a file that exists; an export never replaces one (DEC-522).
    ExportExists,
    /// `journal export` was asked for a stream with no event, whose segment would verify nothing.
    ExportEmpty,
    /// A path the cold command was given, or a file inside its export, could not be read: the
    /// export is not a directory, or an I/O failure (DEC-490 item 4).
    Unreadable,
    /// The cold export holds no segment, a segment file without its manifest, or a manifest
    /// without its file, so no range can be covered (DEC-490 item 3).
    ColdExportIncomplete,
}

impl Refusal {
    /// Stable reason code (ADR-0001 ES-09), the first word of the message.
    pub fn code(self) -> &'static str {
        match self {
            Self::TrustedStart => "trusted_start_invalid",
            Self::ArtifactStore => "artifact_store_unusable",
            Self::Anchor => "anchor_invalid",
            Self::AnchorStream => "anchor_covers_another_stream",
            Self::ExportStreams => "export_mixes_streams",
            Self::ExportStreamId => "export_stream_id_invalid",
            Self::ExportExists => "export_file_exists",
            Self::ExportEmpty => "export_stream_empty",
            Self::Unreadable => "path_unreadable",
            Self::ColdExportIncomplete => "cold_export_incomplete",
        }
    }
}

/// An input refusal as the command reports it: the stable code, then what was wrong.
pub(crate) fn refuse(reason: Refusal, detail: impl fmt::Display) -> anyhow::Error {
    anyhow!("{}: {detail}", reason.code())
}

/// Verifies the export `args` names, writing the report to `report`. The returned outcome carries
/// the exit status: [`Outcome::failed`].
pub fn verify(args: &VerifyArgs, report: &mut impl Write) -> anyhow::Result<Outcome> {
    let start = trusted_start(args.from_seq, args.trusted_prev_hash.as_deref())?;
    let export = fs::read(&args.export)
        .with_context(|| format!("reading the export {}", args.export.display()))?;
    let artifacts = open_source(args.store.as_deref())?;
    let anchor = args.anchor.as_deref().map(read_anchor).transpose()?;
    let outcome = check(&export, start, artifacts.as_ref(), anchor.as_ref())?;
    let lines = [
        format!("export: {}", args.export.display()),
        format!("lines: {}", line_count(&export)),
        format!(
            "trusted start: seq {}, prev_hash {}",
            start.from_seq, start.prev_hash
        ),
        format!("artifact store: {}", shown(args.store.as_deref())),
        format!("anchor: {}", shown(args.anchor.as_deref())),
        format!("result: {outcome}"),
    ];
    writeln!(report, "{}", lines.join("\n")).context("writing the report")?;
    Ok(outcome)
}

/// The path as given, or `none given`, so the report says which inputs the result covers.
pub(crate) fn shown(path: Option<&Path>) -> String {
    path.map_or_else(|| "none given".to_owned(), |p| p.display().to_string())
}

/// Lines the export holds, counted as [`rows`] walks them: every line feed ends one, an empty line
/// counts, and a final line without its feed counts too, so the count and a failure's `seq` agree.
fn line_count(export: &[u8]) -> usize {
    let terminated = export.iter().filter(|b| **b == b'\n').count();
    match export.last() {
        None | Some(b'\n') => terminated,
        Some(_) => terminated.saturating_add(1),
    }
}

/// Runs spec §11 in DEC-782's order: the per-event checks over the whole range first, then the
/// range checks of the stream's type, then the per-range anchor checks. Reading the export is part
/// of check 1, so an unreadable line fails there.
fn check(
    export: &[u8],
    start: TrustedStart,
    artifacts: &dyn ArtifactSource,
    anchor: Option<&Anchor>,
) -> anyhow::Result<Outcome> {
    let rows = match rows(export, start.from_seq) {
        Ok(rows) => rows,
        Err(failure) => return Ok(Outcome::Event(failure)),
    };
    if let Err(failure) = verify_events(&rows, start, artifacts) {
        return Ok(Outcome::Event(failure));
    }
    let (Some(first), Some(last)) = (rows.first(), rows.last()) else {
        return Ok(match anchor {
            Some(_) => Outcome::Range(RangeCheck::AnchorHeadMismatch),
            None => Outcome::Verified(None),
        });
    };
    if let Some(other) = rows.iter().find(|r| r.stream_id != first.stream_id) {
        return Err(refuse(
            Refusal::ExportStreams,
            format_args!(
                "the export mixes streams (`{}` at seq {}, `{}` at seq {}); a segment is one \
                 stream's contiguous seq range (journal spec §6.2)",
                first.stream_id, first.seq, other.stream_id, other.seq
            ),
        ));
    }
    let stream = StreamId::parse(&first.stream_id).ok_or_else(|| {
        refuse(
            Refusal::ExportStreamId,
            format_args!(
                "`{}` is not a stream_id of journal spec §2",
                first.stream_id
            ),
        )
    })?;
    if let Some(failure) = stream_checks(&stream, &rows, start)? {
        return Ok(Outcome::Stream(failure));
    }
    if let Some(anchor) = anchor {
        if !anchor
            .leaves
            .iter()
            .any(|leaf| leaf.stream_id == first.stream_id)
        {
            return Err(refuse(
                Refusal::AnchorStream,
                format_args!(
                    "the anchor names no leaf for `{}`, the stream this export holds, so it \
                     vouches for nothing here (journal spec §10)",
                    first.stream_id
                ),
            ));
        }
        if let Err(check) = verify_anchor(anchor, &stream, &rows) {
            return Ok(Outcome::Range(check));
        }
    }
    Ok(Outcome::Verified(Some(Span {
        stream_id: first.stream_id.clone(),
        first_seq: first.seq,
        last_seq: last.seq,
        last_hash: last.hash,
    })))
}

/// The export's events as stored rows, in file line order, or the first line no canonical body can
/// be read from. Spec §6.2 gives every line a line feed, so a final line without one is truncated
/// and is not read as an event.
fn rows(export: &[u8], from_seq: u64) -> Result<Vec<StoredEvent>, EventFailure> {
    let mut rows = Vec::new();
    let mut rest = export;
    let mut expected_seq = from_seq;
    while !rest.is_empty() {
        let unreadable = EventFailure {
            seq: expected_seq,
            check: EventCheck::NonCanonical,
        };
        let row = rest
            .iter()
            .position(|b| *b == b'\n')
            .and_then(|end| rest.split_at_checked(end))
            .and_then(|(line, after)| Some((row_from_line(line, expected_seq)?, after.get(1..)?)));
        let Some((row, after)) = row else {
            return Err(unreadable);
        };
        rows.push(row);
        rest = after;
        expected_seq = expected_seq.saturating_add(1);
    }
    Ok(rows)
}

/// 64 lowercase hex characters, the form a hash is written in (journal spec §3).
const HEX_DIGEST: usize = 64;

/// The stored row a §6.2 export line describes, or `None` when the line is not one. `"body"` sorts
/// before `"hash"`, so the body is an exact byte slice of the line and is never re-serialized.
fn row_from_line(line: &[u8], expected_seq: u64) -> Option<StoredEvent> {
    let inner = line.strip_prefix(br#"{"body":"#)?.strip_suffix(br#""}"#)?;
    let (rest, hex) = inner
        .len()
        .checked_sub(HEX_DIGEST)
        .and_then(|at| inner.split_at_checked(at))?;
    let hash = Digest::from_hex(str::from_utf8(hex).ok()?)?;
    let body = rest.strip_suffix(br#","hash":""#)?;
    Some(row(body.to_vec(), hash, expected_seq))
}

/// The stored columns of spec §6.1 read from `body`, because an export carries no separate columns
/// and check 2 compares columns with the body. A column the body cannot supply is left at a value
/// the body does not hold (`seq` at the line's expected value, the rest empty or 64 zeros), so a
/// body missing an envelope field fails check 2 instead of being accepted.
fn row(body: Vec<u8>, hash: Digest, expected_seq: u64) -> StoredEvent {
    let parsed = parse(&body).ok();
    let field = |name: &str| parsed.as_ref().and_then(|body| body.get(name));
    let text = |name: &str| {
        field(name)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let int = |name: &str| field(name).and_then(Value::as_int);
    StoredEvent {
        stream_id: text("stream_id"),
        seq: int("seq").unwrap_or(expected_seq),
        event_id: text("event_id"),
        event_type: text("event_type"),
        schema_version: int("schema_version").unwrap_or_default(),
        environment: text("environment"),
        recorded_at: text("recorded_at"),
        prev_hash: Digest::from_hex(&text("prev_hash")).unwrap_or(Digest::ZERO),
        hash,
        body,
    }
}

/// Where verification starts (spec §11): the genesis start by default, or the `seq` and hash the
/// auditor takes from a manifest or anchor. The two are given together or not at all.
pub(crate) fn trusted_start(
    from_seq: Option<u64>,
    trusted_prev_hash: Option<&str>,
) -> anyhow::Result<TrustedStart> {
    match (from_seq, trusted_prev_hash) {
        (None, None) => Ok(TrustedStart::GENESIS),
        (Some(from_seq), Some(hex)) if from_seq >= 1 => Ok(TrustedStart {
            from_seq,
            prev_hash: Digest::from_hex(hex).ok_or_else(|| {
                refuse(
                    Refusal::TrustedStart,
                    format_args!("--trusted-prev-hash is 64 lowercase hex characters, not `{hex}`"),
                )
            })?,
        }),
        (Some(_), Some(_)) => Err(refuse(
            Refusal::TrustedStart,
            "--from-seq is 1 or more (journal spec §3)",
        )),
        _ => Err(refuse(
            Refusal::TrustedStart,
            "--from-seq and --trusted-prev-hash come from one manifest or anchor, together",
        )),
    }
}

/// No artifact store was given, so every reference is missing: an export naming an artifact never
/// verifies without the store it belongs to.
struct NoArtifacts;

impl ArtifactSource for NoArtifacts {
    fn read_artifact(&self, _: &ArtifactRef) -> Result<Vec<u8>, ArtifactError> {
        Err(ArtifactError::Missing)
    }
}

/// The store references are checked against. A `--store` that is not a directory is refused rather
/// than created, so a mistyped path cannot report a verified export it never read.
pub(crate) fn open_source(dir: Option<&Path>) -> anyhow::Result<Box<dyn ArtifactSource>> {
    match dir {
        None => Ok(Box::new(NoArtifacts)),
        Some(dir) if dir.is_dir() => FsArtifactStore::open(dir)
            .map(|store| -> Box<dyn ArtifactSource> { Box::new(store) })
            .map_err(|e| {
                refuse(
                    Refusal::ArtifactStore,
                    format_args!("{}, opening the artifact store {}", e.code(), dir.display()),
                )
            }),
        Some(dir) => Err(refuse(
            Refusal::ArtifactStore,
            format_args!("the artifact store {} is not a directory", dir.display()),
        )),
    }
}

/// The anchor in `path`: the leaves and root an `AnchorComputed` event records (spec §10, DEC-115).
fn read_anchor(path: &Path) -> anyhow::Result<Anchor> {
    let bytes = fs::read(path).with_context(|| format!("reading the anchor {}", path.display()))?;
    parse_anchor(path, &bytes)
}

/// The anchor `bytes` hold, read from `path`, which a refusal names (spec §10, DEC-115 item 6).
pub(crate) fn parse_anchor(path: &Path, bytes: &[u8]) -> anyhow::Result<Anchor> {
    let value = parse(bytes).map_err(|e| {
        refuse(
            Refusal::Anchor,
            format_args!(
                "the anchor {} is not JSON of journal spec §4 ({})",
                path.display(),
                e.kind.code()
            ),
        )
    })?;
    let missing = |what: &str| {
        refuse(
            Refusal::Anchor,
            format_args!(
                "the anchor {} has no {what} (journal spec §10)",
                path.display()
            ),
        )
    };
    let digest = |at: Option<&Value>, what: &str| {
        at.and_then(Value::as_str)
            .and_then(Digest::from_hex)
            .ok_or_else(|| missing(what))
    };
    let leaves = value
        .get("leaves")
        .and_then(Value::as_array)
        .ok_or_else(|| missing("`leaves` array"))?
        .iter()
        .map(|leaf| {
            Ok(AnchorLeaf {
                stream_id: leaf
                    .get("stream_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| missing("leaf `stream_id`"))?
                    .to_owned(),
                seq: leaf
                    .get("seq")
                    .and_then(Value::as_int)
                    .ok_or_else(|| missing("leaf `seq`"))?,
                hash: digest(leaf.get("hash"), "leaf `hash`")?,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(Anchor {
        leaves,
        root: digest(value.get("root"), "`root`")?,
    })
}
