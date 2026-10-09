//! E12-3 (journal spec v0.36 §11 `held_mismatch`, DEC-787 item 7, DEC-892): an agent-stream
//! range's hold anchor, folded only from a `VerifiedPrefix` that `bind` verified from genesis and
//! bound to the trusted start. Every split of every chain is checked against an oracle from §11's
//! words and the full-chain run. Every test calls `bind` and `held_anchor`, so fails at either stub.

use std::collections::BTreeMap;
use std::path::Path;

use mandate_canon::{Digest, Int, Key, Value, parse, to_canonical};
use mandate_journal::{
    AgentStreamCheck, EventCheck, EventFailure, HeldAnchor, PrefixError, StoredEvent, TrustedStart,
    VerifiedPrefix, held_anchor, verify_agent_stream_anchored,
};
use proptest::prelude::*;
use proptest::test_runner::TestRunner;

const AGENT: &str = "agent:ws_01J8Z2:agent_a";
const T: &str = "2026-10-09T12:00:00.000000000Z";

fn section(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get(name).cloned().unwrap()
}

fn list<'a>(value: &'a Value, name: &str) -> &'a [Value] {
    value.get(name).and_then(Value::as_array).unwrap()
}

fn text<'a>(value: &'a Value, name: &str) -> &'a str {
    value.get(name).and_then(Value::as_str).unwrap_or_default()
}

/// The row of `stream` after `before` whose body is `body` with the journal's columns set, so it
/// passes checks 1 to 6; references to artifacts are dropped, since no store is given.
fn sealed(stream: &str, before: Option<&StoredEvent>, body: &Value) -> StoredEvent {
    let (seq, prev_hash) = before.map_or((1, Digest::ZERO), |b| (b.seq + 1, b.hash));
    let mut object = body.as_object().cloned().unwrap();
    let id = match text(body, "event_id") {
        "" => format!("01J8Z6R0A{seq:017}"),
        id => id.to_owned(),
    };
    for (name, value) in [
        ("event_id", Value::Str(id)),
        ("stream_id", Value::Str(stream.to_owned())),
        ("seq", Value::Int(Int::new(seq).unwrap())),
        ("prev_hash", Value::Str(prev_hash.to_hex())),
        ("environment", Value::Str("paper".to_owned())),
        ("recorded_at", Value::Str(T.to_owned())),
        ("artifact_refs", Value::Array(vec![])),
        ("config_refs", Value::Object(BTreeMap::new())),
    ] {
        object.insert(Key::new(name).unwrap(), value);
    }
    let body = Value::Object(object);
    let bytes = to_canonical(&body);
    StoredEvent {
        stream_id: stream.to_owned(),
        seq,
        event_id: text(&body, "event_id").to_owned(),
        event_type: text(&body, "event_type").to_owned(),
        schema_version: body.get("schema_version").and_then(Value::as_int).unwrap(),
        environment: "paper".to_owned(),
        recorded_at: T.to_owned(),
        prev_hash,
        hash: Digest::of(&bytes),
        body: bytes,
    }
}

fn chain(bodies: &[Value]) -> Vec<StoredEvent> {
    let mut rows: Vec<StoredEvent> = Vec::new();
    for body in bodies {
        rows.push(sealed(AGENT, rows.last(), body));
    }
    rows
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

/// The trusted start of the range just after `prefix`.
fn start_after(prefix: &[StoredEvent]) -> TrustedStart {
    let from_seq = u64::try_from(prefix.len()).unwrap() + 1;
    TrustedStart {
        from_seq,
        prev_hash: prefix.last().map_or(Digest::ZERO, |r| r.hash),
    }
}

fn bind(rows: &[StoredEvent], start: TrustedStart) -> Result<VerifiedPrefix<'_>, PrefixError> {
    VerifiedPrefix::bind(rows, start, &BTreeMap::<Digest, Vec<u8>>::new())
}

/// The anchor a caller derives: `Unknown` when `bind` refuses, so the range fails closed.
fn anchor(rows: &[StoredEvent], start: TrustedStart) -> Result<HeldAnchor, String> {
    match bind(rows, start) {
        Ok(prefix) => held_anchor(&prefix).map_err(|e| format!("{e:?}")),
        Err(PrefixError::Unimplemented { story }) => Err(format!("Unimplemented {story}")),
        Err(_) => Ok(HeldAnchor::Unknown),
    }
}

/// The oracle, from §11's words: a hold sets the carried hold, a lift clears it, and every other
/// record keeps it; a written `held` that differs from that, or a version 1 after a version 2,
/// breaks the prefix.
fn oracle(prefix: &[StoredEvent]) -> HeldAnchor {
    let (mut carried, mut last_v2) = (false, None);
    for r in prefix.iter().filter(|r| r.event_type == "AgentModeChanged") {
        let payload = parse(&r.body).unwrap().get("payload").cloned().unwrap();
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

/// Every split `k` of `rows`, `k` = 0 included, through `bind` then `held_anchor`: the anchor is
/// the oracle's; a held failure the full chain reports at or before `k` is a prefix the oracle
/// calls broken; and from a prefix it calls sound, the range from `k + 1` reports what the full
/// chain reports, whenever the full chain's first failure is not inside the prefix.
fn splits_agree(rows: &[StoredEvent]) -> Result<(), String> {
    let run = |rows, start, anchor| {
        verify_agent_stream_anchored(rows, start, anchor)
            .err()
            .map(|f| (f.seq, f.check))
    };
    let full = run(rows, TrustedStart::GENESIS, HeldAnchor::Unknown);
    for k in 0..=rows.len() {
        let (prefix, range) = rows.split_at(k);
        let start = start_after(prefix);
        let got = anchor(prefix, start)?;
        let broken = oracle(prefix) == HeldAnchor::Unknown;
        let before = full.filter(|(at, _)| *at < start.from_seq);
        let held_before = before.is_some_and(|(_, check)| check == AgentStreamCheck::HeldMismatch);
        let ranged = run(range, start, got);
        if got != oracle(prefix)
            || (held_before && !broken)
            || (!broken && before.is_none() && ranged != full)
        {
            return Err(format!(
                "split {k}: {got:?}, range {ranged:?}, full {full:?}"
            ));
        }
    }
    Ok(())
}

fn carried(seq: u64, held: bool) -> HeldAnchor {
    HeldAnchor::Carried { seq, held }
}

/// A sound prefix carries the expected hold at its last version 2 (DEC-892 item 4); one whose hold
/// breaks (item 5), or that is not one agent stream's (item 6), binds and anchors nothing.
#[test]
#[ignore = "pending E12-3"]
fn a_prefix_anchors_the_hold_section_11_carries_or_nothing() {
    let (hold, lift) = (
        mode(2, "owner_hold", true),
        mode(2, "owner_lift_hold", false),
    );
    let pause = |held| mode(2, "owner_pause", held);
    let (v1, copy) = (
        mode(1, "owner_pause", false),
        mode(2, "restriction_changed", false),
    );
    let unknown = HeldAnchor::Unknown;
    let mut cases = vec![
        (vec![], HeldAnchor::NoVersionTwo),
        (vec![other(), v1.clone()], HeldAnchor::NoVersionTwo),
        (vec![hold.clone(), other()], carried(1, true)),
        (vec![hold.clone(), pause(true)], carried(2, true)),
        (vec![hold.clone(), lift, other()], carried(2, false)),
        (vec![copy.clone()], carried(1, false)),
        (vec![mode(2, "owner_hold", false)], unknown),
        (vec![hold.clone(), copy], unknown),
        (
            vec![hold.clone(), mode(2, "owner_lift_hold", true)],
            unknown,
        ),
        (vec![hold.clone(), v1], unknown),
        (vec![mode(2, "kill_switch", true)], unknown),
    ]
    .into_iter()
    .map(|(bodies, want)| (chain(&bodies), want))
    .collect::<Vec<_>>();
    let first = chain(std::slice::from_ref(&hold));
    let foreign = sealed("agent:ws_01J8Z2:agent_b", first.first(), &other());
    cases.push((vec![first[0].clone(), foreign], unknown));
    cases.push((vec![sealed("acct:ws_01J8Z2:A1", None, &hold)], unknown));
    for (rows, want) in cases {
        assert_eq!(anchor(&rows, start_after(&rows)), Ok(want), "{rows:?}");
    }
}

/// DEC-892 items 1 and 3: `bind` refuses a prefix that is forged, short, or not the one before the
/// trusted start, so the caller passes `Unknown`. Unverified, the forged lift below would anchor
/// `held: false` and pass a range that drops the hold.
#[test]
#[ignore = "pending E12-3"]
fn bind_refuses_a_forged_truncated_or_unbound_prefix() {
    use EventCheck::{PrevHashMismatch, RehashMismatch};
    use PrefixError::{Unbound, Unverified};
    let lift = mode(2, "owner_lift_hold", false);
    let good = chain(&[mode(2, "owner_hold", true), other()]);
    let start = start_after(&good);
    let mut forged = good.clone();
    forged[0].body = sealed(AGENT, None, &lift).body;
    let mut rehashed = forged.clone();
    rehashed[0].hash = Digest::of(&rehashed[0].body);
    let last = [good[0].clone(), sealed(AGENT, good.first(), &lift)];
    let at = |seq, check| Unverified(EventFailure { seq, check });
    let (mut wrong_hash, mut wrong_seq, g) = (start, start, &good[..]);
    (wrong_hash.prev_hash, wrong_seq.from_seq) = (Digest::of(b"x"), 4);
    let cases = [
        (&g[..0], start, Unbound, "empty, for from_seq 3"),
        (&g[..1], start, Unbound, "a truncated tail"),
        (&g[..1], TrustedStart::GENESIS, Unbound, "rows for seq 1"),
        (g, wrong_hash, Unbound, "a last hash not prev_hash"),
        (g, wrong_seq, Unbound, "a last seq not from_seq - 1"),
        (&forged, start, at(1, RehashMismatch), "forged, old hash"),
        (&rehashed, start, at(2, PrevHashMismatch), "rehashed"),
        (&last, start, Unbound, "a forged last row rehashed"),
    ];
    for (rows, start, want, name) in cases {
        assert_eq!(bind(rows, start).map(|_| ()), Err(want), "{name}");
        assert_eq!(anchor(rows, start), Ok(HeldAnchor::Unknown), "{name}");
    }
    let sound = anchor(&good, start);
    assert_eq!(sound, Ok(carried(1, true)), "the sound prefix");
}

#[test]
#[ignore = "pending E12-3"]
fn every_split_of_every_vector_chain_agrees_with_the_full_chain() {
    let hold = section("hold");
    let mut chains: Vec<Vec<StoredEvent>> = list(&hold, "range_verification")
        .iter()
        .filter(|case| case.get("from_seq").and_then(Value::as_int) == Some(1))
        .map(|case| chain(list(case, "events")))
        .collect();
    let agent = section("agent_stream");
    let bodies: Vec<Value> = list(&agent, "chain")
        .iter()
        .map(|e| e.get("body").cloned().unwrap())
        .collect();
    chains.push(chain(&bodies));
    assert!(chains.len() >= 14, "{} chains", chains.len());
    assert_eq!(
        anchor(&[], TrustedStart::GENESIS),
        Ok(HeldAnchor::NoVersionTwo),
        "k = 0"
    );
    for rows in chains {
        splits_agree(&rows).unwrap_or_else(|why| panic!("{why} in {rows:?}"));
    }
}

/// Random chains of mode changes and other events, each record writing the hold the carried one
/// asks for unless its draw flips it, so sound and broken prefixes both occur.
#[test]
#[ignore = "pending E12-3"]
fn every_split_of_a_random_chain_agrees_with_the_oracle_and_the_full_chain() {
    let reasons: Vec<&str> =
        "owner_hold owner_lift_hold owner_pause restriction_changed kill_switch"
            .split(' ')
            .collect();
    let record = (0usize..8, 0usize..reasons.len(), prop::bool::weighted(0.15));
    let outcome =
        TestRunner::deterministic().run(&prop::collection::vec(record, 0..12), |records| {
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
