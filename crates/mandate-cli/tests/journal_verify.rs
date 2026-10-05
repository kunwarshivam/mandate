//! `mandate journal verify`: the ordered checks of journal spec §11 over an exported segment
//! (§6.2), its artifact store (§6.3), and an anchor (§10), with the exact report, the exit status
//! behind the outcome, and argument parsing (backlog E5-4).
//!
//! Oracles: the journal test vectors in `fixtures/refcases/journal.json`, the same file
//! `mandate-refcases` reads. Export lines are built here from each vector's `canonical` body and
//! `hash` as spec §6.2 words the format, not with the journal's own `export_line`, so the reader is
//! checked against the format rather than against its writer; `export_line_seq_1` pins that
//! construction. Every tamper case's expected first failure is read from the vector file, not
//! written here, so a changed vector changes the assertion.

use std::fs;
use std::path::{Path, PathBuf};

use clap::Parser;
use mandate_artifacts_fs::FsArtifactStore;
use mandate_canon::Digest;
use mandate_cli::journal::{self, JournalCommand, Outcome, Refusal, Span, VerifyArgs};
use mandate_cli::{Cli, Command};
use mandate_journal::{
    Anchor, AnchorLeaf, AppendOutcome, ArtifactRef, ArtifactStore, MemoryJournal, RangeCheck,
    StreamId, export_segment,
};
use mandate_time::UtcNanos;

/// A hash that is not in the chain, for rewritten `prev_hash` values (as `mandate-refcases` uses).
const FOREIGN_HASH: &str = "abababababababababababababababababababababababababababababababab";
const ZERO_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";
/// The stream the vectors' chain belongs to.
const VECTOR_STREAM: &str = "acct:ws_01J8Z2:01J8Z2ACCT00000000000000A1";

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "mandate-cli-journal-{label}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    /// Writes `bytes` to `name` inside the scratch directory and returns its path as text.
    fn file(&self, name: &str, bytes: &[u8]) -> String {
        let path = self.0.join(name);
        fs::write(&path, bytes).unwrap();
        path.to_str().unwrap().to_owned()
    }

    fn dir(&self, name: &str) -> String {
        let path = self.0.join(name);
        fs::create_dir_all(&path).unwrap();
        path.to_str().unwrap().to_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn parse(argv: &[&str]) -> Result<VerifyArgs, clap::Error> {
    let mut all = vec!["mandate", "journal", "verify"];
    all.extend_from_slice(argv);
    match Cli::try_parse_from(all)?.command {
        Command::Journal(JournalCommand::Verify(args)) => Ok(args),
        other => unreachable!("{other:?} is not journal verify"),
    }
}

/// The journal test vectors.
fn vectors() -> serde_json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    serde_json::from_slice(&fs::read(&path).unwrap()).unwrap()
}

/// Materializes the config artifacts declared by the journal vectors.
fn vector_store(scratch: &Scratch) -> String {
    let store = scratch.dir("vector-artifacts");
    let mut artifacts = FsArtifactStore::open(store.as_str()).unwrap();
    for fixture in vectors()["artifacts"].as_array().unwrap() {
        let bytes = fixture["canonical"].as_str().unwrap().as_bytes();
        let reference = artifacts.put_artifact(bytes).unwrap();
        assert_eq!(
            reference.to_string(),
            fixture["ref"].as_str().unwrap(),
            "the fixture reference must be the digest of its canonical bytes"
        );
    }
    store
}

/// The tamper case named `name`.
fn tamper_case(name: &str) -> serde_json::Value {
    vectors()["tamper_cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"].as_str() == Some(name))
        .unwrap_or_else(|| panic!("no tamper case {name}"))
        .clone()
}

/// The `(seq, check)` a tamper case expects of the per-event checks.
fn expected_event_failure(name: &str) -> (u64, String) {
    let case = tamper_case(name);
    (
        case["expect"]["seq"].as_u64().unwrap(),
        case["expect"]["check"].as_str().unwrap().to_owned(),
    )
}

/// The per-range check a tamper case expects after every per-event check passes.
fn expected_range_check(name: &str) -> String {
    let case = tamper_case(name);
    assert_eq!(case["expect"]["per_event"].as_str(), Some("pass"));
    case["expect"]["range_check"].as_str().unwrap().to_owned()
}

/// One event of an export: its canonical body text and the hash written beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Event {
    body: String,
    hash: String,
}

/// The vectors' five chain events, in `seq` order.
fn chain() -> Vec<Event> {
    vectors()["chain"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| Event {
            body: entry["canonical"].as_str().unwrap().to_owned(),
            hash: entry["hash"].as_str().unwrap().to_owned(),
        })
        .collect()
}

/// The export line of spec §6.2, written out from the format's own words.
fn line(event: &Event) -> String {
    format!("{{\"body\":{},\"hash\":\"{}\"}}", event.body, event.hash)
}

/// A segment file: one line per event, each followed by a line feed (spec §6.2).
fn export(events: &[Event]) -> Vec<u8> {
    events
        .iter()
        .map(|event| format!("{}\n", line(event)))
        .collect::<String>()
        .into_bytes()
}

/// The events of a segment file, read back by slicing each line as §6.2 describes: `"body"` sorts
/// before `"hash"`, so the body is an exact slice.
fn events_of(segment: &[u8]) -> Vec<Event> {
    String::from_utf8(segment.to_vec())
        .unwrap()
        .lines()
        .map(|text| {
            let inner = text
                .strip_prefix("{\"body\":")
                .and_then(|t| t.strip_suffix("\"}"))
                .unwrap();
            let (body, hash) = inner.split_at(inner.len() - 64);
            Event {
                body: body.strip_suffix(",\"hash\":\"").unwrap().to_owned(),
                hash: hash.to_owned(),
            }
        })
        .collect()
}

/// Overwrites the object for `reference` with `bytes`, as a disk fault or an intruder would.
fn tamper(store: &FsArtifactStore, reference: &ArtifactRef, bytes: &[u8]) {
    let path = store.object_path(reference);
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    #[expect(
        clippy::permissions_set_readonly_false,
        reason = "the test plays a writer that ignores the store's read-only mode"
    )]
    permissions.set_readonly(false);
    fs::set_permissions(&path, permissions).unwrap();
    fs::write(&path, bytes).unwrap();
}

fn replace_once(text: &str, from: &str, to: &str) -> String {
    assert_eq!(text.matches(from).count(), 1, "`{from}` occurs once");
    text.replacen(from, to, 1)
}

fn rehash(event: &mut Event) {
    event.hash = Digest::of(event.body.as_bytes()).to_hex();
}

/// The event at the vectors' 1-based `seq`.
fn at(events: &mut [Event], seq: u64) -> &mut Event {
    let index = usize::try_from(seq).unwrap() - 1;
    &mut events[index]
}

/// Verifies an export written into `scratch`, returning the outcome and the whole report.
fn verify(scratch: &Scratch, name: &str, bytes: &[u8], extra: &[&str]) -> (Outcome, String) {
    let path = scratch.file(name, bytes);
    let mut argv = vec![path.as_str()];
    argv.extend_from_slice(extra);
    let args = parse(&argv).unwrap();
    let mut report = Vec::new();
    let outcome = journal::verify(&args, &mut report).unwrap();
    (outcome, String::from_utf8(report).unwrap())
}

/// Verifies an export built from the journal vectors with their declared config artifacts.
fn verify_vector_export(
    scratch: &Scratch,
    name: &str,
    bytes: &[u8],
    extra: &[&str],
) -> (Outcome, String) {
    let store = vector_store(scratch);
    let mut args = vec!["--store", store.as_str()];
    args.extend_from_slice(extra);
    verify(scratch, name, bytes, &args)
}

/// Asserts `outcome` is the per-event failure tamper case `name` expects.
fn assert_tamper(name: &str, outcome: &Outcome) {
    let (seq, check) = expected_event_failure(name);
    match outcome {
        Outcome::Event(failure) => assert_eq!(
            (failure.seq, failure.check.code()),
            (seq, check.as_str()),
            "{name}"
        ),
        other => panic!("{name}: expected a per-event failure, got {other}"),
    }
    assert!(outcome.failed(), "{name} must exit non-zero");
    assert_eq!(outcome.code(), Some(check.as_str()));
}

/// The anchor file of spec §10: the vectors' leaves and root, as `AnchorComputed` records them.
fn anchor_json() -> Vec<u8> {
    let v = vectors();
    let leaves = v["merkle"]["leaves"]
        .as_array()
        .unwrap()
        .iter()
        .map(|leaf| {
            format!(
                "{{\"hash\":\"{}\",\"seq\":{},\"stream_id\":\"{}\"}}",
                leaf["hash"].as_str().unwrap(),
                leaf["seq"].as_u64().unwrap(),
                leaf["stream_id"].as_str().unwrap()
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"leaves\":[{leaves}],\"root\":\"{}\"}}",
        v["merkle"]["root"].as_str().unwrap()
    )
    .into_bytes()
}

/// An anchor file over `heads`, sorted and rooted the way `AnchorComputed` records it, so a test
/// can anchor a stream the export does not hold.
fn anchor_over(heads: Vec<(&str, u64, Digest)>) -> Vec<u8> {
    let anchor = Anchor::compute(
        heads
            .into_iter()
            .map(|(stream_id, seq, hash)| AnchorLeaf {
                stream_id: stream_id.to_owned(),
                seq,
                hash,
            })
            .collect(),
    )
    .unwrap();
    let leaves = anchor
        .leaves
        .iter()
        .map(|leaf| {
            format!(
                "{{\"hash\":\"{}\",\"seq\":{},\"stream_id\":\"{}\"}}",
                leaf.hash.to_hex(),
                leaf.seq,
                leaf.stream_id
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"leaves\":[{leaves}],\"root\":\"{}\"}}",
        anchor.root.to_hex()
    )
    .into_bytes()
}

#[test]
fn the_export_line_is_the_one_the_vectors_publish() {
    let events = chain();
    assert_eq!(
        line(&events[0]),
        vectors()["export_line_seq_1"].as_str().unwrap(),
        "the test builds §6.2 lines the way the vectors do"
    );
    assert_eq!(events_of(&export(&events)), events, "and reads them back");
}

#[test]
fn an_untampered_export_verifies_and_the_report_names_every_input() {
    let scratch = Scratch::new("verified");
    let events = chain();
    let path = scratch.file("segment.jsonl", &export(&events));
    let store = vector_store(&scratch);
    let args = parse(&[path.as_str(), "--store", store.as_str()]).unwrap();
    let mut report = Vec::new();
    let outcome = journal::verify(&args, &mut report).unwrap();
    assert_eq!(
        outcome,
        Outcome::Verified(Some(Span {
            stream_id: VECTOR_STREAM.to_owned(),
            first_seq: 1,
            last_seq: 5,
            last_hash: Digest::from_hex(&events[4].hash).unwrap(),
        }))
    );
    assert!(!outcome.failed());
    assert_eq!(outcome.code(), None);
    assert_eq!(
        String::from_utf8(report).unwrap(),
        format!(
            "export: {path}
lines: 5
trusted start: seq 1, prev_hash {ZERO_HASH}
artifact store: {store}
anchor: none given
result: verified, stream {VECTOR_STREAM}, seq 1 to 5, last hash {}
",
            events[4].hash
        )
    );
}

#[test]
fn a_modified_payload_is_rehash_mismatch() {
    let scratch = Scratch::new("payload");
    let mut events = chain();
    let third = at(&mut events, 3);
    third.body = replace_once(&third.body, r#""verdict":"allow""#, r#""verdict":"deny""#);
    let (outcome, _) = verify_vector_export(&scratch, "segment.jsonl", &export(&events), &[]);
    assert_tamper("payload_modified", &outcome);
}

#[test]
fn a_deleted_event_is_seq_gap() {
    let scratch = Scratch::new("deleted");
    let mut events = chain();
    events.remove(2);
    let (outcome, report) = verify_vector_export(&scratch, "segment.jsonl", &export(&events), &[]);
    assert_tamper("event_deleted", &outcome);
    assert!(report.contains("lines: 4"), "{report}");
    assert!(
        report.contains("result: failed, seq 4, seq_gap"),
        "{report}"
    );
}

#[test]
fn a_prev_hash_changed_without_rehashing_is_rehash_mismatch() {
    let scratch = Scratch::new("prev-stale");
    let mut events = chain();
    let third_hash = events[2].hash.clone();
    let fourth = at(&mut events, 4);
    fourth.body = replace_once(
        &fourth.body,
        &format!(r#""prev_hash":"{third_hash}""#),
        &format!(r#""prev_hash":"{FOREIGN_HASH}""#),
    );
    let (outcome, _) = verify_vector_export(&scratch, "segment.jsonl", &export(&events), &[]);
    assert_tamper("prev_hash_changed_without_rehash", &outcome);
}

#[test]
fn a_prev_hash_rewritten_and_rehashed_is_prev_hash_mismatch() {
    let scratch = Scratch::new("prev-rehashed");
    let mut events = chain();
    let third_hash = events[2].hash.clone();
    let fourth = at(&mut events, 4);
    fourth.body = replace_once(
        &fourth.body,
        &format!(r#""prev_hash":"{third_hash}""#),
        &format!(r#""prev_hash":"{FOREIGN_HASH}""#),
    );
    rehash(fourth);
    let (outcome, _) = verify_vector_export(&scratch, "segment.jsonl", &export(&events), &[]);
    assert_tamper("prev_hash_rewritten_and_rehashed", &outcome);
}

#[test]
fn whitespace_inserted_and_rehashed_is_non_canonical() {
    let scratch = Scratch::new("whitespace");
    let mut events = chain();
    let second = at(&mut events, 2);
    assert!(second.body.starts_with('{'));
    second.body.insert(1, ' ');
    rehash(second);
    let (outcome, _) = verify(&scratch, "segment.jsonl", &export(&events), &[]);
    assert_tamper("whitespace_inserted_and_rehashed", &outcome);
}

#[test]
fn swapped_seq_members_are_a_seq_gap_because_an_export_carries_no_columns() {
    let scratch = Scratch::new("seq-swap");
    let mut events = chain();
    let second = replace_once(&events[1].body, r#""seq":2"#, r#""seq":3"#);
    let third = replace_once(&events[2].body, r#""seq":3"#, r#""seq":2"#);
    at(&mut events, 2).body = second;
    at(&mut events, 3).body = third;
    let (outcome, _) = verify(&scratch, "segment.jsonl", &export(&events), &[]);
    let (seq, check) = expected_event_failure("seq_values_swapped");
    assert_eq!(
        (seq, check.as_str()),
        (2, "column_mismatch"),
        "the vector expects the column check, which needs the stored columns of §6.1"
    );
    match outcome {
        Outcome::Event(failure) => assert_eq!(
            (failure.seq, failure.check.code()),
            (3, "seq_gap"),
            "an export's columns come from the body, so the swap shows as a gap at the line that \
             claims seq 3 where seq 2 belongs"
        ),
        other => panic!("expected a per-event failure, got {other}"),
    }
}

#[test]
fn a_column_only_tamper_cannot_be_expressed_by_an_export() {
    let (seq, check) = expected_event_failure("column_altered");
    assert_eq!((seq, check.as_str()), (5, "column_mismatch"));
    assert_eq!(
        export(&chain()),
        export(&chain()),
        "the vector changes the event_type column alone, and an export stores no column apart \
         from the body, so its input is not expressible and the export is unchanged"
    );
}

#[test]
fn a_tail_truncated_after_an_anchor_passes_every_per_event_check_and_fails_the_anchor() {
    let scratch = Scratch::new("truncated");
    let mut events = chain();
    events.pop();
    let anchor = scratch.file("anchor.json", &anchor_json());
    let (outcome, report) = verify_vector_export(
        &scratch,
        "segment.jsonl",
        &export(&events),
        &["--anchor", anchor.as_str()],
    );
    let expected = expected_range_check("tail_truncated_after_anchor");
    assert_eq!(outcome, Outcome::Range(RangeCheck::AnchorHeadMismatch));
    assert_eq!(outcome.code(), Some(expected.as_str()));
    assert!(outcome.failed());
    assert!(report.contains(&format!("anchor: {anchor}")), "{report}");
    assert!(
        report.contains(&format!("result: failed, {expected}")),
        "{report}"
    );
    let (without_anchor, _) = verify_vector_export(&scratch, "plain.jsonl", &export(&events), &[]);
    assert!(
        !without_anchor.failed(),
        "every per-event check passes; only the anchor catches a truncated tail"
    );
}

#[test]
fn a_chain_rewritten_from_seq_3_passes_every_per_event_check_and_fails_the_anchor() {
    let scratch = Scratch::new("rewritten");
    let original = chain();
    let mut events = original.clone();
    let third = at(&mut events, 3);
    third.body = replace_once(&third.body, r#""verdict":"allow""#, r#""verdict":"deny""#);
    rehash(third);
    let mut previous = events[2].hash.clone();
    for seq in [4_u64, 5] {
        let stale = original[usize::try_from(seq).unwrap() - 2].hash.clone();
        let event = at(&mut events, seq);
        event.body = replace_once(
            &event.body,
            &format!(r#""prev_hash":"{stale}""#),
            &format!(r#""prev_hash":"{previous}""#),
        );
        rehash(event);
        previous = event.hash.clone();
    }
    let anchor = scratch.file("anchor.json", &anchor_json());
    let (outcome, _) = verify_vector_export(
        &scratch,
        "segment.jsonl",
        &export(&events),
        &["--anchor", anchor.as_str()],
    );
    assert_eq!(
        outcome.code(),
        Some(expected_range_check("chain_rewritten_from_seq_3").as_str())
    );
    let (without_anchor, _) = verify_vector_export(&scratch, "plain.jsonl", &export(&events), &[]);
    assert!(
        !without_anchor.failed(),
        "a rewritten chain is self-consistent; only the anchor catches it"
    );
}

#[test]
fn every_tamper_case_of_the_vectors_is_covered_by_a_test() {
    let covered: Vec<String> = [
        "payload_modified",
        "event_deleted",
        "seq_values_swapped",
        "prev_hash_changed_without_rehash",
        "prev_hash_rewritten_and_rehashed",
        "whitespace_inserted_and_rehashed",
        "column_altered",
        "tail_truncated_after_anchor",
        "chain_rewritten_from_seq_3",
    ]
    .iter()
    .map(|name| (*name).to_owned())
    .collect();
    let listed: Vec<String> = vectors()["tamper_cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        listed, covered,
        "a tamper case was added or renamed; cover it or say here why an export cannot express it"
    );
}

#[test]
fn an_untampered_export_passes_the_anchor_it_is_covered_by() {
    let scratch = Scratch::new("anchored");
    let anchor = scratch.file("anchor.json", &anchor_json());
    let events = chain();
    let (outcome, _) = verify_vector_export(
        &scratch,
        "segment.jsonl",
        &export(&events),
        &["--anchor", anchor.as_str()],
    );
    assert_eq!(
        outcome,
        Outcome::Verified(Some(Span {
            stream_id: VECTOR_STREAM.to_owned(),
            first_seq: 1,
            last_seq: 5,
            last_hash: Digest::from_hex(&events[4].hash).unwrap(),
        }))
    );
}

#[test]
fn an_anchor_whose_root_does_not_match_its_leaves_is_anchor_root_mismatch() {
    let scratch = Scratch::new("bad-root");
    let json = String::from_utf8(anchor_json()).unwrap();
    let broken = replace_once(
        &json,
        vectors()["merkle"]["root"].as_str().unwrap(),
        FOREIGN_HASH,
    );
    let anchor = scratch.file("anchor.json", broken.as_bytes());
    let (outcome, _) = verify_vector_export(
        &scratch,
        "segment.jsonl",
        &export(&chain()),
        &["--anchor", anchor.as_str()],
    );
    assert_eq!(outcome.code(), Some("anchor_root_mismatch"));
}

#[test]
fn an_anchor_file_that_is_not_one_is_refused_before_anything_verifies() {
    let scratch = Scratch::new("anchor-bad");
    let export_path = scratch.file("segment.jsonl", &export(&chain()));
    for (name, bytes) in [
        ("not-json.json", b"{".as_slice()),
        ("no-leaves.json", br#"{"root":"x"}"#.as_slice()),
        (
            "bad-leaf.json",
            br#"{"leaves":[{"hash":"aa","seq":1,"stream_id":"ctl:w"}],"root":"bb"}"#.as_slice(),
        ),
    ] {
        let anchor = scratch.file(name, bytes);
        let args = parse(&[export_path.as_str(), "--anchor", anchor.as_str()]).unwrap();
        let mut report = Vec::new();
        let err = journal::verify(&args, &mut report).unwrap_err();
        assert!(format!("{err:#}").contains(&anchor), "{name}: {err:#}");
        assert!(report.is_empty(), "{name}: nothing is reported as verified");
    }
}

#[test]
fn the_first_failure_in_spec_order_is_the_one_reported() {
    let scratch = Scratch::new("first");
    let mut events = chain();
    events.remove(1);
    let second = at(&mut events, 2);
    assert!(second.body.starts_with('{'));
    second.body.insert(1, ' ');
    let (outcome, _) = verify(&scratch, "segment.jsonl", &export(&events), &[]);
    match outcome {
        Outcome::Event(failure) => assert_eq!(
            failure.check.code(),
            "non_canonical",
            "the line is non-canonical, its hash is stale, and its seq gaps; check 1 comes first"
        ),
        other => panic!("expected a per-event failure, got {other}"),
    }
}

#[test]
fn a_final_line_without_its_line_feed_is_non_canonical() {
    let scratch = Scratch::new("no-lf");
    let mut bytes = export(&chain());
    assert_eq!(bytes.pop(), Some(b'\n'));
    let (outcome, _) = verify(&scratch, "segment.jsonl", &bytes, &[]);
    match outcome {
        Outcome::Event(failure) => assert_eq!(
            (failure.seq, failure.check.code()),
            (5, "non_canonical"),
            "spec §6.2 terminates every line, so an unterminated one is truncated"
        ),
        other => panic!("expected a per-event failure, got {other}"),
    }
}

#[test]
fn a_line_that_is_not_an_export_line_is_non_canonical_at_the_seq_it_should_hold() {
    let scratch = Scratch::new("garbage");
    let events = chain();
    let second = &events[1];
    for (label, bad) in [
        ("an empty line", String::new()),
        ("not json", "nonsense".to_owned()),
        ("the body alone", second.body.clone()),
        (
            "keys out of order",
            format!("{{\"hash\":\"{}\",\"body\":{}}}", second.hash, second.body),
        ),
        (
            "an uppercase hash",
            format!(
                "{{\"body\":{},\"hash\":\"{}\"}}",
                second.body,
                second.hash.to_uppercase()
            ),
        ),
        (
            "a short hash",
            format!(
                "{{\"body\":{},\"hash\":\"{}\"}}",
                second.body,
                &second.hash[1..]
            ),
        ),
        ("trailing bytes", format!("{} ", line(second))),
    ] {
        let mut bytes = format!("{}\n", line(&events[0])).into_bytes();
        bytes.extend_from_slice(bad.as_bytes());
        bytes.push(b'\n');
        let (outcome, _) = verify(&scratch, "segment.jsonl", &bytes, &[]);
        match outcome {
            Outcome::Event(failure) => assert_eq!(
                (failure.seq, failure.check.code()),
                (2, "non_canonical"),
                "{label}"
            ),
            other => panic!("{label}: expected a per-event failure, got {other}"),
        }
    }
}

#[test]
fn carriage_returns_are_not_line_separators() {
    let scratch = Scratch::new("crlf");
    let bytes: Vec<u8> = chain()
        .iter()
        .map(|event| format!("{}\r\n", line(event)))
        .collect::<String>()
        .into_bytes();
    let (outcome, _) = verify(&scratch, "segment.jsonl", &bytes, &[]);
    assert_eq!(outcome.code(), Some("non_canonical"));
}

#[test]
fn a_body_missing_an_envelope_field_is_column_mismatch() {
    let scratch = Scratch::new("no-column");
    let mut events = chain();
    let second = at(&mut events, 2);
    second.body = replace_once(&second.body, r#""seq":2,"#, "");
    rehash(second);
    let (outcome, _) = verify(&scratch, "segment.jsonl", &export(&events), &[]);
    match outcome {
        Outcome::Event(failure) => assert_eq!(
            (failure.seq, failure.check.code()),
            (2, "column_mismatch"),
            "a column the body cannot supply is never treated as equal to it"
        ),
        other => panic!("expected a per-event failure, got {other}"),
    }
}

#[test]
fn an_empty_export_verifies_with_no_events() {
    let scratch = Scratch::new("empty");
    let (outcome, report) = verify(&scratch, "segment.jsonl", b"", &[]);
    assert_eq!(outcome, Outcome::Verified(None));
    assert!(report.contains("lines: 0"), "{report}");
    assert!(report.contains("result: verified, no events"), "{report}");
}

#[test]
fn a_partial_segment_verifies_from_its_trusted_start() {
    let scratch = Scratch::new("partial");
    let events = chain();
    let trusted = events[1].hash.clone();
    let (outcome, report) = verify_vector_export(
        &scratch,
        "segment.jsonl",
        &export(&events[2..]),
        &["--from-seq", "3", "--trusted-prev-hash", trusted.as_str()],
    );
    assert!(!outcome.failed(), "{outcome}");
    assert!(
        report.contains(&format!("trusted start: seq 3, prev_hash {trusted}")),
        "{report}"
    );
    assert!(report.contains("seq 3 to 5"), "{report}");
}

#[test]
fn a_trusted_start_the_segment_does_not_chain_to_is_prev_hash_mismatch() {
    let scratch = Scratch::new("bad-start");
    let events = chain();
    let (outcome, _) = verify_vector_export(
        &scratch,
        "segment.jsonl",
        &export(&events[2..]),
        &["--from-seq", "3", "--trusted-prev-hash", FOREIGN_HASH],
    );
    match outcome {
        Outcome::Event(failure) => assert_eq!(
            (failure.seq, failure.check.code()),
            (3, "prev_hash_mismatch"),
            "the start is trusted from a manifest or anchor, never read from the file itself"
        ),
        other => panic!("expected a per-event failure, got {other}"),
    }
}

#[test]
fn a_trusted_start_out_of_range_or_malformed_is_refused() {
    let scratch = Scratch::new("start-bad");
    let path = scratch.file("segment.jsonl", &export(&chain()));
    let upper = FOREIGN_HASH.to_uppercase();
    for bad in [
        ["--from-seq", "0", "--trusted-prev-hash", ZERO_HASH],
        ["--from-seq", "1", "--trusted-prev-hash", "not-a-hash"],
        ["--from-seq", "1", "--trusted-prev-hash", upper.as_str()],
    ] {
        let mut argv = vec![path.as_str()];
        argv.extend_from_slice(&bad);
        let args = parse(&argv).unwrap();
        assert!(
            journal::verify(&args, &mut Vec::new()).is_err(),
            "{bad:?} must be refused"
        );
    }
}

#[test]
fn from_seq_and_the_trusted_prev_hash_are_given_together() {
    assert!(parse(&["x.jsonl", "--from-seq", "3"]).is_err());
    assert!(parse(&["x.jsonl", "--trusted-prev-hash", ZERO_HASH]).is_err());
    let args = parse(&["x.jsonl"]).unwrap();
    assert_eq!((args.from_seq, args.trusted_prev_hash), (None, None));
}

#[test]
fn verify_takes_the_export_path_and_the_optional_store_and_anchor() {
    assert!(parse(&[]).is_err());
    let bare = parse(&["s.jsonl"]).unwrap();
    assert_eq!(bare.export, PathBuf::from("s.jsonl"));
    assert_eq!((bare.store, bare.anchor), (None, None));
    let args = parse(&["s.jsonl", "--store", "art", "--anchor", "a.json"]).unwrap();
    assert_eq!(args.store, Some(PathBuf::from("art")));
    assert_eq!(args.anchor, Some(PathBuf::from("a.json")));
}

#[test]
fn an_export_that_mixes_streams_is_refused() {
    let scratch = Scratch::new("mixed");
    let mut events = events_of(&artifact_stream(""));
    let second = at(&mut events, 2);
    second.body = replace_once(&second.body, ARTIFACT_STREAM, "acct:ws_2:ACCT2");
    rehash(second);
    let path = scratch.file("segment.jsonl", &export(&events));
    let args = parse(&[path.as_str()]).unwrap();
    let err = journal::verify(&args, &mut Vec::new()).unwrap_err();
    assert!(
        format!("{err:#}").contains("mixes streams"),
        "a segment is one stream's contiguous range: {err:#}"
    );
}

#[test]
fn a_missing_export_names_the_path() {
    let scratch = Scratch::new("missing");
    let path = scratch.0.join("absent.jsonl");
    let args = parse(&[path.to_str().unwrap()]).unwrap();
    let err = journal::verify(&args, &mut Vec::new()).unwrap_err();
    assert!(format!("{err:#}").contains("absent.jsonl"), "{err:#}");
}

const ARTIFACT_STREAM: &str = "acct:ws_1:ACCT1";
const DRAFT_TIME: &str = "2026-09-21T14:00:00.000000000Z";
/// The contents of the artifact the `MarkUpdated` event references.
const ARTIFACT: &[u8] = b"model response";

fn draft(event_id: &str, event_type: &str, payload: &str, artifact_refs: &str) -> String {
    format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{event_id}",
        "stream_id":"{ARTIFACT_STREAM}","event_type":"{event_type}","schema_version":1,
        "event_time":"{DRAFT_TIME}","clock_source":"local","causation_id":null,
        "correlation_id":null,"actor":{{"kind":"system","id":"executor","version":"0.1.0",
        "build":"sha256:{}"}},"config_refs":{{}},"payload":{payload},
        "artifact_refs":[{artifact_refs}],"pii_refs":[]}}"#,
        "3".repeat(64)
    )
}

/// A two-event stream, `StreamOpened` then a `MarkUpdated` whose `payload.source` is `reference`
/// (`quote`, with no artifact, when it is empty), exported as a segment. It is sealed by the
/// journal's own append protocol, so the rows are exactly what a store would hold.
fn artifact_stream(reference: &str) -> Vec<u8> {
    let stream = StreamId::parse(ARTIFACT_STREAM).unwrap();
    let mut journal = MemoryJournal::new();
    let epoch = journal.take_ownership(&stream);
    let at = UtcNanos::parse(DRAFT_TIME).unwrap();
    let opened = draft(
        "01J8Z3M4000000000000000000",
        "StreamOpened",
        r#"{"stream_type":"account","workspace_id":"ws_1","broker":"alpaca","account_ref":"ACCT1"}"#,
        "",
    );
    let (source, refs) = if reference.is_empty() {
        ("quote".to_owned(), String::new())
    } else {
        (reference.to_owned(), format!("\"{reference}\""))
    };
    let mark = draft(
        "01J8Z3M4000000000000000001",
        "MarkUpdated",
        &format!(
            r#"{{"instrument_id":"inst","price":"1","source":"{source}","feed":"iex",
            "risk_clock":"{DRAFT_TIME}"}}"#
        ),
        &refs,
    );
    for (head, text) in [(0, opened), (1, mark)] {
        let outcome = journal.append(&stream, head, epoch, at, &[text.as_bytes()]);
        assert!(
            matches!(outcome, AppendOutcome::Committed(_)),
            "{outcome:?}"
        );
    }
    export_segment(journal.rows(&stream))
}

#[test]
fn an_event_whose_artifact_is_stored_and_rehashes_verifies() {
    let scratch = Scratch::new("artifact-ok");
    let reference = ArtifactRef::of(ARTIFACT);
    let store = scratch.dir("artifacts");
    let mut fs_store = FsArtifactStore::open(store.as_str()).unwrap();
    assert_eq!(fs_store.put_artifact(ARTIFACT), Ok(reference));
    let (outcome, report) = verify(
        &scratch,
        "segment.jsonl",
        &artifact_stream(&reference.to_string()),
        &["--store", store.as_str()],
    );
    assert!(!outcome.failed(), "{outcome}");
    assert!(
        report.contains(&format!("artifact store: {store}")),
        "{report}"
    );
}

#[test]
fn an_event_whose_artifact_is_not_stored_is_artifact_missing() {
    let scratch = Scratch::new("artifact-gone");
    let reference = ArtifactRef::of(ARTIFACT);
    let store = scratch.dir("artifacts");
    let (outcome, _) = verify(
        &scratch,
        "segment.jsonl",
        &artifact_stream(&reference.to_string()),
        &["--store", store.as_str()],
    );
    match outcome {
        Outcome::Event(failure) => assert_eq!(
            (failure.seq, failure.check.code()),
            (2, "artifact_missing"),
            "spec §11 check 6 tells an absent artifact from an altered one"
        ),
        other => panic!("expected a per-event failure, got {other}"),
    }
}

#[test]
fn an_event_whose_artifact_was_altered_is_artifact_mismatch() {
    let scratch = Scratch::new("artifact-altered");
    let reference = ArtifactRef::of(ARTIFACT);
    let store = scratch.dir("artifacts");
    let mut fs_store = FsArtifactStore::open(store.as_str()).unwrap();
    fs_store.put_artifact(ARTIFACT).unwrap();
    tamper(&fs_store, &reference, b"tampered");
    let (outcome, _) = verify(
        &scratch,
        "segment.jsonl",
        &artifact_stream(&reference.to_string()),
        &["--store", store.as_str()],
    );
    match outcome {
        Outcome::Event(failure) => assert_eq!(
            (failure.seq, failure.check.code()),
            (2, "artifact_mismatch"),
            "the bytes are re-hashed on the way out, never trusted for being in place"
        ),
        other => panic!("expected a per-event failure, got {other}"),
    }
}

#[test]
fn without_a_store_an_event_that_names_an_artifact_is_artifact_missing() {
    let scratch = Scratch::new("artifact-nostore");
    let reference = ArtifactRef::of(ARTIFACT);
    let (outcome, report) = verify(
        &scratch,
        "segment.jsonl",
        &artifact_stream(&reference.to_string()),
        &[],
    );
    assert_eq!(outcome.code(), Some("artifact_missing"));
    assert!(report.contains("artifact store: none given"), "{report}");
}

#[test]
fn an_export_with_no_artifact_reference_verifies_without_a_store() {
    let scratch = Scratch::new("artifact-none");
    let segment = artifact_stream("");
    let (outcome, _) = verify(&scratch, "segment.jsonl", &segment, &[]);
    let events = events_of(&segment);
    assert_eq!(
        outcome,
        Outcome::Verified(Some(Span {
            stream_id: ARTIFACT_STREAM.to_owned(),
            first_seq: 1,
            last_seq: 2,
            last_hash: Digest::from_hex(&events[1].hash).unwrap(),
        }))
    );
}

#[test]
fn a_store_path_that_is_not_a_directory_is_refused_rather_than_created() {
    let scratch = Scratch::new("store-bad");
    let export_path = scratch.file("segment.jsonl", &export(&chain()));
    let missing = scratch.0.join("absent");
    let args = parse(&[export_path.as_str(), "--store", missing.to_str().unwrap()]).unwrap();
    let err = journal::verify(&args, &mut Vec::new()).unwrap_err();
    assert!(format!("{err:#}").contains("not a directory"), "{err:#}");
    assert!(!missing.exists(), "a mistyped store path is not created");
}

#[test]
fn identical_inputs_give_identical_output() {
    let scratch = Scratch::new("deterministic");
    let events = chain();
    let store = vector_store(&scratch);
    let anchor = scratch.file("anchor.json", &anchor_json());
    let path = scratch.file("segment.jsonl", &export(&events));
    let run = |file: &str, extra: Vec<&str>| {
        let mut argv = vec![file];
        argv.extend_from_slice(&extra);
        let args = parse(&argv).unwrap();
        let mut report = Vec::new();
        let outcome = journal::verify(&args, &mut report).unwrap();
        (outcome, report)
    };
    let with = vec!["--store", store.as_str(), "--anchor", anchor.as_str()];
    let (outcome, report) = run(&path, with.clone());
    assert_eq!((outcome.clone(), report.clone()), run(&path, with));
    assert!(matches!(outcome, Outcome::Verified(Some(_))), "{outcome}");
    assert!(
        !report.is_empty(),
        "the report is what must be reproducible"
    );

    let mut tampered = events.clone();
    at(&mut tampered, 3).body = replace_once(
        &events[2].body,
        r#""verdict":"allow""#,
        r#""verdict":"deny""#,
    );
    let failing = scratch.file("tampered.jsonl", &export(&tampered));
    let with_store = vec!["--store", store.as_str()];
    let (failed, failed_report) = run(&failing, with_store.clone());
    assert_eq!(
        (failed.clone(), failed_report),
        run(&failing, with_store),
        "a failing run repeats its report too"
    );
    assert_eq!(failed.code(), Some("rehash_mismatch"));
}

#[test]
fn an_empty_export_with_an_anchor_is_anchor_head_mismatch() {
    let scratch = Scratch::new("empty-anchored");
    let anchor = scratch.file("anchor.json", &anchor_json());
    let (outcome, report) = verify(
        &scratch,
        "segment.jsonl",
        b"",
        &["--anchor", anchor.as_str()],
    );
    assert_eq!(
        outcome,
        Outcome::Range(RangeCheck::AnchorHeadMismatch),
        "an export with no event holds no anchored head, so a wholly truncated segment never \
         passes the anchor it is covered by"
    );
    assert!(outcome.failed());
    assert!(report.contains("lines: 0"), "{report}");
    assert!(
        report.contains("result: failed, anchor_head_mismatch"),
        "{report}"
    );
}

#[test]
fn an_anchor_that_names_no_leaf_for_the_exports_stream_is_refused() {
    let scratch = Scratch::new("anchor-elsewhere");
    let elsewhere = anchor_over(vec![("acct:ws_other:OTHERACCT", 9, Digest::of(b"o"))]);
    let anchor = scratch.file("anchor.json", &elsewhere);
    let path = scratch.file("segment.jsonl", &export(&chain()));
    let store = vector_store(&scratch);
    let args = parse(&[
        path.as_str(),
        "--store",
        store.as_str(),
        "--anchor",
        anchor.as_str(),
    ])
    .unwrap();
    let mut report = Vec::new();
    let err = journal::verify(&args, &mut report).unwrap_err();
    let text = format!("{err:#}");
    assert!(
        text.starts_with(Refusal::AnchorStream.code()),
        "an anchor that says nothing about this stream is never reported as checked: {text}"
    );
    assert!(text.contains(VECTOR_STREAM), "{text}");
    assert!(report.is_empty(), "nothing is reported as verified");
}

#[test]
fn an_anchor_covering_more_streams_than_the_export_still_checks_its_own() {
    let scratch = Scratch::new("anchor-extra");
    let events = chain();
    let head = Digest::from_hex(&events[4].hash).unwrap();
    let wide = anchor_over(vec![
        (VECTOR_STREAM, 5, head),
        ("acct:ws_other:OTHERACCT", 9, Digest::of(b"o")),
        ("ctl:ws_other", 2, Digest::of(b"c")),
    ]);
    let anchor = scratch.file("anchor.json", &wide);
    let (ok, _) = verify_vector_export(
        &scratch,
        "segment.jsonl",
        &export(&events),
        &["--anchor", anchor.as_str()],
    );
    assert!(!ok.failed(), "{ok}");

    let stale = anchor_over(vec![
        (VECTOR_STREAM, 5, Digest::of(b"not the head")),
        ("acct:ws_other:OTHERACCT", 9, Digest::of(b"o")),
    ]);
    let anchor = scratch.file("stale.json", &stale);
    let (bad, _) = verify_vector_export(
        &scratch,
        "stale.jsonl",
        &export(&events),
        &["--anchor", anchor.as_str()],
    );
    assert_eq!(bad, Outcome::Range(RangeCheck::AnchorHeadMismatch));
}

#[test]
fn the_reported_line_count_agrees_with_the_seq_a_failure_names() {
    let scratch = Scratch::new("line-count");
    let mut bytes = export(&chain());
    bytes.push(b'\n');
    let (outcome, report) = verify(&scratch, "segment.jsonl", &bytes, &[]);
    assert!(
        report.contains("lines: 6"),
        "the blank sixth line is counted, not skipped: {report}"
    );
    assert!(
        report.contains("result: failed, seq 6, non_canonical"),
        "{report}"
    );
    match outcome {
        Outcome::Event(failure) => assert_eq!(failure.seq, 6),
        other => panic!("expected a per-event failure, got {other}"),
    }
}

#[test]
fn every_refusal_reports_its_stable_code_first() {
    let scratch = Scratch::new("codes");
    let events = chain();
    let path = scratch.file("segment.jsonl", &export(&events));
    let store = vector_store(&scratch);
    let store_file = scratch.file("not-a-dir", b"x");
    let bad_anchor = scratch.file("bad.json", b"{");
    let elsewhere = scratch.file(
        "elsewhere.json",
        &anchor_over(vec![("ctl:ws_other", 2, Digest::of(b"c"))]),
    );
    let mut mixed = events_of(&artifact_stream(""));
    let second = at(&mut mixed, 2);
    second.body = replace_once(&second.body, ARTIFACT_STREAM, "acct:ws_2:ACCT2");
    rehash(second);
    let mixed = scratch.file("mixed.jsonl", &export(&mixed));
    let cases: Vec<(Refusal, Vec<&str>)> = vec![
        (
            Refusal::TrustedStart,
            vec![
                path.as_str(),
                "--from-seq",
                "0",
                "--trusted-prev-hash",
                ZERO_HASH,
            ],
        ),
        (
            Refusal::ArtifactStore,
            vec![path.as_str(), "--store", store_file.as_str()],
        ),
        (
            Refusal::Anchor,
            vec![path.as_str(), "--anchor", bad_anchor.as_str()],
        ),
        (
            Refusal::AnchorStream,
            vec![
                path.as_str(),
                "--store",
                store.as_str(),
                "--anchor",
                elsewhere.as_str(),
            ],
        ),
        (Refusal::ExportStreams, vec![mixed.as_str()]),
    ];
    for (reason, argv) in cases {
        let args = parse(&argv).unwrap();
        let err = journal::verify(&args, &mut Vec::new()).unwrap_err();
        let text = format!("{err:#}");
        assert!(
            text.starts_with(&format!("{}: ", reason.code())),
            "{:?} must report `{}` first, got {text}",
            reason,
            reason.code()
        );
    }
    let codes: Vec<&str> = [
        Refusal::TrustedStart,
        Refusal::ArtifactStore,
        Refusal::Anchor,
        Refusal::AnchorStream,
        Refusal::ExportStreams,
        Refusal::ExportStreamId,
    ]
    .iter()
    .map(|r| r.code())
    .collect();
    assert_eq!(
        codes,
        [
            "trusted_start_invalid",
            "artifact_store_unusable",
            "anchor_invalid",
            "anchor_covers_another_stream",
            "export_mixes_streams",
            "export_stream_id_invalid"
        ],
        "the codes are stable: a change here is a change auditors' scripts see"
    );
}
