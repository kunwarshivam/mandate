//! The cold store's property suite: the range walk's invariants over random segment sequences,
//! each checked by an oracle that computes the answer its own way (E5-6a, DEC-287).
//!
//! The oracle (AGENTS.md, "Independent oracles") builds every scenario itself — a random run of
//! one stream's events, split into segments at random boundaries, entered at the genesis or
//! mid-segment inside the first supplied segment, or exactly at any supplied segment's first
//! seq with the earlier segments omitted (DEC-287 item 2: a range may be entered at any
//! supplied segment's first seq, the natural case of reading a range that starts partway
//! through the cold store) — and computes the expected outcome from the scenario it built,
//! never from the crate's walk:
//!
//! 1. **The verified state.** An untampered range's `Verified` is the trusted start advanced by
//!    exactly the events the range walks: `next_seq` one past the last walked event's `seq`,
//!    `last_hash` the last walked event's hash.
//! 2. **The failing check and its seq.** For every tamper, the oracle names the check that owns
//!    it and the `seq` the range expected, by DEC-264's order — a manifest that does not parse,
//!    or that is not in canonical form (DEC-263 item 5: `parse` accepts only canonical bytes),
//!    fails `segment_manifest_mismatch` at the expected seq; a file or edge that disagrees with
//!    its manifest fails it at the manifest's own claimed `first_seq`; a segment that begins
//!    before or after the expected seq, or that ends before it — a stale copy prepended in
//!    front of a boundary entry, which only the first segment's upper carries bound can refuse
//!    at the start itself — fails `segment_gap` at the expected seq; and an event that does
//!    not re-hash inside an otherwise valid segment fails the per-event check 4, surfacing as
//!    the `Event` arm unchanged.
//! 3. **The reachability rule.** Events before the trusted start are never checked: a scenario
//!    may swap the first two rows' hashes and the range still verifies, because the walk starts
//!    at `from_seq`.
//! 4. **The token's two answers.** `verify_tsa` never answers `Ok`: it is
//!    `TsaVerificationIncomplete` exactly when the token contains the anchor's imprint — by this
//!    suite's own containment search — and `TsaTokenInvalid` otherwise.
//! 5. **The proof's root.** An anchor's inclusion proof, folded with the oracle's own §10 walk
//!    from the leaf and the siblings upward, rebuilds the anchor's root.
//!
//! Every part the crate reads is built by the oracle (`to_canonical` of its own six-field
//! object, `export_segment` of the rows), so the crate is exercised only as the verifier. The
//! failure property's expected check and seq vary across cases, so a constant answer — refusal
//! or `Ok` — passes nothing; the do-nothing sweep over the implementation pins the same from
//! the other side.

mod common;

use std::collections::BTreeMap;

use common::{STREAM, journal_with, manifest_value, stream};
use mandate_canon::{Digest, Int, Key, Value, parse as parse_value, to_canonical};
use mandate_journal::{
    Anchor, AnchorLeaf, StoredEvent, TrustedStart, Verified, export_segment, tsa_imprint,
};
use mandate_journal_cold::{
    ColdCheck, ColdError, ColdFailure, ExportBundle, SegmentFile, SegmentManifest, inclusion_proof,
    tsa_imprint_matches, verify_range, verify_tsa,
};
use proptest::collection::vec;
use proptest::prelude::*;

/// One generated scenario: the run's rows, the segment boundaries, the run's segment the range
/// is handed from, and the trusted start the range enters at.
#[derive(Clone, Debug)]
struct Scenario {
    rows: Vec<StoredEvent>,
    /// Segment `i` holds `rows[firsts[i]..firsts[i + 1]]`; the last holds the rest.
    firsts: Vec<usize>,
    /// The run's segments the range is handed: `supplied_from..` — the segment carrying the
    /// entry and every later one; the earlier ones are omitted (DEC-287 item 2).
    supplied_from: usize,
    start: TrustedStart,
}

impl Scenario {
    /// The `i`th supplied segment's rows.
    fn slice(&self, i: usize) -> &[StoredEvent] {
        self.run_slice(self.supplied_from + i)
    }

    /// The run's `i`th segment's rows.
    fn run_slice(&self, i: usize) -> &[StoredEvent] {
        let begin = self.firsts[i];
        let end = self.firsts.get(i + 1).copied().unwrap_or(self.rows.len());
        &self.rows[begin..end]
    }

    fn segment_count(&self) -> usize {
        self.firsts.len() - self.supplied_from
    }

    /// The segment as the exporter builds it: the manifest's canonical bytes (the oracle's own
    /// object) and the file's bytes.
    fn true_parts(&self, i: usize) -> (Vec<u8>, Vec<u8>) {
        let rows = self.slice(i);
        (oracle_manifest_bytes(rows), export_segment(rows))
    }
}

/// The manifest object §6.2 names, from the rows' own edges — the oracle's builder.
fn oracle_manifest_bytes(rows: &[StoredEvent]) -> Vec<u8> {
    let first = rows.first().expect("a segment holds a row");
    let last = rows.last().expect("a segment holds a row");
    to_canonical(&manifest_value(
        STREAM,
        first.seq,
        last.seq,
        &first.prev_hash.to_hex(),
        &last.hash.to_hex(),
        &Digest::of(&export_segment(rows)).to_hex(),
    ))
}

/// The row each run segment begins at: `cuts[i - 1]` true cuts the run after row `i`.
fn segment_firsts(cuts: &[bool]) -> Vec<usize> {
    let mut firsts = vec![0usize];
    for (at, cut) in cuts.iter().enumerate() {
        if *cut {
            firsts.push(at + 1);
        }
    }
    firsts
}

/// A random scenario: 1 to 12 rows, cut after row `i` wherever `cuts[i - 1]` is true, entered
/// at the genesis or inside the first segment (the whole run supplied), or exactly at any later
/// segment's first seq with the earlier segments omitted — a range may be entered at any
/// supplied segment's first seq (DEC-287 item 2) — with `prev_hash` the hash of the event
/// before `from_seq`.
fn scenario() -> BoxedStrategy<Scenario> {
    (1usize..=12)
        .prop_flat_map(|n| vec(any::<bool>(), n - 1))
        .prop_flat_map(|cuts| {
            let firsts = segment_firsts(&cuts);
            let first_len = firsts.get(1).copied().unwrap_or(cuts.len() + 1);
            let mut entries: Vec<usize> = (1..=first_len).collect();
            entries.extend(firsts[1..].iter().map(|first| first + 1));
            (Just(cuts), proptest::sample::select(entries))
        })
        .prop_map(|(cuts, entry)| {
            let rows = journal_with(cuts.len() as u64).rows(&stream()).to_vec();
            let firsts = segment_firsts(&cuts);
            let supplied_from = firsts
                .iter()
                .rposition(|first| *first < entry)
                .expect("segment 0 begins at row 0, before every entry");
            let start = if entry == 1 {
                TrustedStart::GENESIS
            } else {
                TrustedStart {
                    from_seq: entry as u64,
                    prev_hash: rows[entry - 2].hash,
                }
            };
            Scenario {
                rows,
                firsts,
                supplied_from,
                start,
            }
        })
        .boxed()
}

/// One of the six manifest fields a scenario may lie about.
#[derive(Clone, Copy, Debug)]
enum Field {
    Stream,
    FirstSeq,
    LastSeq,
    FirstPrevHash,
    LastHash,
    FileSha256,
}

/// What a scenario does to the range, and where. Every variant fails the oracle's walk.
#[derive(Clone, Copy, Debug)]
enum Tamper {
    /// One byte of segment `i`'s file is flipped.
    FlipFileByte(usize),
    /// Segment `i`'s manifest lies about one field.
    LieField(usize, Field),
    /// Segment `i`'s manifest bytes are not a manifest at all.
    Unparsable(usize),
    /// Segment `i`'s manifest bytes are the six fields in a non-canonical member order, so
    /// only `parse`'s canonical-bytes guard (DEC-263 item 5) can refuse them.
    NonCanonical(usize),
    /// Row `i`'s hash is a lie, the parts rebuilt from the lied row so every segment check
    /// passes and only the per-event walk (DEC-264's check 4) can catch it.
    CorruptEvent(usize),
    /// Segment `i` (never the last, never the only one) is missing.
    Drop(usize),
    /// A stale copy of the first `width` rows stands at `at` — in front of the trusted start
    /// when `at` is 0, where a copy ending before the entry is refused only by the first
    /// segment's upper carries bound, the one check a boundary entry leaves unmasked: the copy
    /// is walked as empty and the run's own first supplied segment then begins exactly at the
    /// entry, so nothing else reports the gap.
    StaleCopy { at: usize, width: usize },
}

/// The manifest bytes with one field lied about, still canonical and parsable.
fn lied_manifest_bytes(true_bytes: &[u8], field: Field, rows: &[StoredEvent]) -> Vec<u8> {
    let mut value = parse_value(true_bytes).expect("the oracle's bytes parse");
    let Value::Object(members) = &mut value else {
        panic!("a manifest's bytes are an object")
    };
    let first = rows.first().expect("a segment holds a row");
    let last = rows.last().expect("a segment holds a row");
    let (key, lied) = match field {
        Field::Stream => ("stream", Value::Str("acct:ws_1:ACCT2".to_owned())),
        Field::FirstSeq => (
            "first_seq",
            Value::Int(Int::new(first.seq + 1).expect("a fixture seq fits")),
        ),
        Field::LastSeq => (
            "last_seq",
            Value::Int(Int::new(last.seq + 1).expect("a fixture seq fits")),
        ),
        Field::FirstPrevHash | Field::LastHash | Field::FileSha256 => (
            Field::key(field),
            Value::Str(Digest::of(b"the lie").to_hex()),
        ),
    };
    members.insert(Key::new(key).expect("the key parses"), lied);
    to_canonical(&value)
}

impl Field {
    fn key(self) -> &'static str {
        match self {
            Self::Stream => "stream",
            Self::FirstSeq => "first_seq",
            Self::LastSeq => "last_seq",
            Self::FirstPrevHash => "first_prev_hash",
            Self::LastHash => "last_hash",
            Self::FileSha256 => "file_sha256",
        }
    }
}

/// The manifest bytes with the six fields serialized in descending member order — canonical
/// order is ascending — so they parse as JSON and yield the true manifest, and only `parse`'s
/// canonical-bytes guard (DEC-263 item 5) can refuse them.
fn non_canonical_manifest_bytes(rows: &[StoredEvent]) -> Vec<u8> {
    let first = rows.first().expect("a segment holds a row");
    let last = rows.last().expect("a segment holds a row");
    format!(
        "{{\"stream\":\"{STREAM}\",\"last_seq\":{},\"last_hash\":\"{}\",\
         \"first_seq\":{},\"first_prev_hash\":\"{}\",\"file_sha256\":\"{}\"}}",
        last.seq,
        last.hash.to_hex(),
        first.seq,
        first.prev_hash.to_hex(),
        Digest::of(&export_segment(rows)).to_hex(),
    )
    .into_bytes()
}

/// The scenario's parts as the range reads them, the tamper applied.
fn tampered_parts(s: &Scenario, tamper: Tamper) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut parts: Vec<(Vec<u8>, Vec<u8>)> =
        (0..s.segment_count()).map(|i| s.true_parts(i)).collect();
    match tamper {
        Tamper::FlipFileByte(at) => {
            let file = &mut parts[at].1;
            let mid = file.len() / 2;
            file[mid] ^= 1;
        }
        Tamper::LieField(at, field) => {
            let rows = s.slice(at);
            parts[at].0 = lied_manifest_bytes(&parts[at].0, field, rows);
        }
        Tamper::Unparsable(at) => parts[at].0 = b"not a manifest".to_vec(),
        Tamper::NonCanonical(at) => parts[at].0 = non_canonical_manifest_bytes(s.slice(at)),
        Tamper::CorruptEvent(row) => {
            let mut lied = s.rows.clone();
            lied[row].hash = Digest::of(b"the lie");
            return parts_from_rows(s, &lied);
        }
        Tamper::Drop(at) => {
            parts.remove(at);
        }
        Tamper::StaleCopy { at, width } => {
            let stale = s.rows[..width].to_vec();
            parts.insert(at, (oracle_manifest_bytes(&stale), export_segment(&stale)));
        }
    }
    parts
}

/// The scenario's supplied segments rebuilt from `rows` — every manifest and file derived from
/// the same rows, so the export is self-consistent and only the walk's own rules can refuse it.
fn parts_from_rows(s: &Scenario, rows: &[StoredEvent]) -> Vec<(Vec<u8>, Vec<u8>)> {
    (0..s.segment_count())
        .map(|i| {
            let begin = s.firsts[s.supplied_from + i];
            let end = s
                .firsts
                .get(s.supplied_from + i + 1)
                .copied()
                .unwrap_or(rows.len());
            let slice = &rows[begin..end];
            (oracle_manifest_bytes(slice), export_segment(slice))
        })
        .collect()
}

/// The parts as `SegmentFile`s over the built byte vectors.
fn files(parts: &[(Vec<u8>, Vec<u8>)]) -> Vec<SegmentFile<'_>> {
    parts
        .iter()
        .map(|(manifest, file)| SegmentFile { manifest, file })
        .collect()
}

/// An empty artifact store: the drafts carry no artifact references, so nothing is read.
fn no_artifacts() -> BTreeMap<Digest, Vec<u8>> {
    BTreeMap::new()
}

/// A scenario paired with one of its own failing tamples: the tamper's indices are drawn
/// against the very scenario the property will apply them to, and the stale copy either
/// follows a supplied segment or is prepended in front of the trusted start.
fn tampered_scenario() -> BoxedStrategy<(Scenario, Tamper)> {
    scenario()
        .prop_flat_map(|s| {
            let segments = s.segment_count();
            let rows = s.rows.len();
            let first_walked = (s.start.from_seq as usize) - 1;
            let walked = rows - first_walked;
            (Just(s), 0usize..segments, 1usize..=rows, any::<u8>()).prop_map(
                move |(s, at, width, pick)| {
                    let field = match pick % 6 {
                        0 => Field::Stream,
                        1 => Field::FirstSeq,
                        2 => Field::LastSeq,
                        3 => Field::FirstPrevHash,
                        4 => Field::LastHash,
                        _ => Field::FileSha256,
                    };
                    let tamper = match pick % 8 {
                        0 => Tamper::FlipFileByte(at),
                        1 => Tamper::LieField(at, field),
                        2 => Tamper::Unparsable(at),
                        3 if segments >= 2 => Tamper::Drop(at.min(segments - 2)),
                        4 => Tamper::NonCanonical(at),
                        5 => Tamper::CorruptEvent(first_walked + (width - 1) % walked),
                        6 => Tamper::StaleCopy {
                            at: (at % segments) + 1,
                            width,
                        },
                        _ => Tamper::StaleCopy { at: 0, width },
                    };
                    (s, tamper)
                },
            )
        })
        .boxed()
}

/// One segment of the oracle's model: a supplied segment's own index and the run segment
/// holding its rows, or a stale copy's width.
enum Modeled {
    Real { at: usize, rows_at: usize },
    Stale { width: usize },
}

/// DEC-264 item 3, from its own words: "each begins where the previous ended, exactly, in both
/// directions" — so a later segment chains only by beginning at the previous end — "and the
/// first carries the trusted start", which §11's reachability clause reads as the start's
/// `from_seq` not sitting outside the first segment's span: a range may enter mid-segment or
/// exactly at a segment's first seq, and a segment that ends before the start or begins after
/// it carries nothing.
fn chains_from_the_words(first_segment: bool, first: u64, last: u64, start: u64) -> bool {
    if first_segment {
        !(start < first || last < start)
    } else {
        first == start
    }
}

/// The oracle's own walk: the failure the range must report, for a tamper the generator proves
/// fails — computed from the scenario, never from the crate.
fn oracle_failure(s: &Scenario, tamper: Tamper) -> Option<ColdFailure> {
    let mut modeled: Vec<Modeled> = (s.supplied_from..s.firsts.len())
        .enumerate()
        .map(|(at, rows_at)| Modeled::Real { at, rows_at })
        .collect();
    match tamper {
        Tamper::Drop(at) => {
            modeled.remove(at);
        }
        Tamper::StaleCopy { at, width } => {
            let at = at.min(modeled.len());
            modeled.insert(at, Modeled::Stale { width });
        }
        _ => {}
    }
    let mut expected = s.start.from_seq;
    let mut first_segment = true;
    for segment in &modeled {
        let (claimed_first, claimed_last, walked_last_seq, at) = match segment {
            Modeled::Stale { width } => (1u64, *width as u64, *width as u64, usize::MAX),
            Modeled::Real { at, rows_at } => {
                let rows = s.run_slice(*rows_at);
                let first = rows.first().expect("a segment holds a row");
                let last = rows.last().expect("a segment holds a row");
                let claimed_first = match tamper {
                    Tamper::LieField(i, Field::FirstSeq) if i == *at => first.seq + 1,
                    _ => first.seq,
                };
                let claimed_last = match tamper {
                    Tamper::LieField(i, Field::LastSeq) if i == *at => last.seq + 1,
                    _ => last.seq,
                };
                (claimed_first, claimed_last, last.seq, *at)
            }
        };
        if at != usize::MAX {
            if matches!(tamper, Tamper::Unparsable(i) if i == at)
                || matches!(tamper, Tamper::NonCanonical(i) if i == at)
            {
                return Some(ColdFailure::Segment {
                    at_seq: expected,
                    check: ColdCheck::SegmentManifestMismatch,
                });
            }
            if matches!(tamper, Tamper::LieField(i, _) if i == at)
                || matches!(tamper, Tamper::FlipFileByte(i) if i == at)
            {
                return Some(ColdFailure::Segment {
                    at_seq: claimed_first,
                    check: ColdCheck::SegmentManifestMismatch,
                });
            }
        }
        if !chains_from_the_words(first_segment, claimed_first, claimed_last, expected) {
            return Some(ColdFailure::Segment {
                at_seq: expected,
                check: ColdCheck::SegmentGap,
            });
        }
        if let (Tamper::CorruptEvent(row), Modeled::Real { rows_at, .. }) = (tamper, segment) {
            let begin = s.firsts[*rows_at];
            let end = s.firsts.get(rows_at + 1).copied().unwrap_or(s.rows.len());
            if begin <= row && row < end {
                return Some(ColdFailure::Event(mandate_journal::EventFailure {
                    seq: s.rows[row].seq,
                    check: mandate_journal::EventCheck::RehashMismatch,
                }));
            }
        }
        expected = walked_last_seq + 1;
        first_segment = false;
    }
    None
}

/// A random anchor over distinct stream heads, and the index of the leaf whose proof is asked
/// for: 1 to 8 leaves, each with its own seq and a hash drawn from its own seed, sorted by
/// `Anchor::compute` as §10 pins.
fn anchored_leaves() -> BoxedStrategy<(Anchor, usize)> {
    (1usize..=8, vec(any::<u64>(), 8))
        .prop_flat_map(|(count, seeds)| {
            let leaves: Vec<AnchorLeaf> = (0..count)
                .map(|i| AnchorLeaf {
                    stream_id: format!("acct:ws_1:ACCT{}", i + 1),
                    seq: seeds[i] % 100 + 1,
                    hash: Digest::of_parts(&[&(i as u64).to_le_bytes(), &seeds[i].to_le_bytes()]),
                })
                .collect();
            let anchor = Anchor::compute(leaves).expect("distinct heads anchor");
            (Just(anchor), 0usize..count)
        })
        .boxed()
}

/// The oracle's own §10 fold: the leaf's side at each level, read from the split rule root to
/// leaf — the largest power of two below each level's width — then the audit path's siblings
/// folded leaf-upward, the sibling on the far side of the leaf at every step. `None` when the
/// path's length is not the tree's depth.
fn oracle_rebuild_root(
    leaf: &Digest,
    index: usize,
    width: usize,
    path: &[Digest],
) -> Option<Digest> {
    let mut sides = Vec::new();
    let (mut lo, mut hi) = (0usize, width);
    while hi - lo > 1 {
        let mut split = 1usize;
        while split * 2 < hi - lo {
            split *= 2;
        }
        sides.push(index < lo + split);
        if index < lo + split {
            hi = lo + split;
        } else {
            lo += split;
        }
    }
    if path.len() != sides.len() {
        return None;
    }
    let mut node = *leaf;
    for (sibling, leaf_on_the_left) in path.iter().zip(sides.iter().rev()) {
        node = if *leaf_on_the_left {
            Digest::of_parts(&[&[0x01], node.as_bytes(), sibling.as_bytes()])
        } else {
            Digest::of_parts(&[&[0x01], sibling.as_bytes(), node.as_bytes()])
        };
    }
    Some(node)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    /// Oracle 1: an untampered range verifies to the trusted start advanced by exactly the
    /// events inside it, whatever the boundaries and wherever the entry sits.
    #[test]
    fn an_untampered_range_verifies_to_the_trusted_start_advanced(s in scenario()) {
        let parts: Vec<(Vec<u8>, Vec<u8>)> =
            (0..s.segment_count()).map(|i| s.true_parts(i)).collect();
        let walked: Vec<&StoredEvent> = s
            .rows
            .iter()
            .filter(|row| row.seq >= s.start.from_seq)
            .collect();
        let last = walked.last().expect("the entry sits inside the run");
        prop_assert_eq!(
            verify_range(s.start, &files(&parts), &no_artifacts()),
            Ok(Verified {
                next_seq: last.seq + 1,
                last_hash: last.hash,
            }),
            "the oracle advanced the trusted start by exactly the {} walked events",
            walked.len()
        );
    }

    /// Oracle 2: every tamper fails the check that owns it, at the seq the range expected —
    /// the segment checks by DEC-264's order, and the per-event check 4 for a lie inside an
    /// otherwise valid segment.
    #[test]
    fn a_tampered_range_fails_the_check_that_owns_it_at_the_expected_seq(
        (s, tamper) in tampered_scenario(),
    ) {
        let parts = tampered_parts(&s, tamper);
        let expected = oracle_failure(&s, tamper)
            .expect("every generated tamper fails the oracle's walk");
        prop_assert_eq!(
            verify_range(s.start, &files(&parts), &no_artifacts()),
            Err(expected),
            "the oracle names the check that owns {:?}",
            tamper
        );
    }

    /// Oracle 3: events before the trusted start are not checked — the first two rows' hashes
    /// may be swapped, and the range still verifies, because the walk starts at `from_seq`.
    /// The whole run is supplied: the two swapped rows sit inside the handed segments, before
    /// the entry.
    #[test]
    fn events_before_the_trusted_start_are_not_checked(
        s in scenario().prop_filter(
            "the whole run is supplied and the entry leaves two rows before it",
            |s| s.supplied_from == 0 && s.start.from_seq >= 3,
        ),
    ) {
        let mut swapped = s.rows.clone();
        let (row_one_hash, row_two_hash) = (swapped[0].hash, swapped[1].hash);
        swapped[0].hash = row_two_hash;
        swapped[1].hash = row_one_hash;
        let parts = parts_from_rows(&s, &swapped);
        let walked: Vec<&StoredEvent> = s
            .rows
            .iter()
            .filter(|row| row.seq >= s.start.from_seq)
            .collect();
        let last = walked.last().expect("the entry sits inside the run");
        prop_assert_eq!(
            verify_range(s.start, &files(&parts), &no_artifacts()),
            Ok(Verified {
                next_seq: last.seq + 1,
                last_hash: last.hash,
            }),
            "rows 1 and 2 carry swapped hashes, but the range starts at {}",
            s.start.from_seq
        );
    }

    /// The manifest round trip over random runs, and the out-of-range refusal: the bytes are
    /// the oracle's object, the hash its own digest of them, and a `seq` above the canonical
    /// integer bound is refused both.
    #[test]
    fn a_manifest_round_trips_and_refuses_a_seq_above_the_bound(
        n in 1usize..=12,
        beyond in 0u64..100,
        first_field in any::<bool>(),
    ) {
        let rows = journal_with(n as u64 - 1).rows(&stream()).to_vec();
        let manifest = SegmentManifest::of(&rows).expect("a contiguous run manifests");
        let bytes = manifest
            .to_canonical_bytes()
            .expect("the run's seqs sit far below the bound");
        prop_assert_eq!(bytes.as_slice(), oracle_manifest_bytes(&rows));
        prop_assert_eq!(SegmentManifest::parse(bytes.as_slice()), Ok(manifest.clone()));
        prop_assert_eq!(
            manifest.manifest_hash(),
            Ok(Digest::of(&oracle_manifest_bytes(&rows)))
        );
        let mut out_of_range = manifest;
        if first_field {
            out_of_range.first_seq = mandate_canon::MAX_INT + 1 + beyond;
        } else {
            out_of_range.last_seq = mandate_canon::MAX_INT + 1 + beyond;
        }
        prop_assert_eq!(
            out_of_range.to_canonical_bytes(),
            Err(ColdError::SeqUnrepresentable)
        );
        prop_assert_eq!(
            out_of_range.manifest_hash(),
            Err(ColdError::SeqUnrepresentable)
        );
    }

    /// Oracle 5, §12's inclusion proof over random anchors: the proof's siblings, folded with
    /// the oracle's own §10 walk from the leaf upward, rebuild the anchor's root.
    #[test]
    fn an_inclusion_proof_rebuilds_the_anchor_s_root((anchor, index) in anchored_leaves()) {
        let stream_id = anchor.leaves[index].stream_id.clone();
        let proof = inclusion_proof(&anchor, &stream_id).expect("a covered stream has a proof");
        let leaf = anchor.leaves[index]
            .leaf_hash()
            .expect("the fixture seqs sit far below the bound");
        let rebuilt = oracle_rebuild_root(&leaf, index, anchor.leaves.len(), &proof)
            .expect("the proof's length is the tree's depth");
        prop_assert_eq!(
            rebuilt, anchor.root,
            "the oracle's own fold over the leaf and the proof rebuilds the root"
        );
    }

    /// Oracle 4: the token entry point never answers `Ok` — incomplete exactly when the token
    /// contains the imprint, by this suite's own containment search, and invalid otherwise.
    #[test]
    fn the_token_entry_point_never_answers_ok(
        marks in 0u8..12,
        flavour in 0u8..4,
        junk in vec(any::<u8>(), 0..64),
    ) {
        let rows = journal_with(marks as u64).rows(&stream()).to_vec();
        let last = rows.last().expect("the run holds a row");
        let anchor = Anchor::compute(vec![AnchorLeaf {
            stream_id: STREAM.to_owned(),
            seq: last.seq,
            hash: last.hash,
        }])
        .expect("one head anchors");
        let imprint = tsa_imprint(&anchor.root);
        let mut token: Vec<u8> = Vec::new();
        match flavour {
            0 => {
                token.extend_from_slice(b"junk before ");
                token.extend_from_slice(imprint.as_bytes());
                token.extend_from_slice(b" junk after");
            }
            1 => token.extend_from_slice(anchor.root.as_bytes()),
            2 => token.extend_from_slice(anchor.root.to_hex().as_bytes()),
            _ => token.extend_from_slice(&junk),
        }
        let contained = token
            .windows(imprint.as_bytes().len())
            .any(|window| window == imprint.as_bytes().as_slice());
        prop_assert_ne!(verify_tsa(&anchor, &token), Ok(()));
        prop_assert_eq!(
            tsa_imprint_matches(&anchor, &token),
            if contained { Ok(()) } else { Err(ColdFailure::TsaTokenInvalid) },
            "the structural check agrees with the suite's own containment search"
        );
        prop_assert_eq!(
            verify_tsa(&anchor, &token),
            if contained {
                Err(ColdFailure::TsaVerificationIncomplete)
            } else {
                Err(ColdFailure::TsaTokenInvalid)
            },
            "the entry point fails closed: incomplete when the imprint matches, invalid otherwise"
        );
    }

    /// The export digest over random bundles: the oracle's own length-prefixed digest over the
    /// parts, and one changed part always changes it.
    #[test]
    fn the_export_digest_follows_the_parts_and_changes_with_them(
        s in scenario(),
        anchors in 1usize..=3,
        flip_anchor in any::<bool>(),
    ) {
        let parts: Vec<(Vec<u8>, Vec<u8>)> =
            (0..s.segment_count()).map(|i| s.true_parts(i)).collect();
        let head = s.rows.last().expect("the run holds a row");
        let built: Vec<Anchor> = (0..anchors)
            .map(|i| {
                Anchor::compute(vec![AnchorLeaf {
                    stream_id: format!("acct:ws_1:ACCT{}", i + 1),
                    seq: head.seq,
                    hash: head.hash,
                }])
                .expect("distinct heads anchor")
            })
            .collect();
        let tokens: Vec<Vec<u8>> = (0..built.len())
            .map(|i| {
                let mut token = tsa_imprint(&built[i].root).as_bytes().to_vec();
                token.extend_from_slice(format!(" token {i}").as_bytes());
                token
            })
            .collect();
        let token_slices: Vec<&[u8]> = tokens.iter().map(Vec::as_slice).collect();
        let segment_files = files(&parts);
        let bundle = ExportBundle {
            segments: &segment_files,
            anchors: &built,
            tokens: &token_slices,
        };
        let mut prefixed: Vec<Vec<u8>> = Vec::new();
        for (manifest, file) in &parts {
            prefixed.push(manifest.clone());
            prefixed.push(file.clone());
        }
        for anchor in &built {
            prefixed.push(anchor.root.as_bytes().to_vec());
        }
        for token in &tokens {
            prefixed.push(token.clone());
        }
        let with_lengths: Vec<Vec<u8>> = prefixed
            .into_iter()
            .map(|mut part| {
                let mut prefixed = (part.len() as u64).to_le_bytes().to_vec();
                prefixed.append(&mut part);
                prefixed
            })
            .collect();
        let oracle = Digest::of_parts(
            &with_lengths.iter().map(Vec::as_slice).collect::<Vec<&[u8]>>(),
        );
        prop_assert_eq!(bundle.verifier_digest(), Ok(oracle));

        let mut changed_anchors = built.clone();
        let mut changed_tokens = tokens.clone();
        if flip_anchor {
            let mut root = *changed_anchors[0].root.as_bytes();
            root[0] ^= 1;
            changed_anchors[0].root = Digest::from_bytes(root);
        } else {
            changed_tokens[0].push(b'.');
        }
        let changed_token_slices: Vec<&[u8]> =
            changed_tokens.iter().map(Vec::as_slice).collect();
        let changed = ExportBundle {
            segments: &segment_files,
            anchors: &changed_anchors,
            tokens: &changed_token_slices,
        };
        prop_assert_ne!(changed.verifier_digest(), bundle.verifier_digest());
    }
}
