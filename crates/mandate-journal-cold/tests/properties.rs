//! The cold store's property suite: the range walk's invariants over random segment sequences,
//! each checked by an oracle that computes the answer its own way (E5-6a, DEC-280).
//!
//! The oracle (AGENTS.md, "Independent oracles") builds every scenario itself — a random run of
//! one stream's events, split into segments at random boundaries, entered at the genesis or
//! mid-segment — and computes the expected outcome from the scenario it built, never from the
//! crate's walk:
//!
//! 1. **The verified state.** An untampered range's `Verified` is the trusted start advanced by
//!    exactly the events the range walks: `next_seq` one past the last walked event's `seq`,
//!    `last_hash` the last walked event's hash.
//! 2. **The failing check and its seq.** For every tamper, the oracle names the check that owns
//!    it and the `seq` the range expected, by DEC-264's order — a manifest that does not parse
//!    fails `segment_manifest_mismatch` at the expected seq; a file or edge that disagrees with
//!    its manifest fails it at the manifest's own claimed `first_seq`; a segment that begins
//!    before or after the expected seq fails `segment_gap` at the expected seq.
//! 3. **The reachability rule.** Events before the trusted start are never checked: a scenario
//!    may swap the first two rows' hashes and the range still verifies, because the walk starts
//!    at `from_seq`.
//! 4. **The token's two answers.** `verify_tsa` never answers `Ok`: it is
//!    `TsaVerificationIncomplete` exactly when the token contains the anchor's imprint — by this
//!    suite's own containment search — and `TsaTokenInvalid` otherwise.
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
    ColdCheck, ColdError, ColdFailure, ExportBundle, SegmentFile, SegmentManifest,
    tsa_imprint_matches, verify_range, verify_tsa,
};
use proptest::collection::vec;
use proptest::prelude::*;

/// One generated scenario: the run's rows, the segment boundaries, and the trusted start the
/// range enters at.
#[derive(Clone, Debug)]
struct Scenario {
    rows: Vec<StoredEvent>,
    /// Segment `i` holds `rows[firsts[i]..firsts[i + 1]]`; the last holds the rest.
    firsts: Vec<usize>,
    start: TrustedStart,
}

impl Scenario {
    /// The `i`th segment's rows.
    fn slice(&self, i: usize) -> &[StoredEvent] {
        let begin = self.firsts[i];
        let end = self.firsts.get(i + 1).copied().unwrap_or(self.rows.len());
        &self.rows[begin..end]
    }

    fn segment_count(&self) -> usize {
        self.firsts.len()
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

/// A random scenario: 1 to 12 rows, cut after row `i` wherever `cuts[i - 1]` is true, entered
/// at the genesis or inside the first segment (the only entries a range can verify).
fn scenario() -> BoxedStrategy<Scenario> {
    (1usize..=12)
        .prop_flat_map(|n| vec(any::<bool>(), n - 1))
        .prop_flat_map(|cuts| {
            let first_len = cuts
                .iter()
                .position(|cut| *cut)
                .map_or(cuts.len() + 1, |at| at + 1);
            (Just(cuts), 1usize..=first_len)
        })
        .prop_map(|(cuts, entry)| {
            let rows = journal_with(cuts.len() as u64).rows(&stream()).to_vec();
            let mut firsts = vec![0usize];
            for (at, cut) in cuts.iter().enumerate() {
                if *cut {
                    firsts.push(at + 1);
                }
            }
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
    /// Segment `i` (never the last, never the only one) is missing.
    Drop(usize),
    /// A stale copy of the first `width` rows stands after segment `at` (never first).
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
/// against the very scenario the property will apply them to.
fn tampered_scenario() -> BoxedStrategy<(Scenario, Tamper)> {
    scenario()
        .prop_flat_map(|s| {
            let segments = s.segment_count();
            let rows = s.rows.len();
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
                    let tamper = match pick % 5 {
                        0 => Tamper::FlipFileByte(at),
                        1 => Tamper::LieField(at, field),
                        2 => Tamper::Unparsable(at),
                        3 if segments >= 2 => Tamper::Drop(at.min(segments - 2)),
                        3 => Tamper::FlipFileByte(at),
                        _ => Tamper::StaleCopy {
                            at: (at % segments) + 1,
                            width,
                        },
                    };
                    (s, tamper)
                },
            )
        })
        .boxed()
}

/// One segment of the oracle's model: the rows it holds, or a stale copy's width.
enum Modeled {
    Real { rows_at: usize },
    Stale { width: usize },
}

/// The oracle's own walk: the failing check and the seq the range expected, for a tamper the
/// generator proves fails — computed from the scenario, never from the crate.
fn oracle_failure(s: &Scenario, tamper: Tamper) -> Option<(ColdCheck, u64)> {
    let mut modeled: Vec<Modeled> = (0..s.segment_count())
        .map(|rows_at| Modeled::Real { rows_at })
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
        let (claimed_first, claimed_last, walked_last_seq, rows_at) = match segment {
            Modeled::Stale { width } => (1u64, *width as u64, *width as u64, usize::MAX),
            Modeled::Real { rows_at } => {
                let rows = s.slice(*rows_at);
                let first = rows.first().expect("a segment holds a row");
                let last = rows.last().expect("a segment holds a row");
                let claimed_first = match tamper {
                    Tamper::LieField(at, Field::FirstSeq) if at == *rows_at => first.seq + 1,
                    _ => first.seq,
                };
                let claimed_last = match tamper {
                    Tamper::LieField(at, Field::LastSeq) if at == *rows_at => last.seq + 1,
                    _ => last.seq,
                };
                (claimed_first, claimed_last, last.seq, *rows_at)
            }
        };
        if rows_at != usize::MAX {
            if matches!(tamper, Tamper::Unparsable(at) if at == rows_at) {
                return Some((ColdCheck::SegmentManifestMismatch, expected));
            }
            if matches!(tamper, Tamper::LieField(at, _) if at == rows_at)
                || matches!(tamper, Tamper::FlipFileByte(at) if at == rows_at)
            {
                return Some((ColdCheck::SegmentManifestMismatch, claimed_first));
            }
        }
        let carries = claimed_first <= expected && expected <= claimed_last;
        let chains = if first_segment {
            carries
        } else {
            claimed_first == expected
        };
        if !chains {
            return Some((ColdCheck::SegmentGap, expected));
        }
        expected = walked_last_seq + 1;
        first_segment = false;
    }
    None
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

    /// Oracle 2: every tamper fails the check that owns it, at the seq the range expected.
    #[test]
    fn a_tampered_range_fails_the_check_that_owns_it_at_the_expected_seq(
        (s, tamper) in tampered_scenario(),
    ) {
        let parts = tampered_parts(&s, tamper);
        let expected = oracle_failure(&s, tamper)
            .expect("every generated tamper fails the oracle's walk");
        prop_assert_eq!(
            verify_range(s.start, &files(&parts), &no_artifacts()),
            Err(ColdFailure::Segment {
                at_seq: expected.1,
                check: expected.0,
            }),
            "the oracle names the check that owns {:?}",
            tamper
        );
    }

    /// Oracle 3: events before the trusted start are not checked — the first two rows' hashes
    /// may be swapped, and the range still verifies, because the walk starts at `from_seq`.
    #[test]
    fn events_before_the_trusted_start_are_not_checked(
        s in scenario().prop_filter(
            "the entry leaves two rows before it",
            |s| s.start.from_seq >= 3,
        ),
    ) {
        let mut swapped = s.rows.clone();
        let (row_one_hash, row_two_hash) = (swapped[0].hash, swapped[1].hash);
        swapped[0].hash = row_two_hash;
        swapped[1].hash = row_one_hash;
        let parts: Vec<(Vec<u8>, Vec<u8>)> = (0..s.segment_count())
            .map(|i| {
                let begin = s.firsts[i];
                let end = s.firsts.get(i + 1).copied().unwrap_or(s.rows.len());
                let rows = &swapped[begin..end];
                (oracle_manifest_bytes(rows), export_segment(rows))
            })
            .collect();
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
