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
//! The journal's cold store as code: segment manifests, the cold per-range checks, and the
//! canonical export of [journal spec §6.2, §11 and §12](../../../docs/specs/journal.md) (E5-6,
//! [task brief](../../../docs/project/tasks/E5-6-journal-cold-store.md), DEC-263 to DEC-265).
//!
//! The hot store is [`mandate_journal`]'s; this crate is the pure half of the cold store — every
//! piece of the slice is deterministic arithmetic over bytes and digests, with no object-storage
//! client, no clock, and no I/O. What it computes:
//!
//! - **The segment manifest** (§6.2, DEC-263): a contiguous run of one stream's events becomes a
//!   canonical-JSON manifest — stream, first and last `seq`, first `prev_hash`, last `hash`, the
//!   file's SHA-256 — whose own hash is what a `SegmentExported` event references. The manifest is
//!   derived from the rows ([`SegmentManifest::of`]) and read back
//!   ([`SegmentManifest::parse`]), and the two directions agree byte for byte.
//! - **The cold per-range checks** (§11, DEC-264): [`verify_range`] walks a range's segment files
//!   in order against a trusted start — each manifest must match its file (the SHA-256 and the
//!   edges recomputed from the file's own lines: `segment_manifest_mismatch`), each segment must
//!   begin where the previous one ended, the first where the trusted start says (or it must
//!   cover the start: a range may enter mid-segment, and an event before the trusted start is
//!   not checked by that range, §11's own rule: `segment_gap`), and the events inside are walked
//!   by [`mandate_journal::verify_events`], whose per-event failures surface unchanged.
//! - **The timestamp token's structural check** (§11, DEC-265): [`verify_tsa`] reads an anchor's
//!   token artifact and fails `tsa_token_invalid` unless the token contains the anchor's imprint
//!   — SHA-256 of the 32 raw root bytes, §10 — so a token that does not even claim this root
//!   fails. Full RFC 3161 signature verification needs a crypto dependency and is Proposed in
//!   DEC-265, not taken here.
//! - **The canonical export** (§12, DEC-265): an [`ExportBundle`]'s parts — segment files,
//!   manifests, anchors, tokens — carry one deterministic [`ExportBundle::verifier_digest`], and
//!   an anchor's inclusion proof for a stream ([`inclusion_proof`]) reproduces the anchor's root
//!   by §10's RFC 6962 walk.
//!
//! The export line's importer ([`import_line`]) is the bridge from a segment file back to a
//! [`mandate_journal::StoredEvent`]: it splits `{"body": …, "hash": …}` without re-hashing, so
//! the per-event checks — not the importer — decide whether the hash is true.

use mandate_canon::Digest;
use mandate_journal::{EventFailure, StoredEvent, StreamId, TrustedStart, Verified};

/// A manifest's canonical JSON bytes: the form §6.2 pins, constructed by
/// [`SegmentManifest::to_canonical_bytes`], read by [`SegmentManifest::parse`], and hashed by
/// [`SegmentManifest::manifest_hash`]. The wrapper carries the canonical-form invariant a bare
/// `Vec<u8>` would lose, the way `mandate-research`'s `ContentHash` carries a content digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalBytes(Vec<u8>);

impl CanonicalBytes {
    /// The bytes as stored, hashed, and parsed.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

/// The manifest of one contiguous run of one stream's events (§6.2): canonical JSON — stream,
/// first and last `seq`, first `prev_hash`, last `hash`, the file's SHA-256 — whose own hash a
/// `SegmentExported` event references (DEC-263).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentManifest {
    /// The stream the segment holds.
    pub stream: StreamId,
    /// The first event's `seq`.
    pub first_seq: u64,
    /// The last event's `seq`.
    pub last_seq: u64,
    /// The first event's `prev_hash`: what the segment chains from.
    pub first_prev_hash: Digest,
    /// The last event's `hash`: what the next segment chains to.
    pub last_hash: Digest,
    /// SHA-256 of the segment file (§6.2's JSON Lines with LF separators and a trailing LF).
    pub file_sha256: Digest,
}

impl SegmentManifest {
    /// Derives the manifest of one contiguous run of one stream's events, in `seq` order.
    ///
    /// # Errors
    /// Returns [`ColdError::EmptySegment`] for no rows, [`ColdError::DiscontiguousRows`] when the
    /// `seq`s are not one ascending run of step 1, and [`ColdError::MixedStreams`] when the rows
    /// name more than one stream.
    pub fn of(rows: &[StoredEvent]) -> Result<Self, ColdError> {
        let _ = rows;
        Err(ColdError::Unimplemented("SegmentManifest::of", "E5-6"))
    }

    /// The manifest's canonical JSON bytes: the object §6.2 names, canonicalized, so the manifest
    /// hash is a digest of exactly these bytes.
    #[must_use]
    pub fn to_canonical_bytes(&self) -> CanonicalBytes {
        CanonicalBytes(Vec::new())
    }

    /// Reads a manifest back from its canonical bytes.
    ///
    /// # Errors
    /// Returns [`ColdError::MalformedManifest`] unless the bytes are canonical JSON of exactly
    /// §6.2's six fields with a stream id and hex digests that parse.
    pub fn parse(bytes: &[u8]) -> Result<Self, ColdError> {
        let _ = bytes;
        Err(ColdError::Unimplemented("SegmentManifest::parse", "E5-6"))
    }

    /// The hash a `SegmentExported` event references: the digest of the manifest's canonical
    /// bytes.
    #[must_use]
    pub fn manifest_hash(&self) -> Digest {
        Digest::ZERO
    }
}

/// One segment of a cold-store range: its manifest's bytes and its file's bytes, as object
/// storage holds them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentFile<'a> {
    /// The manifest's canonical bytes.
    pub manifest: &'a [u8],
    /// The segment file: one export line per event, each followed by a line feed.
    pub file: &'a [u8],
}

/// The cold per-range checks of §11, in the spec's words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColdCheck {
    /// A manifest does not match its file, or its bytes are not a manifest at all.
    SegmentManifestMismatch,
    /// The segments do not chain: one begins before or after where the previous ended (or the
    /// trusted start sits outside the first segment).
    SegmentGap,
    /// A timestamp token does not contain its anchor's imprint (the structural scope; DEC-265).
    TsaTokenInvalid,
}

impl ColdCheck {
    /// The check's code as the spec writes it.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::SegmentManifestMismatch => "segment_manifest_mismatch",
            Self::SegmentGap => "segment_gap",
            Self::TsaTokenInvalid => "tsa_token_invalid",
        }
    }
}

/// Why a cold verification failed: a per-event check inside a segment, unchanged from
/// [`mandate_journal::verify_events`], a cold check reported at the `seq` where the range
/// expected the segment to be, or the timestamp token's structural check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColdFailure {
    /// A per-event check failed inside a segment at this `seq`.
    Event(EventFailure),
    /// A cold check failed where the range expected this `seq`.
    Segment {
        /// The `seq` the range expected when the check failed.
        at_seq: u64,
        /// The check that failed.
        check: ColdCheck,
    },
    /// The timestamp token failed §11's `tsa_token_invalid` check (DEC-265's structural scope).
    TsaTokenInvalid,
    /// The pending E5-6 stub's own report; the arm goes when the story lands.
    Unimplemented(&'static str, &'static str),
}

/// Imports one export line (§6.2: the canonical form of `{"body": <body>, "hash": "<hex>"}`)
/// back to its event, splitting the body out without re-hashing: whether the hash is true is
/// check 4's question, not the importer's.
///
/// # Errors
/// Returns [`ColdError::MalformedLine`] unless the line is exactly that two-member object with
/// a canonical body and a 64-hex digest.
pub fn import_line(line: &[u8]) -> Result<StoredEvent, ColdError> {
    let _ = line;
    Err(ColdError::Unimplemented("import_line", "E5-6"))
}

/// Walks a range's segment files in order against a trusted start (§11): each manifest must
/// match its file — the SHA-256 and the edges recomputed from the file's own lines — each
/// segment must begin where the previous ended, and the first must carry the trusted start,
/// either beginning exactly there or covering it (a range may enter mid-segment, and an event
/// before the trusted start is not checked by that range). The events inside are walked by
/// [`mandate_journal::verify_events`], whose failures surface as [`ColdFailure::Event`].
///
/// # Errors
/// Returns the first [`ColdFailure`] in range order.
pub fn verify_range(
    start: TrustedStart,
    segments: &[SegmentFile<'_>],
    artifacts: &dyn mandate_journal::ArtifactSource,
) -> Result<Verified, ColdFailure> {
    let _ = (start, segments, artifacts);
    Err(ColdFailure::Unimplemented("verify_range", "E5-6"))
}

/// The timestamp token's structural check (§11's `tsa_token_invalid`, DEC-265's scope): the
/// token must contain the anchor's imprint — SHA-256 of the 32 raw root bytes, §10 — so a token
/// that does not even claim this anchor's root fails. Signature verification, the certificate
/// chain, and revocation are the Proposed crypto half.
///
/// # Errors
/// Returns [`ColdFailure::TsaTokenInvalid`] when the token does not contain the imprint.
pub fn verify_tsa(anchor: &mandate_journal::Anchor, token: &[u8]) -> Result<(), ColdFailure> {
    let _ = (anchor, token);
    Err(ColdFailure::Unimplemented("verify_tsa", "E5-6"))
}

/// The anchor's inclusion proof for one stream (§12): the RFC 6962 audit path — the sibling
/// hashes that, with the stream's leaf, rebuild §10's root by its own split rule.
///
/// # Errors
/// Returns [`ColdError::StreamNotAnchored`] when the anchor holds no leaf for this stream: a
/// named refusal, never a silent `None` (DEC-85).
pub fn inclusion_proof(
    anchor: &mandate_journal::Anchor,
    stream_id: &str,
) -> Result<Vec<Digest>, ColdError> {
    let _ = (anchor, stream_id);
    Err(ColdError::Unimplemented("inclusion_proof", "E5-6"))
}

/// A canonical export's parts (§12): segment files with their manifests, the anchors, and the
/// timestamp tokens, from which one deterministic digest is computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportBundle<'a> {
    /// The range's segments, in `seq` order.
    pub segments: &'a [SegmentFile<'a>],
    /// The anchors the export covers, in leaf order.
    pub anchors: &'a [mandate_journal::Anchor],
    /// The timestamp tokens, one per anchor, in the same order.
    pub tokens: &'a [&'a [u8]],
}

impl ExportBundle<'_> {
    /// The verifier's digest (§12): one deterministic digest over the bundle's parts, so any
    /// change to any part changes it.
    ///
    /// # Errors
    /// None; the digest is over bytes the bundle already holds.
    #[must_use]
    pub fn verifier_digest(&self) -> Digest {
        Digest::ZERO
    }
}

/// What the cold store refuses: malformed inputs, never silent guesses (DEC-85).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ColdError {
    /// A segment holds no events.
    #[error("a segment holds no events")]
    EmptySegment,
    /// The rows' `seq`s are not one ascending run of step 1.
    #[error("a segment's rows are not one ascending run of step 1")]
    DiscontiguousRows,
    /// The rows name more than one stream.
    #[error("a segment's rows name more than one stream")]
    MixedStreams,
    /// The bytes are not a canonical manifest of §6.2's six fields.
    #[error("the bytes are not a canonical segment manifest")]
    MalformedManifest,
    /// The bytes are not `{"body": <canonical>, "hash": "<64 hex>"}`.
    #[error("the bytes are not an export line")]
    MalformedLine,
    /// The anchor holds no leaf for the stream a proof was asked for.
    #[error("the anchor holds no leaf for this stream")]
    StreamNotAnchored,
    /// The pending E5-6 stub: every entry point returns this until the implementation lands.
    #[error("{0} is not implemented yet (pending {1})")]
    Unimplemented(&'static str, &'static str),
}

#[cfg(test)]
mod tests {
    use super::ColdCheck;

    /// The spec's three cold check codes, pinned live because the pending E5-6 tests do not run
    /// under the mutation gate (DEC-253 item 2), so a code arm no live test reads would be a
    /// mutant nothing catches.
    #[test]
    fn the_cold_check_codes_are_pinned() {
        assert_eq!(
            ColdCheck::SegmentManifestMismatch.code(),
            "segment_manifest_mismatch"
        );
        assert_eq!(ColdCheck::SegmentGap.code(), "segment_gap");
        assert_eq!(ColdCheck::TsaTokenInvalid.code(), "tsa_token_invalid");
    }
}
