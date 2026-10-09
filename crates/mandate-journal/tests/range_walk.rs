//! E12-3 (journal spec v0.36 §11, §9.13 rules 111 and 132; workspace API §4.8.1 "Coverage",
//! AU-8): a range is walked position by position from `from_seq` to `to_seq`, its count of events
//! walked is reported, and every failure names the position walked, never the `seq` a row holds.
//! The property judges seeded deletions, duplicates, reorders, truncations, left-over rows, and
//! corrupted rows against an oracle that only compares the stored rows with the intact range.

mod common;

use std::collections::BTreeMap;

use common::{edit, journal_with, mark_draft, now, stream};
use mandate_canon::Digest;
use mandate_journal::{
    EventCheck, EventFailure, RangeWalk, RangeWalkError, StoredEvent, TrustedStart, walk_range,
};
use proptest::prelude::*;
use proptest::test_runner::TestRunner;

/// Seq 1 to `n` of one valid chain: a `StreamOpened` and `n − 1` marks.
fn chain(n: u64) -> Vec<StoredEvent> {
    journal_with(n - 1).rows(&stream()).to_vec()
}

fn trusted(from_seq: u64, prev_hash: Digest) -> TrustedStart {
    TrustedStart {
        from_seq,
        prev_hash,
    }
}

/// The trusted start of a range of `chain` entered at `from`.
fn start(chain: &[StoredEvent], from: u64) -> TrustedStart {
    let before = usize::try_from(from).unwrap().checked_sub(2);
    trusted(from, before.map_or(Digest::ZERO, |i| chain[i].hash))
}

/// The rows of `chain` from `from` to `to`, both included.
fn range(chain: &[StoredEvent], from: u64, to: u64) -> Vec<StoredEvent> {
    chain[usize::try_from(from - 1).unwrap()..usize::try_from(to).unwrap()].to_vec()
}

fn walk(chain: &[StoredEvent], rows: &[StoredEvent], from: u64, to: u64) -> RangeWalk {
    walk_range(rows, start(chain, from), to, &BTreeMap::new()).unwrap()
}

fn walked(checked: u64, outcome: Result<Digest, EventFailure>) -> RangeWalk {
    RangeWalk { checked, outcome }
}

fn failed(checked: u64, seq: u64, check: EventCheck) -> RangeWalk {
    walked(checked, Err(EventFailure { seq, check }))
}

#[test]
fn a_walk_counts_every_event_of_its_range_and_ends_on_the_hash_at_to_seq() {
    let c = chain(6);
    for (from, to) in [(1, 6), (3, 5), (4, 4), (6, 6)] {
        let want = walked(to - from + 1, Ok(c[usize::try_from(to - 1).unwrap()].hash));
        let got = walk(&c, &range(&c, from, to), from, to);
        assert_eq!(got, want, "{from}..={to}");
    }
    let got = walk_range(&range(&c, 3, 5), trusted(3, c[0].hash), 5, &BTreeMap::new());
    assert_eq!(got, Ok(failed(0, 3, EventCheck::PrevHashMismatch)));
}

/// Each read of the range 3 to 5 fails where the walk expected an event it did not find. The first
/// two are short reads `verify_events` answers `Ok`; a row left over after `to_seq` is reported at
/// `to_seq`, the last `seq` rule 111 lets a failure name, with every event of the range walked.
#[test]
fn a_missing_extra_or_corrupted_row_fails_at_the_position_walked() {
    use EventCheck::{ColumnMismatch, NonCanonical, SeqGap};
    let c = chain(6);
    let mut column = range(&c, 3, 5);
    column[1].seq = 40;
    let mut garbage = range(&c, 3, 5);
    garbage[2].body = b"not json".to_vec();
    garbage[2].seq = 2;
    let with = |extra: &StoredEvent| [range(&c, 3, 5), vec![extra.clone()]].concat();
    let cases = [
        (range(&c, 3, 4), failed(2, 5, SeqGap), "truncated"),
        (vec![], failed(0, 3, SeqGap), "nothing read"),
        (
            vec![c[2].clone(), c[4].clone()],
            failed(1, 4, SeqGap),
            "seq 4 missing",
        ),
        (column, failed(1, 4, ColumnMismatch), "column 40 at 4"),
        (garbage, failed(2, 5, NonCanonical), "column 2 at 5"),
        (with(&c[5]), failed(3, 5, SeqGap), "seq 6 after the range"),
        (with(&c[4]), failed(3, 5, SeqGap), "seq 5 twice"),
    ];
    for (rows, want, name) in cases {
        assert_eq!(walk(&c, &rows, 3, 5), want, "{name}");
    }
    let mut j = journal_with(1);
    let reference = format!("\"sha256:{}\"", Digest::of(b"absent").to_hex());
    let draft = edit(&mark_draft(2, "1"), "payload.source", Some(&reference));
    let draft = edit(&draft, "artifact_refs", Some(&format!("[{reference}]")));
    j.append(&stream(), 2, 1, now(), &[&draft]);
    let c = j.rows(&stream()).to_vec();
    assert_eq!(
        walk(&c, &c[1..], 2, 3),
        failed(1, 3, EventCheck::ArtifactMissing)
    );
}

#[test]
fn bounds_that_name_no_range_are_refused() {
    let c = chain(3);
    let at = |from, to| walk_range(&c, trusted(from, Digest::ZERO), to, &BTreeMap::new());
    assert_eq!(at(0, 3), Err(RangeWalkError::NotARange));
    assert_eq!(at(3, 2), Err(RangeWalkError::NotARange));
    assert!(at(1, 3).is_ok(), "{:?}", at(1, 3));
}

/// How a property case alters the stored range, at row `k` of it.
#[derive(Debug, Clone, Copy)]
enum Seeded {
    Intact,
    Delete,
    Duplicate,
    Swap,
    Truncate,
    LeftOver,
    Column,
    Garbage,
}

/// The oracle: the first index at which the stored rows differ from the intact range, reported at
/// that position with the check the alteration breaks, or at `to` for a row past the range.
fn expected(intact: &[StoredEvent], stored: &[StoredEvent], from: u64, how: Seeded) -> RangeWalk {
    let count = u64::try_from(intact.len()).unwrap();
    let longest = intact.len().max(stored.len());
    let Some(i) = (0..longest).find(|&i| intact.get(i) != stored.get(i)) else {
        return walked(count, Ok(intact.last().unwrap().hash));
    };
    let i = u64::try_from(i).unwrap();
    let check = match how {
        _ if i == count => return failed(count, from + count - 1, EventCheck::SeqGap),
        Seeded::Column => EventCheck::ColumnMismatch,
        Seeded::Garbage => EventCheck::NonCanonical,
        _ => EventCheck::SeqGap,
    };
    failed(i, from + i, check)
}

#[test]
fn every_seeded_fault_fails_where_an_independent_comparison_says() {
    use Seeded::{Column, Delete, Duplicate, Garbage, Intact, LeftOver, Swap, Truncate};
    let c = chain(12);
    let kinds = [
        Intact, Delete, Duplicate, Swap, Truncate, LeftOver, Column, Garbage,
    ];
    let draw = (1u64..=12, 0u64..12, 0usize..kinds.len(), 0usize..12);
    let outcome = TestRunner::deterministic().run(&draw, |(from, span, kind, k)| {
        let to = (from + span).min(12);
        let intact = range(&c, from, to);
        let (k, how) = (k % intact.len(), kinds[kind]);
        let mut stored = intact.clone();
        match how {
            Intact => {}
            Delete => drop(stored.remove(k)),
            Duplicate => stored.insert(k, intact[k].clone()),
            Swap if k + 1 < stored.len() => stored.swap(k, k + 1),
            Swap => {}
            Truncate => stored.truncate(k),
            LeftOver => stored.push(c.get(usize::try_from(to).unwrap()).unwrap_or(&c[0]).clone()),
            Column => stored[k].seq += 100,
            Garbage => stored[k].body = b"{".to_vec(),
        }
        let got = walk_range(&stored, start(&c, from), to, &BTreeMap::new());
        let want = expected(&intact, &stored, from, how);
        prop_assert_eq!(got, Ok(want), "{:?} at {} of {}..={}", how, k, from, to);
        let failed_at = want.outcome.err().map(|f| f.seq);
        prop_assert!(
            failed_at.is_none_or(|seq| (from..=to).contains(&seq)),
            "rule 111"
        );
        prop_assert!(want.checked <= to - from + 1, "rule 132");
        Ok(())
    });
    if let Err(failure) = outcome {
        panic!("{failure}");
    }
}
