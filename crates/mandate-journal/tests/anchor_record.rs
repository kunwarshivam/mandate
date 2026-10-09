//! E12-3 (journal spec v0.36 §9.14, §10, §11): the anchor an `AnchorComputed` row records, as the
//! verification run reads it for `anchor_head_mismatch`, `anchor_root_mismatch`, and the token
//! check. The leaves and root come back exactly as recorded, so a record whose root or order is
//! wrong still fails §11's checks instead of being repaired on the way in.

use mandate_canon::{Digest, parse, to_canonical};
use mandate_journal::{
    Anchor, AnchorLeaf, AnchorRecord, AnchorRecordError, ArtifactRef, RangeCheck, StoredEvent,
    StreamId, anchor_record, verify_anchor,
};
use proptest::prelude::*;
use proptest::test_runner::TestRunner;

const TOKEN: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// A control-stream row of `event_type` at `version` whose body's payload is `payload` (JSON text).
fn row(event_type: &str, version: u64, payload: &str) -> StoredEvent {
    let body = format!(r#"{{"event_type":"{event_type}","payload":{payload}}}"#);
    let bytes = parse(body.as_bytes()).map_or_else(|_| body.into_bytes(), |v| to_canonical(&v));
    StoredEvent {
        stream_id: "ctl:ws_1".to_owned(),
        seq: 7,
        event_id: "01J8Z6R0A00000000000000007".to_owned(),
        event_type: event_type.to_owned(),
        schema_version: version,
        environment: "paper".to_owned(),
        recorded_at: "2026-10-09T12:00:00.000000000Z".to_owned(),
        prev_hash: Digest::ZERO,
        hash: Digest::of(&bytes),
        body: bytes,
    }
}

/// The `AnchorComputed` row §9.14 records for `leaves` in their order, `root`, and `token`.
fn anchor_row(leaves: &[AnchorLeaf], root: Digest, token: Option<&str>) -> StoredEvent {
    let leaves: Vec<String> = leaves
        .iter()
        .map(|l| {
            format!(
                r#"{{"hash":"{}","seq":{},"stream_id":"{}"}}"#,
                l.hash, l.seq, l.stream_id
            )
        })
        .collect();
    let token = token.map_or_else(|| "null".to_owned(), |t| format!("\"{t}\""));
    let payload = format!(
        r#"{{"leaves":[{}],"root":"{root}","token":{token}}}"#,
        leaves.join(",")
    );
    row("AnchorComputed", 1, &payload)
}

fn leaf(stream_id: &str, seq: u64) -> AnchorLeaf {
    let hash = Digest::of(format!("{stream_id}{seq}").as_bytes());
    AnchorLeaf {
        stream_id: stream_id.to_owned(),
        seq,
        hash,
    }
}

/// Any anchor §10 computes, stamped or with a `null` token, reads back leaf for leaf, root for
/// root, and token for token.
#[test]
fn every_computed_anchor_reads_back_exactly() {
    let heads = prop::collection::btree_map("[a-z]{1,6}", 1u64..1_000_000, 1..9);
    let outcome = TestRunner::deterministic().run(&(heads, any::<bool>()), |(heads, stamped)| {
        let heads = heads
            .iter()
            .map(|(s, seq)| leaf(&format!("agent:ws_1:{s}"), *seq));
        let a = Anchor::compute(heads.collect()).unwrap();
        let token = stamped.then_some(TOKEN);
        let got = anchor_record(&anchor_row(&a.leaves, a.root, token));
        let want = AnchorRecord {
            anchor: a,
            token: token.and_then(ArtifactRef::parse),
        };
        prop_assert_eq!(got, Ok(want));
        Ok(())
    });
    if let Err(failure) = outcome {
        panic!("{failure}");
    }
}

/// A wrong root and unsorted leaves are kept, so §11's `anchor_root_mismatch` still sees them.
#[test]
fn a_wrong_root_and_unsorted_leaves_are_kept_for_the_anchor_checks() {
    let a = Anchor::compute(vec![leaf("acct:ws_1:A1", 4), leaf("ctl:ws_1", 6)]).unwrap();
    let reversed: Vec<AnchorLeaf> = a.leaves.iter().rev().cloned().collect();
    let wrong_root = Digest::of(b"not the root");
    for (leaves, root) in [(reversed, a.root), (a.leaves.clone(), wrong_root)] {
        let got = anchor_record(&anchor_row(&leaves, root, None));
        let want = Anchor { leaves, root };
        assert_eq!(got.as_ref().map(|r| &r.anchor), Ok(&want));
        let unlisted = StreamId::parse("agent:ws_1:Z").unwrap();
        let judged = got.map(|r| verify_anchor(&r.anchor, &unlisted, &[]));
        assert_eq!(judged, Ok(Err(RangeCheck::AnchorRootMismatch)), "{want:?}");
    }
}

/// Another type or version is not an anchor; a payload missing a member, or holding one not of
/// §9.14's type (a bare-hex or numeric token, a `sha256:` root, a leaf's text `seq`), is malformed.
#[test]
fn a_row_that_is_not_a_well_formed_anchor_is_refused() {
    use AnchorRecordError::{Malformed, NotAnAnchor};
    let h = Digest::of(b"h").to_hex();
    let good = format!(r#"{{"hash":"{h}","seq":1,"stream_id":"ctl:ws_1"}}"#);
    let p = format!(r#"{{"leaves":[{good}],"root":"{h}","token":null}}"#);
    let mut cases = vec![
        (row("SegmentExported", 1, &p), NotAnAnchor),
        (row("AnchorComputed", 2, &p), NotAnAnchor),
    ];
    for bad in [
        format!(r#"{{"leaves":[{good}],"token":null}}"#),
        format!(r#"{{"leaves":[{good}],"root":"{h}"}}"#),
        format!(r#"{{"leaves":{good},"root":"{h}","token":null}}"#),
        format!(r#"{{"leaves":[{good}],"root":"sha256:{h}","token":null}}"#),
        format!(r#"{{"leaves":[{good}],"root":"{h}","token":"{h}"}}"#),
        format!(r#"{{"leaves":[{good}],"root":"{h}","token":7}}"#),
        p.replace(&format!(r#""hash":"{h}""#), r#""hash":"x""#),
        p.replace(r#""seq":1"#, r#""seq":"1""#),
        p.replace(r#","stream_id":"ctl:ws_1""#, ""),
        "not json".to_owned(),
    ] {
        cases.push((row("AnchorComputed", 1, &bad), Malformed));
    }
    for (r, want) in cases {
        let body = String::from_utf8_lossy(&r.body).into_owned();
        assert_eq!(anchor_record(&r), Err(want), "{} {body}", r.event_type);
    }
}
