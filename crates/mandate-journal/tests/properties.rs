//! Journal invariants as properties (journal spec §1, §5.1, §10, §11). Each oracle computes the
//! expected result its own way: a model of heads and epochs, direct byte checks on the chain, and
//! an independent bottom-up Merkle construction.

mod common;

use common::{STREAM, edit, event_id, journal_with, mark_draft, now, stream};
use mandate_canon::Digest;
use mandate_journal::{
    Anchor, AnchorLeaf, AppendOutcome, MemoryJournal, StoredEvent, TrustedStart, merkle_root,
    verify_anchor, verify_events,
};
use proptest::collection::vec;
use proptest::prelude::*;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
enum Op {
    /// A batch of new marks, sent with the head and epoch off by the given amounts.
    Append {
        size: usize,
        head_off: i8,
        epoch_off: i8,
    },
    /// Re-send the n-th committed batch (modulo the count) with arbitrary head and epoch.
    Retry {
        batch: usize,
        head: u8,
        epoch: u8,
    },
    TakeOwnership,
    /// A batch whose last draft is invalid.
    Invalid {
        size: usize,
    },
}

fn op() -> impl Strategy<Value = Op> {
    let off = prop_oneof![6 => Just(0i8), 1 => -2i8..=2];
    prop_oneof![
        6 => (1usize..4, off.clone(), off).prop_map(|(size, head_off, epoch_off)| Op::Append { size, head_off, epoch_off }),
        2 => (any::<usize>(), any::<u8>(), any::<u8>()).prop_map(|(batch, head, epoch)| Op::Retry { batch, head, epoch }),
        1 => Just(Op::TakeOwnership),
        1 => (1usize..3).prop_map(|size| Op::Invalid { size }),
    ]
}

/// Chain facts checked on the bytes, without the verifier: gapless seqs from 1, each `prev_hash`
/// the previous `hash`, each `hash` the SHA-256 of the body, and the body carrying its own seq.
fn assert_chain(rows: &[StoredEvent]) -> Result<(), TestCaseError> {
    let mut prev = Digest::ZERO;
    for (row, seq) in rows.iter().zip(1u64..) {
        prop_assert_eq!(row.seq, seq);
        prop_assert_eq!(row.prev_hash, prev);
        prop_assert_eq!(row.hash, Digest::of(&row.body));
        let text = String::from_utf8(row.body.clone()).unwrap();
        prop_assert!(text.contains(&format!(",\"seq\":{seq},")), "{}", text);
        prop_assert!(
            text.contains(&format!("\"prev_hash\":\"{}\"", prev.to_hex())),
            "{}",
            text
        );
        prev = row.hash;
    }
    Ok(())
}

proptest! {
    #[test]
    #[ignore = "pending E5-1"]
    fn appends_are_append_only_gapless_fenced_and_idempotent(ops in vec(op(), 1..25)) {
        let s = stream();
        let mut j = journal_with(0);
        let mut epoch: u64 = 1;
        let mut next_id: u64 = 1;
        let mut batches: Vec<(Vec<Vec<u8>>, Vec<StoredEvent>)> = Vec::new();
        for op in ops {
            let before = j.rows(&s).to_vec();
            let head = before.len() as u64;
            match op {
                Op::Append { size, head_off, epoch_off } => {
                    let drafts: Vec<Vec<u8>> = (0..size).map(|k| mark_draft(next_id + k as u64, &format!("{}.5", next_id))).collect();
                    let sent_head = head.checked_add_signed(i64::from(head_off)).unwrap_or(u64::MAX);
                    let sent_epoch = epoch.checked_add_signed(i64::from(epoch_off)).unwrap_or(u64::MAX);
                    let refs: Vec<&[u8]> = drafts.iter().map(Vec::as_slice).collect();
                    let outcome = j.append(&s, sent_head, sent_epoch, now(), &refs);
                    if sent_epoch != epoch {
                        prop_assert_eq!(outcome, AppendOutcome::Fenced { current_epoch: epoch });
                    } else if sent_head != head {
                        let last = before.last().unwrap();
                        prop_assert_eq!(outcome, AppendOutcome::HeadMismatch { actual_seq: head, actual_hash: last.hash });
                    } else {
                        let AppendOutcome::Committed(rows) = outcome else { return Err(TestCaseError::fail(format!("{outcome:?}"))) };
                        prop_assert_eq!(rows.iter().map(|r| r.seq).collect::<Vec<_>>(), (head + 1..=head + size as u64).collect::<Vec<_>>());
                        next_id += size as u64;
                        batches.push((drafts, rows));
                    }
                }
                Op::Retry { batch, head, epoch: e } => {
                    if batches.is_empty() { continue; }
                    let (drafts, rows) = &batches[batch % batches.len()];
                    let refs: Vec<&[u8]> = drafts.iter().map(Vec::as_slice).collect();
                    let outcome = j.append(&s, u64::from(head), u64::from(e), now(), &refs);
                    prop_assert_eq!(outcome, AppendOutcome::AlreadyCommitted(rows.clone()));
                }
                Op::TakeOwnership => {
                    epoch += 1;
                    prop_assert_eq!(j.take_ownership(&s), epoch);
                }
                Op::Invalid { size } => {
                    let mut drafts: Vec<Vec<u8>> = (0..size).map(|k| mark_draft(next_id + k as u64, "1")).collect();
                    let bad = edit(drafts.last().unwrap(), "payload.price", Some("\"1_000\""));
                    *drafts.last_mut().unwrap() = bad;
                    let refs: Vec<&[u8]> = drafts.iter().map(Vec::as_slice).collect();
                    let outcome = j.append(&s, head, epoch, now(), &refs);
                    prop_assert!(matches!(outcome, AppendOutcome::Invalid { draft, .. } if draft == size - 1), "{:?}", outcome);
                }
            }
            let after = j.rows(&s);
            prop_assert!(after.len() >= before.len());
            prop_assert_eq!(&after[..before.len()], before.as_slice(), "a stored row changed");
            prop_assert_eq!(j.head(&s).writer_epoch, epoch);
        }
        assert_chain(j.rows(&s))?;
        let verified = verify_events(j.rows(&s), TrustedStart::GENESIS, &BTreeMap::new());
        prop_assert!(verified.is_ok(), "{:?}", verified);
    }
}

fn anchored(rows: &[StoredEvent]) -> Anchor {
    let last = rows.last().unwrap();
    Anchor::compute(vec![
        AnchorLeaf {
            stream_id: STREAM.to_owned(),
            seq: last.seq,
            hash: last.hash,
        },
        AnchorLeaf {
            stream_id: "ctl:ws_1".to_owned(),
            seq: 7,
            hash: Digest::of(b"ctl"),
        },
    ])
    .unwrap()
}

fn detected(rows: &[StoredEvent], anchor: &Anchor) -> bool {
    verify_events(rows, TrustedStart::GENESIS, &BTreeMap::new()).is_err()
        || verify_anchor(anchor, &stream(), rows).is_err()
}

#[derive(Debug, Clone)]
enum Tamper {
    Byte {
        row: usize,
        at: usize,
        xor: u8,
        rehash: bool,
    },
    Column {
        row: usize,
        which: u8,
    },
    Delete {
        row: usize,
    },
    Swap {
        a: usize,
        b: usize,
    },
}

fn tamper() -> impl Strategy<Value = Tamper> {
    prop_oneof![
        (any::<usize>(), any::<usize>(), 1u8..=255, any::<bool>()).prop_map(
            |(row, at, xor, rehash)| Tamper::Byte {
                row,
                at,
                xor,
                rehash
            }
        ),
        (any::<usize>(), 0u8..9).prop_map(|(row, which)| Tamper::Column { row, which }),
        any::<usize>().prop_map(|row| Tamper::Delete { row }),
        (any::<usize>(), any::<usize>()).prop_map(|(a, b)| Tamper::Swap { a, b }),
    ]
}

proptest! {
    /// M4 exit criterion: tampering with any event is detected (with the head anchored).
    #[test]
    #[ignore = "pending E5-1"]
    fn any_tampering_is_detected(marks in 0u64..6, t in tamper()) {
        let mut rows = journal_with(marks).rows(&stream()).to_vec();
        let anchor = anchored(&rows);
        prop_assert!(!detected(&rows, &anchor));
        let n = rows.len();
        match t {
            Tamper::Byte { row, at, xor, rehash } => {
                let r = &mut rows[row % n];
                let at = at % r.body.len();
                r.body[at] ^= xor;
                if rehash { r.hash = Digest::of(&r.body); }
            }
            Tamper::Column { row, which } => {
                let r = &mut rows[row % n];
                match which {
                    0 => r.seq += 1,
                    1 => r.event_id = event_id(999),
                    2 => r.event_type = "FillApplied".into(),
                    3 => r.schema_version += 1,
                    4 => r.environment = "live".into(),
                    5 => r.recorded_at = "2026-09-21T14:00:00.000000001Z".into(),
                    6 => r.prev_hash = Digest::of(b"other"),
                    7 => r.hash = Digest::of(b"other"),
                    _ => r.stream_id = "acct:ws_1:OTHER".into(),
                }
            }
            Tamper::Delete { row } => { rows.remove(row % n); }
            Tamper::Swap { a, b } => {
                prop_assume!(a % n != b % n);
                rows.swap(a % n, b % n);
            }
        }
        prop_assert!(detected(&rows, &anchor));
    }

    /// A consistent rewrite passes the per-event checks by design; only the anchor catches it.
    #[test]
    #[ignore = "pending E5-1"]
    fn rewritten_chains_pass_per_event_checks_and_fail_the_anchor(marks in 1u64..6, from in any::<usize>()) {
        let mut rows = journal_with(marks).rows(&stream()).to_vec();
        let anchor = anchored(&rows);
        let from = 1 + from % (rows.len() - 1);
        let mut prev = rows[from - 1].hash;
        for (k, r) in rows.iter_mut().enumerate().skip(from) {
            let mut text = String::from_utf8(r.body.clone()).unwrap();
            if k == from {
                text = text.replacen("\"source\":\"quote\"", "\"source\":\"trade\"", 1);
            }
            text = text.replacen(&format!("\"prev_hash\":\"{}\"", r.prev_hash.to_hex()), &format!("\"prev_hash\":\"{}\"", prev.to_hex()), 1);
            r.body = text.into_bytes();
            r.prev_hash = prev;
            r.hash = Digest::of(&r.body);
            prev = r.hash;
        }
        prop_assert!(verify_events(&rows, TrustedStart::GENESIS, &BTreeMap::new()).is_ok());
        prop_assert!(verify_anchor(&anchor, &stream(), &rows).is_err());
    }

    #[test]
    #[ignore = "pending E5-1"]
    fn merkle_root_matches_bottom_up_construction(
        heads in proptest::collection::btree_map("[a-z]{1,6}:[A-Za-z0-9_-]{1,8}", (0u64..1 << 53, any::<[u8; 32]>()), 1..20)
    ) {
        let leaves: Vec<AnchorLeaf> = heads
            .iter()
            .map(|(id, (seq, hash))| AnchorLeaf { stream_id: id.clone(), seq: *seq, hash: Digest::from_bytes(*hash) })
            .collect();
        let mut level: Vec<Digest> = leaves
            .iter()
            .map(|l| {
                let json = format!(r#"{{"hash":"{}","seq":{},"stream_id":"{}"}}"#, l.hash.to_hex(), l.seq, l.stream_id);
                Digest::of_parts(&[&[0], json.as_bytes()])
            })
            .collect();
        while level.len() > 1 {
            level = level
                .chunks(2)
                .map(|pair| match pair {
                    [a, b] => Digest::of_parts(&[&[1], a.as_bytes(), b.as_bytes()]),
                    [a] => *a,
                    _ => unreachable!(),
                })
                .collect();
        }
        prop_assert_eq!(merkle_root(&leaves), Some(level[0]));
        let anchor = Anchor::compute(leaves.iter().rev().cloned().collect()).unwrap();
        prop_assert_eq!(anchor.root, level[0]);
        prop_assert_eq!(anchor.leaves, leaves);
    }
}

#[test]
#[ignore = "pending E5-1"]
fn equivalent_decimals_are_the_same_draft() {
    let s = stream();
    let mut j = journal_with(0);
    let AppendOutcome::Committed(rows) = j.append(&s, 1, 1, now(), &[&mark_draft(1, "150.01")])
    else {
        panic!("not committed")
    };
    for spelling in ["150.010", "15001e-2", "1.5001E+2", "0150.01", "150.0100000"] {
        assert_eq!(
            j.append(&s, 0, 0, now(), &[&mark_draft(1, spelling)]),
            AppendOutcome::AlreadyCommitted(rows.clone()),
            "{spelling}"
        );
    }
    assert_eq!(
        j.append(&s, 2, 1, now(), &[&mark_draft(1, "150.02")]),
        AppendOutcome::IdempotencyConflict { stored_seq: 2 }
    );
    let _: &MemoryJournal = &j;
}
