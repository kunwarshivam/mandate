//! Draft validation and the append protocol (journal spec §3, §4, §5.1, §9) beyond the reference
//! vectors, which `mandate-refcases` runs.

mod common;

use common::{STREAM, T, edit, event_id, journal_with, mark_draft, now, opened_draft, stream};
use mandate_canon::{Digest, parse, to_canonical};
use mandate_journal::{
    AppendOutcome, Draft, Environment, InvalidReason, MemoryJournal, StreamId, StreamType, seal,
};

fn reason(draft: &[u8]) -> (InvalidReason, String) {
    let e = Draft::parse(draft).unwrap_err();
    (e.reason, e.path)
}

#[test]
fn valid_drafts_parse() {
    let d = Draft::parse(&mark_draft(1, "150.010")).unwrap();
    assert_eq!(d.event_id(), event_id(1));
    assert_eq!(d.event_type(), "MarkUpdated");
    assert_eq!(d.schema_version(), 1);
    assert_eq!(d.environment(), Environment::Paper);
    assert_eq!(d.stream_id(), &stream());
    let text = String::from_utf8(d.canonical_bytes().to_vec()).unwrap();
    assert!(text.contains(r#""price":"150.01""#), "{text}");
    assert_eq!(
        parse(d.canonical_bytes()).map(|v| to_canonical(&v)),
        Ok(d.canonical_bytes().to_vec())
    );
}

#[test]
fn draft_rejections() {
    use InvalidReason::*;
    let m = mark_draft(1, "1");
    let digest = format!("\"sha256:{}\"", "a".repeat(64));
    let cases: Vec<(Vec<u8>, InvalidReason, &str)> = vec![
        (b"[]".to_vec(), Schema, ""),
        (edit(&m, "seq", Some("6")), Schema, "seq"),
        (
            edit(&m, "recorded_at", Some(&format!("\"{T}\""))),
            Schema,
            "recorded_at",
        ),
        (edit(&m, "event_time", None), Schema, "event_time"),
        (
            edit(&m, "envelope_version", Some("2")),
            Schema,
            "envelope_version",
        ),
        (
            edit(&m, "environment", Some("\"prod\"")),
            NonCanonical,
            "environment",
        ),
        (
            edit(&m, "event_id", Some("\"01J8Z3M4I00000000000000001\"")),
            NonCanonical,
            "event_id",
        ),
        (
            edit(&m, "event_id", Some("\"81J8Z3M4000000000000000001\"")),
            NonCanonical,
            "event_id",
        ),
        (
            edit(&m, "event_id", Some("\"01j8z3m4000000000000000001\"")),
            NonCanonical,
            "event_id",
        ),
        (
            edit(&m, "event_time", Some("\"2026-09-21T14:00:00Z\"")),
            NonCanonical,
            "event_time",
        ),
        (
            edit(&m, "clock_source", Some("\"gps\"")),
            NonCanonical,
            "clock_source",
        ),
        (
            edit(&m, "causation_id", Some("\"x\"")),
            NonCanonical,
            "causation_id",
        ),
        (
            edit(&m, "schema_version", Some("\"1\"")),
            Schema,
            "schema_version",
        ),
        (
            edit(&m, "event_type", Some("\"FillReversed\"")),
            UnknownEventType,
            "event_type",
        ),
        (
            edit(&m, "event_type", Some("\"DecisionMade\"")),
            WrongStream,
            "event_type",
        ),
        (
            edit(&m, "stream_id", Some("\"acct:ws 1:A\"")),
            NonCanonical,
            "stream_id",
        ),
        (
            edit(&m, "stream_id", Some("\"acct:ws_1\"")),
            NonCanonical,
            "stream_id",
        ),
        (
            edit(&m, "stream_id", Some("\"\"")),
            NonCanonical,
            "stream_id",
        ),
        (edit(&m, "actor.build", Some("null")), Schema, "actor.build"),
        (
            edit(&m, "actor.kind", Some("\"robot\"")),
            NonCanonical,
            "actor.kind",
        ),
        (edit(&m, "actor.id", Some("\"\"")), NonCanonical, "actor.id"),
        (edit(&m, "actor.extra", Some("1")), Schema, "actor.extra"),
        (edit(&m, "config_refs", Some("[]")), Schema, "config_refs"),
        (
            edit(&m, "config_refs.weights", Some(&digest)),
            Schema,
            "config_refs.weights",
        ),
        (
            edit(&m, "config_refs.rule_set", Some("\"sha256:1\"")),
            NonCanonical,
            "config_refs.rule_set",
        ),
        (
            edit(&m, "config_refs.rule_set", Some("null")),
            NonCanonical,
            "config_refs.rule_set",
        ),
        (
            edit(&m, "schema_version", Some("2")),
            UnknownSchema,
            "payload",
        ),
        (edit(&m, "payload.feed", None), Schema, "payload.feed"),
        (
            edit(&m, "payload.extra", Some("\"x\"")),
            Schema,
            "payload.extra",
        ),
        (
            edit(&m, "payload.price", Some("\"1,5\"")),
            NonCanonical,
            "payload.price",
        ),
        (
            edit(&m, "payload.price", Some("\"1e-30\"")),
            NonCanonical,
            "payload.price",
        ),
        (
            edit(&m, "payload.price", Some("null")),
            Schema,
            "payload.price",
        ),
        (
            edit(&m, "payload.source", Some("\"\"")),
            NonCanonical,
            "payload.source",
        ),
        (
            edit(&m, "payload.source", Some("7")),
            Schema,
            "payload.source",
        ),
        (edit(&m, "payload", Some("[]")), Schema, "payload"),
        (
            edit(&m, "payload.source", Some(&digest)),
            ArtifactRefs,
            "artifact_refs",
        ),
        (
            edit(&m, "artifact_refs", Some(&format!("[{digest}]"))),
            ArtifactRefs,
            "artifact_refs",
        ),
        (
            edit(&m, "artifact_refs", Some("[\"sha256:x\"]")),
            NonCanonical,
            "artifact_refs[0]",
        ),
        (
            edit(&m, "pii_refs", Some(r#"["b","a"]"#)),
            PiiRefs,
            "pii_refs",
        ),
        (
            edit(&m, "pii_refs", Some(r#"["a","a"]"#)),
            PiiRefs,
            "pii_refs",
        ),
        (edit(&m, "pii_refs", Some("[1]")), Schema, "pii_refs[0]"),
        (
            edit(
                &opened_draft("paper"),
                "payload.account_ref",
                Some("\"OTHER\""),
            ),
            StreamMismatch,
            "stream_id",
        ),
        (
            edit(
                &opened_draft("paper"),
                "payload.workspace_id",
                Some("\"ws_2\""),
            ),
            StreamMismatch,
            "stream_id",
        ),
        (
            edit(
                &opened_draft("paper"),
                "payload.stream_type",
                Some("\"agent\""),
            ),
            NonCanonical,
            "payload.stream_type",
        ),
        (
            edit(
                &opened_draft("paper"),
                "stream_id",
                Some("\"agent:ws_1:ACCT1\""),
            ),
            StreamMismatch,
            "stream_id",
        ),
    ];
    for (draft, expected, path) in cases {
        assert_eq!(
            reason(&draft),
            (expected, path.to_owned()),
            "{}",
            String::from_utf8_lossy(&draft)
        );
    }
    assert_eq!(
        reason(b"{\"a\":1.5}").0,
        Json(mandate_canon::ParseErrorKind::Float)
    );
    assert_eq!(
        reason(b"{\"a\":1,\"a\":1}").0,
        Json(mandate_canon::ParseErrorKind::DuplicateKey)
    );
}

#[test]
fn accepted_variants() {
    let m = mark_draft(1, "1");
    let digest = format!("\"sha256:{}\"", "a".repeat(64));
    let ok = [
        edit(
            &m,
            "actor",
            Some(r#"{"kind":"broker","id":"alpaca","version":"v2","build":null}"#),
        ),
        edit(&m, "config_refs.rule_set", Some(&digest)),
        edit(
            &edit(&m, "payload.source", Some(&digest)),
            "artifact_refs",
            Some(&format!("[{digest}]")),
        ),
        edit(&m, "pii_refs", Some(r#"["a","b"]"#)),
        edit(&m, "causation_id", Some(&format!("\"{}\"", event_id(9)))),
        edit(&m, "environment", Some("\"backtest\"")),
    ];
    for draft in ok {
        assert!(
            Draft::parse(&draft).is_ok(),
            "{}",
            String::from_utf8_lossy(&draft)
        );
    }
}

#[test]
fn gate_decided_checks_are_from_the_spec_list() {
    let gate = |check: &str| {
        format!(
            r#"{{"envelope_version":1,"environment":"paper","event_id":"{}","stream_id":"{STREAM}",
            "event_type":"GateDecided","schema_version":1,"event_time":"{T}","clock_source":"local",
            "causation_id":null,"correlation_id":null,
            "actor":{{"kind":"system","id":"executor","version":"0.1.0","build":"sha256:{h}"}},
            "config_refs":{{"fee_config":"sha256:{h}","instrument_snapshot":"sha256:{h}",
            "mandate_version":"sha256:{h}","rule_set":"sha256:{h}","trading_calendar":"sha256:{h}"}},
            "payload":{{"intent_id":"{}","verdict":"deny","reason_code":"order_size","data_profile":"iex",
            "quotes_used":[],"marks_used":[],"checks":[{{"id":"{check}","result":"fail",
            "inputs":{{"max":"10","nested":{{"a":[1,true,null]}}}},"computed":{{}}}}]}},
            "artifact_refs":[],"pii_refs":[]}}"#,
            event_id(1),
            event_id(2),
            h = "1".repeat(64)
        )
    };
    assert!(Draft::parse(gate("order_size").as_bytes()).is_ok());
    let e = Draft::parse(gate("order_volume").as_bytes()).unwrap_err();
    assert_eq!(
        (e.reason, e.path.as_str()),
        (InvalidReason::NonCanonical, "payload.checks[0].id")
    );
}

#[test]
fn stream_ids() {
    for (text, kind) in [
        ("acct:ws_1:01J8Z2ACCT00000000000000A1", StreamType::Account),
        ("agent:ws-1:agent_a", StreamType::Agent),
        ("ctl:ws_1", StreamType::Control),
        ("clock:WS1", StreamType::Scheduler),
    ] {
        let id = StreamId::parse(text).unwrap();
        assert_eq!(
            (id.as_str(), id.stream_type(), id.to_string()),
            (text, kind, text.to_owned())
        );
    }
    for bad in [
        "",
        "acct:ws",
        "acct:ws:a:b",
        "ctl:ws:x",
        "clock:",
        "agent::a",
        "acct:ws:a b",
        "user:ws",
        "acct:ws:é",
    ] {
        assert_eq!(StreamId::parse(bad), None, "{bad:?}");
    }
    for env in [Environment::Paper, Environment::Live, Environment::Backtest] {
        assert_eq!(Environment::parse(env.as_str()), Some(env));
    }
    assert_eq!(Environment::parse("prod"), None);
}

#[test]
fn stream_rules() {
    let s = stream();
    let mut j = MemoryJournal::new();
    assert_eq!(j.head(&s).seq, 0);
    assert_eq!(j.head(&s).hash, Digest::ZERO);
    assert_eq!(j.head(&s).writer_epoch, 0);
    let invalid = |o: AppendOutcome| match o {
        AppendOutcome::Invalid { draft, error } => (draft, error.reason),
        other => panic!("{other:?}"),
    };
    assert_eq!(
        invalid(j.append(&s, 0, 0, now(), &[])),
        (0, InvalidReason::EmptyBatch)
    );
    assert_eq!(
        invalid(j.append(&s, 0, 0, now(), &[&mark_draft(1, "1")])),
        (0, InvalidReason::NotStreamOpened)
    );
    let other = StreamId::parse("acct:ws_1:OTHER").unwrap();
    assert_eq!(
        invalid(j.append(&other, 0, 0, now(), &[&opened_draft("paper")])),
        (0, InvalidReason::StreamMismatch)
    );
    assert_eq!(
        invalid(j.append(
            &s,
            0,
            0,
            now(),
            &[
                &opened_draft("paper"),
                &mark_draft(1, "1"),
                &mark_draft(1, "2")
            ]
        )),
        (2, InvalidReason::DuplicateEventId)
    );
    assert_eq!(
        invalid(j.append(
            &s,
            0,
            0,
            now(),
            &[
                &opened_draft("paper"),
                &edit(&mark_draft(1, "1"), "environment", Some("\"backtest\""))
            ]
        )),
        (1, InvalidReason::EnvironmentMismatch)
    );
    assert_eq!(
        invalid(j.append(
            &s,
            0,
            0,
            now(),
            &[&opened_draft("paper"), &opened_draft("paper")]
        )),
        (1, InvalidReason::DuplicateEventId)
    );
    assert_eq!(j.rows(&s).len(), 0, "rejected batches write nothing");

    let AppendOutcome::Committed(rows) = j.append(
        &s,
        0,
        0,
        now(),
        &[
            &opened_draft("paper"),
            &mark_draft(1, "1"),
            &mark_draft(2, "2"),
        ],
    ) else {
        panic!("epoch 0 may write an unclaimed stream, and a batch commits all-or-nothing")
    };
    assert_eq!(rows.iter().map(|r| r.seq).collect::<Vec<_>>(), [1, 2, 3]);
    assert_eq!(rows[0].prev_hash, Digest::ZERO);
    assert_eq!(rows[1].prev_hash, rows[0].hash);
    assert_eq!(rows[2].prev_hash, rows[1].hash);
    assert!(
        rows.iter()
            .all(|r| r.recorded_at == T && r.environment == "paper" && r.stream_id == STREAM)
    );
    assert_eq!(j.rows(&s), rows.as_slice());
    assert_eq!(j.event(&event_id(2)), Some(&rows[2]));
    assert_eq!(j.event(&event_id(7)), None);

    let reopened = edit(
        &opened_draft("paper"),
        "event_id",
        Some(&format!("\"{}\"", event_id(50))),
    );
    assert_eq!(
        invalid(j.append(&s, 3, 0, now(), &[&reopened])),
        (0, InvalidReason::StreamAlreadyOpened)
    );
    let backtest = edit(&mark_draft(3, "1"), "environment", Some("\"backtest\""));
    assert_eq!(
        invalid(j.append(&s, 3, 0, now(), &[&backtest])),
        (0, InvalidReason::EnvironmentMismatch)
    );

    let AppendOutcome::AlreadyCommitted(again) = j.append(
        &s,
        0,
        9,
        now(),
        &[&mark_draft(2, "2"), &mark_draft(1, "1.0")],
    ) else {
        panic!("retrying a committed batch returns the stored events in batch order")
    };
    assert_eq!(again, vec![rows[2].clone(), rows[1].clone()]);

    let elsewhere = edit(
        &opened_draft("paper"),
        "stream_id",
        Some("\"acct:ws_1:OTHER\""),
    );
    let elsewhere = edit(&elsewhere, "payload.account_ref", Some("\"OTHER\""));
    assert_eq!(
        j.append(&other, 0, 0, now(), &[&elsewhere]),
        AppendOutcome::IdempotencyConflict { stored_seq: 1 },
        "event IDs are global: the same event_id in another stream is a conflict"
    );
}

#[test]
fn ownership_and_fencing() {
    let s = stream();
    let mut j = journal_with(2);
    assert_eq!(j.head(&s).writer_epoch, 1);
    assert_eq!(j.take_ownership(&s), 2);
    assert_eq!(
        j.append(&s, 3, 1, now(), &[&mark_draft(3, "1")]),
        AppendOutcome::Fenced { current_epoch: 2 }
    );
    assert_eq!(
        j.append(&s, 3, 3, now(), &[&mark_draft(3, "1")]),
        AppendOutcome::Fenced { current_epoch: 2 }
    );
    let head = j.head(&s);
    assert_eq!(
        j.append(&s, 2, 2, now(), &[&mark_draft(3, "1")]),
        AppendOutcome::HeadMismatch {
            actual_seq: 3,
            actual_hash: head.hash
        }
    );
    assert!(matches!(
        j.append(&s, 3, 2, now(), &[&mark_draft(3, "1")]),
        AppendOutcome::Committed(_)
    ));
}

#[test]
fn seal_bounds_and_names() {
    let d = Draft::parse(&mark_draft(1, "1")).unwrap();
    assert_eq!(
        seal(&d, 1 << 53, Digest::ZERO, now()).unwrap_err().reason,
        InvalidReason::NonCanonical
    );
    assert_eq!(
        seal(&d, 0, Digest::ZERO, now()).unwrap_err().reason,
        InvalidReason::NonCanonical,
        "seq starts at 1 (journal spec §3)"
    );
    let row = seal(&d, (1 << 53) - 1, Digest::ZERO, now()).unwrap();
    assert_eq!(row.hash, Digest::of(&row.body));
    let names: Vec<&str> = [
        AppendOutcome::Committed(vec![]),
        AppendOutcome::AlreadyCommitted(vec![]),
        AppendOutcome::HeadMismatch {
            actual_seq: 0,
            actual_hash: Digest::ZERO,
        },
        AppendOutcome::IdempotencyConflict { stored_seq: 0 },
        AppendOutcome::Fenced { current_epoch: 0 },
        AppendOutcome::Invalid {
            draft: 0,
            error: Draft::parse(b"1").unwrap_err(),
        },
        AppendOutcome::Unavailable,
        AppendOutcome::Ambiguous,
    ]
    .iter()
    .map(AppendOutcome::name)
    .collect();
    assert_eq!(
        names,
        [
            "Committed",
            "AlreadyCommitted",
            "HeadMismatch",
            "IdempotencyConflict",
            "Fenced",
            "Invalid",
            "Unavailable",
            "Ambiguous"
        ]
    );
    use InvalidReason::*;
    let codes: Vec<&str> = [
        Json(mandate_canon::ParseErrorKind::Float),
        Schema,
        NonCanonical,
        MissingConfigRef,
        UnknownEventType,
        UnknownSchema,
        WrongStream,
        StreamMismatch,
        EnvironmentMismatch,
        NotStreamOpened,
        StreamAlreadyOpened,
        DuplicateEventId,
        EmptyBatch,
        ArtifactRefs,
        PiiRefs,
    ]
    .iter()
    .map(|r| r.code())
    .collect();
    assert_eq!(
        codes,
        [
            "float",
            "schema",
            "non_canonical",
            "missing_config_ref",
            "unknown_event_type",
            "unknown_schema",
            "wrong_stream",
            "stream_mismatch",
            "environment_mismatch",
            "not_stream_opened",
            "stream_already_opened",
            "duplicate_event_id",
            "empty_batch",
            "artifact_refs",
            "pii_refs"
        ]
    );
    let e = Draft::parse(&edit(&mark_draft(1, "1"), "payload.feed", None)).unwrap_err();
    assert_eq!(
        e.to_string(),
        "missing, unknown, or mistyped field at `payload.feed`"
    );
}
