//! The cold store's hand-calculated cases: one per manifest rule, per cold check, and per
//! invariant of the range walk (E5-6, DEC-263 to DEC-265).
//!
//! Every segment is built the way the exporter builds it — a contiguous run of one stream's
//! rows, [`SegmentManifest::of`] for the manifest, [`mandate_journal::export_segment`] for the
//! file — and every expected value is recomputed by the test's own oracle: the manifest's six
//! fields from the rows themselves, the file's SHA-256 by its own digest, the canonical bytes
//! from the test's own object, the sibling roots by [`mandate_journal::merkle_root`].
//!
//! Every error-expecting case states its positive first, so a vacuous refusal passes nothing.
//! The token's entry point is pinned the other way round: it refuses even a token whose imprint
//! matches, because the crypto half is Proposed (DEC-265 item 1), and the refusal is named, so
//! a vacuous `Ok` passes nothing either.

mod common;

use std::collections::BTreeMap;

use common::{STREAM, journal_with, manifest_value, parsed, stream};
use mandate_canon::{Digest, Value, to_canonical};
use mandate_journal::{
    Anchor, AnchorLeaf, StoredEvent, TrustedStart, Verified, export_line, export_segment,
    merkle_root, tsa_imprint, verify_events,
};
use mandate_journal_cold::{
    ColdCheck, ColdError, ColdFailure, ExportBundle, SegmentFile, SegmentManifest, import_line,
    inclusion_proof, tsa_imprint_matches, verify_range, verify_tsa,
};

/// Eight rows of one stream, `seq` 1 to 8.
fn rows() -> Vec<StoredEvent> {
    journal_with(7).rows(&stream()).to_vec()
}

/// One segment of a contiguous run: its manifest's bytes and its file's bytes.
fn segment(rows: &[StoredEvent]) -> (Vec<u8>, Vec<u8>) {
    let manifest = SegmentManifest::of(rows).expect("a contiguous run manifests");
    (
        manifest
            .to_canonical_bytes()
            .expect("the run's seqs sit far below the canonical bound")
            .as_slice()
            .to_vec(),
        export_segment(rows),
    )
}

/// An anchor over the stream's head — derived through a segment manifest, §10's own shape — and
/// a token holding its imprint, for the token's two checks.
fn anchored_token() -> (Anchor, Vec<u8>) {
    let all = rows();
    let manifest = SegmentManifest::of(&all).expect("a contiguous run manifests");
    let leaf = AnchorLeaf {
        stream_id: STREAM.to_owned(),
        seq: manifest.last_seq,
        hash: manifest.last_hash,
    };
    let anchor = Anchor::compute(vec![leaf]).expect("one head anchors");
    let imprint = tsa_imprint(&anchor.root);
    let mut token = imprint.as_bytes().to_vec();
    token.extend_from_slice(b"the rest of the DER");
    (anchor, token)
}

/// The manifest object §6.2 names, from the rows' own edges, as the test's oracle.
fn oracle_manifest_bytes(rows: &[StoredEvent]) -> Vec<u8> {
    let first = rows.first().expect("the run holds a first row");
    let last = rows.last().expect("the run holds a last row");
    to_canonical(&manifest_value(
        STREAM,
        first.seq,
        last.seq,
        &first.prev_hash.to_hex(),
        &last.hash.to_hex(),
        &Digest::of(&export_segment(rows)).to_hex(),
    ))
}

/// A range's segments as `SegmentFile`s over the built byte vectors.
fn files<'a>(parts: &'a [(Vec<u8>, Vec<u8>)]) -> Vec<SegmentFile<'a>> {
    parts
        .iter()
        .map(|(manifest, file)| SegmentFile { manifest, file })
        .collect()
}

/// An empty artifact store: the drafts carry no artifact references, so nothing is read.
fn no_artifacts() -> BTreeMap<Digest, Vec<u8>> {
    BTreeMap::new()
}

#[test]
fn a_manifest_of_one_contiguous_run_names_its_edges() {
    let all = rows();
    let manifest = SegmentManifest::of(&all[0..3]).expect("a contiguous run manifests");
    assert_eq!(manifest.stream, stream());
    assert_eq!(manifest.first_seq, 1);
    assert_eq!(manifest.last_seq, 3);
    assert_eq!(
        manifest.first_prev_hash,
        Digest::ZERO,
        "seq 1 chains from 64 zeros"
    );
    assert_eq!(manifest.last_hash, all[2].hash);
    assert_eq!(
        manifest.file_sha256,
        Digest::of(&export_segment(&all[0..3])),
        "the file's SHA-256 is recomputed by the test's own digest"
    );
}

#[test]
fn an_empty_segment_is_refused_after_one_row_manifests() {
    let all = rows();
    let one_row = SegmentManifest::of(&all[0..1]);
    assert!(
        one_row.is_ok(),
        "a one-row segment is a segment: {one_row:?}"
    );
    assert_eq!(
        SegmentManifest::of(&[]),
        Err(ColdError::EmptySegment),
        "no rows is not a segment, and the refusal names it"
    );
}

#[test]
fn rows_not_in_one_ascending_run_are_refused() {
    let all = rows();
    let contiguous = SegmentManifest::of(&all[0..4]);
    assert!(
        contiguous.is_ok(),
        "the contiguous run manifests, so the refusal below is about the gap: {contiguous:?}"
    );
    let gapped: Vec<StoredEvent> = vec![all[0].clone(), all[2].clone()];
    assert_eq!(
        SegmentManifest::of(&gapped),
        Err(ColdError::DiscontiguousRows),
        "a run that skips a seq is refused, and the refusal names it"
    );
}

#[test]
fn rows_of_two_streams_are_refused() {
    let all = rows();
    let one_stream = SegmentManifest::of(&all[0..4]);
    assert!(
        one_stream.is_ok(),
        "the one-stream run manifests, so the refusal below is about the second stream: {one_stream:?}"
    );
    let mut other_stream = all[1].clone();
    other_stream.stream_id = "acct:ws_1:ACCT2".to_owned();
    assert_eq!(
        SegmentManifest::of(&[all[0].clone(), other_stream]),
        Err(ColdError::MixedStreams),
        "a segment holds one stream, and the refusal names the mix"
    );
}

#[test]
fn the_manifest_s_bytes_are_the_six_fields_in_canonical_form() {
    let all = rows();
    let manifest = SegmentManifest::of(&all[0..3]).expect("a contiguous run manifests");
    assert_eq!(
        manifest
            .to_canonical_bytes()
            .expect("the run's seqs sit far below the canonical bound")
            .as_slice(),
        oracle_manifest_bytes(&all[0..3]),
        "the manifest's bytes are exactly the object §6.2 names, canonicalized"
    );
}

#[test]
fn a_manifest_round_trips_through_its_canonical_bytes() {
    let all = rows();
    let manifest = SegmentManifest::of(&all[0..3]).expect("a contiguous run manifests");
    let bytes = manifest
        .to_canonical_bytes()
        .expect("the run's seqs sit far below the canonical bound");
    let read = SegmentManifest::parse(bytes.as_slice()).expect("the manifest's own bytes parse");
    assert_eq!(
        read.stream,
        stream(),
        "the stream reads back as the rows' own"
    );
    assert_eq!(
        read.first_seq, all[0].seq,
        "the first seq is the first row's own"
    );
    assert_eq!(
        read.last_seq, all[2].seq,
        "the last seq is the last row's own"
    );
    assert_eq!(
        read.first_prev_hash, all[0].prev_hash,
        "the first prev_hash is the first row's own"
    );
    assert_eq!(
        read.last_hash, all[2].hash,
        "the last hash is the last row's own"
    );
    assert_eq!(
        read.file_sha256,
        Digest::of(&export_segment(&all[0..3])),
        "the file's SHA-256 is recomputed by the test's own digest"
    );
    assert_eq!(
        read, manifest,
        "the six fields read back are the manifest's own, so the round trip carries them"
    );
    assert_eq!(
        read.to_canonical_bytes()
            .expect("the read-back seqs sit far below the canonical bound"),
        bytes,
        "canonical bytes are idempotent: reading back and re-serializing changes nothing"
    );
    let other = SegmentManifest::of(&all[4..8]).expect("another contiguous run manifests");
    let other_bytes = other
        .to_canonical_bytes()
        .expect("the run's seqs sit far below the canonical bound");
    assert_ne!(
        other_bytes, bytes,
        "different rows give different bytes: the canonical form follows the run, not a constant"
    );
    assert_ne!(
        SegmentManifest::parse(other_bytes.as_slice()),
        SegmentManifest::parse(bytes.as_slice()),
        "different bytes parse to a different manifest: the two directions do not collapse"
    );
}

#[test]
fn a_manifest_s_hash_is_the_digest_of_its_canonical_bytes() {
    let all = rows();
    let manifest = SegmentManifest::of(&all[0..3]).expect("a contiguous run manifests");
    assert_eq!(
        manifest
            .manifest_hash()
            .expect("the run's seqs sit far below the canonical bound"),
        Digest::of(&oracle_manifest_bytes(&all[0..3])),
        "the hash a SegmentExported references is the test's own digest of the canonical bytes its own oracle builds"
    );
    let longer = SegmentManifest::of(&all[0..4]).expect("a contiguous run manifests");
    assert_ne!(
        longer
            .manifest_hash()
            .expect("the run's seqs sit far below the canonical bound"),
        manifest
            .manifest_hash()
            .expect("the run's seqs sit far below the canonical bound"),
        "one more row is a different manifest, and the digest follows the bytes, not a constant"
    );
}

#[test]
fn a_manifest_whose_seq_is_above_the_canonical_bound_is_refused_its_bytes_and_hash() {
    let all = rows();
    let manifest = SegmentManifest::of(&all[0..3]).expect("a contiguous run manifests");
    assert!(
        manifest.to_canonical_bytes().is_ok() && manifest.manifest_hash().is_ok(),
        "the run's own seqs sit far below the bound, so the refusals below are about the bound"
    );
    let mut beyond_last = manifest.clone();
    beyond_last.last_seq = mandate_canon::MAX_INT + 1;
    assert_eq!(
        beyond_last.to_canonical_bytes(),
        Err(ColdError::SeqUnrepresentable),
        "a seq above the canonical integer bound has no canonical bytes: a named refusal, never a degenerate value"
    );
    assert_eq!(
        beyond_last.manifest_hash(),
        Err(ColdError::SeqUnrepresentable),
        "and no hash either: an out-of-range manifest is never hashed at all"
    );
    let mut beyond_first = manifest.clone();
    beyond_first.first_seq = mandate_canon::MAX_INT + 1;
    assert_eq!(
        beyond_first.to_canonical_bytes(),
        Err(ColdError::SeqUnrepresentable),
        "the first seq is bound by the same canonical integer bound"
    );
    let mut different_edge = beyond_last.clone();
    different_edge.last_hash = Digest::of(b"a different edge");
    assert_eq!(
        different_edge.manifest_hash(),
        Err(ColdError::SeqUnrepresentable),
        "the refusal stands whatever the other fields hold, so two out-of-range manifests can never share a digest"
    );
}

#[test]
fn a_manifest_in_the_wrong_member_order_is_refused() {
    let all = rows();
    let manifest = SegmentManifest::of(&all[0..3]).expect("a contiguous run manifests");
    let bytes = manifest
        .to_canonical_bytes()
        .expect("the run's seqs sit far below the canonical bound");
    assert!(
        SegmentManifest::parse(bytes.as_slice()).is_ok(),
        "the manifest's own bytes parse, so the refusal below is about the order"
    );
    let swapped = format!(
        "{{\"file_sha256\":\"{}\",\"last_hash\":\"{}\",\"first_prev_hash\":\"{}\",\"last_seq\":3,\"first_seq\":1,\"stream\":\"{STREAM}\"}}",
        Digest::of(&export_segment(&all[0..3])).to_hex(),
        all[2].hash.to_hex(),
        Digest::ZERO.to_hex(),
    );
    assert_eq!(
        SegmentManifest::parse(swapped.as_bytes()),
        Err(ColdError::MalformedManifest),
        "canonical order is the six keys sorted; a swapped manifest is refused, not re-ordered"
    );
    let mut extra = parsed(bytes.as_slice());
    let Value::Object(members) = &mut extra else {
        panic!("a manifest's bytes are an object")
    };
    members.insert(
        mandate_canon::Key::new("extra").expect("the key parses"),
        Value::Int(mandate_canon::Int::new(7).expect("a fixture int fits")),
    );
    assert_eq!(
        SegmentManifest::parse(&to_canonical(&extra)),
        Err(ColdError::MalformedManifest),
        "a manifest is exactly the six fields: a seventh member is refused, not ignored"
    );
}

#[test]
fn an_edited_manifest_parses_to_a_different_hash() {
    let all = rows();
    let manifest = SegmentManifest::of(&all[0..3]).expect("a contiguous run manifests");
    let edited_value = {
        let mut value = parsed(
            manifest
                .to_canonical_bytes()
                .expect("the run's seqs sit far below the canonical bound")
                .as_slice(),
        );
        let Value::Object(members) = &mut value else {
            panic!("a manifest's bytes are an object")
        };
        members.insert(
            mandate_canon::Key::new("last_hash").expect("the key parses"),
            Value::Str(Digest::of(b"a different edge").to_hex()),
        );
        value
    };
    let edited = SegmentManifest::parse(&to_canonical(&edited_value))
        .expect("a canonical edit of the six fields still parses");
    assert_ne!(
        edited
            .manifest_hash()
            .expect("the edit's seqs sit far below the canonical bound"),
        manifest
            .manifest_hash()
            .expect("the run's seqs sit far below the canonical bound"),
        "the manifest's hash follows its bytes: one edited field is a different manifest"
    );
}

#[test]
fn a_line_imports_to_its_event() {
    let all = rows();
    let line = export_line(&all[2]);
    let imported = import_line(&line).expect("an export line imports");
    assert_eq!(
        imported, all[2],
        "the event comes back whole: body, hash, columns"
    );
}

#[test]
fn a_line_without_its_two_members_is_refused() {
    let all = rows();
    let well_formed = import_line(&export_line(&all[2]));
    assert!(
        well_formed.is_ok(),
        "the well-formed line imports, so the refusals below are about the shape: {well_formed:?}"
    );
    assert_eq!(
        import_line(b"{}"),
        Err(ColdError::MalformedLine),
        "a line is body and hash, not an empty object"
    );
    assert_eq!(
        import_line(
            b"{\"hash\":\"0000000000000000000000000000000000000000000000000000000000000000\"}"
        ),
        Err(ColdError::MalformedLine),
        "a line without its body is not a line"
    );
}

#[test]
fn a_line_in_the_wrong_member_order_is_refused() {
    let all = rows();
    let canonical_line = import_line(&export_line(&all[2]));
    assert!(
        canonical_line.is_ok(),
        "the canonical line imports, so the refusal below is about the order: {canonical_line:?}"
    );
    let hex = all[2].hash.to_hex();
    let swapped = format!(
        "{{\"hash\":\"{hex}\",\"body\":{}}}",
        String::from_utf8_lossy(&all[2].body)
    );
    assert_eq!(
        import_line(swapped.as_bytes()),
        Err(ColdError::MalformedLine),
        "canonical order is body before hash; a swapped line is refused, not re-ordered"
    );
}

#[test]
fn a_wrong_hash_imports_and_the_event_check_catches_it() {
    let all = rows();
    let mut lied = all[2].clone();
    lied.hash = Digest::of(b"not the body");
    let imported =
        import_line(&export_line(&lied)).expect("the importer splits, it does not judge");
    assert_ne!(
        imported.hash,
        Digest::of(&imported.body),
        "the lie is visible in the imported event, for the per-event check to catch"
    );
    assert_eq!(
        verify_events(
            std::slice::from_ref(&imported),
            TrustedStart {
                from_seq: all[2].seq,
                prev_hash: all[2].prev_hash
            },
            &no_artifacts()
        ),
        Err(mandate_journal::EventFailure {
            seq: all[2].seq,
            check: mandate_journal::EventCheck::RehashMismatch
        }),
        "check 4, not the importer, decides whether the hash is true"
    );
}

#[test]
fn a_range_of_two_segments_verifies() {
    let all = rows();
    let first = segment(&all[0..4]);
    let second = segment(&all[4..8]);
    let parts = [first, second];
    assert_eq!(
        verify_range(TrustedStart::GENESIS, &files(&parts), &no_artifacts()),
        Ok(Verified {
            next_seq: 9,
            last_hash: all[7].hash
        })
    );
}

#[test]
fn a_manifest_that_does_not_match_its_file_fails_the_mismatch_check() {
    let all = rows();
    let (manifest, mut file) = segment(&all[0..4]);
    file[0] ^= 1;
    let parts = [(manifest, file)];
    assert_eq!(
        verify_range(TrustedStart::GENESIS, &files(&parts), &no_artifacts()),
        Err(ColdFailure::Segment {
            at_seq: 1,
            check: ColdCheck::SegmentManifestMismatch
        }),
        "a file whose bytes differ from its manifest's SHA-256 is a mismatch at the segment's first seq"
    );
}

#[test]
fn a_manifest_whose_edges_disagree_with_its_file_fails_the_mismatch_check() {
    let all = rows();
    let (_, file) = segment(&all[4..8]);
    let lying = to_canonical(&manifest_value(
        STREAM,
        5,
        8,
        &all[4].prev_hash.to_hex(),
        &Digest::of(b"not the last hash").to_hex(),
        &Digest::of(&file).to_hex(),
    ));
    let parts = [(lying, file)];
    assert_eq!(
        verify_range(TrustedStart::GENESIS, &files(&parts), &no_artifacts()),
        Err(ColdFailure::Segment {
            at_seq: 5,
            check: ColdCheck::SegmentManifestMismatch
        }),
        "the file's SHA-256 matches, but the manifest's last hash is not the file's last line's hash"
    );
}

#[test]
fn a_gap_between_segments_fails_segment_gap_at_the_expected_seq() {
    let all = rows();
    let first = segment(&all[0..4]);
    let second = segment(&all[5..8]);
    let parts = [first, second];
    assert_eq!(
        verify_range(TrustedStart::GENESIS, &files(&parts), &no_artifacts()),
        Err(ColdFailure::Segment {
            at_seq: 5,
            check: ColdCheck::SegmentGap
        }),
        "the second segment begins at 6 where the range expected 5, and the failure names 5"
    );
}

#[test]
fn a_later_segment_that_begins_before_the_expected_seq_fails_segment_gap() {
    let all = rows();
    let first = segment(&all[0..4]);
    let stale = segment(&all);
    let overlapping = segment(&all[2..8]);
    let stale_parts = [first.clone(), stale];
    assert_eq!(
        verify_range(TrustedStart::GENESIS, &files(&stale_parts), &no_artifacts()),
        Err(ColdFailure::Segment {
            at_seq: 5,
            check: ColdCheck::SegmentGap
        }),
        "a second segment that begins at 1 where the range expected 5 is a gap: a stale full copy must not stand beside the verified 1 to 4"
    );
    let overlapping_parts = [first, overlapping];
    assert_eq!(
        verify_range(
            TrustedStart::GENESIS,
            &files(&overlapping_parts),
            &no_artifacts()
        ),
        Err(ColdFailure::Segment {
            at_seq: 5,
            check: ColdCheck::SegmentGap
        }),
        "a second segment that begins at 3 where the range expected 5 is a gap even though it covers 5: two cold copies of 3 and 4 must not both stand"
    );
}

#[test]
fn a_first_segment_that_does_not_carry_the_trusted_start_fails_segment_gap() {
    let all = rows();
    let start = TrustedStart {
        from_seq: 3,
        prev_hash: all[1].hash,
    };
    let only = segment(&all[4..8]);
    let parts = [only];
    assert_eq!(
        verify_range(start, &files(&parts), &no_artifacts()),
        Err(ColdFailure::Segment {
            at_seq: 3,
            check: ColdCheck::SegmentGap
        }),
        "a range starting at 3 cannot begin with a segment that starts at 5"
    );
}

#[test]
fn a_range_may_enter_mid_segment() {
    let all = rows();
    let start = TrustedStart {
        from_seq: 3,
        prev_hash: all[1].hash,
    };
    let whole = segment(&all);
    let parts = [whole];
    assert_eq!(
        verify_range(start, &files(&parts), &no_artifacts()),
        Ok(Verified {
            next_seq: 9,
            last_hash: all[7].hash
        }),
        "a segment may begin before the trusted start: the range enters at 3 and walks 3 to 8"
    );
}

#[test]
fn an_event_before_the_trusted_start_is_not_checked() {
    let all = rows();
    let start = TrustedStart {
        from_seq: 3,
        prev_hash: all[1].hash,
    };
    let mut first = all[0].clone();
    first.hash = all[1].hash;
    let mut second = all[1].clone();
    second.hash = all[0].hash;
    let mut tampered = vec![first, second];
    tampered.extend(all[2..].iter().cloned());
    let manifest = SegmentManifest::of(&tampered).expect("the run is still contiguous");
    let file = export_segment(&tampered);
    let parts = [(
        manifest
            .to_canonical_bytes()
            .expect("the run's seqs sit far below the canonical bound")
            .as_slice()
            .to_vec(),
        file,
    )];
    assert_eq!(
        verify_range(start, &files(&parts), &no_artifacts()),
        Ok(Verified {
            next_seq: 9,
            last_hash: all[7].hash
        }),
        "seq 1 and 2 carry swapped hashes, but the range starts at 3 and §11 does not check what is before its trusted start"
    );
}

#[test]
fn an_unparsable_manifest_fails_the_mismatch_check_at_the_expected_seq() {
    let all = rows();
    let first = segment(&all[0..4]);
    let parts = [first, (b"not a manifest".to_vec(), Vec::new())];
    assert_eq!(
        verify_range(TrustedStart::GENESIS, &files(&parts), &no_artifacts()),
        Err(ColdFailure::Segment {
            at_seq: 5,
            check: ColdCheck::SegmentManifestMismatch
        }),
        "bytes that are not a manifest are a mismatch where the range expected the segment: after 1 to 4, at 5"
    );
}

#[test]
fn a_per_event_failure_inside_a_segment_surfaces_as_the_event_check() {
    let all = rows();
    let mut lied = all[4].clone();
    lied.hash = Digest::of(b"not the body");
    let mut tampered: Vec<StoredEvent> = all[0..4].to_vec();
    tampered.push(lied);
    tampered.extend(all[5..].iter().cloned());
    let manifest = SegmentManifest::of(&tampered).expect("the run is still contiguous");
    let file = export_segment(&tampered);
    let parts = [(
        manifest
            .to_canonical_bytes()
            .expect("the run's seqs sit far below the canonical bound")
            .as_slice()
            .to_vec(),
        file,
    )];
    assert_eq!(
        verify_range(TrustedStart::GENESIS, &files(&parts), &no_artifacts()),
        Err(ColdFailure::Event(mandate_journal::EventFailure {
            seq: all[4].seq,
            check: mandate_journal::EventCheck::RehashMismatch
        })),
        "a rehash lie inside a segment surfaces as §11's own per-event check, unchanged"
    );
}

#[test]
fn an_empty_range_returns_the_trusted_start() {
    let start = TrustedStart {
        from_seq: 5,
        prev_hash: Digest::of(b"a trusted edge"),
    };
    assert_eq!(
        verify_range(start, &[], &no_artifacts()),
        Ok(Verified {
            next_seq: 5,
            last_hash: start.prev_hash
        }),
        "no segments verify nothing, and the range's state is the trusted start itself"
    );
}

#[test]
fn a_token_passes_the_structural_check_only_with_the_anchor_s_imprint() {
    let (anchor, token) = anchored_token();
    assert!(
        tsa_imprint_matches(&anchor, &token).is_ok(),
        "a token containing the imprint — SHA-256 of the 32 raw root bytes — claims this root"
    );
    assert_eq!(
        tsa_imprint_matches(&anchor, b"no imprint here"),
        Err(ColdFailure::TsaTokenInvalid),
        "a token without the imprint does not claim this root"
    );
    let mut root_only = anchor.root.as_bytes().to_vec();
    root_only.extend_from_slice(b"the raw root is not the imprint");
    assert_eq!(
        tsa_imprint_matches(&anchor, &root_only),
        Err(ColdFailure::TsaTokenInvalid),
        "the root itself is not the imprint: SHA-256 of the root's bytes"
    );
}

#[test]
fn the_tsa_entry_point_refuses_until_the_crypto_half_lands() {
    let (anchor, token) = anchored_token();
    assert_eq!(
        verify_tsa(&anchor, &token),
        Err(ColdFailure::TsaVerificationIncomplete),
        "the imprint matches, but the signature, the chain and revocation are Proposed (DEC-265 item 1): the entry point never answers Ok for what it has not proven"
    );
    assert_eq!(
        verify_tsa(&anchor, b"no imprint here"),
        Err(ColdFailure::TsaTokenInvalid),
        "the structural check runs first: a token without the imprint is invalid on its face"
    );
    let mut root_only = anchor.root.as_bytes().to_vec();
    root_only.extend_from_slice(b"the raw root is not the imprint");
    assert_eq!(
        verify_tsa(&anchor, &root_only),
        Err(ColdFailure::TsaTokenInvalid),
        "and a token holding the root without the imprint does not even claim it"
    );
}

#[test]
fn an_inclusion_proof_lists_the_sibling_roots_and_rebuilds_the_root() {
    let leaves: Vec<AnchorLeaf> = ["a:ws:1", "b:ws:1", "c:ws:1"]
        .iter()
        .enumerate()
        .map(|(i, stream_id)| AnchorLeaf {
            stream_id: (*stream_id).to_owned(),
            seq: u64::try_from(i).expect("a fixture seq fits") + 1,
            hash: Digest::of_parts(&[&[u8::try_from(i).expect("a fixture index fits")]]),
        })
        .collect();
    let anchor = Anchor::compute(leaves.clone()).expect("three heads anchor");
    let left_two = merkle_root(&leaves[0..2]).expect("two leaves root");
    let right_one = merkle_root(&leaves[2..]).expect("one leaf roots");
    let proof = inclusion_proof(&anchor, "a:ws:1").expect("a covered stream has a proof");
    assert_eq!(
        proof,
        vec![leaves[1].leaf_hash().expect("the leaf hashes"), right_one],
        "a's proof is its sibling leaf's hash, then the whole right subtree's root"
    );
    assert_eq!(
        inclusion_proof(&anchor, "c:ws:1"),
        Ok(vec![left_two]),
        "c's proof is the whole left subtree's root, and nothing else"
    );
    let rebuilt = mandate_canon::Digest::of_parts(&[
        &[0x01],
        mandate_canon::Digest::of_parts(&[
            &[0x01],
            leaves[0].leaf_hash().expect("the leaf hashes").as_bytes(),
            leaves[1].leaf_hash().expect("the leaf hashes").as_bytes(),
        ])
        .as_bytes(),
        right_one.as_bytes(),
    ]);
    assert_eq!(
        rebuilt, anchor.root,
        "the test's own §10 walk over a's leaf and proof rebuilds the anchor's root"
    );
    assert_eq!(
        inclusion_proof(&anchor, "absent:ws:1"),
        Err(ColdError::StreamNotAnchored),
        "a stream the anchor does not cover is a named refusal, not a proof"
    );
}

#[test]
fn an_anchor_whose_leaves_do_not_produce_its_root_is_refused_a_proof() {
    let leaves: Vec<AnchorLeaf> = ["a:ws:1", "b:ws:1", "c:ws:1"]
        .iter()
        .enumerate()
        .map(|(i, stream_id)| AnchorLeaf {
            stream_id: (*stream_id).to_owned(),
            seq: u64::try_from(i).expect("a fixture seq fits") + 1,
            hash: Digest::of_parts(&[&[u8::try_from(i).expect("a fixture index fits")]]),
        })
        .collect();
    let anchored = Anchor::compute(leaves.clone()).expect("three heads anchor");
    let proofs = inclusion_proof(&anchored, "a:ws:1");
    assert!(
        proofs.is_ok(),
        "an anchor whose root its leaves do produce has proofs, so the refusal below is about the root: {proofs:?}"
    );
    let lying = Anchor {
        leaves,
        root: Digest::of(b"not the root of these leaves"),
    };
    assert_eq!(
        inclusion_proof(&lying, "a:ws:1"),
        Err(ColdError::MalformedAnchor),
        "a proof over leaves that do not produce the anchor's root would rebuild nothing: the refusal names the anchor, not the stream"
    );
}

#[test]
fn the_export_digest_follows_the_parts_and_changes_with_them() {
    let all = rows();
    let first = segment(&all[0..4]);
    let second = segment(&all[4..8]);
    let parts = [first, second];
    let anchors = [Anchor::compute(vec![AnchorLeaf {
        stream_id: STREAM.to_owned(),
        seq: 8,
        hash: all[7].hash,
    }])
    .expect("one head anchors")];
    let imprint = tsa_imprint(&anchors[0].root);
    let token = imprint.as_bytes().to_vec();
    let tokens: Vec<&[u8]> = vec![token.as_slice()];
    let bundle = ExportBundle {
        segments: &files(&parts),
        anchors: &anchors,
        tokens: &tokens,
    };
    let oracle = |bundle: &ExportBundle<'_>| -> Digest {
        let mut prefixed: Vec<Vec<u8>> = Vec::new();
        for part in bundle.segments {
            prefixed.push(part.manifest.to_vec());
            prefixed.push(part.file.to_vec());
        }
        for anchor in bundle.anchors {
            prefixed.push(anchor.root.as_bytes().to_vec());
        }
        for token in bundle.tokens {
            prefixed.push(token.to_vec());
        }
        let with_lengths: Vec<Vec<u8>> = prefixed
            .into_iter()
            .map(|mut part| {
                let mut prefixed = (u64::try_from(part.len()).expect("a part length fits"))
                    .to_le_bytes()
                    .to_vec();
                prefixed.append(&mut part);
                prefixed
            })
            .collect();
        Digest::of_parts(&with_lengths.iter().map(Vec::as_slice).collect::<Vec<_>>())
    };
    assert_eq!(
        bundle.verifier_digest(),
        Ok(oracle(&bundle)),
        "the verifier's digest is the test's own length-prefixed digest over the parts, in order"
    );
    let mut changed_file = parts[1].1.clone();
    let last = changed_file.len();
    changed_file[last - 2] ^= 1;
    let changed_parts = [parts[0].clone(), (parts[1].0.clone(), changed_file)];
    let changed_bundle = ExportBundle {
        segments: &files(&changed_parts),
        anchors: &anchors,
        tokens: &tokens,
    };
    assert_ne!(
        changed_bundle.verifier_digest(),
        bundle.verifier_digest(),
        "one byte of one part changes the digest"
    );
}
