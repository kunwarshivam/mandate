//! A journal read as the runtime folds it (journal spec §2, §3; DEC-155 item 2; DEC-914 item 2;
//! DEC-1012): one `FoldedEvent` per row, in the read's own order, every member from the row's own
//! body, the actor's kind mapped fail-closed, and a read with an unreadable row refused whole.

use mandate_approval::ActorKind;
use mandate_canon::Digest;
use mandate_journal::StoredEvent;
use mandate_runtime::{EventId, Seq, TailError, tailed};

const STREAM: &str = "ctl:ws1";

fn stored(seq: u64, event_type: &str, body: String) -> StoredEvent {
    StoredEvent {
        stream_id: STREAM.into(),
        seq,
        event_id: format!("01J8Z3M4{seq:018}"),
        event_type: event_type.into(),
        schema_version: 1,
        environment: "paper".into(),
        recorded_at: "2026-10-16T13:00:00.000000000Z".into(),
        prev_hash: Digest::of(b"prev"),
        hash: Digest::of(body.as_bytes()),
        body: body.into_bytes(),
    }
}

/// A row whose envelope names `actor` as its `actor` member and `causation` as its cause.
fn row(seq: u64, actor: &str, causation: &str) -> StoredEvent {
    stored(
        seq,
        "OwnerCommandIssued",
        format!(r#"{{"actor":{actor},"causation_id":{causation},"payload":{{"seq":"{seq}"}}}}"#),
    )
}

fn kind(kind: &str) -> String {
    format!(r#"{{"kind":"{kind}","id":"x","version":"1","build":null}}"#)
}

#[test]
fn each_row_is_handed_once_in_the_read_s_own_order_with_its_own_members() {
    let rows = [
        row(7, &kind("user"), r#""01J8Z3M4000000000000000003""#),
        row(3, &kind("system"), "null"),
    ];
    let handed = tailed(&rows).unwrap();
    assert_eq!(handed.len(), 2);
    assert_eq!(handed[0].seq, Seq(7), "never re-sorted");
    assert_eq!(handed[1].seq, Seq(3));
    assert_eq!(handed[0].stream, STREAM);
    assert_eq!(
        handed[0].event_id,
        EventId("01J8Z3M4000000000000000007".into())
    );
    assert_eq!(handed[0].event_type, "OwnerCommandIssued");
    assert_eq!(
        handed[0].causation_id,
        Some(EventId("01J8Z3M4000000000000000003".into()))
    );
    assert_eq!(handed[1].causation_id, None);
    assert_eq!(
        handed[0].payload.get("seq").and_then(|v| v.as_str()),
        Some("7")
    );
}

#[test]
fn every_actor_kind_folds_its_own_and_anything_else_folds_system() {
    let cases = [
        (kind("user"), ActorKind::User),
        (kind("system"), ActorKind::System),
        (kind("agent"), ActorKind::Agent),
        (kind("broker"), ActorKind::Broker),
        (kind("platform_operator"), ActorKind::PlatformOperator),
        (kind("client"), ActorKind::System),
        (kind("User"), ActorKind::System),
        (r#"{"id":"x"}"#.to_owned(), ActorKind::System),
        ("null".to_owned(), ActorKind::System),
    ];
    for (actor, expected) in cases {
        let handed = tailed(&[row(1, &actor, "null")]).unwrap();
        assert_eq!(handed[0].actor, expected, "{actor}");
    }
}

#[test]
fn a_read_with_a_row_that_cannot_be_folded_is_refused_whole() {
    let good = row(1, &kind("user"), "null");
    let unreadable = stored(2, "OwnerCommandIssued", "not json".to_owned());
    let no_payload = stored(2, "OwnerCommandIssued", r#"{"actor":null}"#.to_owned());
    assert_eq!(tailed(&[good.clone(), unreadable]), Err(TailError::Body));
    assert_eq!(tailed(&[good, no_payload]), Err(TailError::Payload));
    assert_eq!(TailError::Body.field(), "stored body");
    assert_eq!(TailError::Payload.field(), "payload");
    assert_eq!(tailed(&[]), Ok(Vec::new()));
}
