//! E12-3 run logic, the walk stage: what the run walks, the record it drafts, and the result it
//! serves (workspace API §4.8.1 "Coverage", "The record", "Result"; AU-6 and AU-8; journal spec
//! §9.13 rules 107, 110, 111, 132 and 133 and §11's per-event checks; DEC-787 item 1, DEC-788
//! items 1 and 2, DEC-897 items 4 and 5(a), DEC-916). The oracles are the chains this file builds
//! itself: every hash, position and count a test expects is computed from the rows it wrote,
//! never from the code under test, and §9.13's own validator (`Draft::parse`) judges every record
//! the run drafts.

mod common;

use common::{T, WS_A, WS_B, actor, event_id, permitted, tenant, text};
use mandate_audit::{
    ColdSource, ControlSnapshot, FirstFailure, RecordedStart, Requester, RequesterKind, Run,
    RunEnvelope, RunResult, VerificationRefusal, VerificationRequest, plan, walk,
};
use mandate_canon::{Digest, Int, Key, Object, Value, parse, to_canonical};
use mandate_journal::{
    AnchorLeaf, ArtifactError, ArtifactRef, ArtifactSource, ColdRead, Draft, Environment,
    StartRequest, StoredEvent, merkle_root,
};
use mandate_time::UtcNanos;
use proptest::prelude::*;
use proptest::test_runner::{RngAlgorithm, TestCaseError, TestRng, TestRunner};

const NOT_FOUND: VerificationRefusal = VerificationRefusal::NotFound;
const ZERO: Digest = Digest::ZERO;

/// What the tests compare: the run's `checked`, `first_failure`, `to_hash` and `result`.
type Seen = Result<(u64, Option<FirstFailure>, Option<Digest>, RunResult), VerificationRefusal>;

fn own() -> String {
    format!("acct:{}:ACCT1", text(WS_A))
}

fn ctl() -> String {
    format!("ctl:{}", text(WS_A))
}

fn word(digit: &str) -> Digest {
    Digest::from_hex(&digit.repeat(64)).unwrap()
}

fn member(name: &str) -> Key {
    Key::new(name).unwrap()
}

fn text_value(value: &str) -> Value {
    Value::Str(value.to_owned())
}

fn number(value: u64) -> Value {
    Value::Int(Int::new(value).unwrap())
}

fn hex(digest: Digest) -> Value {
    text_value(&digest.to_hex())
}

/// A cold store whose one manifest object `bytes` sits under `its_hash`; any other is absent.
struct Cold(Digest, Vec<u8>);

impl ColdSource for Cold {
    fn manifest(&self, manifest_hash: &Digest) -> ColdRead {
        if manifest_hash == &self.0 {
            ColdRead::Read(self.1.clone())
        } else {
            ColdRead::Absent
        }
    }
}

/// A cold store with no object to answer: a genesis start never asks it.
struct NoCold;

impl ColdSource for NoCold {
    fn manifest(&self, _: &Digest) -> ColdRead {
        ColdRead::Absent
    }
}

/// An artifact store with nothing in it.
struct NoArtifacts;

impl ArtifactSource for NoArtifacts {
    fn read_artifact(&self, _: &ArtifactRef) -> Result<Vec<u8>, ArtifactError> {
        Err(ArtifactError::Missing)
    }
}

/// An artifact source answering under the one reference `under` with `answer`, `Missing` else.
struct OneArtifact(ArtifactRef, Result<Vec<u8>, ArtifactError>);

impl ArtifactSource for OneArtifact {
    fn read_artifact(&self, reference: &ArtifactRef) -> Result<Vec<u8>, ArtifactError> {
        if reference == &self.0 {
            self.1.clone()
        } else {
            Err(ArtifactError::Missing)
        }
    }
}

/// One row of `stream_id` at `seq` chaining from `prev_hash`, of `event_type`, carrying `payload`
/// and `artifact_refs` (§3's envelope with the journal's assigned fields): canonical (§11 check 1),
/// its columns equal to its body (check 2) and its hash its body's (check 4), by construction.
fn row(
    stream_id: &str,
    seq: u64,
    prev_hash: Digest,
    event_type: &str,
    payload: Value,
    artifact_refs: &[&str],
) -> StoredEvent {
    let mut body = Object::new();
    body.insert(member("envelope_version"), number(1));
    body.insert(member("environment"), text_value("paper"));
    body.insert(member("event_id"), text_value(&event_id(seq)));
    body.insert(member("stream_id"), text_value(stream_id));
    body.insert(member("event_type"), text_value(event_type));
    body.insert(member("schema_version"), number(1));
    body.insert(member("event_time"), text_value(T));
    body.insert(member("clock_source"), text_value("local"));
    body.insert(member("causation_id"), Value::Null);
    body.insert(member("correlation_id"), Value::Null);
    body.insert(member("actor"), parse(actor().as_bytes()).unwrap());
    body.insert(member("config_refs"), Value::Object(Object::new()));
    body.insert(member("payload"), payload);
    body.insert(
        member("artifact_refs"),
        Value::Array(
            artifact_refs
                .iter()
                .map(|reference| text_value(reference))
                .collect(),
        ),
    );
    body.insert(member("pii_refs"), Value::Array(Vec::new()));
    body.insert(member("prev_hash"), hex(prev_hash));
    body.insert(member("recorded_at"), text_value(T));
    body.insert(member("seq"), number(seq));
    let bytes = to_canonical(&Value::Object(body));
    StoredEvent {
        stream_id: stream_id.to_owned(),
        seq,
        event_id: event_id(seq),
        event_type: event_type.to_owned(),
        schema_version: 1,
        environment: "paper".to_owned(),
        recorded_at: T.to_owned(),
        prev_hash,
        hash: Digest::of(&bytes),
        body: bytes,
    }
}

/// The `MarkUpdated` payload every clean range row of this file carries; no walk reads a member of
/// it, so no test does either.
fn mark() -> Value {
    let mut payload = Object::new();
    payload.insert(member("instrument_id"), text_value("inst"));
    payload.insert(member("price"), text_value("1"));
    payload.insert(member("source"), text_value("quote"));
    payload.insert(member("feed"), text_value("iex"));
    payload.insert(member("risk_clock"), text_value(T));
    Value::Object(payload)
}

/// `count` clean rows of `stream_id` chaining from the position `seq` holding `prev_hash`; the
/// hashes they return, in the order written, are what every expectation below is built from.
fn chained_from(
    stream_id: &str,
    (seq, prev_hash): (u64, Digest),
    count: u64,
) -> (Vec<StoredEvent>, Vec<Digest>) {
    let mut rows = Vec::new();
    let mut hashes = Vec::new();
    let mut prev = prev_hash;
    for at in seq..seq + count {
        let written = row(stream_id, at, prev, "MarkUpdated", mark(), &[]);
        prev = written.hash;
        hashes.push(written.hash);
        rows.push(written);
    }
    (rows, hashes)
}

/// The clean chain of `stream_id` over `1..=head`: a full range's rows, from seq 1 with 64 zeros
/// (§9.13: rule 107's `prev_hash` for `from_seq` 1).
fn clean_chain(stream_id: &str, head: u64) -> (Vec<StoredEvent>, Vec<Digest>) {
    chained_from(stream_id, (1, ZERO), head)
}

/// A clean [`row`] of the own stream whose `config_refs` names one reference, `reference`:
/// canonical, its columns its body's, and its hash its body's, so only check 6's `config_refs`
/// half can fail it (§11 check 6: every referenced artifact exists and re-hashes).
fn configured(seq: u64, prev_hash: Digest, reference: &str) -> StoredEvent {
    let clean = row(&own(), seq, prev_hash, "MarkUpdated", mark(), &[]);
    let body = match parse(&clean.body).expect("the clean row parses") {
        Value::Object(mut members) => {
            members.insert(
                member("config_refs"),
                parse(format!(r#"{{"profile":"{reference}"}}"#).as_bytes())
                    .expect("the config refs are canonical"),
            );
            Value::Object(members)
        }
        _ => panic!("the clean row is an object"),
    };
    let bytes = to_canonical(&body);
    StoredEvent {
        hash: Digest::of(&bytes),
        body: bytes,
        ..clean
    }
}

/// The control row of a `SegmentExported` of `stream_id` over seqs `first..=last`, at ctl `seq`:
/// `first_prev_hash` is its first row's `prev_hash` and `last_hash` its last row's. The cold-store
/// manifest object returned beside it carries the same six members in §6.2's form, so rule 117's
/// `manifest_hash`, their hash, is what both the row and the cold bytes name, by construction.
fn segment_start(
    stream_id: &str,
    seq: u64,
    first: u64,
    first_prev: Digest,
    last: u64,
    last_hash: Digest,
) -> (StoredEvent, Digest, Vec<u8>) {
    let mut manifest = Object::new();
    manifest.insert(member("file_sha256"), hex(word("6")));
    manifest.insert(member("first_prev_hash"), hex(first_prev));
    manifest.insert(member("first_seq"), number(first));
    manifest.insert(member("last_hash"), hex(last_hash));
    manifest.insert(member("last_seq"), number(last));
    manifest.insert(member("stream"), text_value(stream_id));
    let bytes = to_canonical(&Value::Object(manifest.clone()));
    let manifest_hash = Digest::of(&bytes);
    let mut payload = manifest;
    payload.insert(member("stream_id"), text_value(stream_id));
    payload.insert(member("manifest_hash"), hex(manifest_hash));
    let segment = row(
        &ctl(),
        seq,
        ZERO,
        "SegmentExported",
        Value::Object(payload),
        &[],
    );
    (segment, manifest_hash, bytes)
}

/// The control row of a stamped `AnchorComputed` at ctl `seq`, whose leaf for `stream_id` names
/// position `leaf_seq` at `leaf_hash` (§9.14; DEC-896 item 4 refuses an unstamped anchor), so a
/// range entered from it starts at `leaf_seq + 1` over `leaf_hash`.
fn anchor_start(stream_id: &str, seq: u64, leaf_seq: u64, leaf_hash: Digest) -> StoredEvent {
    let leaf = AnchorLeaf {
        stream_id: stream_id.to_owned(),
        seq: leaf_seq,
        hash: leaf_hash,
    };
    let root = merkle_root(&[leaf]).unwrap();
    let token = format!("sha256:{}", "a".repeat(64));
    let mut written_leaf = Object::new();
    written_leaf.insert(member("hash"), hex(leaf_hash));
    written_leaf.insert(member("seq"), number(leaf_seq));
    written_leaf.insert(member("stream_id"), text_value(stream_id));
    let mut payload = Object::new();
    payload.insert(
        member("leaves"),
        Value::Array(vec![Value::Object(written_leaf)]),
    );
    payload.insert(member("root"), text_value(&root.to_hex()));
    payload.insert(member("token"), text_value(&token));
    row(
        &ctl(),
        seq,
        ZERO,
        "AnchorComputed",
        Value::Object(payload),
        &[&token],
    )
}

/// §4.8.1's one snapshot of DEC-787 item 1, as this file hands it to the run: the stream's head
/// at start, the workspace's own control rows, and the stream's rows the snapshot holds, the last
/// two of which the tests built.
struct Snap {
    head: u64,
    control: Vec<StoredEvent>,
    rows: Vec<StoredEvent>,
}

fn snap(head: u64, control: Vec<StoredEvent>, rows: Vec<StoredEvent>) -> Snap {
    Snap {
        head,
        control,
        rows,
    }
}

/// The cold store and the artifact store the run reads: a genesis-range test reads neither.
struct Stores<'a> {
    cold: &'a dyn ColdSource,
    artifacts: &'a dyn ArtifactSource,
}

fn answered_by_nothing() -> Stores<'static> {
    Stores {
        cold: &NoCold,
        artifacts: &NoArtifacts,
    }
}

/// The requesting principal's envelope (§3, rule 110): the `event_id` §3.4 derives from the
/// request's `Idempotency-Key` (DEC-788 item 2) and the arrival time are the writer's members.
fn written_by(kind: RequesterKind, id: &str, version: &str, build: Option<Digest>) -> RunEnvelope {
    RunEnvelope {
        environment: Environment::Paper,
        event_id: event_id(99),
        event_time: UtcNanos::parse(T).unwrap(),
        clock_source: "local".to_owned(),
        correlation_id: None,
        requester: Requester {
            kind,
            id: id.to_owned(),
            version: version.to_owned(),
            build,
        },
    }
}

/// The envelope of a `user` requester, the one every run this file composes records unless a case
/// names another.
fn by_user() -> RunEnvelope {
    written_by(RequesterKind::User, "user_owner_01", "1", None)
}

/// `WS_A`'s run of `stream_id` over `span` (`to_seq` `None` for the head at start) from `by`, over
/// one `snapped` snapshot, answered by `stores`, recorded by `envelope`: the plan and the walk
/// compose as the route will call them (§4.8.1's refusal steps 4 to 7, then the coverage walk).
fn run_of(
    stream_id: &str,
    span: (u64, Option<u64>),
    by: StartRequest<'_>,
    snapped: &Snap,
    stores: &Stores<'_>,
    envelope: &RunEnvelope,
) -> Result<Run, VerificationRefusal> {
    let context = tenant(WS_A);
    let request = VerificationRequest {
        stream_id,
        from_seq: span.0,
        to_seq: span.1,
        trusted_start: by,
    };
    let snapshot = ControlSnapshot {
        stream_head: Some(snapped.head),
        control: &snapped.control,
    };
    let witnessed = permitted(&context);
    plan(&witnessed, &request, &snapshot, stores.cold).and_then(|planned| {
        walk(
            &witnessed,
            &request,
            &planned,
            &snapshot,
            &snapped.rows,
            stores.cold,
            stores.artifacts,
            envelope,
        )
    })
}

/// What a walk found, as the tests compare it: `checked`, `first_failure`, `to_hash` and `result`.
fn outcome(run: &Run) -> (u64, Option<FirstFailure>, Option<Digest>, RunResult) {
    (
        run.checked(),
        run.first_failure(),
        run.to_hash(),
        run.result(),
    )
}

/// The own account stream's walk over `span` from genesis, over the snapshot `we_read`, recorded
/// by a `user`.
fn own_walked(span: (u64, Option<u64>), we_read: &Snap) -> Seen {
    run_of(
        &own(),
        span,
        StartRequest::Genesis,
        we_read,
        &answered_by_nothing(),
        &by_user(),
    )
    .map(|run| outcome(&run))
}

fn failed(check: &'static str, seq: u64, checked: u64) -> Seen {
    Ok((
        checked,
        Some(FirstFailure {
            check,
            seq: Some(seq),
        }),
        None,
        RunResult::Fail,
    ))
}

fn passed(checked: u64, to_hash: Digest) -> Seen {
    Ok((checked, None, Some(to_hash), RunResult::Pass))
}

/// §9.13's own verdict on the record the run drafted: the validator's `(event_type, version)`
/// when it parses, its `(reason, path)` when it refuses.
fn judged(draft: &[u8]) -> Result<(String, u64), (String, String)> {
    Draft::parse(draft)
        .map(|draft| (draft.event_type().to_owned(), draft.schema_version()))
        .map_err(|why| (why.reason.code().to_owned(), why.path.as_str().to_owned()))
}

/// The member at a `.`-separated path of a parsed record; a member the record does not carry
/// reads as `None`, so a wrong record shows as the mismatch at that member.
fn at<'a>(record: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(record, Value::get)
}

/// The member `name` of the record's one range (`payload.ranges[0].<name>`); `None` when the
/// record carries no range or no such member.
fn range_at<'a>(record: &'a Value, name: &str) -> Option<&'a Value> {
    at(record, "payload.ranges")
        .and_then(Value::as_array)
        .and_then(|ranges| ranges.first())
        .and_then(|range| range.get(name))
}

/// §4.8.1 "Coverage" (AU-8) and DEC-787 item 1: the run reads the events of `from_seq` to
/// `to_seq` in one snapshot and walks them in `seq` order; `checked` counts the range's events
/// walked, and `pass` requires `checked = to_seq − from_seq + 1` (rule 132). The same snapshot
/// holds the rows after `to_seq`, which the segment checks compare but never count (§4.8.1
/// "Coverage"), so a range ending before the head walks its own positions only, crosses its
/// trusted start's segment boundary without stopping at it, and records `to_hash` as the event at
/// `to_seq`'s, never the head's.
#[test]
fn a_clean_range_walks_every_event_but_never_its_tail() {
    let (rows, hashes) = clean_chain(&own(), 12);
    let (segment, manifest_hash, bytes) = segment_start(&own(), 1, 4, hashes[2], 12, hashes[11]);
    let cold = Cold(manifest_hash, bytes);
    let manifest = StartRequest::Manifest { manifest_hash };
    let middle = run_of(
        &own(),
        (4, Some(10)),
        manifest,
        &snap(12, vec![segment], rows),
        &Stores {
            cold: &cold,
            artifacts: &NoArtifacts,
        },
        &by_user(),
    )
    .map(|run| outcome(&run));
    assert_eq!(
        middle,
        passed(7, hashes[9]),
        "4..=10 out of a chain of 12, from a segment at 4, across the segment's end"
    );
    let (whole, whole_hashes) = clean_chain(&own(), 5);
    let full = own_walked((1, None), &snap(5, Vec::new(), whole));
    assert_eq!(
        full,
        passed(5, whole_hashes[4]),
        "1..=5, a genesis range to the head"
    );
    let (tail_rows, _) = clean_chain(&own(), 5);
    let head = own_walked((1, Some(3)), &snap(5, Vec::new(), tail_rows));
    assert_eq!(
        head,
        passed(3, whole_hashes[2]),
        "1..=3 of a chain of 5: the tail rows are never counted"
    );
}

/// §11 check 6 and DEC-897 item 5(a) on the check's other half: the `config_refs` values of the
/// range's rows are `sha256:` refs (journal §6.3), so the same existence-and-re-hash loop reads
/// them — a config the store cannot answer fails `artifact_missing` at its event, an outage
/// never a pass, and a store answering the right bytes keeps the range whole. The wrong-bytes
/// arm fails `artifact_mismatch` beside them.
#[test]
fn a_config_ref_the_store_cannot_answer_fails_the_range_alike() {
    let under = ArtifactRef::of(b"a config the range reads");
    let reference = under.to_string();
    let mut rows = Vec::new();
    let mut hashes = Vec::new();
    let mut prev = ZERO;
    for seq in 1..=5 {
        let written = if seq == 3 {
            configured(3, prev, &reference)
        } else {
            row(&own(), seq, prev, "MarkUpdated", mark(), &[])
        };
        prev = written.hash;
        hashes.push(written.hash);
        rows.push(written);
    }
    let cases = [
        (
            "the store answers with the right bytes",
            Ok(b"a config the range reads".to_vec()),
            passed(5, hashes[4]),
        ),
        (
            "the store does not answer",
            Err(ArtifactError::Unavailable),
            failed("artifact_missing", 3, 2),
        ),
        (
            "the store answers with other bytes",
            Ok(b"other bytes".to_vec()),
            failed("artifact_mismatch", 3, 2),
        ),
    ];
    for (name, answer, want) in cases {
        let answered = OneArtifact(under, answer);
        let got = run_of(
            &own(),
            (1, Some(5)),
            StartRequest::Genesis,
            &snap(5, Vec::new(), rows.clone()),
            &Stores {
                cold: &NoCold,
                artifacts: &answered,
            },
            &by_user(),
        )
        .map(|run| outcome(&run));
        assert_eq!(got, want, "{name}");
    }
}

/// §4.8.1 "Coverage": "The first event read must be `from_seq` and the last `to_seq`"; §11 check 5:
/// `prev_hash` equals "the previous event's hash (or the trusted start)". The position
/// `from_seq` is judged against the trusted start's `prev_hash` whatever the row there holds: a
/// first row chaining elsewhere fails `prev_hash_mismatch` at `from_seq` (§9.13 rule 111: the
/// position walked, never the `seq` the row there names), and a store that lost its `from_seq`
/// row fails `seq_gap` at `from_seq` (AU-8's seeded deletion at the boundary); each walked
/// nothing.
#[test]
fn the_walk_starts_at_the_from_event_and_its_trusted_start() {
    let (prefix, prefix_hashes) = clean_chain(&own(), 3);
    let (range_rows, range_hashes) = chained_from(&own(), (4, prefix_hashes[2]), 7);
    let mut lost = range_rows.clone();
    let removed = lost.remove(0);
    assert_eq!(removed.seq, 4, "the fixture removes the from_seq row");
    let mut elsewhere = range_rows.clone();
    elsewhere[0] = row(&own(), 4, word("a"), "MarkUpdated", mark(), &[]);
    let (segment, manifest_hash, bytes) =
        segment_start(&own(), 1, 4, prefix_hashes[2], 10, range_hashes[6]);
    let manifest = StartRequest::Manifest { manifest_hash };
    let cases = [
        (
            "a first row chaining elsewhere",
            elsewhere,
            failed("prev_hash_mismatch", 4, 0),
        ),
        (
            "a store that lost its from_seq row",
            lost,
            failed("seq_gap", 4, 0),
        ),
    ];
    for (name, rows, want) in cases {
        let mut chain_seen = prefix.clone();
        chain_seen.extend(rows);
        let cold = Cold(manifest_hash, bytes.clone());
        let got = run_of(
            &own(),
            (4, Some(10)),
            manifest,
            &snap(10, vec![segment.clone()], chain_seen),
            &Stores {
                cold: &cold,
                artifacts: &NoArtifacts,
            },
            &by_user(),
        )
        .map(|run| outcome(&run));
        assert_eq!(got, want, "{name}");
    }
}

/// AU-8: "a missing, reordered, or extra event fails", §11 check 3 (`seq_gap` at the position
/// walked): a deleted row, a duplicated row, two swapped rows and a truncated read each fail
/// `seq_gap` at the position the walk first cannot reconcile, counting only the positions
/// before it — never past the recorded failure (rule 132's bound on `checked`).
#[test]
fn missing_reordered_duplicated_and_truncated_ranges_fail_seq_gap() {
    let (rows, hashes) = clean_chain(&own(), 8);
    let mut deleted = rows.clone();
    let at = deleted.remove(4);
    assert_eq!(at.seq, 5, "the fixture deletes the seq 5 row");
    let mut duplicated = rows.clone();
    duplicated.insert(2, rows[1].clone());
    let mut swapped = rows.clone();
    swapped.swap(5, 6);
    let cases = [
        ("a deleted row", deleted, failed("seq_gap", 5, 4)),
        ("a duplicated row", duplicated, failed("seq_gap", 3, 2)),
        ("two swapped rows", swapped, failed("seq_gap", 6, 5)),
        (
            "a truncated read",
            rows[..6].to_vec(),
            failed("seq_gap", 7, 6),
        ),
    ];
    for (name, rows, want) in cases {
        let got = own_walked((1, Some(8)), &snap(8, Vec::new(), rows));
        assert_eq!(got, want, "{name}");
    }
    let walked = own_walked((1, None), &snap(8, Vec::new(), rows.clone()));
    assert_eq!(walked, passed(8, hashes[7]), "the clean chain, to the head");
}

/// §11 checks 1 and 2 at the run's own level (the seeded property pins check 4's rehash and the
/// walk pins checks 3 and 5; the journal's own unit tests pin `check_row`, this is the run over
/// them): a row the walk reaches whose body does not re-canonicalize to its own bytes fails
/// `non_canonical`, and one whose `seq` column is not its body's fails `column_mismatch` — each
/// at the position the walk judged, whatever the failing row's own `seq` column says, counting
/// only the positions that walked before it (§9.13 rule 111); the clean chain still passes.
#[test]
fn non_canonical_and_column_mismatch_rows_fail_at_their_positions() {
    let (rows, hashes) = clean_chain(&own(), 6);
    let mut spaced = rows.clone();
    spaced[3].body.insert(1, b' ');
    assert_eq!(
        own_walked((1, Some(6)), &snap(6, Vec::new(), spaced)),
        failed("non_canonical", 4, 3),
        "a body that does not re-canonicalize fails check 1 at its position"
    );
    let mut mislabeled = rows.clone();
    mislabeled[2].seq = mislabeled[2].seq.saturating_add(1);
    assert_eq!(
        own_walked((1, Some(6)), &snap(6, Vec::new(), mislabeled)),
        failed("column_mismatch", 3, 2),
        "a seq column that is not its body's fails check 2 at its position"
    );
    assert_eq!(
        own_walked((1, Some(6)), &snap(6, Vec::new(), rows)),
        passed(6, hashes[5]),
        "the clean chain still passes"
    );
}

/// AU-1 and DEC-916: a verification reads only streams whose `stream_id` names the caller's
/// workspace, and the walk's rows are no exception — another workspace's rows (cleanly chained,
/// so no per-event check can tell them from the range's own), a sibling stream's of the same
/// workspace, one row of another stream among the range's own, and rows whose `stream_id` does
/// not parse all refuse the run the one `NotFound` (DEC-760 item 4's shape), before any position
/// is judged and with nothing recorded, as does a run of the deployment meter stream of the
/// workspace's own segment, no workspace's (journal spec v0.53 §2); the range's own rows still
/// walk.
#[test]
fn rows_of_another_stream_refuse_the_run_alike() {
    let (own_rows, own_hashes) = clean_chain(&own(), 3);
    let mut one_foreign = own_rows.clone();
    one_foreign.push(row(
        &format!("acct:{}:ACCT1", text(WS_B)),
        4,
        own_hashes[2],
        "MarkUpdated",
        mark(),
        &[],
    ));
    let cases = [
        (
            "another workspace's rows",
            clean_chain(&format!("acct:{}:ACCT1", text(WS_B)), 6).0,
        ),
        (
            "a sibling stream's rows",
            clean_chain(&format!("acct:{}:ACCT2", text(WS_A)), 6).0,
        ),
        (
            "rows whose stream id does not parse",
            clean_chain("acct::ACCT1", 6).0,
        ),
        ("one row of another stream among them", one_foreign),
    ];
    for (name, rows) in cases {
        let got = own_walked((1, Some(6)), &snap(6, Vec::new(), rows));
        assert_eq!(got, Err(NOT_FOUND), "{name}");
    }
    let deployment = format!("dmeter:{}", text(WS_A));
    let walked = run_of(
        &deployment,
        (1, Some(6)),
        StartRequest::Genesis,
        &snap(6, Vec::new(), clean_chain(&deployment, 6).0),
        &answered_by_nothing(),
        &by_user(),
    );
    assert_eq!(
        walked.err(),
        Some(NOT_FOUND),
        "the deployment meter stream of the workspace's own segment"
    );
    let (whole, whole_hashes) = clean_chain(&own(), 6);
    let walked = own_walked((1, Some(6)), &snap(6, Vec::new(), whole));
    assert_eq!(
        walked,
        passed(6, whole_hashes[5]),
        "the range's own clean rows"
    );
}

/// §11 check 6 and DEC-897 item 5(a): every referenced artifact exists and re-hashes, and an
/// artifact the store cannot answer fails `artifact_missing` at its event — an outage of the
/// store is a found fault, never a pass. Bytes that do not re-hash to their reference fail
/// `artifact_mismatch`; a store answering with the right bytes keeps the range whole.
#[test]
fn an_artifact_the_store_cannot_answer_fails_artifact_missing() {
    let under = ArtifactRef::of(b"an artifact the range reads");
    let reference = under.to_string();
    let mut rows = Vec::new();
    let mut hashes = Vec::new();
    let mut prev = ZERO;
    for seq in 1..=5 {
        let refs: Vec<&str> = if seq == 3 { vec![&reference] } else { vec![] };
        let written = row(&own(), seq, prev, "MarkUpdated", mark(), &refs);
        prev = written.hash;
        hashes.push(written.hash);
        rows.push(written);
    }
    let cases = [
        (
            "the store answers with the right bytes",
            Ok(b"an artifact the range reads".to_vec()),
            passed(5, hashes[4]),
        ),
        (
            "the store does not answer",
            Err(ArtifactError::Unavailable),
            failed("artifact_missing", 3, 2),
        ),
        (
            "the store answers with other bytes",
            Ok(b"other bytes".to_vec()),
            failed("artifact_mismatch", 3, 2),
        ),
        (
            "nothing is stored under the reference",
            Err(ArtifactError::Missing),
            failed("artifact_missing", 3, 2),
        ),
    ];
    for (name, answer, want) in cases {
        let answered = OneArtifact(under, answer);
        let got = run_of(
            &own(),
            (1, Some(5)),
            StartRequest::Genesis,
            &snap(5, Vec::new(), rows.clone()),
            &Stores {
                cold: &NoCold,
                artifacts: &answered,
            },
            &by_user(),
        )
        .map(|run| outcome(&run));
        assert_eq!(got, want, "{name}");
    }
}

/// §4.8.1 "The record" and §4.8: the run's record is the `VerificationRun` version 2 draft
/// appended on the workspace's own `ctl:{ws}` with `trigger` `request` and one checked range,
/// every member §9.13 lists at the value the walk found — `to_hash` `null` exactly when a
/// failure left none (rule 111) — `causation_id` `null` (DEC-897 item 4), and rule 110's actor
/// for the requester. §9.13's own validator (`Draft::parse`) accepts each record: rules 107,
/// 110, 111, 132 and 133 judge it as the journal's append will.
#[test]
fn the_record_drafts_the_run_and_claims_no_cause() {
    let user = by_user();
    let service = written_by(
        RequesterKind::System,
        "control_services",
        "0.1.0",
        Some(word("d")),
    );
    let (full_rows, full_hashes) = clean_chain(&own(), 6);
    let passed_run = run_of(
        &own(),
        (1, None),
        StartRequest::Genesis,
        &snap(6, Vec::new(), full_rows),
        &answered_by_nothing(),
        &user,
    )
    .expect("a clean genesis range runs");
    let record = parse(passed_run.draft()).expect("the record is canonical JSON");
    assert_eq!(
        judged(passed_run.draft()),
        Ok(("VerificationRun".to_owned(), 2)),
        "§9.13's validator accepts the record"
    );
    let zero = ZERO.to_hex();
    let stream = own();
    let control = ctl();
    let writer = event_id(99);
    let head_walked = full_hashes[5].to_hex();
    let members = [
        ("stream_id", control.as_str()),
        ("event_id", writer.as_str()),
        ("payload.trigger", "request"),
        ("payload.result", "pass"),
    ];
    for (path, want) in members {
        assert_eq!(
            at(&record, path).and_then(Value::as_str),
            Some(want),
            "the genesis pass record"
        );
    }
    let range_members = [
        ("stream_id", stream.as_str()),
        ("prev_hash", zero.as_str()),
        ("to_hash", head_walked.as_str()),
    ];
    for (path, want) in range_members {
        assert_eq!(
            range_at(&record, path).and_then(Value::as_str),
            Some(want),
            "the genesis pass record"
        );
    }
    let counts = [("from_seq", 1), ("to_seq", 6), ("checked", 6)];
    for (path, want) in counts {
        assert_eq!(
            range_at(&record, path).and_then(Value::as_int),
            Some(want),
            "the genesis pass record"
        );
    }
    for path in ["causation_id", "correlation_id"] {
        assert_eq!(
            at(&record, path),
            Some(&Value::Null),
            "the genesis pass record"
        );
    }
    for path in ["failure", "incomplete"] {
        assert_eq!(
            range_at(&record, path),
            Some(&Value::Null),
            "the genesis pass record"
        );
    }
    let requester = [
        ("actor.kind", "user"),
        ("actor.id", "user_owner_01"),
        ("actor.version", "1"),
    ];
    for (path, want) in requester {
        assert_eq!(
            at(&record, path).and_then(Value::as_str),
            Some(want),
            "the genesis pass record: rule 110's actor"
        );
    }
    assert_eq!(
        at(&record, "actor.build"),
        Some(&Value::Null),
        "a user carries no build"
    );
    assert_eq!(
        at(&record, "payload.ranges")
            .and_then(Value::as_array)
            .map(<[Value]>::len),
        Some(1),
        "a requested run records exactly one range"
    );
    let start = range_at(&record, "start").cloned().unwrap_or(Value::Null);
    assert_eq!(
        start.get("kind").and_then(Value::as_str),
        Some("genesis"),
        "rule 132's start kind"
    );
    assert_eq!(
        start.get("manifest_hash"),
        Some(&Value::Null),
        "the start's other member is null"
    );
    assert_eq!(
        start.get("anchor_event_id"),
        Some(&Value::Null),
        "the start's other member is null"
    );

    let (rows, hashes) = clean_chain(&own(), 12);
    let (segment, manifest_hash, bytes) = segment_start(&own(), 1, 4, hashes[2], 12, hashes[11]);
    let manifest = StartRequest::Manifest { manifest_hash };
    let mut corrupt = rows.clone();
    corrupt[6].hash = word("c");
    let head_walked = hashes[9].to_hex();
    let cases = [
        (
            "a clean manifest range",
            &rows[..],
            &service,
            (7, None, None, "pass", Some(head_walked.clone())),
        ),
        (
            "a range failing check 4 at seq 7",
            &corrupt[..],
            &user,
            (3, Some("rehash_mismatch"), Some(7), "fail", None),
        ),
    ];
    for (name, rows, written_by_envelope, seen) in cases {
        let (checked, failed_check, failed_seq, result, to_hash) = seen;
        let cold = Cold(manifest_hash, bytes.clone());
        let run = run_of(
            &own(),
            (4, Some(10)),
            manifest,
            &snap(12, vec![segment.clone()], rows.to_vec()),
            &Stores {
                cold: &cold,
                artifacts: &NoArtifacts,
            },
            written_by_envelope,
        )
        .expect(name);
        let record = parse(run.draft()).expect("the record is canonical JSON");
        assert_eq!(
            judged(run.draft()),
            Ok(("VerificationRun".to_owned(), 2)),
            "{name}: §9.13's validator accepts the record"
        );
        assert_eq!(
            at(&record, "causation_id"),
            Some(&Value::Null),
            "{name}: the requested run claims no cause (DEC-897 item 4)"
        );
        assert_eq!(
            range_at(&record, "from_seq").and_then(Value::as_int),
            Some(4),
            "{name}"
        );
        assert_eq!(
            range_at(&record, "to_seq").and_then(Value::as_int),
            Some(10),
            "{name}"
        );
        assert_eq!(
            range_at(&record, "prev_hash").and_then(Value::as_str),
            Some(hashes[2].to_hex().as_str()),
            "{name}: the trusted start the plan bound (rule 107)"
        );
        assert_eq!(
            range_at(&record, "checked").and_then(Value::as_int),
            Some(checked),
            "{name}"
        );
        assert_eq!(
            range_at(&record, "to_hash").and_then(Value::as_str),
            to_hash.as_deref(),
            "{name}: `to_hash` is null exactly when a failure left none (rule 111)"
        );
        assert_eq!(
            at(&record, "payload.result").and_then(Value::as_str),
            Some(result),
            "{name}"
        );
        let found_failure = range_at(&record, "failure").cloned();
        assert_eq!(
            range_at(&record, "failure") == Some(&Value::Null),
            failed_check.is_none(),
            "{name}: `failure` is null exactly when the walk found none"
        );
        assert_eq!(
            found_failure
                .as_ref()
                .and_then(|found| found.get("check").and_then(Value::as_str)),
            failed_check,
            "{name}"
        );
        assert_eq!(
            found_failure
                .as_ref()
                .and_then(|found| found.get("seq").and_then(Value::as_int)),
            failed_seq,
            "{name}"
        );
        assert_eq!(
            range_at(&record, "incomplete"),
            Some(&Value::Null),
            "{name}: a range has one outcome (rule 132)"
        );
        let start = range_at(&record, "start").cloned().unwrap_or(Value::Null);
        assert_eq!(
            start.get("kind").and_then(Value::as_str),
            Some("manifest"),
            "{name}"
        );
        assert_eq!(
            start.get("manifest_hash").and_then(Value::as_str),
            Some(manifest_hash.to_hex().as_str()),
            "{name}: bare hex, as the record holds §9.13's digest"
        );
        assert_eq!(start.get("anchor_event_id"), Some(&Value::Null), "{name}");
        assert_eq!(
            at(&record, "actor.kind").and_then(Value::as_str),
            Some(written_by_envelope.requester.kind.code()),
            "{name}: rule 110's actor kind"
        );
        match written_by_envelope.requester.kind {
            RequesterKind::User => assert_eq!(
                at(&record, "actor.build"),
                Some(&Value::Null),
                "{name}: a user carries no build"
            ),
            RequesterKind::System => assert_eq!(
                at(&record, "actor.build").and_then(Value::as_str),
                Some(format!("sha256:{}", word("d").to_hex()).as_str()),
                "{name}: a system actor's build"
            ),
        }
    }

    let anchor = anchor_start(&own(), 1, 9, word("1"));
    let (anchored_rows, _) = chained_from(&own(), (10, word("9")), 3);
    let mut anchored = clean_chain(&own(), 9).0;
    anchored.extend(anchored_rows);
    let run = run_of(
        &own(),
        (10, Some(12)),
        StartRequest::Anchor {
            anchor_event_id: &anchor.event_id,
        },
        &snap(12, vec![anchor.clone()], anchored),
        &answered_by_nothing(),
        &user,
    )
    .expect("an anchor-start range runs and fails");
    let record = parse(run.draft()).expect("the record is canonical JSON");
    assert_eq!(
        judged(run.draft()),
        Ok(("VerificationRun".to_owned(), 2)),
        "an anchor start's range with a non-null failure parses (rule 132)"
    );
    let start = range_at(&record, "start").cloned().unwrap_or(Value::Null);
    assert_eq!(start.get("kind").and_then(Value::as_str), Some("anchor"));
    assert_eq!(start.get("manifest_hash"), Some(&Value::Null));
    assert_eq!(
        start.get("anchor_event_id").and_then(Value::as_str),
        Some(anchor.event_id.as_str())
    );
    let failure = range_at(&record, "failure").cloned().unwrap_or(Value::Null);
    assert_eq!(
        failure.get("check").and_then(Value::as_str),
        Some("prev_hash_mismatch")
    );
    assert_eq!(failure.get("seq").and_then(Value::as_int), Some(10));
    assert_eq!(
        range_at(&record, "checked").and_then(Value::as_int),
        Some(0)
    );
    assert_eq!(
        range_at(&record, "to_hash"),
        Some(&Value::Null),
        "a failure left no head (rule 111)"
    );
    assert_eq!(
        range_at(&record, "incomplete"),
        Some(&Value::Null),
        "a failed range records one outcome (rule 132)"
    );
    assert_eq!(
        at(&record, "payload.result").and_then(Value::as_str),
        Some("fail")
    );
}

/// §4.8.1 "Result" and "The record", AU-6: `GET /verifications/{id}` "rebuilds the result from
/// that record alone", so every member the route serves — the stream and range (`to_seq` never
/// `null`), `checked`, `first_failure` (its `seq` inside the range, rule 111), `incomplete`, the
/// trusted start and the three-way `result` — reads out of the drafted record at the value the
/// run serves, on a passed range and a failed one alike.
#[test]
fn the_result_rebuilds_from_the_record_alone() {
    let (rows, hashes) = clean_chain(&own(), 9);
    let (segment, manifest_hash, bytes) = segment_start(&own(), 1, 4, hashes[2], 9, hashes[8]);
    let manifest = StartRequest::Manifest { manifest_hash };
    let mut corrupt = rows.clone();
    corrupt[5].hash = word("c");
    let cases = [
        ("a clean range", rows.clone(), passed(6, hashes[8])),
        (
            "a range failing check 4",
            corrupt,
            failed("rehash_mismatch", 6, 2),
        ),
    ];
    for (name, rows, want) in cases {
        let cold = Cold(manifest_hash, bytes.clone());
        let run = run_of(
            &own(),
            (4, Some(9)),
            manifest,
            &snap(9, vec![segment.clone()], rows),
            &Stores {
                cold: &cold,
                artifacts: &NoArtifacts,
            },
            &by_user(),
        )
        .expect(name);
        let record = parse(run.draft()).expect("the record is canonical JSON");
        let stream = own();
        assert_eq!(
            (
                range_at(&record, "stream_id").and_then(Value::as_str),
                range_at(&record, "from_seq").and_then(Value::as_int),
                range_at(&record, "to_seq").and_then(Value::as_int),
                Some(range_at(&record, "to_seq") != Some(&Value::Null)),
                range_at(&record, "checked").and_then(Value::as_int),
                range_at(&record, "to_hash").and_then(Value::as_str),
                at(&record, "payload.result").and_then(Value::as_str),
            ),
            (
                Some(stream.as_str()),
                Some(run.from_seq()),
                Some(run.to_seq()),
                Some(true),
                Some(run.checked()),
                run.to_hash().map(|hash| hash.to_hex()).as_deref(),
                Some(run.result().code()),
            ),
            "{name}: the served members rebuild from the record"
        );
        let found_failure = range_at(&record, "failure").cloned();
        assert_eq!(
            range_at(&record, "failure") == Some(&Value::Null),
            run.first_failure().is_none(),
            "{name}: the record's failure is null exactly when the run serves none (rule 111)"
        );
        assert_eq!(
            found_failure
                .as_ref()
                .and_then(|found| found.get("check").and_then(Value::as_str)),
            run.first_failure().map(|failure| failure.check),
            "{name}: the record's failure is the served first_failure (rule 111)"
        );
        assert_eq!(
            found_failure
                .as_ref()
                .and_then(|found| found.get("seq").and_then(Value::as_int)),
            run.first_failure().and_then(|failure| failure.seq),
            "{name}: the first failure's position serves from the record (rule 111)"
        );
        assert_eq!(
            range_at(&record, "incomplete") == Some(&Value::Null),
            run.incomplete().is_none(),
            "{name}: the record's incomplete is null on a range that is not incomplete (rule 132)"
        );
        assert_eq!(
            range_at(&record, "incomplete")
                .and_then(|incomplete| incomplete.get("check").and_then(Value::as_str)),
            run.incomplete().map(|incomplete| incomplete.check()),
            "{name}: only the token check can be incomplete (§11)"
        );
        assert_eq!(
            range_at(&record, "incomplete")
                .and_then(|incomplete| incomplete.get("cause").and_then(Value::as_str)),
            run.incomplete().map(|incomplete| incomplete.cause.code()),
            "{name}: the record's incomplete is the served one (DEC-789 item 7)"
        );
        let start = range_at(&record, "start").cloned().unwrap_or(Value::Null);
        let start_hash = start
            .get("manifest_hash")
            .and_then(Value::as_str)
            .and_then(Digest::from_hex);
        match run.trusted_start() {
            RecordedStart::Manifest { manifest_hash } => {
                assert_eq!(
                    start.get("kind").and_then(Value::as_str),
                    Some("manifest"),
                    "{name}"
                );
                assert_eq!(
                    start_hash,
                    Some(*manifest_hash),
                    "{name}: the trusted start serves from the record (DEC-788 item 1)"
                );
                assert_eq!(start.get("anchor_event_id"), Some(&Value::Null), "{name}");
            }
            RecordedStart::Genesis => {
                assert_eq!(
                    start.get("kind").and_then(Value::as_str),
                    Some("genesis"),
                    "{name}"
                );
                assert_eq!(start.get("manifest_hash"), Some(&Value::Null), "{name}");
                assert_eq!(start.get("anchor_event_id"), Some(&Value::Null), "{name}");
            }
            RecordedStart::Anchor { anchor_event_id } => {
                assert_eq!(
                    start.get("kind").and_then(Value::as_str),
                    Some("anchor"),
                    "{name}"
                );
                assert_eq!(start.get("manifest_hash"), Some(&Value::Null), "{name}");
                assert_eq!(
                    start.get("anchor_event_id").and_then(Value::as_str),
                    Some(anchor_event_id.as_str()),
                    "{name}"
                );
            }
        }
        let seen_run: Seen = Ok(outcome(&run));
        assert_eq!(seen_run, want, "{name}: the oracle's case holds");
    }
}

/// AU-8's oracle over seeded corruptions of the stored range ("An oracle that counts the range
/// independently; seeded deletions, duplicates, and truncations of the stored range each fail"):
/// each seeded case's expectation is computed from the mutation and the hashes of the chain this
/// test built, never from the walk's own judgement, and the walk must meet it — the `checked`
/// count, the first failure's check and position, the head it verified, and the three-way result
/// (rules 132 and 133).
#[test]
fn seeded_corruptions_fail_where_the_oracle_walks() {
    let strategy = (2u64..=24, proptest::option::of(1u64..24), 0u8..6, 0u8..255);
    let body = |(head, tail, kind, raw): (u64, Option<u64>, u8, u8)| -> Result<(), TestCaseError> {
        let (mut rows, hashes) = clean_chain(&own(), head);
        let at = 1 + raw as u64 % head;
        let want = match kind {
            0 => passed(head, hashes[head as usize - 1]),
            1 => {
                rows.remove(at as usize - 1);
                failed("seq_gap", at, at - 1)
            }
            2 => {
                rows.insert(at as usize, rows[at as usize - 1].clone());
                if at < head {
                    failed("seq_gap", at + 1, at)
                } else {
                    failed("seq_gap", head, head)
                }
            }
            3 => {
                let swap_at = 1 + raw as u64 % (head - 1);
                rows.swap(swap_at as usize - 1, swap_at as usize);
                failed("seq_gap", swap_at, swap_at - 1)
            }
            4 => match tail {
                None => passed(head, hashes[head as usize - 1]),
                Some(kept) => {
                    let kept = kept % head;
                    rows.truncate(kept as usize);
                    failed("seq_gap", kept + 1, kept)
                }
            },
            _ => {
                rows[at as usize - 1] = row(&own(), at, word("c"), "MarkUpdated", mark(), &[]);
                failed("prev_hash_mismatch", at, at - 1)
            }
        };
        let got = own_walked((1, Some(head)), &snap(head, Vec::new(), rows));
        let why = format!("head {head}, kind {kind} at {at}: want {want:?}, got {got:?}");
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
