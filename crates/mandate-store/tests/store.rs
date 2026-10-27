//! The journal port over the in-memory journal, and the envelope a long-running writer wraps each
//! draft in (E21-29, E21-30; DEC-1012; journal spec §3, §5.1): ownership fences, a draft the
//! journal accepts reads back as written, and `artifact_refs` lists every digest the payload names.

use mandate_canon::{Digest, Object, Value, parse};
use mandate_journal::{AppendOutcome, Environment, StreamId};
use mandate_store::envelope::{Envelope, Fields, draft_bytes, object, text};
use mandate_store::{MemoryStore, Store};
use mandate_time::UtcNanos;

/// 2026-10-16T13:00:00Z.
const T: i64 = 1_792_155_600;

fn stream() -> StreamId {
    StreamId::parse("clock:ws_1").unwrap()
}

fn envelope() -> Envelope<'static> {
    Envelope {
        stream: "clock:ws_1",
        environment: Environment::Paper,
        actor_kind: "system",
        actor_id: "scheduler",
        build: "mandate-store-tests/0",
        version: "0",
        event_time: UtcNanos::from_parts(T, 0).unwrap(),
    }
}

/// The scheduler stream's `StreamOpened` at seq 1 under `epoch` (journal spec §9.20).
fn opened(epoch: u64) -> Vec<u8> {
    let payload = object(vec![
        ("stream_type", text("scheduler")),
        ("workspace_id", text("ws_1")),
    ])
    .unwrap();
    let event_id = format!("2{epoch:05}{:014}{:06}", 0, 0);
    draft_bytes(
        &envelope(),
        &Fields {
            event_id: &event_id,
            event_type: "StreamOpened",
            schema_version: 1,
            causation_id: None,
            correlation_id: None,
            config_refs: &Object::new(),
            payload: &payload,
        },
    )
    .unwrap()
}

#[test]
fn each_owner_takes_a_newer_epoch_than_the_last() {
    let mut first = MemoryStore::default();
    let mut second = first.clone();
    let taken = first.take_ownership(&stream()).unwrap();
    let newer = second.take_ownership(&stream()).unwrap();
    assert!(taken >= 1, "an owner holds epoch {taken}");
    assert!(newer > taken, "{newer} fences {taken}");
}

#[test]
fn a_draft_the_journal_accepts_reads_back_as_written() {
    let mut store = MemoryStore::default();
    let epoch = store.take_ownership(&stream()).unwrap();
    let draft = opened(epoch);
    let at = UtcNanos::from_parts(T, 0).unwrap();
    let outcome = store.append(&stream(), 0, epoch, at, std::slice::from_ref(&draft));
    assert!(
        matches!(outcome, AppendOutcome::Committed(_)),
        "{outcome:?}"
    );
    let rows = store.clone().rows(&stream()).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].event_type, "StreamOpened");
    let (stored, drafted) = (parse(&rows[0].body).unwrap(), parse(&draft).unwrap());
    for member in ["event_id", "payload", "actor", "event_time"] {
        assert_eq!(stored.get(member), drafted.get(member), "{member}");
    }
}

#[test]
fn the_envelope_lists_every_digest_the_payload_names_once_in_order() {
    let a = format!("sha256:{}", Digest::of(b"a").to_hex());
    let b = format!("sha256:{}", Digest::of(b"b").to_hex());
    let payload = object(vec![
        ("first", Value::Str(b.clone())),
        (
            "nested",
            object(vec![(
                "list",
                Value::Array(vec![
                    Value::Str(a.clone()),
                    Value::Str(b.clone()),
                    text("sha256:not-a-digest"),
                    Value::Null,
                    Value::Bool(true),
                ]),
            )])
            .unwrap(),
        ),
    ])
    .unwrap();
    let bytes = draft_bytes(
        &envelope(),
        &Fields {
            event_id: "200001000000000000000000",
            event_type: "StreamOpened",
            schema_version: 1,
            causation_id: Some("01J8Z3M4000000000000000001"),
            correlation_id: None,
            config_refs: &Object::new(),
            payload: &payload,
        },
    )
    .unwrap();
    let body = parse(&bytes).unwrap();
    let mut expected = vec![a, b];
    expected.sort();
    assert_eq!(
        body.get("artifact_refs"),
        Some(&Value::Array(
            expected.into_iter().map(Value::Str).collect()
        ))
    );
    assert_eq!(
        body.get("causation_id"),
        Some(&text("01J8Z3M4000000000000000001"))
    );
    assert_eq!(body.get("clock_source"), Some(&text("scheduler")));
    let build = format!("sha256:{}", Digest::of(b"mandate-store-tests/0").to_hex());
    assert_eq!(
        body.get("actor").and_then(|a| a.get("build")),
        Some(&Value::Str(build))
    );
}

/// A reader past a head it holds reads only the records appended since, and none when its held
/// head is not the journal's: it must read the whole stream again rather than fold a fork.
#[test]
fn rows_from_a_held_head_read_only_what_follows_it() {
    let mut store = MemoryStore::default();
    let epoch = store.take_ownership(&stream()).unwrap();
    let committed = store.append(
        &stream(),
        0,
        epoch,
        UtcNanos::from_parts(T, 0).unwrap(),
        &[opened(epoch)],
    );
    assert!(
        matches!(committed, AppendOutcome::Committed(_)),
        "{committed:?}"
    );
    let all = store.rows(&stream()).unwrap();
    let head = all.last().unwrap();
    let past = mandate_journal::TrustedStart {
        from_seq: head.seq + 1,
        prev_hash: head.hash,
    };
    assert_eq!(store.rows_from(&stream(), past), Some(Vec::new()));
    assert_eq!(
        store.rows_from(&stream(), mandate_journal::TrustedStart::GENESIS),
        Some(all.clone())
    );
    let forked = mandate_journal::TrustedStart {
        from_seq: 1,
        prev_hash: Digest::of(b"another chain"),
    };
    assert_eq!(store.rows_from(&stream(), forked), None);
    let skipped = mandate_journal::TrustedStart {
        from_seq: 0,
        prev_hash: Digest::ZERO,
    };
    assert_eq!(
        store.rows_from(&stream(), skipped),
        None,
        "a seq the stream never held"
    );
}
