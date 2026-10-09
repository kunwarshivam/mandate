//! `mandate journal verify-cold`: journal spec §11's per-range checks over a cold-store export —
//! a directory holding each segment's file (§6.2's JSON Lines) beside its manifest — against a
//! trusted start and, when they are given, an artifact store (§6.3), an anchor (§10) and the
//! anchor's timestamp token, through `mandate-journal-cold`'s own checks (backlog E5-8, DEC-490;
//! E5-4's `verify` is the hot-store half over one segment file).
//!
//! The command owes the auditor one thing above all: it never vouches for an export it cannot
//! cover. A path it cannot read, a segment without its manifest, a manifest without its file, or
//! no segment at all is refused with a stable code before anything is reported; the segment
//! checks fail closed in DEC-264's order and surface as the cold crate's own failure; an anchor
//! whose head the verified range does not hold fails `anchor_head_mismatch`; and a timestamp
//! token is never reported verified until DEC-265 item 1's crypto half lands — a token holding
//! the anchor's imprint is [`ColdFailure::TsaVerificationIncomplete`], a non-zero exit.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::Args;
use mandate_journal::{
    Anchor, ArtifactSource, RangeCheck, StoredEvent, StreamId, TrustedStart, verify_anchor,
};
use mandate_journal_cold::{
    ColdCheck, ColdFailure, SegmentFile, SegmentManifest, import_line, verify_range, verify_tsa,
};

use crate::journal::{
    Refusal, Span, StreamFailure, open_source, parse_anchor, refuse, shown, stream_checks,
    trusted_start,
};

#[derive(Debug, Args)]
pub struct VerifyColdArgs {
    /// The cold export directory (DEC-490): each segment is `<name>.jsonl`, one §6.2 export line
    /// per event in `seq` order, beside its manifest `<name>.manifest.json`, the canonical bytes
    /// of DEC-263's six fields. Segments are walked in the order of their manifests' `first_seq`,
    /// whatever their names; a manifest that is not one is walked last.
    pub export: PathBuf,
    /// The artifact store directory holding every artifact the export's events reference. Without
    /// it, an event that names one fails `artifact_missing`.
    #[arg(long)]
    pub store: Option<PathBuf>,
    /// An anchor file to check the verified range against once every segment check has passed:
    /// canonical JSON `{"leaves":[{"hash":…,"seq":…,"stream_id":…}],"root":…}`, as
    /// `AnchorComputed` records it. Its head must be an event the range verified.
    #[arg(long)]
    pub anchor: Option<PathBuf>,
    /// The anchor's RFC 3161 timestamp token, as stored. It must contain the anchor's imprint
    /// (`tsa_token_invalid` otherwise), and it is never reported verified until DEC-265 item 1's
    /// crypto half lands: the result is `tsa_verification_incomplete`, a non-zero exit. A token
    /// needs `--anchor`; [`verify`] refuses one without it (`anchor_invalid`), whoever calls it.
    #[arg(long, requires = "anchor")]
    pub token: Option<PathBuf>,
    /// The `seq` the range starts at, from the manifest or anchor the start is trusted from.
    /// Default 1: the whole stream from its start. The first segment may begin before it.
    #[arg(long, requires = "trusted_prev_hash")]
    pub from_seq: Option<u64>,
    /// The trusted hash the event at `--from-seq` chains from, from the same manifest or anchor.
    /// Default 64 zeros, the genesis start.
    #[arg(long, requires = "from_seq")]
    pub trusted_prev_hash: Option<String>,
}

/// The result word a run with a token ends on until DEC-265 item 1's crypto half lands: the
/// token holds the anchor's imprint, and nothing more was proven (DEC-490 item 6).
pub const TSA_VERIFICATION_INCOMPLETE: &str = "tsa_verification_incomplete";

/// The run's result: everything verified, or the one failure reported first. The segment checks
/// and the per-event checks run in range order first (DEC-264), then the range checks of the
/// stream's type (DEC-782), then the anchor's two checks, then the token's, so a failing export is described by its first failing check alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColdOutcome {
    /// Every check passed over the span the trusted start and the segments cover. Never the
    /// answer when a token was given (DEC-265 item 1).
    Verified(Span),
    /// The cold crate's own failure, unchanged: a per-event check inside a segment, a segment
    /// check at the `seq` the range expected, or the token's two answers.
    Cold(ColdFailure),
    /// An anchor check failed although every segment and per-event check passed.
    Range(RangeCheck),
    /// A range check of the stream's type failed, at an event (spec §11, DEC-782).
    Stream(StreamFailure),
}

impl ColdOutcome {
    /// The check code that failed — spec §11's word, or [`TSA_VERIFICATION_INCOMPLETE`] — or
    /// `None` when everything verified.
    #[must_use]
    pub fn code(&self) -> Option<&'static str> {
        match self {
            Self::Verified(_) => None,
            Self::Cold(ColdFailure::Event(failure)) => Some(failure.check.code()),
            Self::Cold(ColdFailure::Segment { check, .. }) => Some(check.code()),
            Self::Cold(ColdFailure::TsaTokenInvalid) => Some(ColdCheck::TsaTokenInvalid.code()),
            Self::Cold(ColdFailure::TsaVerificationIncomplete) => Some(TSA_VERIFICATION_INCOMPLETE),
            Self::Range(check) => Some(check.code()),
            Self::Stream(failure) => Some(failure.code),
        }
    }

    /// Whether the command must exit with a non-zero status: anything but a verified span.
    #[must_use]
    pub fn failed(&self) -> bool {
        self.code().is_some()
    }
}

impl fmt::Display for ColdOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Verified(span) => write!(
                f,
                "verified, stream {}, seq {} to {}, last hash {}",
                span.stream_id, span.first_seq, span.last_seq, span.last_hash
            ),
            Self::Cold(ColdFailure::Event(failure)) => {
                write!(f, "failed, seq {}, {}", failure.seq, failure.check.code())
            }
            Self::Cold(ColdFailure::Segment { at_seq, check }) => {
                write!(f, "failed, seq {at_seq}, {}", check.code())
            }
            Self::Cold(ColdFailure::TsaTokenInvalid) => {
                write!(f, "failed, {}", ColdCheck::TsaTokenInvalid.code())
            }
            Self::Cold(ColdFailure::TsaVerificationIncomplete) => {
                write!(f, "failed, {TSA_VERIFICATION_INCOMPLETE}")
            }
            Self::Range(check) => write!(f, "failed, {}", check.code()),
            Self::Stream(failure) => write!(f, "failed, seq {}, {}", failure.seq, failure.code),
        }
    }
}

/// Verifies the cold export `args` names, writing the report to `report`. The returned outcome
/// carries the exit status: [`ColdOutcome::failed`]. An input the command cannot cover is
/// refused through the error with its [`crate::journal::Refusal`] code first, and nothing is
/// reported.
///
/// # Errors
/// Returns the input refusal, code first, when the export cannot be covered.
pub fn verify(args: &VerifyColdArgs, report: &mut impl Write) -> anyhow::Result<ColdOutcome> {
    if args.token.is_some() && args.anchor.is_none() {
        return Err(refuse(
            Refusal::Anchor,
            "--token is checked against an anchor's root, so it needs --anchor: a token with no \
             anchor vouches for nothing (DEC-490 item 6)",
        ));
    }
    let start = trusted_start(args.from_seq, args.trusted_prev_hash.as_deref())?;
    let segments = read_export(&args.export)?;
    let artifacts = open_source(args.store.as_deref())?;
    let anchor = args
        .anchor
        .as_deref()
        .map(|path| parse_anchor(path, &readable(path)?))
        .transpose()?;
    let token = args.token.as_deref().map(readable).transpose()?;
    let outcome = check(
        start,
        &segments,
        artifacts.as_ref(),
        anchor.as_ref(),
        token.as_deref(),
    )?;
    let lines = [
        format!("export: {}", args.export.display()),
        format!("segments: {}", segments.len()),
        format!(
            "trusted start: seq {}, prev_hash {}",
            start.from_seq, start.prev_hash
        ),
        format!("artifact store: {}", shown(args.store.as_deref())),
        format!("anchor: {}", shown(args.anchor.as_deref())),
        format!("token: {}", shown(args.token.as_deref())),
        format!("result: {outcome}"),
    ];
    writeln!(report, "{}", lines.join("\n")).context("writing the report")?;
    Ok(outcome)
}

/// One segment of the export as read from the directory: its name, the manifest's bytes and the
/// file's bytes. The walk order is the manifests' (DEC-490 item 2), so the manifest is parsed
/// once here to sort by, and again by the walk, which reports what it cannot parse.
struct Segment {
    name: String,
    manifest: Vec<u8>,
    file: Vec<u8>,
}

impl Segment {
    /// Where the segment sorts in the walk: by its manifest's `(first_seq, last_seq)`, then its
    /// name; a manifest that does not parse claims no place and sorts after every one that does.
    fn walk_key(&self) -> (bool, u64, u64, &str) {
        match SegmentManifest::parse(&self.manifest) {
            Ok(manifest) => (false, manifest.first_seq, manifest.last_seq, &self.name),
            Err(_) => (true, 0, 0, &self.name),
        }
    }
}

/// The suffix of a segment file's name, after the segment's own name.
const FILE_SUFFIX: &str = ".jsonl";
/// The suffix of a manifest's name, after the segment's own name.
const MANIFEST_SUFFIX: &str = ".manifest.json";

/// The export's segments, paired by name and in walk order, or the refusal: an unreadable path
/// (DEC-490 item 4), or a directory that is not a whole export (item 3).
fn read_export(dir: &Path) -> anyhow::Result<Vec<Segment>> {
    let entries = fs::read_dir(dir).map_err(|e| unreadable(dir, &e))?;
    let mut files: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut manifests: BTreeMap<String, PathBuf> = BTreeMap::new();
    for entry in entries {
        let path = entry.map_err(|e| unreadable(dir, &e))?.path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            return Err(refuse(
                Refusal::Unreadable,
                format_args!(
                    "{} has a name that is not UTF-8, so the command cannot tell whether it is a \
                     segment of the export (DEC-490 item 2)",
                    path.display()
                ),
            ));
        };
        if let Some(name) = file_name.strip_suffix(MANIFEST_SUFFIX) {
            manifests.insert(name.to_owned(), path);
        } else if let Some(name) = file_name.strip_suffix(FILE_SUFFIX) {
            files.insert(name.to_owned(), path);
        }
    }
    let incomplete = |name: &str, what: &str| {
        refuse(
            Refusal::ColdExportIncomplete,
            format_args!(
                "segment `{name}` in {} has no {what}, so the export is not a whole cold store \
                 (journal spec §6.2) and no range in it is vouched for",
                dir.display()
            ),
        )
    };
    if let Some(name) = files.keys().find(|name| !manifests.contains_key(*name)) {
        return Err(incomplete(name, "manifest"));
    }
    if let Some(name) = manifests.keys().find(|name| !files.contains_key(*name)) {
        return Err(incomplete(name, "segment file"));
    }
    if files.is_empty() {
        return Err(refuse(
            Refusal::ColdExportIncomplete,
            format_args!(
                "{} holds no segment (`<name>{FILE_SUFFIX}` beside `<name>{MANIFEST_SUFFIX}`), \
                 so there is no range to verify",
                dir.display()
            ),
        ));
    }
    let mut segments = Vec::new();
    for (name, file_path) in files {
        let manifest_path = manifests
            .get(&name)
            .ok_or_else(|| incomplete(&name, "manifest"))?;
        segments.push(Segment {
            manifest: readable(manifest_path)?,
            file: readable(&file_path)?,
            name,
        });
    }
    segments.sort_by(|a, b| a.walk_key().cmp(&b.walk_key()));
    Ok(segments)
}

/// The bytes at `path`, or the `path_unreadable` refusal naming it (DEC-490 item 4).
fn readable(path: &Path) -> anyhow::Result<Vec<u8>> {
    fs::read(path).map_err(|e| unreadable(path, &e))
}

/// The refusal for a path the command could not read: the code first, then the path and why.
fn unreadable(path: &Path, error: &std::io::Error) -> anyhow::Error {
    refuse(
        Refusal::Unreadable,
        format_args!("{} could not be read ({error})", path.display()),
    )
}

/// Runs the checks in DEC-490 item 5's order: the cold walk over the segments from the trusted
/// start, the export's one stream, the range checks of its type (DEC-782 item 1), the anchor's
/// head and root over the rows the walk verified, then the token, which never verifies.
fn check(
    start: TrustedStart,
    segments: &[Segment],
    artifacts: &dyn ArtifactSource,
    anchor: Option<&Anchor>,
    token: Option<&[u8]>,
) -> anyhow::Result<ColdOutcome> {
    let files = segments
        .iter()
        .map(|segment| SegmentFile {
            manifest: &segment.manifest,
            file: &segment.file,
        })
        .collect::<Vec<_>>();
    let verified = match verify_range(start, &files, artifacts) {
        Ok(verified) => verified,
        Err(failure) => return Ok(ColdOutcome::Cold(failure)),
    };
    let stream = one_stream(segments)?;
    let rows = walked_rows(segments, start.from_seq)?;
    let last_seq = verified
        .next_seq
        .checked_sub(1)
        .filter(|last| *last >= start.from_seq)
        .ok_or_else(|| {
            refuse(
                Refusal::ColdExportIncomplete,
                format_args!(
                    "no event at or after seq {} was walked, so there is no range to vouch for",
                    start.from_seq
                ),
            )
        })?;
    if let Some(failure) = stream_checks(&stream, &rows, start)? {
        return Ok(ColdOutcome::Stream(failure));
    }
    if let Some(anchor) = anchor {
        if !anchor
            .leaves
            .iter()
            .any(|leaf| leaf.stream_id == stream.as_str())
        {
            return Err(refuse(
                Refusal::AnchorStream,
                format_args!(
                    "the anchor names no leaf for `{}`, the stream this export holds, so it \
                     vouches for nothing here (journal spec §10)",
                    stream.as_str()
                ),
            ));
        }
        if let Err(failure) = verify_anchor(anchor, &stream, &rows) {
            return Ok(ColdOutcome::Range(failure));
        }
        if let Some(token) = token
            && let Err(failure) = verify_tsa(anchor, token)
        {
            return Ok(ColdOutcome::Cold(failure));
        }
    }
    Ok(ColdOutcome::Verified(Span {
        stream_id: stream.as_str().to_owned(),
        first_seq: start.from_seq,
        last_seq,
        last_hash: verified.last_hash,
    }))
}

/// The one stream every manifest names (DEC-490 item 3), once the walk has pinned each segment's
/// rows to its own manifest's stream, or the `export_mixes_streams` refusal naming two.
fn one_stream(segments: &[Segment]) -> anyhow::Result<StreamId> {
    let mut first: Option<(StreamId, u64)> = None;
    for segment in segments {
        let manifest = SegmentManifest::parse(&segment.manifest).map_err(|e| {
            refuse(
                Refusal::ColdExportIncomplete,
                format_args!(
                    "segment `{}`'s manifest cannot be read after it verified ({e})",
                    segment.name
                ),
            )
        })?;
        match &first {
            None => first = Some((manifest.stream, manifest.first_seq)),
            Some((stream, first_seq)) if *stream != manifest.stream => {
                return Err(refuse(
                    Refusal::ExportStreams,
                    format_args!(
                        "the export mixes streams (`{}` at seq {first_seq}, `{}` at seq {}); a \
                         cold export is one stream's range (journal spec §6.2)",
                        stream.as_str(),
                        manifest.stream.as_str(),
                        manifest.first_seq
                    ),
                ));
            }
            Some(_) => {}
        }
    }
    first
        .map(|(stream, _)| stream)
        .ok_or_else(|| refuse(Refusal::ColdExportIncomplete, "the export holds no segment"))
}

/// The events the walk verified — every line at or after `from_seq`, read back through
/// `import_line` — for the anchor's checks, which cover verified events only (DEC-490 item 5).
fn walked_rows(segments: &[Segment], from_seq: u64) -> anyhow::Result<Vec<StoredEvent>> {
    let mut rows = Vec::new();
    for segment in segments {
        for line in segment.file.split(|byte| *byte == b'\n') {
            if line.is_empty() {
                continue;
            }
            let row = import_line(line).map_err(|e| {
                refuse(
                    Refusal::ColdExportIncomplete,
                    format_args!(
                        "segment `{}` holds a line that cannot be read after it verified ({e})",
                        segment.name
                    ),
                )
            })?;
            if row.seq >= from_seq {
                rows.push(row);
            }
        }
    }
    Ok(rows)
}

/// In-module tests the independent review of #662 required (finding M1 and minor m1), under
/// DEC-77's allowance for a test a review ruling requires: no suite in `tests/` reaches these two
/// refusals, so without them the mutation gate could not judge either branch.
#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "test setup and assertions: a failure here is the test failing, not a trading path"
)]
mod review_tests {
    use std::ffi::OsStr;
    use std::fs;
    use std::os::unix::ffi::OsStrExt;
    use std::path::PathBuf;

    use super::{VerifyColdArgs, read_export, verify};

    /// A scratch directory of this process, made empty.
    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mandate-cli-cold-review-{label}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("the scratch directory is created");
        dir
    }

    /// The library call refuses a token given without an anchor, code first, before it reads
    /// anything and before any report line: the token would otherwise be read, never checked, and
    /// the run reported `verified` beside it (DEC-490 item 6; the binary's `requires` is not the
    /// only caller).
    #[test]
    fn a_token_without_an_anchor_is_refused_before_anything_is_read() {
        let dir = scratch("token");
        let args = VerifyColdArgs {
            export: dir.join("no-such-export"),
            store: None,
            anchor: None,
            token: Some(dir.join("no-such-token.tsr")),
            from_seq: None,
            trusted_prev_hash: None,
        };
        let mut report = Vec::new();
        let refused = verify(&args, &mut report).expect_err("a token alone is refused");
        assert!(
            refused.to_string().starts_with("anchor_invalid: --token"),
            "the refusal names its code first and the token: {refused}"
        );
        assert!(report.is_empty(), "nothing is reported before a refusal");
        fs::remove_dir_all(&dir).expect("the scratch directory is removed");
    }

    /// A directory entry whose name is not UTF-8 is refused with `path_unreadable` rather than
    /// skipped: the command cannot tell whether it is a segment, so it reads nothing of the export
    /// (DEC-490 item 2).
    #[test]
    fn a_name_that_is_not_utf8_is_refused_not_skipped() {
        let dir = scratch("utf8");
        fs::write(dir.join(OsStr::from_bytes(b"\xff.jsonl")), b"")
            .expect("a file with a non-UTF-8 name is written");
        let refused = match read_export(&dir) {
            Ok(_) => panic!("an entry whose name is not UTF-8 is refused"),
            Err(refused) => refused,
        };
        assert!(
            refused.to_string().starts_with("path_unreadable: "),
            "the refusal names its code first: {refused}"
        );
        fs::remove_dir_all(&dir).expect("the scratch directory is removed");
    }
}
