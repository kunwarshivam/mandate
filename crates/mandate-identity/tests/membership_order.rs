//! E9-7 (DEC-659): the writer's order guard, which refuses before commit a membership record whose
//! `seq` is not above, or whose `event_time` is before, the stream's last membership record's, so
//! the fold never reads a record out of order (DEC-657 item 4) from a writer that obeys it.

mod membership;

use std::collections::BTreeMap;

use mandate_identity::{
    InvitationId, MembershipEvent as Event, MembershipRecord, PrincipalId, RecordRefusal,
    check_order,
};
use mandate_identity_seal::Seal;
use mandate_time::UtcNanos;
use membership::{
    B, EPOCH, activated, fold, founded, founding, invited, reactivated, record, sequenced, set, t,
};
use proptest::prelude::*;
use proptest::test_runner::{TestCaseError, TestRunner};

const VIEWER: u8 = 0b01000;
const REFUSED: Result<(), RecordRefusal> = Err(RecordRefusal::OutOfOrder);

/// One record of each of the seven §9.12 membership types, the founding grant aside.
fn every_type() -> [(&'static str, Event); 7] {
    let member = PrincipalId(B);
    let granted = Event::RoleChanged {
        member,
        added: BTreeMap::new(),
        removed: set(VIEWER),
        independent_approval_required: false,
    };
    [
        ("invited", invited(1, set(VIEWER), 0)),
        (
            "invitation revoked",
            Event::InvitationRevoked {
                invitation: InvitationId(1),
            },
        ),
        ("activated", activated(B, Some(1), set(VIEWER), false, 0)),
        ("role changed", granted),
        ("deactivated", Event::Deactivated { member }),
        ("reactivated", reactivated(B, set(VIEWER), false, 0)),
        ("removed", Event::Removed { member }),
    ]
}

/// The stream's last membership record in the cases below: `seq` 4 at instant 3_600.
fn last() -> MembershipRecord {
    record(4, 3_600, invited(1, set(VIEWER), 3_600))
}

/// DEC-659: the first membership record has nothing to follow, so it passes whatever its `seq`,
/// instant, and type, though the same record after one at a higher `seq` is refused.
#[test]
#[ignore = "pending E9-7"]
fn the_first_membership_record_passes() {
    let ahead = record(u64::MAX, 90_000, invited(1, set(VIEWER), 90_000));
    let founding = [("founding", founded(true))];
    for (name, event) in every_type().into_iter().chain(founding) {
        for (seq, at) in [(1, 0), (0, -3_600), (9, 7_200)] {
            let next = record(seq, at, event.clone());
            assert_eq!(check_order(None, &next), Ok(()), "{name} at seq {seq}");
            assert_eq!(
                check_order(Some(&ahead), &next),
                REFUSED,
                "{name} not first"
            );
        }
    }
}

/// DEC-659 and DEC-657 item 4: a record with a higher `seq` and an `event_time` not before the
/// last record's passes, an equal `event_time` too, since only "before" is out of order; one
/// nanosecond earlier, the same record is refused.
#[test]
#[ignore = "pending E9-7"]
fn a_later_or_equal_event_time_with_a_higher_seq_passes() {
    let event = || Event::Removed {
        member: PrincipalId(B),
    };
    for (seq, at) in [(5, 3_600), (5, 3_601), (9, 3_600), (9, 90_000)] {
        let next = record(seq, at, event());
        assert_eq!(
            check_order(Some(&last()), &next),
            Ok(()),
            "seq {seq} at {at}"
        );
    }
    let early = UtcNanos::from_parts(EPOCH + 3_599, 999_999_999).unwrap();
    let next = MembershipRecord::new(Seal::grant(), 5, early, event());
    assert_eq!(check_order(Some(&last()), &next), REFUSED, "1 ns before");
    let within = |nanos| UtcNanos::from_parts(EPOCH + 3_600, nanos).unwrap();
    let same_second = MembershipRecord::new(Seal::grant(), 4, within(500), event());
    let earlier = MembershipRecord::new(Seal::grant(), 5, within(499), event());
    let later = MembershipRecord::new(Seal::grant(), 5, within(501), event());
    assert_eq!(
        check_order(Some(&same_second), &earlier),
        REFUSED,
        "1 ns before, same second"
    );
    assert_eq!(
        check_order(Some(&same_second), &later),
        Ok(()),
        "1 ns after, same second"
    );
}

/// DEC-659: an `event_time` before the last record's is refused, even by one nanosecond and even
/// with a higher `seq`, so a late record never latches the fold unreadable after commit; at the
/// last record's own instant it passes.
#[test]
#[ignore = "pending E9-7"]
fn an_earlier_event_time_is_refused() {
    let event = || Event::Deactivated {
        member: PrincipalId(B),
    };
    for (seq, at) in [(5, 3_599), (9, 0), (u64::MAX, -86_400)] {
        let next = record(seq, at, event());
        assert_eq!(
            check_order(Some(&last()), &next),
            REFUSED,
            "seq {seq} at {at}"
        );
        let on_time = record(seq, 3_600, event());
        assert_eq!(check_order(Some(&last()), &on_time), Ok(()), "seq {seq}");
    }
    let one_ns_early = UtcNanos::from_parts(EPOCH + 3_599, 999_999_999).unwrap();
    let next = MembershipRecord::new(Seal::grant(), 5, one_ns_early, event());
    assert_eq!(check_order(Some(&last()), &next), REFUSED, "1 ns before");
    let within = |nanos| UtcNanos::from_parts(EPOCH + 3_600, nanos).unwrap();
    let same_second = MembershipRecord::new(Seal::grant(), 4, within(500), event());
    let earlier = MembershipRecord::new(Seal::grant(), 5, within(499), event());
    let later = MembershipRecord::new(Seal::grant(), 5, within(501), event());
    assert_eq!(
        check_order(Some(&same_second), &earlier),
        REFUSED,
        "1 ns before, same second"
    );
    assert_eq!(
        check_order(Some(&same_second), &later),
        Ok(()),
        "1 ns after, same second"
    );
}

/// DEC-659: a `seq` equal to or below the last record's is refused, whatever its `event_time`;
/// the next `seq` at the same instant passes.
#[test]
#[ignore = "pending E9-7"]
fn an_equal_or_lower_seq_is_refused() {
    let event = || invited(2, set(VIEWER), 7_200);
    for (seq, at) in [(4, 3_600), (4, 7_200), (3, 7_200), (0, 90_000)] {
        let next = record(seq, at, event());
        assert_eq!(
            check_order(Some(&last()), &next),
            REFUSED,
            "seq {seq} at {at}"
        );
        let next_seq = record(5, at, event());
        assert_eq!(
            check_order(Some(&last()), &next_seq),
            Ok(()),
            "seq 5 at {at}"
        );
    }
    let top = record(u64::MAX, 0, event());
    let again = record(u64::MAX, 0, event());
    assert_eq!(
        check_order(Some(&top), &again),
        REFUSED,
        "no seq above u64::MAX"
    );
}

/// DEC-659: the guard applies to all seven membership record types, as last record and as next.
#[test]
#[ignore = "pending E9-7"]
fn every_membership_record_type_is_guarded() {
    for (before, b) in every_type() {
        for (after, a) in every_type() {
            let last = record(4, 3_600, b.clone());
            let check = |seq, at| check_order(Some(&last), &record(seq, at, a.clone()));
            assert_eq!(check(5, 3_600), Ok(()), "{after} after {before}");
            assert_eq!(
                check(5, 3_599),
                REFUSED,
                "{after} back in time after {before}"
            );
            assert_eq!(
                check(4, 3_600),
                REFUSED,
                "{after} at the same seq as {before}"
            );
        }
    }
}

/// DEC-659 with DEC-657 item 4 as its backstop: after the founding grant and an invitation at
/// `seq` 2, instant 3_600, a second invitation the guard passes leaves the fold readable, and one
/// it refuses is the record the fold would read as out of order, so a writer that obeys the guard
/// never latches the fold unreadable by order.
#[test]
#[ignore = "pending E9-7"]
fn a_record_the_guard_passes_is_one_the_fold_reads_in_order() {
    let base = || vec![founding(), (3_600, invited(1, set(VIEWER), 3_600))];
    assert!(
        !fold(base()).unreadable(t(90_000)),
        "the base history is readable"
    );
    let last = record(2, 3_600, invited(1, set(VIEWER), 3_600));
    for (seq, at, passes) in [
        (3, 3_600, true),
        (3, 7_200, true),
        (3, 3_599, false),
        (2, 7_200, false),
    ] {
        let candidate = invited(2, set(VIEWER), at);
        let guarded = check_order(Some(&last), &record(seq, at, candidate.clone()));
        assert_eq!(
            guarded,
            if passes { Ok(()) } else { REFUSED },
            "seq {seq} at {at}"
        );
        let mut history: Vec<_> = base()
            .into_iter()
            .zip(1..)
            .map(|((a, e), q)| (q, a, e))
            .collect();
        history.push((seq, at, candidate));
        let unreadable = sequenced(history).unreadable(t(90_000));
        assert_eq!(
            unreadable, !passes,
            "the fold agrees at seq {seq}, instant {at}"
        );
    }
}

/// The oracle, written apart from the crate: the gaps in `seq` and in whole nanoseconds, as
/// signed integers wide enough that neither wraps; the next record must be at least one `seq`
/// on and no nanosecond back.
fn oracle(last: Option<(u64, i64, u32)>, next: (u64, i64, u32)) -> bool {
    let nanos = |secs: i64, ns: u32| i128::from(secs) * 1_000_000_000 + i128::from(ns);
    last.is_none_or(|(seq, secs, ns)| {
        let seq_gap = i128::from(next.0) - i128::from(seq);
        let time_gap = nanos(next.1, next.2) - nanos(secs, ns);
        seq_gap >= 1 && time_gap >= 0
    })
}

/// DEC-659 over random pairs of records of every type, near each boundary: `seq`s around 0 and
/// `u64::MAX`, and instants a second or a nanosecond apart, against [`oracle`].
#[test]
#[ignore = "pending E9-7"]
fn the_guard_equals_an_independent_oracle_over_random_pairs() {
    let seq = (0..2usize, 0..4u64).prop_map(|(b, o)| [0, u64::MAX - 3][b] + o);
    let nanos = (0..3usize).prop_map(|i| [0, 1, 999_999_999][i]);
    let side = (seq, 0..3i64, nanos, 0..7usize);
    let strategy = (proptest::bool::weighted(0.9), side.clone(), side);
    let config = ProptestConfig {
        cases: 1_024,
        failure_persistence: None,
        ..Default::default()
    };
    let build = |(seq, secs, ns, kind): (u64, i64, u32, usize)| {
        let at = UtcNanos::from_parts(EPOCH + secs, ns).unwrap();
        let event = every_type()[kind].1.clone();
        MembershipRecord::new(Seal::grant(), seq, at, event)
    };
    type Side = (u64, i64, u32, usize);
    let body = |(has_last, l, n): (bool, Side, Side)| -> Result<(), TestCaseError> {
        let expected = oracle(has_last.then_some((l.0, l.1, l.2)), (n.0, n.1, n.2));
        let last = has_last.then(|| build(l));
        let got = check_order(last.as_ref(), &build(n));
        let want = if expected { Ok(()) } else { REFUSED };
        prop_assert_eq!(got, want, "last {:?} next {:?}", has_last.then_some(l), n);
        Ok(())
    };
    if let Err(failure) = TestRunner::new(config).run(&strategy, body) {
        panic!("{failure}");
    }
}

/// The oracle itself, against literal cases, so the property never trusts an oracle that agrees
/// with everything.
#[test]
fn the_oracle_refuses_what_the_rule_refuses() {
    assert!(oracle(None, (0, 0, 0)));
    assert!(oracle(Some((4, 1, 0)), (5, 1, 0)));
    assert!(!oracle(Some((4, 1, 0)), (5, 0, 999_999_999)));
    assert!(!oracle(Some((4, 1, 0)), (4, 2, 0)));
    assert!(!oracle(Some((u64::MAX, 0, 0)), (u64::MAX, 0, 0)));
}
