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
//!   ([`SegmentManifest::parse`]), and the two directions agree byte for byte. A `seq` above the
//!   canonical integer bound (2^53 − 1) has no canonical bytes and no hash — both refuse
//!   ([`ColdError::SeqUnrepresentable`]), as `mandate_journal::AnchorLeaf::leaf_hash` refuses the
//!   same `seq` — so the digest a `SegmentExported` references is never a shared degenerate one.
//! - **The cold per-range checks** (§11, DEC-264): [`verify_range`] walks a range's segment files
//!   in order against a trusted start — each manifest must match its file (the SHA-256 and the
//!   edges recomputed from the file's own lines: `segment_manifest_mismatch`), each segment must
//!   begin where the previous one ended, the first where the trusted start says (or it must
//!   cover the start: a range may enter mid-segment, and an event before the trusted start is
//!   not checked by that range, §11's own rule: `segment_gap`), and the events inside are walked
//!   by [`mandate_journal::verify_events`], whose per-event failures surface unchanged.
//! - **The timestamp token's checks** (§11, DEC-265): the structural half is
//!   [`tsa_imprint_matches`] — the token artifact must contain the anchor's imprint, SHA-256 of
//!   the 32 raw root bytes (§10), so a token that does not even claim this anchor's root fails
//!   `tsa_token_invalid` — and [`verify_tsa`], the check's entry point, **fails closed**: the
//!   structural half runs first, and a token whose imprint matches is still refused as incomplete
//!   ([`ColdFailure::TsaVerificationIncomplete`]), never `Ok`, because RFC 3161 signature
//!   verification, the certificate chain, and revocation are the Proposed crypto half of DEC-265
//!   item 1, and an `Ok` would overstate what was proven.
//! - **The canonical export** (§12, DEC-265): an [`ExportBundle`]'s parts — segment files,
//!   manifests, anchors, tokens — carry one deterministic [`ExportBundle::verifier_digest`], and
//!   an anchor's inclusion proof for a stream ([`inclusion_proof`]) reproduces the anchor's root
//!   by §10's RFC 6962 walk.
//!
//! The export line's importer ([`import_line`]) is the bridge from a segment file back to a
//! [`mandate_journal::StoredEvent`]: it splits `{"body": …, "hash": …}` without re-hashing, so
//! the per-event checks — not the importer — decide whether the hash is true.

use mandate_canon::{Digest, Int, Key, Object, Value, parse, to_canonical};
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
        let first = rows.first().ok_or(ColdError::EmptySegment)?;
        let last = rows.last().ok_or(ColdError::EmptySegment)?;
        for pair in rows.windows(2) {
            let [a, b] = pair else {
                continue;
            };
            if b.seq.checked_sub(a.seq) != Some(1) {
                return Err(ColdError::DiscontiguousRows);
            }
        }
        if rows.iter().any(|row| row.stream_id != first.stream_id) {
            return Err(ColdError::MixedStreams);
        }
        let stream = StreamId::parse(&first.stream_id).ok_or(ColdError::MixedStreams)?;
        Ok(Self {
            stream,
            first_seq: first.seq,
            last_seq: last.seq,
            first_prev_hash: first.prev_hash,
            last_hash: last.hash,
            file_sha256: Digest::of(&mandate_journal::export_segment(rows)),
        })
    }

    /// The manifest's canonical JSON bytes: the object §6.2 names, canonicalized, so the manifest
    /// hash is a digest of exactly these bytes. A `seq` above the canonical integer bound
    /// (`mandate_canon::MAX_INT`, 2^53 − 1) has no canonical form — the same `seq`
    /// `mandate_journal::AnchorLeaf::leaf_hash` refuses.
    ///
    /// # Errors
    /// Returns [`ColdError::SeqUnrepresentable`] when either `seq` field exceeds the canonical
    /// integer bound, never a degenerate value two manifests could share.
    pub fn to_canonical_bytes(&self) -> Result<CanonicalBytes, ColdError> {
        let first_seq = Int::new(self.first_seq).ok_or(ColdError::SeqUnrepresentable)?;
        let last_seq = Int::new(self.last_seq).ok_or(ColdError::SeqUnrepresentable)?;
        let mut object = Object::new();
        object.insert(
            Key::new("stream").map_err(|_| ColdError::MalformedManifest)?,
            Value::Str(self.stream.as_str().to_owned()),
        );
        object.insert(
            Key::new("first_seq").map_err(|_| ColdError::MalformedManifest)?,
            Value::Int(first_seq),
        );
        object.insert(
            Key::new("last_seq").map_err(|_| ColdError::MalformedManifest)?,
            Value::Int(last_seq),
        );
        object.insert(
            Key::new("first_prev_hash").map_err(|_| ColdError::MalformedManifest)?,
            Value::Str(self.first_prev_hash.to_hex()),
        );
        object.insert(
            Key::new("last_hash").map_err(|_| ColdError::MalformedManifest)?,
            Value::Str(self.last_hash.to_hex()),
        );
        object.insert(
            Key::new("file_sha256").map_err(|_| ColdError::MalformedManifest)?,
            Value::Str(self.file_sha256.to_hex()),
        );
        Ok(CanonicalBytes(to_canonical(&Value::Object(object))))
    }

    /// Reads a manifest back from its canonical bytes.
    ///
    /// # Errors
    /// Returns [`ColdError::MalformedManifest`] unless the bytes are canonical JSON of exactly
    /// §6.2's six fields with a stream id and hex digests that parse.
    pub fn parse(bytes: &[u8]) -> Result<Self, ColdError> {
        let value = parse(bytes).map_err(|_| ColdError::MalformedManifest)?;
        if to_canonical(&value) != bytes {
            return Err(ColdError::MalformedManifest);
        }
        let members = value.as_object().ok_or(ColdError::MalformedManifest)?;
        if members.len() != 6 {
            return Err(ColdError::MalformedManifest);
        }
        let stream_text = members
            .get("stream")
            .and_then(Value::as_str)
            .ok_or(ColdError::MalformedManifest)?;
        let stream = StreamId::parse(stream_text).ok_or(ColdError::MalformedManifest)?;
        let first_seq = members
            .get("first_seq")
            .and_then(Value::as_int)
            .ok_or(ColdError::MalformedManifest)?;
        let last_seq = members
            .get("last_seq")
            .and_then(Value::as_int)
            .ok_or(ColdError::MalformedManifest)?;
        let first_prev_hash = members
            .get("first_prev_hash")
            .and_then(Value::as_str)
            .and_then(Digest::from_hex)
            .ok_or(ColdError::MalformedManifest)?;
        let last_hash = members
            .get("last_hash")
            .and_then(Value::as_str)
            .and_then(Digest::from_hex)
            .ok_or(ColdError::MalformedManifest)?;
        let file_sha256 = members
            .get("file_sha256")
            .and_then(Value::as_str)
            .and_then(Digest::from_hex)
            .ok_or(ColdError::MalformedManifest)?;
        Ok(Self {
            stream,
            first_seq,
            last_seq,
            first_prev_hash,
            last_hash,
            file_sha256,
        })
    }

    /// The hash a `SegmentExported` event references: the digest of the manifest's canonical
    /// bytes, and so refused with them when a `seq` exceeds the canonical integer bound — the
    /// reference is never a shared degenerate digest.
    ///
    /// # Errors
    /// Returns [`ColdError::SeqUnrepresentable`] when either `seq` field exceeds the canonical
    /// integer bound, exactly as [`SegmentManifest::to_canonical_bytes`] does.
    pub fn manifest_hash(&self) -> Result<Digest, ColdError> {
        let bytes = self.to_canonical_bytes()?;
        Ok(Digest::of(bytes.as_slice()))
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
/// expected the segment to be, or the timestamp token's check — its structural half, or the
/// crypto half still being Proposed (DEC-265 item 1).
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
    /// The token's imprint matches, but the crypto half — the RFC 3161 signature, the
    /// certificate chain, revocation — is Proposed in DEC-265 item 1 and not taken, so the
    /// check's entry point refuses rather than vouch for a token: verification is incomplete.
    TsaVerificationIncomplete,
}

/// Imports one export line (§6.2: the canonical form of `{"body": <body>, "hash": "<hex>"}`)
/// back to its event, splitting the body out without re-hashing: whether the hash is true is
/// check 4's question, not the importer's.
///
/// # Errors
/// Returns [`ColdError::MalformedLine`] unless the line is exactly that two-member object with
/// a canonical body and a 64-hex digest.
pub fn import_line(line: &[u8]) -> Result<StoredEvent, ColdError> {
    let value = parse(line).map_err(|_| ColdError::MalformedLine)?;
    if to_canonical(&value) != line {
        return Err(ColdError::MalformedLine);
    }
    let members = value.as_object().ok_or(ColdError::MalformedLine)?;
    if members.len() != 2 {
        return Err(ColdError::MalformedLine);
    }
    let body = members.get("body").ok_or(ColdError::MalformedLine)?;
    let body_bytes = to_canonical(body);
    let hash = members
        .get("hash")
        .and_then(Value::as_str)
        .and_then(Digest::from_hex)
        .ok_or(ColdError::MalformedLine)?;
    let text = |name: &str| {
        body.get(name)
            .and_then(Value::as_str)
            .ok_or(ColdError::MalformedLine)
    };
    let int = |name: &str| {
        body.get(name)
            .and_then(Value::as_int)
            .ok_or(ColdError::MalformedLine)
    };
    Ok(StoredEvent {
        stream_id: text("stream_id")?.to_owned(),
        seq: int("seq")?,
        event_id: text("event_id")?.to_owned(),
        event_type: text("event_type")?.to_owned(),
        schema_version: int("schema_version")?,
        environment: text("environment")?.to_owned(),
        recorded_at: text("recorded_at")?.to_owned(),
        prev_hash: body
            .get("prev_hash")
            .and_then(Value::as_str)
            .and_then(Digest::from_hex)
            .ok_or(ColdError::MalformedLine)?,
        hash,
        body: body_bytes,
    })
}

/// Splits a segment file (§6.2: one export line per event, each followed by a line feed) into
/// its events, importing each line without re-hashing.
fn import_rows(file: &[u8]) -> Result<Vec<StoredEvent>, ColdError> {
    let mut rows = Vec::new();
    let mut rest = file;
    while !rest.is_empty() {
        let end = rest
            .iter()
            .position(|byte| *byte == b'\n')
            .ok_or(ColdError::MalformedLine)?;
        let (line, after) = rest.split_at_checked(end).ok_or(ColdError::MalformedLine)?;
        rows.push(import_line(line)?);
        rest = after.get(1..).unwrap_or(&[]);
    }
    Ok(rows)
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
    let mut expected = start.from_seq;
    let mut prev_hash = start.prev_hash;
    let mut first_segment = true;
    for segment in segments {
        let manifest =
            SegmentManifest::parse(segment.manifest).map_err(|_| ColdFailure::Segment {
                at_seq: expected,
                check: ColdCheck::SegmentManifestMismatch,
            })?;
        let rows = import_rows(segment.file).map_err(|_| ColdFailure::Segment {
            at_seq: manifest.first_seq,
            check: ColdCheck::SegmentManifestMismatch,
        })?;
        let head = rows.first();
        let tail = rows.last();
        let edges_match = match (head, tail) {
            (Some(head), Some(tail)) => {
                manifest.file_sha256 == Digest::of(segment.file)
                    && manifest.first_seq == head.seq
                    && manifest.last_seq == tail.seq
                    && manifest.first_prev_hash == head.prev_hash
                    && manifest.last_hash == tail.hash
                    && manifest.stream.as_str() == head.stream_id
                    && rows.iter().all(|row| row.stream_id == head.stream_id)
            }
            _ => false,
        };
        if !edges_match {
            return Err(ColdFailure::Segment {
                at_seq: manifest.first_seq,
                check: ColdCheck::SegmentManifestMismatch,
            });
        }
        let carries_start = manifest.first_seq <= expected && expected <= manifest.last_seq;
        let chains = if first_segment {
            carries_start
        } else {
            manifest.first_seq == expected
        };
        if !chains {
            return Err(ColdFailure::Segment {
                at_seq: expected,
                check: ColdCheck::SegmentGap,
            });
        }
        let skip = rows.iter().take_while(|row| row.seq < expected).count();
        let walk = rows.get(skip..).unwrap_or(&[]);
        let verified = mandate_journal::verify_events(
            walk,
            TrustedStart {
                from_seq: expected,
                prev_hash,
            },
            artifacts,
        )
        .map_err(ColdFailure::Event)?;
        expected = verified.next_seq;
        prev_hash = verified.last_hash;
        first_segment = false;
    }
    Ok(Verified {
        next_seq: expected,
        last_hash: prev_hash,
    })
}

/// The timestamp token's structural check, under its own name (§11's `tsa_token_invalid`,
/// DEC-265's structural half): the token artifact must contain the anchor's imprint — SHA-256 of
/// the 32 raw root bytes, §10, [`mandate_journal::tsa_imprint`] — so a token that does not even
/// claim this anchor's root fails. Containing the root itself is not containing the imprint.
///
/// # Errors
/// Returns [`ColdFailure::TsaTokenInvalid`] when the token does not contain the imprint.
pub fn tsa_imprint_matches(
    anchor: &mandate_journal::Anchor,
    token: &[u8],
) -> Result<(), ColdFailure> {
    let imprint = mandate_journal::tsa_imprint(&anchor.root);
    let needle = imprint.as_bytes().as_slice();
    if token.windows(needle.len()).any(|window| window == needle) {
        Ok(())
    } else {
        Err(ColdFailure::TsaTokenInvalid)
    }
}

/// The `tsa_token_invalid` check's entry point, and it fails closed: the structural half
/// ([`tsa_imprint_matches`]) runs first, and a token whose imprint matches is refused as
/// incomplete, never `Ok` — the RFC 3161 signature, the certificate chain, and revocation are
/// the Proposed crypto half (DEC-265 item 1), and an `Ok` here would overstate what was proven.
/// When that half lands, the incomplete arm goes and the full verification answers.
///
/// # Errors
/// Returns [`ColdFailure::TsaTokenInvalid`] when the token does not contain the anchor's
/// imprint, and [`ColdFailure::TsaVerificationIncomplete`] when it does: the crypto half is
/// Proposed, so no token is vouched for yet.
pub fn verify_tsa(anchor: &mandate_journal::Anchor, token: &[u8]) -> Result<(), ColdFailure> {
    tsa_imprint_matches(anchor, token)?;
    Err(ColdFailure::TsaVerificationIncomplete)
}

/// The anchor's inclusion proof for one stream (§12): the RFC 6962 audit path — the sibling
/// hashes that, with the stream's leaf, rebuild §10's root by its own split rule.
///
/// # Errors
/// Returns [`ColdError::StreamNotAnchored`] when the anchor holds no leaf for this stream, and
/// [`ColdError::MalformedAnchor`] when the anchor's leaves do not produce its root — a proof
/// that rebuilt nothing would vouch for anything. Both are named refusals, never a silent
/// `None` (DEC-85).
pub fn inclusion_proof(
    anchor: &mandate_journal::Anchor,
    stream_id: &str,
) -> Result<Vec<Digest>, ColdError> {
    let computed =
        mandate_journal::merkle_root(&anchor.leaves).ok_or(ColdError::MalformedAnchor)?;
    if computed != anchor.root {
        return Err(ColdError::MalformedAnchor);
    }
    let index = anchor
        .leaves
        .iter()
        .position(|leaf| leaf.stream_id == stream_id)
        .ok_or(ColdError::StreamNotAnchored)?;
    audit_path(&anchor.leaves, index).ok_or(ColdError::MalformedAnchor)
}

/// The RFC 6962 audit path for `index`: the sibling subtree roots, from the leaf's level upward,
/// by §10's own split rule (the largest power of two below the level's width). Each sibling
/// subtree's root is `mandate_journal::merkle_root` over that subtree's own leaves — the cold
/// crate holds no second copy of the root walk.
fn audit_path(leaves: &[mandate_journal::AnchorLeaf], index: usize) -> Option<Vec<Digest>> {
    if leaves.len() < 2 {
        return Some(Vec::new());
    }
    let mut split = 1usize;
    while let Some(doubled) = split.checked_mul(2) {
        if doubled < leaves.len() {
            split = doubled;
        } else {
            break;
        }
    }
    let (left, right) = leaves.split_at_checked(split)?;
    if index < split {
        let mut path = audit_path(left, index)?;
        path.push(mandate_journal::merkle_root(right)?);
        Some(path)
    } else {
        let offset = index.checked_sub(split)?;
        let mut path = audit_path(right, offset)?;
        path.push(mandate_journal::merkle_root(left)?);
        Some(path)
    }
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
    /// The verifier's digest (§12): one deterministic digest over the bundle's parts — each
    /// segment's manifest bytes then file bytes, each anchor's root bytes, each token's bytes,
    /// in order, every part prefixed by its byte length as a little-endian `u64` (DEC-265
    /// item 3) — so any change to any part changes it, and no two different part lists
    /// concatenate to the same bytes. The digest is computed from the parts or refused; the
    /// signature leaves no room for a made-up answer.
    ///
    /// # Errors
    /// Returns [`ColdError::PartUnrepresentable`] when a part's byte length cannot be written
    /// as the `u64` length prefix — unreachable on a 64-bit target, where
    /// `u64::try_from(usize)` cannot fail; the arm is the refusal the fallible signature
    /// exists for, so the digest is computed from the parts or refused, never invented.
    pub fn verifier_digest(&self) -> Result<Digest, ColdError> {
        let mut parts: Vec<Vec<u8>> = Vec::new();
        for segment in self.segments {
            parts.push(segment.manifest.to_vec());
            parts.push(segment.file.to_vec());
        }
        for anchor in self.anchors {
            parts.push(anchor.root.as_bytes().to_vec());
        }
        for token in self.tokens {
            parts.push(token.to_vec());
        }
        let mut flattened: Vec<Vec<u8>> = Vec::new();
        for mut part in parts {
            let length = u64::try_from(part.len()).map_err(|_| ColdError::PartUnrepresentable)?;
            let mut prefixed = length.to_le_bytes().to_vec();
            prefixed.append(&mut part);
            flattened.push(prefixed);
        }
        let slices = flattened.iter().map(Vec::as_slice).collect::<Vec<&[u8]>>();
        Ok(Digest::of_parts(&slices))
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
    /// The anchor's leaves do not produce its root, so no proof over them rebuilds the root.
    #[error("the anchor's leaves do not produce its root")]
    MalformedAnchor,
    /// A `seq` above the canonical integer bound (2^53 − 1) has no canonical bytes and no digest.
    #[error("a seq above the canonical integer bound has no canonical bytes")]
    SeqUnrepresentable,
    /// A part of the export whose byte length cannot be written as the `u64` length prefix.
    /// Unreachable on a 64-bit target, where `u64::try_from(usize)` cannot fail; the arm
    /// exists so the fallible `verifier_digest` has a refusal to return rather than a
    /// made-up digest.
    #[error("a part's byte length cannot be a u64 length prefix")]
    PartUnrepresentable,
    /// The anchor holds no leaf for the stream a proof was asked for.
    #[error("the anchor holds no leaf for this stream")]
    StreamNotAnchored,
}

#[cfg(test)]
mod tests {
    use super::{CanonicalBytes, ColdCheck};

    /// The newtype reads its bytes back exactly: the accessor's contract, pinned live for the
    /// mutation gate while the crate's real tests are pending (DEC-253 item 2), and permanent
    /// once they land.
    #[test]
    fn canonical_bytes_read_back_exactly() {
        let bytes = CanonicalBytes(vec![7, 9]);
        assert_eq!(bytes.as_slice().to_vec(), vec![7, 9]);
        let empty = CanonicalBytes(Vec::new());
        assert!(empty.as_slice().is_empty());
    }

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
