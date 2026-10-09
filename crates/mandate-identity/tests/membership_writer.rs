//! E9-7 (DEC-659 items 2 and 8, DEC-646): the one writer of `Member*` records on the control
//! stream reads the last membership record, stamps `event_time`, runs the order guard, and appends
//! at the head it read, all in one attempt; a moved head re-runs the attempt, and a refusal commits
//! nothing and is never clamped.

use std::collections::VecDeque;
use std::convert::Infallible;

use mandate_identity::{
    ATTEMPTS, Appended, ControlEntry, ControlStream, ControlView, InvitationId,
    MembershipEvent as Event, MembershipRecord, RecordRefusal, WriteError, write_membership,
};
use mandate_identity_seal::Seal;
use mandate_time::UtcNanos;
use proptest::prelude::*;
use proptest::test_runner::{TestCaseError, TestRunner};

const EPOCH: i64 = 1_790_000_000;
type Outcome = Result<MembershipRecord, WriteError<Infallible>>;
const REFUSED: Outcome = Err(WriteError::Refused(RecordRefusal::OutOfOrder));

/// A record a concurrent writer commits between the writer's read and its append: a membership
/// record, which that writer's own guard drops when out of order, or another type.
#[derive(Debug, Clone, Copy)]
enum Interloper {
    Member(i64),
    Other(i64),
}

/// The control stream as a compare-and-append over its rows, `seq` from 1, with the interlopers
/// that commit, one per append call, just before the writer's own append lands.
#[derive(Default)]
struct Stream {
    rows: Vec<ControlEntry>,
    pending: VecDeque<Interloper>,
    reads: u32,
}

impl Stream {
    fn with(rows: &[Interloper]) -> Self {
        let mut stream = Self::default();
        rows.iter().for_each(|row| stream.commit(*row));
        stream
    }

    fn commit(&mut self, row: Interloper) {
        let seq = self.rows.len() as u64 + 1;
        let last = self.rows.iter().rev().find_map(|row| match row {
            ControlEntry::Membership(r) => Some(r.event_time()),
            ControlEntry::Other { .. } => None,
        });
        let entry = match row {
            Interloper::Member(at) if last.is_some_and(|l| t(at) < l) => None,
            Interloper::Member(at) => Some(ControlEntry::Membership(record(seq, at, event(9)))),
            Interloper::Other(at) => Some(ControlEntry::Other {
                seq,
                event_time: t(at),
            }),
        };
        self.rows.extend(entry);
    }
}

impl ControlStream for Stream {
    type Error = Infallible;

    fn read(&mut self) -> Result<ControlView, Infallible> {
        self.reads += 1;
        let (head, tail) = (self.rows.len() as u64, self.rows.clone());
        Ok(ControlView { head, tail })
    }

    fn append(&mut self, head: u64, record: &MembershipRecord) -> Result<Appended, Infallible> {
        if let Some(row) = self.pending.pop_front() {
            self.commit(row);
        }
        if head != self.rows.len() as u64 {
            return Ok(Appended::HeadMoved);
        }
        self.rows.push(ControlEntry::Membership(record.clone()));
        Ok(Appended::Committed)
    }
}

fn t(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(EPOCH + secs, 0).unwrap()
}

fn event(invitation: u128) -> Event {
    let invitation = InvitationId(invitation);
    Event::InvitationRevoked { invitation }
}

fn record(seq: u64, at: i64, event: Event) -> MembershipRecord {
    MembershipRecord::new(Seal::grant(), seq, t(at), event)
}

fn write(stream: &mut Stream, at: UtcNanos) -> Outcome {
    write_membership(stream, || at, event(1))
}

fn mine(seq: u64, at: UtcNanos) -> MembershipRecord {
    MembershipRecord::new(Seal::grant(), seq, at, event(1))
}

/// The writer committed `event(1)` at exactly this `seq` and instant, as the stream's last row.
fn committed(stream: &Stream, got: Outcome, seq: u64, at: UtcNanos) {
    let want = mine(seq, at);
    assert_eq!(got, Ok(want.clone()), "the writer's answer");
    assert_eq!(stream.rows.last(), Some(&ControlEntry::Membership(want)));
    assert_eq!(stream.rows.len() as u64, seq, "nothing else was written");
}

/// The accessors the store's adapter maps a record into a journal draft with.
#[test]
fn a_record_reads_back_what_it_was_built_from() {
    let r = record(7, 60, event(3));
    assert_eq!((r.seq(), r.event_time(), r.event()), (7, t(60), &event(3)));
}

/// DEC-659 item 4: with no membership record yet, the first commits at head + 1, after a stream
/// opening or on an empty stream.
#[test]
#[ignore = "pending E9-7"]
fn the_first_membership_record_commits_at_the_head() {
    let mut empty = Stream::default();
    let got = write(&mut empty, t(0));
    committed(&empty, got, 1, t(0));
    let mut opened = Stream::with(&[Interloper::Other(3_600)]);
    let got = write(&mut opened, t(0));
    committed(&opened, got, 2, t(0));
}

/// An in-order record commits at the next `seq` with the clock's own instant, later or equal.
#[test]
#[ignore = "pending E9-7"]
fn an_in_order_record_commits_with_its_own_instant() {
    for (at, seq) in [(3_600, 3), (3_601, 3), (90_000, 3)] {
        let mut stream = Stream::with(&[Interloper::Other(0), Interloper::Member(3_600)]);
        let got = write(&mut stream, t(at));
        committed(&stream, got, seq, t(at));
    }
}

/// DEC-659 items 3 and 8: a clock behind the last membership record, even by 1 ns, is refused
/// and writes nothing; never clamped, the record commits with the clock's instant once past it.
#[test]
#[ignore = "pending E9-7"]
fn a_record_behind_the_last_is_refused_and_never_clamped() {
    let mut stream = Stream::with(&[Interloper::Other(0), Interloper::Member(7_200)]);
    let before = stream.rows.clone();
    let one_ns_early = UtcNanos::from_parts(EPOCH + 7_199, 999_999_999).unwrap();
    for at in [t(0), t(3_600), one_ns_early] {
        assert_eq!(write(&mut stream, at), REFUSED, "at {at}");
        assert_eq!(stream.rows, before, "nothing was written at {at}");
    }
    let got = write(&mut stream, t(7_201));
    committed(&stream, got, 3, t(7_201));
}

/// Two records in one second are ordered by their nanoseconds: an equal instant passes, one
/// nanosecond later passes, and one nanosecond earlier is refused.
#[test]
#[ignore = "pending E9-7"]
fn records_within_one_second_are_ordered_by_nanosecond() {
    let within = |nanos| UtcNanos::from_parts(EPOCH + 3_600, nanos).unwrap();
    let mut stream = Stream::with(&[Interloper::Other(0)]);
    let got = write(&mut stream, within(500));
    committed(&stream, got, 2, within(500));
    assert_eq!(write(&mut stream, within(499)), REFUSED, "1 ns before");
    let got = write(&mut stream, within(500));
    committed(&stream, got, 3, within(500));
    let got = write(&mut stream, within(501));
    committed(&stream, got, 4, within(501));
}

/// DEC-659 item 1: `last` is the last membership record, so a later record of another type
/// neither refuses a record after it nor admits one before it.
#[test]
#[ignore = "pending E9-7"]
fn last_is_the_last_membership_record_whatever_follows_it() {
    let mut stream = Stream::with(&[Interloper::Member(3_600), Interloper::Other(9_000)]);
    let got = write(&mut stream, t(5_000));
    committed(&stream, got, 3, t(5_000));
    let mut stream = Stream::with(&[Interloper::Member(9_000), Interloper::Other(3_600)]);
    let before = stream.rows.clone();
    assert_eq!(write(&mut stream, t(5_000)), REFUSED);
    assert_eq!(stream.rows, before, "nothing was written");
}

/// DEC-659 item 2: a membership record a concurrent writer commits after the read moves the head,
/// so the attempt re-runs against it: a stale `last` never admits a record now behind it.
#[test]
#[ignore = "pending E9-7"]
fn a_moved_head_reruns_the_guard_against_the_new_last() {
    let mut stream = Stream::with(&[Interloper::Member(0)]);
    stream.pending.push_back(Interloper::Member(7_200));
    assert_eq!(write(&mut stream, t(3_600)), REFUSED);
    assert_eq!(stream.rows.len(), 2, "only the concurrent record");
    assert_eq!(stream.reads, 2, "the guard ran again after the head moved");
}

/// A moved head re-reads and re-stamps: the retry commits after the concurrent records, of either
/// type, at the new head and the clock's instant at the retry.
#[test]
#[ignore = "pending E9-7"]
fn a_moved_head_retries_at_the_new_head_with_a_fresh_instant() {
    let mut stream = Stream::with(&[Interloper::Member(0)]);
    stream
        .pending
        .extend([Interloper::Member(1_000), Interloper::Other(90_000)]);
    let mut clock = [t(500), t(2_000), t(3_000)].into_iter();
    let got = write_membership(&mut stream, || clock.next().unwrap(), event(1));
    committed(&stream, got, 4, t(3_000));
}

/// A head that moves on every attempt ends in `Contended` after [`ATTEMPTS`] reads, with nothing
/// of the writer's committed.
#[test]
#[ignore = "pending E9-7"]
fn a_head_that_always_moves_is_contended() {
    let pending = (0..ATTEMPTS).map(|i| Interloper::Other(i.into())).collect();
    let mut stream = Stream {
        pending,
        ..Stream::default()
    };
    assert_eq!(write(&mut stream, t(0)), Err(WriteError::Contended));
    let counts = (stream.reads, stream.rows.len() as u32);
    assert_eq!(counts, (ATTEMPTS, ATTEMPTS), "only the concurrent records");
}

/// Over random writes, clocks, and concurrent records, an accumulator kept apart from the writer
/// and the stream predicts each answer and every row, and the stream's membership records stay in
/// `seq` and `event_time` order.
#[test]
#[ignore = "pending E9-7"]
fn the_stream_stays_ordered_under_random_interleavings() {
    let interloper = prop_oneof![
        (0..20i64).prop_map(Interloper::Member),
        (0..20i64).prop_map(Interloper::Other),
    ];
    let writes = prop::collection::vec((0..20i64, prop::collection::vec(interloper, 0..4)), 1..12);
    let mut config = ProptestConfig::with_cases(512);
    config.failure_persistence = None;
    let body = |writes: Vec<(i64, Vec<Interloper>)>| -> Result<(), TestCaseError> {
        let mut stream = Stream::default();
        let mut model: Vec<(i64, bool)> = Vec::new();
        let last = |m: &[(i64, bool)]| m.iter().rev().find(|r| r.1).map(|r| r.0);
        for (at, concurrent) in writes {
            stream.pending = concurrent.iter().copied().collect();
            let mut queue = concurrent.into_iter();
            let want = loop {
                if last(&model).is_some_and(|l| at < l) {
                    break REFUSED;
                }
                let landed = match queue.next() {
                    Some(Interloper::Member(c)) if last(&model).is_none_or(|l| c >= l) => (c, true),
                    Some(Interloper::Other(c)) => (c, false),
                    Some(Interloper::Member(_)) | None => {
                        model.push((at, true));
                        break Ok(mine(model.len() as u64, t(at)));
                    }
                };
                model.push(landed);
            };
            prop_assert_eq!(write(&mut stream, t(at)), want, "write at {}", at);
            stream.pending.clear();
        }
        let rows: Vec<(u64, UtcNanos, bool)> = (stream.rows.iter())
            .map(|row| match row {
                ControlEntry::Membership(r) => (r.seq(), r.event_time(), true),
                ControlEntry::Other { seq, event_time } => (*seq, *event_time, false),
            })
            .collect();
        let members: Vec<_> = rows.iter().filter(|r| r.2).collect();
        let ordered = members
            .windows(2)
            .all(|w| w[0].0 < w[1].0 && w[0].1 <= w[1].1);
        prop_assert!(ordered, "membership records out of order: {:?}", members);
        let model = model.iter().zip(1..).map(|(&(a, m), s)| (s, t(a), m));
        prop_assert_eq!(rows, model.collect::<Vec<_>>());
        Ok(())
    };
    if let Err(failure) = TestRunner::new(config).run(&writes, body) {
        panic!("{failure}");
    }
}
