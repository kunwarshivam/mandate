//! DEC-2819: `replay_theses` folds an agent stream's own thesis records as the admission judged
//! them, never judging again, into the working universe, the lineages, the surviving theses and
//! every instrument the stream ever admitted. The daemon's research source and the executor read
//! the same replay. Every expected value is written out from mandate spec §8.5 and §8.6 by hand.

mod common;

use std::collections::BTreeSet;

use common::{INSTRUMENT_1, INSTRUMENT_2, INSTRUMENT_3, asset, lineage_id, thesis_id};
use mandate_canon::Value;
use mandate_research::{ReplayError, ThesisRecord, UniverseEntry, WorkingUniverse, replay_theses};
use mandate_time::UtcNanos;

const T: i64 = 1_792_000_000;

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).expect("a fixture instant")
}

fn stamp(secs: i64) -> String {
    at(secs).to_string()
}

/// A thesis record's payload: the members the replay reads, each as the journal spec types it.
fn thesis(
    thesis: &str,
    lineage: &str,
    revision: u32,
    instrument: &str,
    reason: Option<&str>,
    expires: i64,
) -> Value {
    let (admitted, reason) = match reason {
        None => ("true", "null".to_owned()),
        Some(code) => ("false", format!(r#""{code}""#)),
    };
    let text = format!(
        r#"{{"thesis_id":"{thesis}","lineage_id":"{lineage}","revision":{revision},"instrument_id":"{instrument}","expires_at":"{}","admitted":{admitted},"reason":{reason}}}"#,
        stamp(expires)
    );
    mandate_canon::parse(text.as_bytes()).expect("a fixture payload")
}

fn invalidated(thesis: &str) -> Value {
    mandate_canon::parse(format!(r#"{{"thesis_id":"{thesis}"}}"#).as_bytes())
        .expect("a fixture payload")
}

fn kind(revision: u32) -> &'static str {
    if revision == 0 {
        "ThesisProposed"
    } else {
        "ThesisRevised"
    }
}

fn records<'a>(rows: &'a [(&'static str, Value)]) -> Vec<ThesisRecord<'a>> {
    rows.iter()
        .map(|(event_type, payload)| ThesisRecord {
            event_type,
            payload,
        })
        .collect()
}

fn universe(ids: &[&str]) -> WorkingUniverse {
    WorkingUniverse::Known {
        instruments: ids.iter().map(|id| asset(id)).collect(),
        pinned: false,
    }
}

fn ever(ids: &[&str]) -> BTreeSet<mandate_research::AssetId> {
    ids.iter().map(|id| asset(id)).collect()
}

/// Every instrument the stream admitted stays in `admitted` after its thesis ends, whether it
/// expired, was invalidated, or lost its instrument to another lineage's thesis, while the working
/// universe holds only the surviving theses' instruments.
#[test]
fn an_instrument_once_admitted_stays_admitted_after_its_thesis_ends() {
    let rows = [
        (
            "ThesisProposed",
            thesis("th-1", "th-1", 0, INSTRUMENT_1, None, T + 3_600),
        ),
        (
            "ThesisProposed",
            thesis("th-2", "th-2", 0, INSTRUMENT_2, None, T + 600),
        ),
        (
            "ThesisProposed",
            thesis("th-3", "th-3", 0, INSTRUMENT_3, None, T + 7_200),
        ),
        ("ThesisInvalidated", invalidated("th-3")),
    ];
    let replayed = replay_theses(records(&rows), at(T + 1_200)).expect("it replays");
    assert_eq!(replayed.universe, universe(&[INSTRUMENT_1]));
    assert_eq!(
        replayed.active,
        [UniverseEntry {
            instrument: asset(INSTRUMENT_1),
            thesis_id: thesis_id("th-1"),
            lineage_id: lineage_id("th-1"),
            revision: 0,
            expires_at: at(T + 3_600),
            invalidated: false,
        }]
    );
    assert_eq!(
        replayed.admitted,
        ever(&[INSTRUMENT_1, INSTRUMENT_2, INSTRUMENT_3]),
        "expired and invalidated instruments were admitted all the same"
    );
}

/// A refused thesis admits nothing, so its instrument is never in `admitted`; a lineage-retiring
/// refusal removes the instrument its lineage held, which stays admitted.
#[test]
fn a_refusal_admits_nothing_and_a_retirement_removes_what_its_lineage_held() {
    let rows = [
        (
            "ThesisProposed",
            thesis("th-1", "th-1", 0, INSTRUMENT_1, None, T + 3_600),
        ),
        (
            "ThesisProposed",
            thesis(
                "th-9",
                "th-9",
                0,
                INSTRUMENT_2,
                Some("eligibility_floor"),
                T + 3_600,
            ),
        ),
        (
            kind(1),
            thesis(
                "th-1b",
                "th-1",
                1,
                INSTRUMENT_1,
                Some("lineage_retired"),
                T + 3_600,
            ),
        ),
    ];
    let replayed = replay_theses(records(&rows), at(T)).expect("it replays");
    assert_eq!(replayed.universe, universe(&[]));
    assert!(replayed.active.is_empty());
    assert_eq!(replayed.admitted, ever(&[INSTRUMENT_1]));
    assert!(
        replayed
            .lineages
            .lineage(&lineage_id("th-1"))
            .is_some_and(|lineage| lineage.retired)
    );
}

/// Records of any other kind are passed over; a thesis record or an invalidation the replay cannot
/// read gives no replay, naming the member, so a reader holds rather than guess.
#[test]
fn another_record_is_passed_over_and_an_unreadable_one_gives_nothing() {
    let other = mandate_canon::parse(br#"{"anything":1}"#).expect("a payload");
    let rows = [
        ("DecisionMade", other.clone()),
        (
            "ThesisProposed",
            thesis("th-1", "th-1", 0, INSTRUMENT_1, None, T + 3_600),
        ),
    ];
    let replayed = replay_theses(records(&rows), at(T)).expect("it replays");
    assert_eq!(replayed.admitted, ever(&[INSTRUMENT_1]));
    for (event_type, payload, member) in [
        ("ThesisProposed", other.clone(), "thesis_id"),
        ("ThesisInvalidated", other.clone(), "thesis_id"),
        (
            "ThesisProposed",
            mandate_canon::parse(
                br#"{"thesis_id":"th-1","lineage_id":"th-1","revision":0,"instrument_id":"x","expires_at":"2026-10-16T15:00:00Z","admitted":"yes","reason":null}"#,
            )
            .expect("a payload"),
            "admitted",
        ),
        (
            "ThesisProposed",
            mandate_canon::parse(
                br#"{"thesis_id":"th-1","lineage_id":"th-1","revision":0,"instrument_id":"x","expires_at":"2026-10-16T15:00:00Z","admitted":true,"reason":"eligibility_floor"}"#,
            )
            .expect("a payload"),
            "reason",
        ),
    ] {
        let rows = [(event_type, payload)];
        assert_eq!(
            replay_theses(records(&rows), at(T)),
            Err(ReplayError::Unreadable { member }),
            "{event_type}"
        );
    }
}
