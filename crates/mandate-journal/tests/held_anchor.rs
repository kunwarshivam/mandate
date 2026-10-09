//! E12-3 (journal spec v0.36 §11 `held_mismatch`, DEC-787 item 7, DEC-892): the hold anchor of an
//! agent-stream range, folded from the stored prefix `seq` 1 to `from_seq − 1`. A test oracle
//! derives each prefix's anchor from §11's expected-hold rule, and every split of every chain
//! (the `hold` and `agent_stream` vectors, and random chains) is checked against the full-chain
//! run: anchored on a valid prefix the range reports what the full chain reports, and a prefix the
//! full chain fails anchors nothing (DEC-885 I1 and I5's analogues).

use std::path::Path;

use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_journal::{
    AgentStreamCheck, HeldAnchor, StoredEvent, TrustedStart, held_anchor,
    verify_agent_stream_anchored,
};
use proptest::prelude::*;
use proptest::test_runner::TestRunner;

const AGENT: &str = "agent:ws_01J8Z2:agent_a";

fn section(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    parse(&std::fs::read(path).unwrap())
        .unwrap()
        .get(name)
        .cloned()
        .unwrap()
}

fn list<'a>(value: &'a Value, name: &str) -> &'a [Value] {
    value
        .get(name)
        .and_then(Value::as_array)
        .unwrap_or_default()
}

fn text<'a>(value: &'a Value, name: &str) -> &'a str {
    value.get(name).and_then(Value::as_str).unwrap_or_default()
}

/// The stored row at `seq` of `stream` whose body is `body`; its columns are read from the body.
fn row_of(stream: &str, seq: u64, body: &Value) -> StoredEvent {
    let bytes = to_canonical(body);
    let id = text(body, "event_id");
    StoredEvent {
        stream_id: stream.to_owned(),
        seq,
        event_id: if id.is_empty() {
            format!("01J8Z6R0A{seq:017}")
        } else {
            id.to_owned()
        },
        event_type: text(body, "event_type").to_owned(),
        schema_version: body.get("schema_version").and_then(Value::as_int).unwrap(),
        environment: "paper".to_owned(),
        recorded_at: "2026-10-09T12:00:00.000000000Z".to_owned(),
        prev_hash: Digest::ZERO,
        hash: Digest::of(&bytes),
        body: bytes,
    }
}

/// An `AgentModeChanged` body at `version` with `reason` and, at version 2, `held`.
fn mode(version: u64, reason: &str, held: bool) -> Value {
    let held = if version == 2 {
        format!(r#","held":{held}"#)
    } else {
        String::new()
    };
    let payload = format!(
        r#"{{"from":"normal","to":"paused","reason":"{reason}","lifecycle":"normal"{held}}}"#
    );
    let body = format!(
        r#"{{"event_type":"AgentModeChanged","schema_version":{version},"payload":{payload}}}"#
    );
    parse(body.as_bytes()).unwrap()
}

fn other() -> Value {
    parse(br#"{"event_type":"IntentProposed","schema_version":1,"payload":{}}"#).unwrap()
}

fn chain(bodies: &[Value]) -> Vec<StoredEvent> {
    bodies
        .iter()
        .zip(1..)
        .map(|(body, seq)| row_of(AGENT, seq, body))
        .collect()
}

/// Every full agent chain the vectors hold: the `hold` ranges from seq 1 and `agent_stream`'s chain.
fn vector_chains() -> Vec<Vec<StoredEvent>> {
    let hold = section("hold");
    let mut chains: Vec<Vec<StoredEvent>> = list(&hold, "range_verification")
        .iter()
        .filter(|case| case.get("from_seq").and_then(Value::as_int) == Some(1))
        .map(|case| chain(list(case, "events")))
        .collect();
    let agent = section("agent_stream");
    chains.push(chain(
        &list(&agent, "chain")
            .iter()
            .map(|e| e.get("body").cloned().unwrap())
            .collect::<Vec<_>>(),
    ));
    assert!(chains.len() >= 14, "{} chains", chains.len());
    chains
}

/// The oracle, from §11's words: a hold sets the carried hold, a lift clears it, and every other
/// record keeps it; a written `held` that differs from that, or a version 1 after a version 2,
/// breaks the prefix, and so does a prefix that is not one agent stream's seq 1 to k.
fn oracle(prefix: &[StoredEvent]) -> HeldAnchor {
    let readable = prefix.iter().zip(1..).all(|(r, seq)| {
        r.seq == seq && r.stream_id == prefix[0].stream_id && r.stream_id.starts_with("agent:")
    });
    if !readable {
        return HeldAnchor::Unknown;
    }
    let (mut carried, mut last_v2) = (false, None);
    for r in prefix.iter().filter(|r| r.event_type == "AgentModeChanged") {
        let Ok(body) = parse(&r.body) else {
            return HeldAnchor::Unknown;
        };
        let payload = body.get("payload").cloned().unwrap_or(Value::Null);
        match (r.schema_version, last_v2) {
            (2, _) => {}
            (_, None) => continue,
            _ => return HeldAnchor::Unknown,
        }
        let expected = match text(&payload, "reason") {
            "owner_hold" => true,
            "owner_lift_hold" => false,
            _ => carried,
        };
        if (payload.get("held") == Some(&Value::Bool(true))) != expected {
            return HeldAnchor::Unknown;
        }
        (carried, last_v2) = (expected, Some(r.seq));
    }
    last_v2.map_or(HeldAnchor::NoVersionTwo, |seq| HeldAnchor::Carried {
        seq,
        held: carried,
    })
}

fn run(rows: &[StoredEvent], from_seq: u64, anchor: HeldAnchor) -> Option<(u64, AgentStreamCheck)> {
    let start = TrustedStart {
        from_seq,
        prev_hash: Digest::ZERO,
    };
    verify_agent_stream_anchored(rows, start, anchor)
        .err()
        .map(|f| (f.seq, f.check))
}

/// Every split `k` of `rows`, `k` = 0 included: the prefix anchors as the oracle says, a prefix
/// the full chain fails within anchors nothing, and otherwise the range from `k + 1` reports
/// exactly what the full chain reports.
fn splits_agree(rows: &[StoredEvent]) -> Result<(), String> {
    let full = run(rows, 1, HeldAnchor::Unknown);
    for k in 0..=rows.len() {
        let (prefix, range) = rows.split_at(k);
        let anchor = held_anchor(prefix).map_err(|e| format!("{e:?}"))?;
        let seq = u64::try_from(k).unwrap();
        let broken =
            full.is_some_and(|(at, check)| at <= seq && check == AgentStreamCheck::HeldMismatch);
        let ranged = run(range, seq + 1, anchor);
        let fine = anchor == oracle(prefix) && broken == (anchor == HeldAnchor::Unknown);
        if !(fine && (broken || ranged == full)) {
            return Err(format!(
                "split {k}: anchor {anchor:?}, oracle {:?}, range {ranged:?}, full {full:?}",
                oracle(prefix)
            ));
        }
    }
    Ok(())
}

#[test]
#[ignore = "pending E12-3"]
fn a_valid_prefix_carries_the_expected_hold_at_its_last_version_2() {
    let (hold, lift) = (
        mode(2, "owner_hold", true),
        mode(2, "owner_lift_hold", false),
    );
    let cases = [
        (vec![], HeldAnchor::NoVersionTwo),
        (
            vec![other(), mode(1, "owner_pause", false)],
            HeldAnchor::NoVersionTwo,
        ),
        (
            vec![hold.clone(), other()],
            HeldAnchor::Carried { seq: 1, held: true },
        ),
        (
            vec![hold.clone(), mode(2, "owner_pause", true)],
            HeldAnchor::Carried { seq: 2, held: true },
        ),
        (
            vec![hold, lift, other()],
            HeldAnchor::Carried {
                seq: 2,
                held: false,
            },
        ),
        (
            vec![mode(2, "restriction_changed", false)],
            HeldAnchor::Carried {
                seq: 1,
                held: false,
            },
        ),
    ];
    for (bodies, want) in cases {
        assert_eq!(held_anchor(&chain(&bodies)), Ok(want), "{bodies:?}");
    }
}

#[test]
#[ignore = "pending E12-3"]
fn a_prefix_that_breaks_the_hold_or_is_not_a_prefix_anchors_nothing() {
    let hold = mode(2, "owner_hold", true);
    let mut cases: Vec<Vec<StoredEvent>> = [
        vec![mode(2, "owner_hold", false)],
        vec![hold.clone(), mode(2, "restriction_changed", false)],
        vec![hold.clone(), mode(2, "owner_lift_hold", true)],
        vec![hold.clone(), mode(1, "owner_pause", false)],
        vec![mode(2, "kill_switch", true)],
    ]
    .iter()
    .map(|bodies| chain(bodies))
    .collect();
    let good = chain(&[hold.clone(), other(), other()]);
    cases.push(good[1..].to_vec());
    cases.push(vec![good[0].clone(), good[2].clone()]);
    cases.push(vec![good[0].clone(), good[0].clone()]);
    cases.push(vec![good[1].clone(), good[0].clone()]);
    cases.push(vec![
        good[0].clone(),
        row_of("agent:ws_01J8Z2:agent_b", 2, &other()),
    ]);
    cases.push(vec![row_of("acct:ws_01J8Z2:A1", 1, &hold)]);
    let mut garbage = good.clone();
    garbage[0].body = b"not json".to_vec();
    cases.push(garbage);
    for prefix in cases {
        assert_eq!(held_anchor(&prefix), Ok(HeldAnchor::Unknown), "{prefix:?}");
    }
}

#[test]
#[ignore = "pending E12-3"]
fn every_split_of_every_vector_chain_agrees_with_the_full_chain() {
    for rows in vector_chains() {
        assert_eq!(
            held_anchor(&[]),
            Ok(HeldAnchor::NoVersionTwo),
            "k = 0 is genesis"
        );
        if let Err(why) = splits_agree(&rows) {
            panic!(
                "{why} in {:?}",
                rows.iter().map(|r| &r.event_type).collect::<Vec<_>>()
            );
        }
    }
}

/// Random chains of mode changes and other events, each record writing the hold the carried one
/// asks for unless its draw flips it, so valid and broken prefixes both occur.
#[test]
#[ignore = "pending E12-3"]
fn every_split_of_a_random_chain_agrees_with_the_oracle_and_the_full_chain() {
    let reasons = [
        "owner_hold",
        "owner_lift_hold",
        "owner_pause",
        "restriction_changed",
        "kill_switch",
    ];
    let record = (0usize..8, 0usize..reasons.len(), prop::bool::weighted(0.15));
    let draw = prop::collection::vec(record, 0..12);
    let outcome = TestRunner::deterministic().run(&draw, |records| {
        let mut carried = false;
        let bodies: Vec<Value> = records
            .iter()
            .map(|&(kind, reason, flip)| match kind {
                0 | 1 => other(),
                2 => mode(1, "owner_pause", false),
                _ => {
                    carried = match reasons[reason] {
                        "owner_hold" => true,
                        "owner_lift_hold" => false,
                        _ => carried,
                    };
                    mode(2, reasons[reason], carried != flip)
                }
            })
            .collect();
        splits_agree(&chain(&bodies)).map_err(TestCaseError::fail)
    });
    if let Err(failure) = outcome {
        panic!("{failure}");
    }
}
