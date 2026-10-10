//! E12-3 run logic, the plan stage's stream and range refusals (workspace API §4.8.1
//! "Verification", AU-1, AU-8; DEC-788 item 4). The oracles are §4.8.1's rules written out here;
//! none calls the code under test to decide what it should return. The trusted start and the cold
//! store are `verification_plan_start.rs`'s.

mod common;

use common::{WS_A, WS_B, permitted, tenant, text};
use mandate_audit::{
    ColdSource, ControlSnapshot, MAX_RUN_EVENTS as MAX, RecordedStart, VerificationRefusal,
    VerificationRequest, plan,
};
use mandate_canon::Digest;
use mandate_journal::{ColdRead, StartRequest, TrustedStart, TrustedStartError};
use proptest::prelude::*;
use proptest::test_runner::{RngAlgorithm, TestCaseError, TestRng, TestRunner};

const RANGE: VerificationRefusal = VerificationRefusal::Range;
const REFUSED: VerificationRefusal = VerificationRefusal::TrustedStart(TrustedStartError::Refused);

type Seen = Result<(u64, TrustedStart, RecordedStart), VerificationRefusal>;

/// A cold store that cannot answer: a genesis start never asks it.
struct NoCold;

impl ColdSource for NoCold {
    fn manifest(&self, _: &Digest) -> ColdRead {
        ColdRead::Unreadable
    }
}

/// `WS_A`'s genesis plan of `stream` over `from..=to`, at stream head `head`, over no control row.
fn run(stream: &str, (from_seq, to_seq): (u64, Option<u64>), head: Option<u64>) -> Seen {
    let ctx = tenant(WS_A);
    let trusted_start = StartRequest::Genesis;
    let request = VerificationRequest {
        stream_id: stream,
        from_seq,
        to_seq,
        trusted_start,
    };
    let snapshot = ControlSnapshot {
        stream_head: head,
        control: &[],
    };
    let planned = plan(&permitted(&ctx), &request, &snapshot, &NoCold);
    planned.map(|p| (p.to_seq(), p.start(), p.recorded_start().clone()))
}

fn own() -> String {
    format!("acct:{}:ACCT1", text(WS_A))
}

/// AU-1, §4.8.1 refusal step 4, DEC-760 item 1: another workspace's stream (`WS_B`'s, and one whose
/// segment has `WS_A`'s as a prefix), an own stream that holds no event, and malformed ids are all
/// the one `NotFound`, while the same request on an own stream that holds events is planned.
#[test]
#[ignore = "pending E12-3"]
fn foreign_absent_and_malformed_streams_refuse_alike() {
    let w = text(WS_A);
    let refused = [
        (format!("acct:{}:ACCT1", text(WS_B)), Some(5)),
        (format!("acct:{w}0:ACCT1"), Some(5)),
        (own(), None),
        (format!("acct:{w}"), Some(5)),
        (format!("acct:{w}:ACCT1:x"), Some(5)),
        (format!("acct:{w}:AC CT1"), Some(5)),
        ("acct::ACCT1".to_owned(), Some(5)),
        (String::new(), Some(5)),
    ];
    for (stream, head) in &refused {
        match run(stream, (1, Some(1)), *head) {
            Err(VerificationRefusal::NotFound) => {}
            other => panic!("{stream:?} at head {head:?}: the one NotFound, got {other:?}"),
        }
    }
    let planned = run(&own(), (1, Some(1)), Some(5));
    assert_eq!(
        planned,
        Ok((1, TrustedStart::GENESIS, RecordedStart::Genesis))
    );
}

/// §4.8.1 refusal step 5 (DEC-897 item 2 for `from_seq` 0): `to_seq` `None` is the head at start;
/// `from_seq` 0, a `to_seq` below `from_seq` and one above the head are `Range`; any other range
/// reaches the trusted start, which a genesis start passes only from seq 1. Every boundary is a
/// written row, then a seeded property draws the rest.
#[test]
#[ignore = "pending E12-3"]
fn range_refusals_follow_the_head_at_start() {
    let genesis = |to| Ok((to, TrustedStart::GENESIS, RecordedStart::Genesis));
    let rows = [
        (7, (1, Some(7)), genesis(7)),
        (7, (1, Some(8)), Err(RANGE)),
        (1, (1, None), genesis(1)),
        (1, (2, None), Err(RANGE)),
        (7, (7, Some(7)), Err(REFUSED)),
        (7, (8, Some(8)), Err(RANGE)),
        (7, (1, Some(1)), genesis(1)),
        (7, (1, Some(0)), Err(RANGE)),
        (7, (5, Some(5)), Err(REFUSED)),
        (7, (5, Some(4)), Err(RANGE)),
        (7, (0, Some(3)), Err(RANGE)),
    ];
    for (head, span, want) in rows {
        assert_eq!(run(&own(), span, Some(head)), want, "head {head}, {span:?}");
    }
    let strategy = (1..40_u64, 0..45_u64, proptest::option::of(0..45_u64));
    let body = |(head, from, to): (u64, u64, Option<u64>)| -> Result<(), TestCaseError> {
        let last = to.unwrap_or(head);
        let want = match from {
            _ if from == 0 || last < from || last > head => Err(RANGE),
            1 => Ok((last, TrustedStart::GENESIS, RecordedStart::Genesis)),
            _ => Err(REFUSED),
        };
        let got = run(&own(), (from, to), Some(head));
        let why = format!("head {head}, {from}..{to:?}: want {want:?}, got {got:?}");
        if got == want {
            Ok(())
        } else {
            Err(TestCaseError::fail(why))
        }
    };
    let config = ProptestConfig {
        cases: 256,
        failure_persistence: None,
        ..ProptestConfig::default()
    };
    let rng = TestRng::from_seed(RngAlgorithm::ChaCha, &[7; 32]);
    if let Err(failure) = TestRunner::new_with_rng(config, rng).run(&strategy, body) {
        panic!("{failure}");
    }
}

/// §4.8.1 "One run covers at most 1,000,000 events", DEC-787's "the bound applies to the range
/// only": 1,000,000 events pass the bound and 1,000,001 are `Range`, from seq 1 or after a prefix
/// of 2,000,000, with `to_seq` given or the head. A range that passes it meets the trusted start.
#[test]
#[ignore = "pending E12-3"]
fn size_bound_counts_the_range_only() {
    let cases = [
        (
            (1, Some(MAX)),
            Ok((MAX, TrustedStart::GENESIS, RecordedStart::Genesis)),
        ),
        ((1, Some(MAX + 1)), Err(RANGE)),
        ((1, None), Err(RANGE)),
        ((2 * MAX + 1, Some(3 * MAX)), Err(REFUSED)),
        ((2 * MAX + 1, None), Err(REFUSED)),
        ((2 * MAX, None), Err(RANGE)),
        ((2 * MAX, Some(3 * MAX)), Err(RANGE)),
    ];
    for (span, want) in cases {
        let got = run(&own(), span, Some(3 * MAX));
        assert_eq!(got, want, "{span:?}");
    }
}
