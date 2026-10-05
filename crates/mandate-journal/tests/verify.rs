//! Verification (journal spec §11), anchors (§10), and export (§6.2) beyond the reference vectors.

mod common;

use std::collections::BTreeMap;

use common::{STREAM, edit, journal_with, mark_draft, now, stream};
use mandate_canon::Digest;
use mandate_journal::{
    AgentStreamCheck, Anchor, AnchorLeaf, AppendOutcome, EventCheck, EventFailure, RangeCheck,
    TrustedStart, Verified, export_line, export_segment, merkle_root, tsa_imprint, verify_anchor,
    verify_events,
};

#[test]
fn trusted_start_mid_stream() {
    let j = journal_with(4);
    let rows = j.rows(&stream());
    let start = TrustedStart {
        from_seq: 3,
        prev_hash: rows[1].hash,
    };
    assert_eq!(
        verify_events(&rows[2..], start, &BTreeMap::new()),
        Ok(Verified {
            next_seq: 6,
            last_hash: rows[4].hash
        })
    );
    let wrong = TrustedStart {
        from_seq: 3,
        prev_hash: rows[0].hash,
    };
    assert_eq!(
        verify_events(&rows[2..], wrong, &BTreeMap::new()),
        Err(EventFailure {
            seq: 3,
            check: EventCheck::PrevHashMismatch
        })
    );
    let off = TrustedStart {
        from_seq: 2,
        prev_hash: rows[1].hash,
    };
    assert_eq!(
        verify_events(&rows[2..], off, &BTreeMap::new()),
        Err(EventFailure {
            seq: 3,
            check: EventCheck::SeqGap
        })
    );
    assert_eq!(
        verify_events(&[], TrustedStart::GENESIS, &BTreeMap::new()),
        Ok(Verified {
            next_seq: 1,
            last_hash: Digest::ZERO
        })
    );
}

#[test]
fn check_order_and_columns() {
    let rows = journal_with(1).rows(&stream()).to_vec();
    let failure = |rows: &[mandate_journal::StoredEvent]| {
        verify_events(rows, TrustedStart::GENESIS, &BTreeMap::new())
            .err()
            .map(|f| (f.seq, f.check))
    };
    let mut spaced = rows.clone();
    spaced[1].body.insert(1, b' ');
    spaced[1].hash = Digest::of(&spaced[1].body);
    assert_eq!(
        failure(&spaced),
        Some((2, EventCheck::NonCanonical)),
        "valid JSON that is not canonical fails check 1 even with a matching hash"
    );
    let mut garbage = rows.clone();
    garbage[0].body = b"not json".to_vec();
    assert_eq!(failure(&garbage), Some((1, EventCheck::NonCanonical)));
    type Alter = fn(&mut mandate_journal::StoredEvent);
    let alterations: [Alter; 8] = [
        |r| r.stream_id = "acct:ws_1:X".into(),
        |r| r.seq += 10,
        |r| r.event_id = "01J8Z3M4000000000000000099".into(),
        |r| r.event_type = "OrderSubmitted".into(),
        |r| r.schema_version = 2,
        |r| r.environment = "live".into(),
        |r| r.recorded_at = "2026-09-21T14:00:00.000000001Z".into(),
        |r| r.prev_hash = Digest::of(b"x"),
    ];
    for alter in alterations {
        let mut altered = rows.clone();
        alter(&mut altered[1]);
        assert_eq!(
            failure(&altered),
            Some((altered[1].seq, EventCheck::ColumnMismatch)),
            "every stored column is compared with the body"
        );
    }
    let mut stale_hash = rows.clone();
    stale_hash[1].hash = Digest::of(b"x");
    assert_eq!(failure(&stale_hash), Some((2, EventCheck::RehashMismatch)));
}

#[test]
fn artifacts_and_configuration_objects_are_checked() {
    let s = stream();
    let content = b"model response".to_vec();
    let digest = Digest::of(&content);
    let reference = format!("\"sha256:{}\"", digest.to_hex());
    let config = b"fee configuration".to_vec();
    let config_digest = Digest::of(&config);
    let config_reference = format!("\"sha256:{}\"", config_digest.to_hex());
    let mut j = journal_with(0);
    let draft = edit(&mark_draft(1, "1"), "payload.source", Some(&reference));
    let draft = edit(&draft, "artifact_refs", Some(&format!("[{reference}]")));
    let draft = edit(&draft, "config_refs.fee_config", Some(&config_reference));
    assert!(matches!(
        j.append(&s, 1, 1, now(), &[&draft]),
        AppendOutcome::Committed(_)
    ));
    let rows = j.rows(&s);
    let mut store = BTreeMap::new();
    assert_eq!(
        verify_events(rows, TrustedStart::GENESIS, &store),
        Err(EventFailure {
            seq: 2,
            check: EventCheck::ArtifactMissing
        })
    );
    store.insert(digest, b"tampered".to_vec());
    assert_eq!(
        verify_events(rows, TrustedStart::GENESIS, &store),
        Err(EventFailure {
            seq: 2,
            check: EventCheck::ArtifactMismatch
        })
    );
    store.insert(digest, content);
    assert_eq!(
        verify_events(rows, TrustedStart::GENESIS, &store),
        Err(EventFailure {
            seq: 2,
            check: EventCheck::ArtifactMissing
        })
    );
    store.insert(config_digest, b"tampered configuration".to_vec());
    assert_eq!(
        verify_events(rows, TrustedStart::GENESIS, &store),
        Err(EventFailure {
            seq: 2,
            check: EventCheck::ArtifactMismatch
        })
    );
    store.insert(config_digest, config);
    assert!(verify_events(rows, TrustedStart::GENESIS, &store).is_ok());
}

#[test]
fn anchors() {
    let rows = journal_with(2).rows(&stream()).to_vec();
    let head = rows.last().unwrap();
    let leaf = |id: &str, seq, hash| AnchorLeaf {
        stream_id: id.to_owned(),
        seq,
        hash,
    };
    let anchor = Anchor::compute(vec![
        leaf("ctl:ws_1", 4, Digest::of(b"c")),
        leaf(STREAM, head.seq, head.hash),
        leaf("agent:ws_1:a", 9, Digest::of(b"a")),
    ])
    .unwrap();
    assert_eq!(
        anchor
            .leaves
            .iter()
            .map(|l| l.stream_id.as_str())
            .collect::<Vec<_>>(),
        [STREAM, "agent:ws_1:a", "ctl:ws_1"]
    );
    assert_eq!(verify_anchor(&anchor, &stream(), &rows), Ok(()));

    let mut bad_root = anchor.clone();
    bad_root.root = Digest::of(b"x");
    assert_eq!(
        verify_anchor(&bad_root, &stream(), &rows),
        Err(RangeCheck::AnchorRootMismatch)
    );
    let mut unsorted = anchor.clone();
    unsorted.leaves.swap(0, 1);
    unsorted.root = merkle_root(&unsorted.leaves).unwrap();
    assert_eq!(
        verify_anchor(&unsorted, &stream(), &rows),
        Err(RangeCheck::AnchorRootMismatch)
    );
    let mut repeated = anchor.clone();
    repeated.leaves[2].stream_id = "agent:ws_1:a".into();
    repeated.root = merkle_root(&repeated.leaves).unwrap();
    assert_eq!(
        verify_anchor(&repeated, &stream(), &rows),
        Err(RangeCheck::AnchorRootMismatch)
    );

    assert_eq!(
        verify_anchor(&bad_root, &stream(), &rows[..2]),
        Err(RangeCheck::AnchorHeadMismatch),
        "the head check runs before the root check"
    );
    let mut moved = rows.clone();
    moved[2].stream_id = "acct:ws_1:OTHER".into();
    assert_eq!(
        verify_anchor(&anchor, &stream(), &moved),
        Err(RangeCheck::AnchorHeadMismatch)
    );
    let mut rehashed = rows.clone();
    rehashed[2].hash = Digest::of(b"y");
    assert_eq!(
        verify_anchor(&anchor, &stream(), &rehashed),
        Err(RangeCheck::AnchorHeadMismatch)
    );

    let other = Anchor::compute(vec![leaf("ctl:ws_1", 4, Digest::of(b"c"))]).unwrap();
    assert_eq!(other.root, other.leaves[0].leaf_hash().unwrap());
    assert_eq!(
        verify_anchor(&other, &stream(), &[]),
        Ok(()),
        "an anchor without a leaf for this stream checks only its root"
    );

    assert_eq!(Anchor::compute(vec![]), None);
    assert_eq!(
        Anchor::compute(vec![
            leaf("a:b", 1, Digest::ZERO),
            leaf("a:b", 2, Digest::ZERO)
        ]),
        None
    );
    assert_eq!(
        Anchor::compute(vec![leaf("a:b", 1 << 53, Digest::ZERO)]),
        None
    );
    assert_eq!(merkle_root(&[]), None);
    assert_eq!(tsa_imprint(&Digest::ZERO), Digest::of(&[0; 32]));
    assert_eq!(
        (
            RangeCheck::AnchorHeadMismatch.code(),
            RangeCheck::AnchorRootMismatch.code()
        ),
        ("anchor_head_mismatch", "anchor_root_mismatch")
    );
    let codes: Vec<&str> = [
        EventCheck::NonCanonical,
        EventCheck::ColumnMismatch,
        EventCheck::SeqGap,
        EventCheck::RehashMismatch,
        EventCheck::PrevHashMismatch,
        EventCheck::ArtifactMissing,
        EventCheck::ArtifactMismatch,
    ]
    .iter()
    .map(|c| c.code())
    .collect();
    assert_eq!(
        codes,
        [
            "non_canonical",
            "column_mismatch",
            "seq_gap",
            "rehash_mismatch",
            "prev_hash_mismatch",
            "artifact_missing",
            "artifact_mismatch"
        ]
    );
}

#[test]
fn export_lines_embed_the_exact_body() {
    let rows = journal_with(2).rows(&stream()).to_vec();
    let segment = export_segment(&rows);
    assert!(segment.ends_with(b"\n"));
    let lines: Vec<&[u8]> = segment
        .split(|b| *b == b'\n')
        .filter(|l| !l.is_empty())
        .collect();
    assert_eq!(lines.len(), rows.len());
    for (line, row) in lines.iter().zip(&rows) {
        assert_eq!(*line, export_line(row).as_slice());
        let body_slice = &line[8..8 + row.body.len()];
        assert_eq!(body_slice, row.body.as_slice());
        let parsed = mandate_canon::parse(line).unwrap();
        assert_eq!(mandate_canon::to_canonical(&parsed), line.to_vec());
        assert_eq!(
            parsed.get("hash").and_then(|h| h.as_str()),
            Some(row.hash.to_hex().as_str())
        );
    }
    assert_eq!(export_segment(&[]), Vec::<u8>::new());
}

/// The agent stream's per-range codes as journal spec §11 writes them.
#[test]
fn agent_stream_range_checks_report_the_spec_codes() {
    assert_eq!(
        [
            AgentStreamCheck::IntentActionMismatch.code(),
            AgentStreamCheck::ModeEventMismatch.code(),
        ],
        ["intent_action_mismatch", "mode_event_mismatch"]
    );
}
