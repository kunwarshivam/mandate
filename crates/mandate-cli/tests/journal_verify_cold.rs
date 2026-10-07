//! `mandate journal verify-cold`: journal spec §11's per-range checks over a cold-store export —
//! a directory of segment files and their manifests (§6.2) — against a trusted start, an artifact
//! store (§6.3), an anchor (§10) and its timestamp token, with the exact report, the exit status
//! behind the outcome, the refusals and their codes, and argument parsing (backlog E5-8, DEC-490).
//!
//! Oracles: the journal test vectors in `fixtures/refcases/journal.json` for the per-event tamper
//! cases, replayed through a cold export as E5-4 replays them through a segment file, and a
//! stream the journal's own append protocol sealed for the cold cases. Every segment file is
//! written from §6.2's own words (`{"body":…,"hash":"…"}` and a line feed), and every manifest
//! is the test's own six-field object read back from the file's first and last lines
//! ([`manifest_for`]), never the cold crate's derivation — one live test pins that the two agree.
//! Every expected `seq` and code is computed from the scenario the test built, and every
//! pending test fails on the stub with the stub's own report (DEC-77, DEC-137).

use std::fs;
use std::path::{Path, PathBuf};

use clap::Parser;
use mandate_artifacts_fs::FsArtifactStore;
use mandate_canon::{Digest, Int, Key, Object, Value, parse, to_canonical};
use mandate_cli::journal::cold::{self, ColdOutcome, TSA_VERIFICATION_INCOMPLETE, VerifyColdArgs};
use mandate_cli::journal::{JournalCommand, Refusal, Span};
use mandate_cli::{Cli, Command};
use mandate_journal::{
    Anchor, AnchorLeaf, AppendOutcome, ArtifactRef, ArtifactStore, EventCheck, EventFailure,
    MemoryJournal, RangeCheck, StoredEvent, StreamId, export_segment,
};
use mandate_journal_cold::{ColdCheck, ColdFailure, SegmentManifest};
use mandate_time::UtcNanos;

/// A hash that is not in any chain, for rewritten `prev_hash` values and lying manifests.
const FOREIGN_HASH: &str = "abababababababababababababababababababababababababababababababab";
const ZERO_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";
/// The stream the vectors' chain belongs to.
const VECTOR_STREAM: &str = "acct:ws_01J8Z2:01J8Z2ACCT00000000000000A1";
/// The stream the sealed eight-row fixture belongs to.
const STREAM: &str = "acct:ws_1:ACCT1";
const DRAFT_TIME: &str = "2026-09-21T14:00:00.000000000Z";

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("mandate-cli-cold-{label}-{}", std::process::id()));
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

    /// An empty cold export directory named `name`.
    fn export(&self, name: &str) -> Export {
        Export(PathBuf::from(self.dir(name)))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A cold export directory under construction: `<name>.jsonl` beside `<name>.manifest.json`
/// (DEC-490 item 2).
struct Export(PathBuf);

impl Export {
    fn path(&self) -> &str {
        self.0.to_str().unwrap()
    }

    /// Writes segment `name` with the manifest the oracle derives from `file`.
    fn segment(&self, name: &str, file: &[u8]) {
        self.raw(name, file, &manifest_for(file));
    }

    /// Writes segment `name` with exactly these manifest bytes.
    fn raw(&self, name: &str, file: &[u8], manifest: &[u8]) {
        self.file_only(name, file);
        self.manifest_only(name, manifest);
    }

    fn file_only(&self, name: &str, file: &[u8]) {
        fs::write(self.0.join(format!("{name}.jsonl")), file).unwrap();
    }

    fn manifest_only(&self, name: &str, manifest: &[u8]) {
        fs::write(self.0.join(format!("{name}.manifest.json")), manifest).unwrap();
    }
}

fn parse_args(argv: &[&str]) -> Result<VerifyColdArgs, clap::Error> {
    let mut all = vec!["mandate", "journal", "verify-cold"];
    all.extend_from_slice(argv);
    match Cli::try_parse_from(all)?.command {
        Command::Journal(JournalCommand::VerifyCold(args)) => Ok(args),
        other => unreachable!("{other:?} is not journal verify-cold"),
    }
}

/// Verifies the export at `export`, returning the outcome and the whole report.
fn verify_cold(export: &str, extra: &[&str]) -> (ColdOutcome, String) {
    let mut argv = vec![export];
    argv.extend_from_slice(extra);
    let args = parse_args(&argv).unwrap();
    let mut report = Vec::new();
    let outcome = cold::verify(&args, &mut report).unwrap();
    (outcome, String::from_utf8(report).unwrap())
}

/// The refusal the command answers for `export`, as text, with nothing reported.
fn refusal(export: &str, extra: &[&str]) -> String {
    let mut argv = vec![export];
    argv.extend_from_slice(extra);
    let args = parse_args(&argv).unwrap();
    let mut report = Vec::new();
    let err = cold::verify(&args, &mut report).unwrap_err();
    assert!(
        report.is_empty(),
        "nothing is reported for a refused input: {}",
        String::from_utf8(report).unwrap()
    );
    format!("{err:#}")
}

/// A segment check's failure where the range expected `at_seq`.
fn segment_failure(at_seq: u64, check: ColdCheck) -> ColdOutcome {
    ColdOutcome::Cold(ColdFailure::Segment { at_seq, check })
}

/// A per-event failure inside a segment.
fn event_failure(seq: u64, check: EventCheck) -> ColdOutcome {
    ColdOutcome::Cold(ColdFailure::Event(EventFailure { seq, check }))
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

fn replace_once(text: &str, from: &str, to: &str) -> String {
    assert_eq!(text.matches(from).count(), 1, "`{from}` occurs once");
    text.replacen(from, to, 1)
}

fn rehash(event: &mut Event) {
    event.hash = Digest::of(event.body.as_bytes()).to_hex();
}

/// The event at the 1-based `seq`.
fn at(events: &mut [Event], seq: u64) -> &mut Event {
    let index = usize::try_from(seq).unwrap() - 1;
    &mut events[index]
}

/// The member `name` of a canonical body.
fn member(body: &str, name: &str) -> Value {
    parse(body.as_bytes()).unwrap().get(name).unwrap().clone()
}

/// DEC-263's six-field manifest object in canonical form, as the test's own oracle writes it.
fn manifest_bytes(
    stream: &str,
    first_seq: u64,
    last_seq: u64,
    first_prev_hash: &str,
    last_hash: &str,
    file_sha256: &str,
) -> Vec<u8> {
    let mut object = Object::new();
    object.insert(Key::new("stream").unwrap(), Value::Str(stream.to_owned()));
    object.insert(
        Key::new("first_seq").unwrap(),
        Value::Int(Int::new(first_seq).unwrap()),
    );
    object.insert(
        Key::new("last_seq").unwrap(),
        Value::Int(Int::new(last_seq).unwrap()),
    );
    object.insert(
        Key::new("first_prev_hash").unwrap(),
        Value::Str(first_prev_hash.to_owned()),
    );
    object.insert(
        Key::new("last_hash").unwrap(),
        Value::Str(last_hash.to_owned()),
    );
    object.insert(
        Key::new("file_sha256").unwrap(),
        Value::Str(file_sha256.to_owned()),
    );
    to_canonical(&Value::Object(object))
}

/// The manifest §6.2 names for `file`, as the test's oracle reads it off the file itself: the
/// stream, the first `seq` and `prev_hash` from the first line's body, the last `seq` from the
/// last line's body, the last line's hash, and the file's own SHA-256.
fn manifest_for(file: &[u8]) -> Vec<u8> {
    let events = events_of(file);
    let first = events.first().expect("a segment holds a first line");
    let last = events.last().expect("a segment holds a last line");
    manifest_bytes(
        member(&first.body, "stream_id").as_str().unwrap(),
        member(&first.body, "seq").as_int().unwrap(),
        member(&last.body, "seq").as_int().unwrap(),
        member(&first.body, "prev_hash").as_str().unwrap(),
        &last.hash,
        &Digest::of(file).to_hex(),
    )
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

/// An anchor file over `heads`, sorted and rooted the way `AnchorComputed` records it, and its
/// root, so a test can anchor any head and stamp its root.
fn anchor_over(heads: Vec<(&str, u64, Digest)>) -> (Vec<u8>, Digest) {
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
    (
        format!(
            "{{\"leaves\":[{leaves}],\"root\":\"{}\"}}",
            anchor.root.to_hex()
        )
        .into_bytes(),
        anchor.root,
    )
}

/// A timestamp token that claims `root`: §10's imprint, SHA-256 of the 32 raw root bytes, as the
/// test's own digest, inside some other bytes.
fn token_for(root: &Digest) -> Vec<u8> {
    let mut token = b"TSTInfo ".to_vec();
    token.extend_from_slice(Digest::of(root.as_bytes()).as_bytes());
    token.extend_from_slice(b" signed by nobody yet");
    token
}

fn event_id(n: u64) -> String {
    format!("01J8Z3M4{n:018}")
}

fn opened_draft() -> String {
    format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{}","stream_id":"{STREAM}",
        "event_type":"StreamOpened","schema_version":1,"event_time":"{DRAFT_TIME}","clock_source":"local",
        "causation_id":null,"correlation_id":null,
        "actor":{{"kind":"system","id":"executor","version":"0.1.0","build":"sha256:{}"}},
        "config_refs":{{}},"payload":{{"stream_type":"account","workspace_id":"ws_1","broker":"alpaca",
        "account_ref":"ACCT1"}},"artifact_refs":[],"pii_refs":[]}}"#,
        event_id(0),
        "3".repeat(64)
    )
}

fn mark_draft(n: u64) -> String {
    format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{}","stream_id":"{STREAM}",
        "event_type":"MarkUpdated","schema_version":1,"event_time":"{DRAFT_TIME}","clock_source":"local",
        "causation_id":null,"correlation_id":null,
        "actor":{{"kind":"system","id":"executor","version":"0.1.0","build":"sha256:{}"}},
        "config_refs":{{}},"payload":{{"instrument_id":"inst","price":"1","source":"quote",
        "feed":"iex","risk_clock":"{DRAFT_TIME}"}},"artifact_refs":[],"pii_refs":[]}}"#,
        event_id(n),
        "3".repeat(64)
    )
}

/// Eight rows of one stream, `seq` 1 to 8 — `StreamOpened` then seven `MarkUpdated` — sealed by
/// the journal's own append protocol, so the rows are exactly what a store would hold. None
/// references an artifact.
fn sealed_rows() -> Vec<StoredEvent> {
    let stream = StreamId::parse(STREAM).unwrap();
    let mut journal = MemoryJournal::new();
    let epoch = journal.take_ownership(&stream);
    let now = UtcNanos::parse(DRAFT_TIME).unwrap();
    let outcome = journal.append(&stream, 0, epoch, now, &[opened_draft().as_bytes()]);
    assert!(
        matches!(outcome, AppendOutcome::Committed(_)),
        "{outcome:?}"
    );
    for n in 1..=7 {
        let outcome = journal.append(&stream, n, epoch, now, &[mark_draft(n).as_bytes()]);
        assert!(
            matches!(outcome, AppendOutcome::Committed(_)),
            "{outcome:?}"
        );
    }
    journal.rows(&stream).to_vec()
}

/// The sealed rows' segment file for `seq` `from` to `to`, inclusive.
fn sealed_file(rows: &[StoredEvent], from: u64, to: u64) -> Vec<u8> {
    let first = usize::try_from(from).unwrap() - 1;
    let last = usize::try_from(to).unwrap();
    export_segment(&rows[first..last])
}

/// The eight sealed rows as text events, for tampering.
fn sealed_events(rows: &[StoredEvent]) -> Vec<Event> {
    events_of(&export_segment(rows))
}

/// Asserts `outcome` is the per-event failure tamper case `name` expects, and that the report
/// names it at the same `seq`.
fn assert_tamper(name: &str, outcome: &ColdOutcome, report: &str) {
    let (seq, check) = expected_event_failure(name);
    match outcome {
        ColdOutcome::Cold(ColdFailure::Event(failure)) => assert_eq!(
            (failure.seq, failure.check.code()),
            (seq, check.as_str()),
            "{name}"
        ),
        other => panic!("{name}: expected a per-event failure, got {other}"),
    }
    assert!(outcome.failed(), "{name} must exit non-zero");
    assert_eq!(outcome.code(), Some(check.as_str()));
    assert!(
        report.contains(&format!("result: failed, seq {seq}, {check}")),
        "{name}: {report}"
    );
}

/// The vectors' chain as a two-segment cold export, `seq` 1 to 3 and 4 to 5, with the manifests
/// the oracle derives from each file.
fn vector_export(scratch: &Scratch, name: &str, events: &[Event]) -> Export {
    let export_dir = scratch.export(name);
    export_dir.segment("seq-1", &export(&events[..3]));
    export_dir.segment("seq-4", &export(&events[3..]));
    export_dir
}

#[test]
fn verify_cold_takes_the_export_directory_and_the_optional_inputs() {
    assert!(parse_args(&[]).is_err());
    let bare = parse_args(&["cold"]).unwrap();
    assert_eq!(bare.export, PathBuf::from("cold"));
    assert_eq!((bare.store, bare.anchor, bare.token), (None, None, None));
    assert_eq!((bare.from_seq, bare.trusted_prev_hash), (None, None));
    let args = parse_args(&[
        "cold",
        "--store",
        "art",
        "--anchor",
        "a.json",
        "--token",
        "t.tsr",
        "--from-seq",
        "3",
        "--trusted-prev-hash",
        ZERO_HASH,
    ])
    .unwrap();
    assert_eq!(args.store, Some(PathBuf::from("art")));
    assert_eq!(args.anchor, Some(PathBuf::from("a.json")));
    assert_eq!(args.token, Some(PathBuf::from("t.tsr")));
    assert_eq!(args.from_seq, Some(3));
    assert_eq!(args.trusted_prev_hash.as_deref(), Some(ZERO_HASH));
}

#[test]
fn a_token_needs_its_anchor_and_the_trusted_start_comes_as_a_pair() {
    assert!(
        parse_args(&["cold", "--token", "t.tsr"]).is_err(),
        "a token is checked against an anchor's root, so it has no meaning without one"
    );
    assert!(parse_args(&["cold", "--from-seq", "3"]).is_err());
    assert!(parse_args(&["cold", "--trusted-prev-hash", ZERO_HASH]).is_err());
}

#[test]
fn the_cold_outcome_codes_and_result_lines_are_pinned() {
    let span = Span {
        stream_id: STREAM.to_owned(),
        first_seq: 3,
        last_seq: 8,
        last_hash: Digest::of(b"the last hash"),
    };
    let verified = ColdOutcome::Verified(span.clone());
    assert_eq!(verified.code(), None);
    assert!(!verified.failed());
    assert_eq!(
        verified.to_string(),
        format!(
            "verified, stream {STREAM}, seq 3 to 8, last hash {}",
            span.last_hash.to_hex()
        )
    );
    let cases = [
        (
            event_failure(4, EventCheck::RehashMismatch),
            "rehash_mismatch",
            "failed, seq 4, rehash_mismatch",
        ),
        (
            segment_failure(5, ColdCheck::SegmentGap),
            "segment_gap",
            "failed, seq 5, segment_gap",
        ),
        (
            segment_failure(1, ColdCheck::SegmentManifestMismatch),
            "segment_manifest_mismatch",
            "failed, seq 1, segment_manifest_mismatch",
        ),
        (
            ColdOutcome::Cold(ColdFailure::TsaTokenInvalid),
            "tsa_token_invalid",
            "failed, tsa_token_invalid",
        ),
        (
            ColdOutcome::Cold(ColdFailure::TsaVerificationIncomplete),
            "tsa_verification_incomplete",
            "failed, tsa_verification_incomplete",
        ),
        (
            ColdOutcome::Range(RangeCheck::AnchorHeadMismatch),
            "anchor_head_mismatch",
            "failed, anchor_head_mismatch",
        ),
        (
            ColdOutcome::Range(RangeCheck::AnchorRootMismatch),
            "anchor_root_mismatch",
            "failed, anchor_root_mismatch",
        ),
    ];
    for (outcome, code, text) in cases {
        assert_eq!(outcome.code(), Some(code), "{outcome:?}");
        assert!(outcome.failed(), "{outcome:?} must exit non-zero");
        assert_eq!(outcome.to_string(), text);
    }
    assert_eq!(
        TSA_VERIFICATION_INCOMPLETE, "tsa_verification_incomplete",
        "the one word that is not §11's: a token check that cannot finish (DEC-490 item 6)"
    );
}

#[test]
fn the_cold_refusal_codes_are_pinned() {
    assert_eq!(
        [
            Refusal::Unreadable.code(),
            Refusal::ColdExportIncomplete.code()
        ],
        ["path_unreadable", "cold_export_incomplete"],
        "the codes are stable: a change here is a change auditors' scripts see"
    );
}

#[test]
fn the_oracle_s_manifest_is_the_one_the_cold_crate_derives() {
    let rows = sealed_rows();
    let file = export_segment(&rows[2..6]);
    let derived = SegmentManifest::of(&rows[2..6])
        .unwrap()
        .to_canonical_bytes()
        .unwrap()
        .as_slice()
        .to_vec();
    assert_eq!(
        manifest_for(&file),
        derived,
        "the test reads DEC-263's six fields off the file's own lines; the crate derives them \
         from the rows; the bytes agree"
    );
    assert_eq!(
        manifest_bytes(
            STREAM,
            3,
            6,
            &rows[2].prev_hash.to_hex(),
            &rows[5].hash.to_hex(),
            &Digest::of(&file).to_hex()
        ),
        derived,
        "and the oracle's fields are the rows' own edges"
    );
}

#[test]
fn every_tamper_case_of_the_vectors_is_covered_by_a_cold_test() {
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
        "a tamper case was added or renamed; cover it or say here why a cold export cannot express it"
    );
}

#[test]
fn a_column_only_tamper_cannot_be_expressed_by_a_cold_export() {
    let (seq, check) = expected_event_failure("column_altered");
    assert_eq!((seq, check.as_str()), (5, "column_mismatch"));
    let change = tamper_case("column_altered")["change"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        change.contains("column") && change.contains("FillReversed"),
        "the vector changes a stored column alone: {change}"
    );
    let file = export(&chain());
    assert!(
        !String::from_utf8(file.clone())
            .unwrap()
            .contains("FillReversed"),
        "a cold export stores each body and its hash, no column apart from them, so the altered \
         value has nowhere to go without changing a body, which is another case's input"
    );
    assert_eq!(
        events_of(&file)[4].body,
        chain()[4].body,
        "and the body at seq 5 is the vector's own"
    );
    let manifest = manifest_for(&file);
    assert!(
        !String::from_utf8(manifest)
            .unwrap()
            .contains("FillReversed"),
        "nor does a manifest carry a column: it reads its edges off the file's lines"
    );
}

#[test]
fn an_untampered_cold_export_verifies_and_the_report_names_every_input() {
    let scratch = Scratch::new("verified");
    let events = chain();
    let export_dir = vector_export(&scratch, "cold", &events);
    let store = vector_store(&scratch);
    let (outcome, report) = verify_cold(export_dir.path(), &["--store", store.as_str()]);
    assert_eq!(
        outcome,
        ColdOutcome::Verified(Span {
            stream_id: VECTOR_STREAM.to_owned(),
            first_seq: 1,
            last_seq: 5,
            last_hash: Digest::from_hex(&events[4].hash).unwrap(),
        })
    );
    assert!(!outcome.failed());
    assert_eq!(outcome.code(), None);
    assert_eq!(
        report,
        format!(
            "export: {}
segments: 2
trusted start: seq 1, prev_hash {ZERO_HASH}
artifact store: {store}
anchor: none given
token: none given
result: verified, stream {VECTOR_STREAM}, seq 1 to 5, last hash {}
",
            export_dir.path(),
            events[4].hash
        )
    );
}

#[test]
fn segments_are_walked_in_manifest_order_whatever_their_names() {
    let scratch = Scratch::new("names");
    let rows = sealed_rows();
    let export_dir = scratch.export("cold");
    export_dir.segment("zz-last-by-name", &sealed_file(&rows, 1, 3));
    export_dir.segment("mm", &sealed_file(&rows, 4, 4));
    export_dir.segment("aa-first-by-name", &sealed_file(&rows, 5, 8));
    let (outcome, report) = verify_cold(export_dir.path(), &[]);
    assert_eq!(
        outcome,
        ColdOutcome::Verified(Span {
            stream_id: STREAM.to_owned(),
            first_seq: 1,
            last_seq: 8,
            last_hash: rows[7].hash,
        }),
        "the manifests say 1 to 3, 4, 5 to 8; the names say otherwise and are not read"
    );
    assert!(report.contains("segments: 3"), "{report}");
}

#[test]
fn a_modified_payload_is_rehash_mismatch() {
    let scratch = Scratch::new("payload");
    let mut events = chain();
    let third = at(&mut events, 3);
    third.body = replace_once(&third.body, r#""verdict":"allow""#, r#""verdict":"deny""#);
    let export_dir = vector_export(&scratch, "cold", &events);
    let store = vector_store(&scratch);
    let (outcome, report) = verify_cold(export_dir.path(), &["--store", store.as_str()]);
    assert_tamper("payload_modified", &outcome, &report);
}

#[test]
fn a_deleted_event_is_seq_gap() {
    let scratch = Scratch::new("deleted");
    let mut events = chain();
    events.remove(2);
    let export_dir = vector_export(&scratch, "cold", &events);
    let store = vector_store(&scratch);
    let (outcome, report) = verify_cold(export_dir.path(), &["--store", store.as_str()]);
    assert_tamper("event_deleted", &outcome, &report);
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
    let export_dir = vector_export(&scratch, "cold", &events);
    let store = vector_store(&scratch);
    let (outcome, report) = verify_cold(export_dir.path(), &["--store", store.as_str()]);
    assert_tamper("prev_hash_changed_without_rehash", &outcome, &report);
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
    let export_dir = vector_export(&scratch, "cold", &events);
    let store = vector_store(&scratch);
    let (outcome, report) = verify_cold(export_dir.path(), &["--store", store.as_str()]);
    assert_tamper("prev_hash_rewritten_and_rehashed", &outcome, &report);
}

#[test]
fn whitespace_inserted_and_rehashed_is_segment_manifest_mismatch_in_a_cold_export() {
    let scratch = Scratch::new("whitespace");
    let mut events = chain();
    let second = at(&mut events, 2);
    assert!(second.body.starts_with('{'));
    second.body.insert(1, ' ');
    rehash(second);
    let export_dir = vector_export(&scratch, "cold", &events);
    let (outcome, report) = verify_cold(export_dir.path(), &[]);
    let (seq, check) = expected_event_failure("whitespace_inserted_and_rehashed");
    assert_eq!(
        (seq, check.as_str()),
        (2, "non_canonical"),
        "the vector expects check 1 at the event itself"
    );
    assert_eq!(
        outcome,
        segment_failure(1, ColdCheck::SegmentManifestMismatch),
        "a body that is not canonical makes its line not an export line, and the segment check \
         — the manifest against the file's own lines — runs before the per-event walk \
         (DEC-264 items 2 and 4), so the first failure in range order is the segment's, at the \
         segment holding seq 2, which begins at 1"
    );
    assert!(
        report.contains("result: failed, seq 1, segment_manifest_mismatch"),
        "{report}"
    );
}

#[test]
fn swapped_seq_members_are_a_seq_gap_because_an_export_carries_no_columns() {
    let scratch = Scratch::new("seq-swap");
    let mut events = chain();
    let second = replace_once(&events[1].body, r#""seq":2"#, r#""seq":3"#);
    let third = replace_once(&events[2].body, r#""seq":3"#, r#""seq":2"#);
    at(&mut events, 2).body = second;
    at(&mut events, 3).body = third;
    let export_dir = vector_export(&scratch, "cold", &events);
    let (outcome, _) = verify_cold(export_dir.path(), &[]);
    let (seq, check) = expected_event_failure("seq_values_swapped");
    assert_eq!(
        (seq, check.as_str()),
        (2, "column_mismatch"),
        "the vector expects the column check, which needs the stored columns of §6.1"
    );
    assert_eq!(
        outcome,
        event_failure(3, EventCheck::SeqGap),
        "a cold export's columns come from the body, so the swap shows as a gap at the line that \
         claims seq 3 where seq 2 belongs, as E5-4's command reports it"
    );
}

#[test]
fn a_tail_truncated_after_an_anchor_passes_every_check_and_fails_the_anchor() {
    let scratch = Scratch::new("truncated");
    let mut events = chain();
    events.pop();
    let export_dir = vector_export(&scratch, "cold", &events);
    let store = vector_store(&scratch);
    let anchor = scratch.file("anchor.json", &anchor_json());
    let (outcome, report) = verify_cold(
        export_dir.path(),
        &["--store", store.as_str(), "--anchor", anchor.as_str()],
    );
    let expected = expected_range_check("tail_truncated_after_anchor");
    assert_eq!(outcome, ColdOutcome::Range(RangeCheck::AnchorHeadMismatch));
    assert_eq!(outcome.code(), Some(expected.as_str()));
    assert!(outcome.failed());
    assert!(report.contains(&format!("anchor: {anchor}")), "{report}");
    assert!(
        report.contains(&format!("result: failed, {expected}")),
        "{report}"
    );
    let (without_anchor, _) = verify_cold(export_dir.path(), &["--store", store.as_str()]);
    assert_eq!(
        without_anchor,
        ColdOutcome::Verified(Span {
            stream_id: VECTOR_STREAM.to_owned(),
            first_seq: 1,
            last_seq: 4,
            last_hash: Digest::from_hex(&events[3].hash).unwrap(),
        }),
        "every segment and per-event check passes; only the anchor catches a truncated tail"
    );
}

#[test]
fn a_chain_rewritten_from_seq_3_passes_every_check_and_fails_the_anchor() {
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
    let export_dir = vector_export(&scratch, "cold", &events);
    let store = vector_store(&scratch);
    let anchor = scratch.file("anchor.json", &anchor_json());
    let (outcome, _) = verify_cold(
        export_dir.path(),
        &["--store", store.as_str(), "--anchor", anchor.as_str()],
    );
    assert_eq!(
        outcome.code(),
        Some(expected_range_check("chain_rewritten_from_seq_3").as_str())
    );
    let (without_anchor, _) = verify_cold(export_dir.path(), &["--store", store.as_str()]);
    assert!(
        !without_anchor.failed(),
        "a rewritten chain with rebuilt manifests is self-consistent; only the anchor catches it"
    );
}

#[test]
fn an_untampered_export_passes_the_anchor_it_is_covered_by() {
    let scratch = Scratch::new("anchored");
    let events = chain();
    let export_dir = vector_export(&scratch, "cold", &events);
    let store = vector_store(&scratch);
    let anchor = scratch.file("anchor.json", &anchor_json());
    let (outcome, report) = verify_cold(
        export_dir.path(),
        &["--store", store.as_str(), "--anchor", anchor.as_str()],
    );
    assert_eq!(
        outcome,
        ColdOutcome::Verified(Span {
            stream_id: VECTOR_STREAM.to_owned(),
            first_seq: 1,
            last_seq: 5,
            last_hash: Digest::from_hex(&events[4].hash).unwrap(),
        })
    );
    assert!(report.contains(&format!("anchor: {anchor}")), "{report}");
}

#[test]
fn an_anchor_whose_head_sits_in_an_earlier_segment_is_checked_there() {
    let scratch = Scratch::new("anchor-earlier-segment");
    let rows = sealed_rows();
    let export_dir = scratch.export("cold");
    export_dir.segment("seq-1", &sealed_file(&rows, 1, 4));
    export_dir.segment("seq-5", &sealed_file(&rows, 5, 8));
    let (anchor_bytes, _) = anchor_over(vec![(STREAM, 4, rows[3].hash)]);
    let anchor = scratch.file("anchor.json", &anchor_bytes);
    let (outcome, _) = verify_cold(export_dir.path(), &["--anchor", anchor.as_str()]);
    assert_eq!(
        outcome,
        ColdOutcome::Verified(Span {
            stream_id: STREAM.to_owned(),
            first_seq: 1,
            last_seq: 8,
            last_hash: rows[7].hash,
        }),
        "the anchored head is the first segment's last event: every walked segment's events are \
         offered to the anchor, not the last segment's alone"
    );
    let (anchor_bytes, _) = anchor_over(vec![(STREAM, 4, rows[4].hash)]);
    let anchor = scratch.file("lying.json", &anchor_bytes);
    let (outcome, _) = verify_cold(export_dir.path(), &["--anchor", anchor.as_str()]);
    assert_eq!(
        outcome,
        ColdOutcome::Range(RangeCheck::AnchorHeadMismatch),
        "and the head's hash is compared, not only its seq"
    );
}

#[test]
fn an_anchor_whose_root_does_not_match_its_leaves_is_anchor_root_mismatch() {
    let scratch = Scratch::new("bad-root");
    let export_dir = vector_export(&scratch, "cold", &chain());
    let store = vector_store(&scratch);
    let json = String::from_utf8(anchor_json()).unwrap();
    let broken = replace_once(
        &json,
        vectors()["merkle"]["root"].as_str().unwrap(),
        FOREIGN_HASH,
    );
    let anchor = scratch.file("anchor.json", broken.as_bytes());
    let (outcome, _) = verify_cold(
        export_dir.path(),
        &["--store", store.as_str(), "--anchor", anchor.as_str()],
    );
    assert_eq!(outcome, ColdOutcome::Range(RangeCheck::AnchorRootMismatch));
}

#[test]
fn an_anchor_whose_head_lies_before_the_trusted_start_is_anchor_head_mismatch() {
    let scratch = Scratch::new("anchor-early");
    let rows = sealed_rows();
    let export_dir = scratch.export("cold");
    export_dir.segment("all", &sealed_file(&rows, 1, 8));
    let (early, _) = anchor_over(vec![(STREAM, 2, rows[1].hash)]);
    let anchor = scratch.file("early.json", &early);
    let trusted = rows[1].hash.to_hex();
    let (outcome, report) = verify_cold(
        export_dir.path(),
        &[
            "--from-seq",
            "3",
            "--trusted-prev-hash",
            trusted.as_str(),
            "--anchor",
            anchor.as_str(),
        ],
    );
    assert_eq!(
        outcome,
        ColdOutcome::Range(RangeCheck::AnchorHeadMismatch),
        "the anchored head at seq 2 is before the trusted start, so the range never verified \
         it, and the anchor is not reported as checked (DEC-490 item 5)"
    );
    assert!(
        report.contains("result: failed, anchor_head_mismatch"),
        "{report}"
    );
    let (covering, _) = anchor_over(vec![(STREAM, 3, rows[2].hash)]);
    let anchor = scratch.file("covering.json", &covering);
    let (outcome, _) = verify_cold(
        export_dir.path(),
        &[
            "--from-seq",
            "3",
            "--trusted-prev-hash",
            trusted.as_str(),
            "--anchor",
            anchor.as_str(),
        ],
    );
    assert!(
        !outcome.failed(),
        "an anchored head at the trusted start itself is the first event the range verifies: {outcome}"
    );
}

#[test]
fn an_anchor_that_names_no_leaf_for_the_exports_stream_is_refused() {
    let scratch = Scratch::new("anchor-elsewhere");
    let export_dir = vector_export(&scratch, "cold", &chain());
    let store = vector_store(&scratch);
    let (elsewhere, _) = anchor_over(vec![("acct:ws_other:OTHERACCT", 9, Digest::of(b"o"))]);
    let anchor = scratch.file("anchor.json", &elsewhere);
    let text = refusal(
        export_dir.path(),
        &["--store", store.as_str(), "--anchor", anchor.as_str()],
    );
    assert!(
        text.starts_with(Refusal::AnchorStream.code()),
        "an anchor that says nothing about this stream is never reported as checked: {text}"
    );
    assert!(text.contains(VECTOR_STREAM), "{text}");
}

#[test]
fn a_flipped_byte_in_a_segment_file_is_segment_manifest_mismatch_at_its_first_seq() {
    let scratch = Scratch::new("flipped");
    let rows = sealed_rows();
    let export_dir = scratch.export("cold");
    export_dir.segment("seq-1", &sealed_file(&rows, 1, 4));
    let second = sealed_file(&rows, 5, 8);
    let mut flipped = second.clone();
    flipped[0] ^= 1;
    export_dir.raw("seq-5", &flipped, &manifest_for(&second));
    let (outcome, report) = verify_cold(export_dir.path(), &[]);
    assert_eq!(
        outcome,
        segment_failure(5, ColdCheck::SegmentManifestMismatch),
        "the file's bytes differ from its manifest's SHA-256: a mismatch at the segment's first seq"
    );
    assert!(outcome.failed());
    assert!(
        report.contains("result: failed, seq 5, segment_manifest_mismatch"),
        "{report}"
    );
}

#[test]
fn a_final_line_without_its_line_feed_is_segment_manifest_mismatch() {
    let scratch = Scratch::new("no-lf");
    let rows = sealed_rows();
    let export_dir = scratch.export("cold");
    export_dir.segment("seq-1", &sealed_file(&rows, 1, 4));
    let mut truncated = sealed_file(&rows, 5, 8);
    assert_eq!(truncated.pop(), Some(b'\n'));
    let manifest = manifest_for(&truncated);
    export_dir.raw("seq-5", &truncated, &manifest);
    let (outcome, _) = verify_cold(export_dir.path(), &[]);
    assert_eq!(
        outcome,
        segment_failure(5, ColdCheck::SegmentManifestMismatch),
        "spec §6.2 terminates every line; a file whose last line is not terminated is not the \
         segment its manifest describes, however the manifest was computed"
    );
}

#[test]
fn a_manifest_whose_edges_lie_is_segment_manifest_mismatch() {
    let scratch = Scratch::new("edges");
    let rows = sealed_rows();
    let export_dir = scratch.export("cold");
    export_dir.segment("seq-1", &sealed_file(&rows, 1, 4));
    let second = sealed_file(&rows, 5, 8);
    let lying = manifest_bytes(
        STREAM,
        5,
        8,
        &rows[4].prev_hash.to_hex(),
        FOREIGN_HASH,
        &Digest::of(&second).to_hex(),
    );
    export_dir.raw("seq-5", &second, &lying);
    let (outcome, _) = verify_cold(export_dir.path(), &[]);
    assert_eq!(
        outcome,
        segment_failure(5, ColdCheck::SegmentManifestMismatch),
        "the file's SHA-256 matches, but the manifest's last hash is not the last line's hash"
    );
}

#[test]
fn a_manifest_that_names_another_stream_than_its_file_is_segment_manifest_mismatch() {
    let scratch = Scratch::new("stream-lie");
    let rows = sealed_rows();
    let export_dir = scratch.export("cold");
    export_dir.segment("seq-1", &sealed_file(&rows, 1, 4));
    let second = sealed_file(&rows, 5, 8);
    let lying = manifest_bytes(
        "acct:ws_2:ACCT2",
        5,
        8,
        &rows[4].prev_hash.to_hex(),
        &rows[7].hash.to_hex(),
        &Digest::of(&second).to_hex(),
    );
    export_dir.raw("seq-5", &second, &lying);
    let (outcome, report) = verify_cold(export_dir.path(), &[]);
    assert_eq!(
        outcome,
        segment_failure(5, ColdCheck::SegmentManifestMismatch),
        "every edge and the SHA-256 match, but the manifest claims a stream its lines do not \
         carry: the segment is pinned to its own manifest's stream before anything compares \
         the segments (DEC-490 item 3)"
    );
    assert!(
        report.contains("result: failed, seq 5, segment_manifest_mismatch"),
        "{report}"
    );
}

#[test]
fn a_manifest_that_is_not_one_is_segment_manifest_mismatch_at_the_seq_the_range_expected() {
    let scratch = Scratch::new("not-manifest");
    let rows = sealed_rows();
    let export_dir = scratch.export("cold");
    export_dir.segment("seq-1", &sealed_file(&rows, 1, 4));
    export_dir.raw(
        "aaa-sorts-first-by-name",
        &sealed_file(&rows, 5, 8),
        b"not a manifest",
    );
    let (outcome, report) = verify_cold(export_dir.path(), &[]);
    assert_eq!(
        outcome,
        segment_failure(5, ColdCheck::SegmentManifestMismatch),
        "bytes that are not a manifest claim no place: they are walked last, and fail where the \
         range expected the next segment, after 1 to 4 (DEC-490 item 2, DEC-264 item 1)"
    );
    assert!(
        report.contains("result: failed, seq 5, segment_manifest_mismatch"),
        "{report}"
    );
}

#[test]
fn a_gap_between_segments_fails_segment_gap_at_the_expected_seq() {
    let scratch = Scratch::new("gap");
    let rows = sealed_rows();
    let export_dir = scratch.export("cold");
    export_dir.segment("seq-1", &sealed_file(&rows, 1, 4));
    export_dir.segment("seq-6", &sealed_file(&rows, 6, 8));
    let (outcome, report) = verify_cold(export_dir.path(), &[]);
    assert_eq!(
        outcome,
        segment_failure(5, ColdCheck::SegmentGap),
        "the second segment begins at 6 where the range expected 5, and the failure names 5"
    );
    assert!(outcome.failed());
    assert!(
        report.contains("result: failed, seq 5, segment_gap"),
        "{report}"
    );
}

#[test]
fn a_stale_or_overlapping_copy_beside_verified_segments_fails_segment_gap() {
    let scratch = Scratch::new("stale");
    let rows = sealed_rows();
    let stale = scratch.export("stale");
    stale.segment("seq-1", &sealed_file(&rows, 1, 4));
    stale.segment("seq-5", &sealed_file(&rows, 5, 8));
    stale.segment("whole", &sealed_file(&rows, 1, 8));
    assert_eq!(
        verify_cold(stale.path(), &[]).0,
        segment_failure(5, ColdCheck::SegmentGap),
        "a full copy sorts beside 1 to 4 and begins at 1 where the range expected 5: two cold \
         copies of the same events never both stand"
    );
    let overlapping = scratch.export("overlapping");
    overlapping.segment("seq-1", &sealed_file(&rows, 1, 4));
    overlapping.segment("seq-3", &sealed_file(&rows, 3, 8));
    assert_eq!(
        verify_cold(overlapping.path(), &[]).0,
        segment_failure(5, ColdCheck::SegmentGap),
        "a segment beginning at 3 where the range expected 5 is a gap even though it covers 5"
    );
}

#[test]
fn a_first_segment_that_does_not_carry_the_trusted_start_fails_segment_gap() {
    let scratch = Scratch::new("start-gap");
    let rows = sealed_rows();
    let export_dir = scratch.export("cold");
    export_dir.segment("seq-5", &sealed_file(&rows, 5, 8));
    let trusted = rows[1].hash.to_hex();
    let (outcome, report) = verify_cold(
        export_dir.path(),
        &["--from-seq", "3", "--trusted-prev-hash", trusted.as_str()],
    );
    assert_eq!(
        outcome,
        segment_failure(3, ColdCheck::SegmentGap),
        "a range starting at 3 cannot begin with a segment that starts at 5"
    );
    assert!(
        report.contains(&format!("trusted start: seq 3, prev_hash {trusted}")),
        "{report}"
    );
}

#[test]
fn a_range_may_enter_mid_segment_from_its_trusted_start() {
    let scratch = Scratch::new("mid");
    let rows = sealed_rows();
    let export_dir = scratch.export("cold");
    export_dir.segment("seq-1", &sealed_file(&rows, 1, 5));
    export_dir.segment("seq-6", &sealed_file(&rows, 6, 8));
    let trusted = rows[1].hash.to_hex();
    let (outcome, report) = verify_cold(
        export_dir.path(),
        &["--from-seq", "3", "--trusted-prev-hash", trusted.as_str()],
    );
    assert_eq!(
        outcome,
        ColdOutcome::Verified(Span {
            stream_id: STREAM.to_owned(),
            first_seq: 3,
            last_seq: 8,
            last_hash: rows[7].hash,
        }),
        "the first segment begins before the trusted start: the range enters at 3 and walks to 8"
    );
    assert!(
        report.contains(&format!("trusted start: seq 3, prev_hash {trusted}")),
        "{report}"
    );
    assert!(
        report.contains(&format!(
            "result: verified, stream {STREAM}, seq 3 to 8, last hash {}",
            rows[7].hash.to_hex()
        )),
        "{report}"
    );
}

#[test]
fn an_event_before_the_trusted_start_is_not_checked() {
    let scratch = Scratch::new("before-start");
    let rows = sealed_rows();
    let mut events = sealed_events(&rows);
    let first_hash = events[0].hash.clone();
    let second_hash = events[1].hash.clone();
    at(&mut events, 1).hash = second_hash;
    at(&mut events, 2).hash = first_hash;
    let export_dir = scratch.export("cold");
    export_dir.segment("all", &export(&events));
    let trusted = rows[1].hash.to_hex();
    let (outcome, _) = verify_cold(
        export_dir.path(),
        &["--from-seq", "3", "--trusted-prev-hash", trusted.as_str()],
    );
    assert_eq!(
        outcome,
        ColdOutcome::Verified(Span {
            stream_id: STREAM.to_owned(),
            first_seq: 3,
            last_seq: 8,
            last_hash: rows[7].hash,
        }),
        "seq 1 and 2 carry swapped hashes, but the range starts at 3 and §11 does not check what \
         is before its trusted start"
    );
    let (from_genesis, _) = verify_cold(export_dir.path(), &[]);
    assert_eq!(
        from_genesis,
        event_failure(1, EventCheck::RehashMismatch),
        "the same export from the genesis start is caught at the first swapped hash"
    );
}

#[test]
fn the_first_failure_in_range_order_is_the_one_reported() {
    let scratch = Scratch::new("first");
    let rows = sealed_rows();
    let mut lied = sealed_events(&rows[..4]);
    at(&mut lied, 2).hash = FOREIGN_HASH.to_owned();
    let lied_file = export(&lied);
    let clean = sealed_file(&rows, 5, 8);
    let mut flipped = clean.clone();
    flipped[0] ^= 1;
    let lie_then_flip = scratch.export("lie-then-flip");
    lie_then_flip.segment("seq-1", &lied_file);
    lie_then_flip.raw("seq-5", &flipped, &manifest_for(&clean));
    assert_eq!(
        verify_cold(lie_then_flip.path(), &[]).0,
        event_failure(2, EventCheck::RehashMismatch),
        "the first segment's events are walked before the second segment's manifest is read"
    );
    let flip_then_lie = scratch.export("flip-then-lie");
    let first = sealed_file(&rows, 1, 4);
    let mut first_flipped = first.clone();
    first_flipped[0] ^= 1;
    flip_then_lie.raw("seq-1", &first_flipped, &manifest_for(&first));
    let mut later_lied = sealed_events(&rows[4..]);
    at(&mut later_lied, 2).hash = FOREIGN_HASH.to_owned();
    flip_then_lie.segment("seq-5", &export(&later_lied));
    assert_eq!(
        verify_cold(flip_then_lie.path(), &[]).0,
        segment_failure(1, ColdCheck::SegmentManifestMismatch),
        "and the first segment's manifest check comes before anything in the second"
    );
}

#[test]
fn a_cold_export_that_mixes_streams_is_refused() {
    let scratch = Scratch::new("mixed");
    let rows = sealed_rows();
    let mut foreign = sealed_events(&rows[4..]);
    let mut previous: Option<String> = None;
    for event in &mut foreign {
        event.body = replace_once(
            &event.body,
            &format!(r#""stream_id":"{STREAM}""#),
            r#""stream_id":"acct:ws_2:ACCT2""#,
        );
        if let Some(previous) = &previous {
            let stale = member(&event.body, "prev_hash")
                .as_str()
                .unwrap()
                .to_owned();
            event.body = replace_once(
                &event.body,
                &format!(r#""prev_hash":"{stale}""#),
                &format!(r#""prev_hash":"{previous}""#),
            );
        }
        rehash(event);
        previous = Some(event.hash.clone());
    }
    let export_dir = scratch.export("cold");
    export_dir.segment("seq-1", &sealed_file(&rows, 1, 4));
    export_dir.segment("seq-5", &export(&foreign));
    let text = refusal(export_dir.path(), &[]);
    assert!(
        text.starts_with(Refusal::ExportStreams.code()),
        "seq 5 to 8 chain from seq 4's hash but belong to another stream: a cold export is one \
         stream's range, and the walk alone cannot tell (DEC-490 item 3): {text}"
    );
    assert!(
        text.contains("acct:ws_2:ACCT2") && text.contains(STREAM),
        "{text}"
    );
}

#[test]
fn a_token_whose_imprint_matches_is_reported_incomplete_never_verified() {
    let scratch = Scratch::new("token-incomplete");
    let rows = sealed_rows();
    let export_dir = scratch.export("cold");
    export_dir.segment("seq-1", &sealed_file(&rows, 1, 4));
    export_dir.segment("seq-5", &sealed_file(&rows, 5, 8));
    let (anchor_bytes, root) = anchor_over(vec![(STREAM, 8, rows[7].hash)]);
    let anchor = scratch.file("anchor.json", &anchor_bytes);
    let token = scratch.file("anchor.tsr", &token_for(&root));
    let (outcome, report) = verify_cold(
        export_dir.path(),
        &["--anchor", anchor.as_str(), "--token", token.as_str()],
    );
    assert_eq!(
        outcome,
        ColdOutcome::Cold(ColdFailure::TsaVerificationIncomplete),
        "the token holds the anchor's imprint, and the signature, the chain and revocation are \
         DEC-265 item 1's Proposed half: the command never answers verified for what it has not \
         proven"
    );
    assert!(outcome.failed(), "and the exit is non-zero");
    assert_eq!(outcome.code(), Some(TSA_VERIFICATION_INCOMPLETE));
    assert!(report.contains(&format!("token: {token}")), "{report}");
    assert!(
        report.contains("result: failed, tsa_verification_incomplete"),
        "{report}"
    );
    let (without_token, _) = verify_cold(export_dir.path(), &["--anchor", anchor.as_str()]);
    assert!(
        !without_token.failed(),
        "the same export and anchor verify without the token: {without_token}"
    );
}

#[test]
fn a_token_without_the_anchor_s_imprint_is_tsa_token_invalid() {
    let scratch = Scratch::new("token-invalid");
    let rows = sealed_rows();
    let export_dir = scratch.export("cold");
    export_dir.segment("all", &sealed_file(&rows, 1, 8));
    let (anchor_bytes, root) = anchor_over(vec![(STREAM, 8, rows[7].hash)]);
    let anchor = scratch.file("anchor.json", &anchor_bytes);
    let mut root_only = b"TSTInfo ".to_vec();
    root_only.extend_from_slice(root.as_bytes());
    for (name, bytes) in [
        ("none.tsr", b"no imprint here".as_slice()),
        ("root-only.tsr", root_only.as_slice()),
    ] {
        let token = scratch.file(name, bytes);
        let (outcome, report) = verify_cold(
            export_dir.path(),
            &["--anchor", anchor.as_str(), "--token", token.as_str()],
        );
        assert_eq!(
            outcome,
            ColdOutcome::Cold(ColdFailure::TsaTokenInvalid),
            "{name}: a token without the imprint — SHA-256 of the 32 raw root bytes, not the root \
             itself — does not even claim this anchor's root"
        );
        assert!(
            report.contains("result: failed, tsa_token_invalid"),
            "{name}: {report}"
        );
    }
}

#[test]
fn a_token_is_checked_only_after_the_range_and_the_anchor_pass() {
    let scratch = Scratch::new("token-order");
    let rows = sealed_rows();
    let (anchor_bytes, root) = anchor_over(vec![(STREAM, 8, rows[7].hash)]);
    let anchor = scratch.file("anchor.json", &anchor_bytes);
    let token = scratch.file("anchor.tsr", &token_for(&root));
    let with_both = ["--anchor", anchor.as_str(), "--token", token.as_str()];
    let flipped = scratch.export("flipped");
    flipped.segment("seq-1", &sealed_file(&rows, 1, 4));
    let second = sealed_file(&rows, 5, 8);
    let mut bytes = second.clone();
    bytes[0] ^= 1;
    flipped.raw("seq-5", &bytes, &manifest_for(&second));
    assert_eq!(
        verify_cold(flipped.path(), &with_both).0,
        segment_failure(5, ColdCheck::SegmentManifestMismatch),
        "a failing segment is reported before the token is looked at"
    );
    let truncated = scratch.export("truncated");
    truncated.segment("seq-1", &sealed_file(&rows, 1, 4));
    assert_eq!(
        verify_cold(truncated.path(), &with_both).0,
        ColdOutcome::Range(RangeCheck::AnchorHeadMismatch),
        "an anchor whose head the range does not hold is reported before the token"
    );
}

#[test]
fn a_directory_with_no_segment_is_refused() {
    let scratch = Scratch::new("empty");
    let export_dir = scratch.export("cold");
    let text = refusal(export_dir.path(), &[]);
    assert!(
        text.starts_with(Refusal::ColdExportIncomplete.code()),
        "a directory with no segment is no range, and nothing in it is reported verified: {text}"
    );
}

#[test]
fn an_unpaired_segment_or_manifest_is_refused() {
    let scratch = Scratch::new("unpaired");
    let rows = sealed_rows();
    let lone_file = scratch.export("lone-file");
    lone_file.segment("seq-1", &sealed_file(&rows, 1, 4));
    lone_file.file_only("seq-5", &sealed_file(&rows, 5, 8));
    let text = refusal(lone_file.path(), &[]);
    assert!(
        text.starts_with(Refusal::ColdExportIncomplete.code()),
        "a segment file without its manifest cannot be checked, and 1 to 4 alone is not the \
         export: {text}"
    );
    assert!(
        text.contains("seq-5"),
        "the refusal names the unpaired name: {text}"
    );
    let lone_manifest = scratch.export("lone-manifest");
    lone_manifest.segment("seq-1", &sealed_file(&rows, 1, 4));
    lone_manifest.manifest_only("seq-5", &manifest_for(&sealed_file(&rows, 5, 8)));
    let text = refusal(lone_manifest.path(), &[]);
    assert!(
        text.starts_with(Refusal::ColdExportIncomplete.code()),
        "a manifest without its file is a segment the export claims and does not hold: {text}"
    );
    assert!(text.contains("seq-5"), "{text}");
}

#[test]
fn an_unreadable_export_anchor_or_token_path_is_refused_with_a_code() {
    let scratch = Scratch::new("unreadable");
    let rows = sealed_rows();
    let export_dir = scratch.export("cold");
    export_dir.segment("all", &sealed_file(&rows, 1, 8));
    let (anchor_bytes, _) = anchor_over(vec![(STREAM, 8, rows[7].hash)]);
    let anchor = scratch.file("anchor.json", &anchor_bytes);
    let absent_dir = scratch.0.join("absent");
    let absent = absent_dir.to_str().unwrap();
    let a_file = scratch.file("a-file.jsonl", b"");
    let inner_dir = scratch.export("inner-dir");
    inner_dir.segment("seq-1", &sealed_file(&rows, 1, 4));
    inner_dir.manifest_only("absent", &manifest_for(&sealed_file(&rows, 5, 8)));
    fs::create_dir_all(inner_dir.0.join("absent.jsonl")).unwrap();
    let cases: [(&str, Vec<&str>); 5] = [
        ("a missing export", vec![absent]),
        ("an export that is a file", vec![a_file.as_str()]),
        ("a segment file that is a directory", vec![inner_dir.path()]),
        (
            "a missing anchor",
            vec![export_dir.path(), "--anchor", absent],
        ),
        (
            "a missing token",
            vec![
                export_dir.path(),
                "--anchor",
                anchor.as_str(),
                "--token",
                absent,
            ],
        ),
    ];
    for (label, argv) in cases {
        let (export, extra) = argv.split_first().unwrap();
        let text = refusal(export, extra);
        assert!(
            text.starts_with(&format!("{}: ", Refusal::Unreadable.code())),
            "{label}: the code comes first, so a script reads it where an auditor does: {text}"
        );
        assert!(
            text.contains("absent") || text.contains("a-file.jsonl"),
            "{label}: {text}"
        );
    }
    assert!(
        !absent_dir.exists(),
        "a mistyped export path is never created"
    );
}

#[test]
fn every_cold_refusal_reports_its_stable_code_first() {
    let scratch = Scratch::new("codes");
    let export_dir = vector_export(&scratch, "cold", &chain());
    let store = vector_store(&scratch);
    let store_file = scratch.file("not-a-dir", b"x");
    let bad_anchor = scratch.file("bad.json", b"{");
    let (elsewhere_bytes, _) = anchor_over(vec![("ctl:ws_other", 2, Digest::of(b"c"))]);
    let elsewhere = scratch.file("elsewhere.json", &elsewhere_bytes);
    let empty = scratch.export("empty");
    let absent_dir = scratch.0.join("absent");
    let absent = absent_dir.to_str().unwrap();
    let cases: Vec<(Refusal, Vec<&str>)> = vec![
        (
            Refusal::TrustedStart,
            vec![
                export_dir.path(),
                "--from-seq",
                "0",
                "--trusted-prev-hash",
                ZERO_HASH,
            ],
        ),
        (
            Refusal::TrustedStart,
            vec![
                export_dir.path(),
                "--from-seq",
                "1",
                "--trusted-prev-hash",
                "not-a-hash",
            ],
        ),
        (
            Refusal::ArtifactStore,
            vec![export_dir.path(), "--store", store_file.as_str()],
        ),
        (
            Refusal::Anchor,
            vec![export_dir.path(), "--anchor", bad_anchor.as_str()],
        ),
        (
            Refusal::AnchorStream,
            vec![
                export_dir.path(),
                "--store",
                store.as_str(),
                "--anchor",
                elsewhere.as_str(),
            ],
        ),
        (Refusal::Unreadable, vec![absent]),
        (Refusal::ColdExportIncomplete, vec![empty.path()]),
    ];
    for (reason, argv) in cases {
        let (export, extra) = argv.split_first().unwrap();
        let text = refusal(export, extra);
        assert!(
            text.starts_with(&format!("{}: ", reason.code())),
            "{:?} must report `{}` first, got {text}",
            reason,
            reason.code()
        );
    }
}

#[test]
fn without_a_store_an_event_that_names_an_artifact_is_artifact_missing() {
    let scratch = Scratch::new("artifact-nostore");
    let events = chain();
    let referencing = events
        .iter()
        .position(|event| {
            let body = parse(event.body.as_bytes()).unwrap();
            let config = body
                .get("config_refs")
                .and_then(Value::as_object)
                .is_some_and(|refs| !refs.is_empty());
            let artifacts = body
                .get("artifact_refs")
                .and_then(Value::as_array)
                .is_some_and(|refs| !refs.is_empty());
            config || artifacts
        })
        .expect("the vectors' chain references the config artifacts");
    let seq = u64::try_from(referencing).unwrap() + 1;
    let export_dir = vector_export(&scratch, "cold", &events);
    let (outcome, report) = verify_cold(export_dir.path(), &[]);
    assert_eq!(
        outcome,
        event_failure(seq, EventCheck::ArtifactMissing),
        "without a store every reference is missing, the safe direction"
    );
    assert!(report.contains("artifact store: none given"), "{report}");
}

#[test]
fn an_altered_artifact_is_artifact_mismatch() {
    let scratch = Scratch::new("artifact-altered");
    let events = chain();
    let export_dir = vector_export(&scratch, "cold", &events);
    let store = vector_store(&scratch);
    let fixture = &vectors()["artifacts"][0];
    let reference = ArtifactRef::parse(fixture["ref"].as_str().unwrap()).unwrap();
    let seq = events
        .iter()
        .position(|event| event.body.contains(fixture["ref"].as_str().unwrap()))
        .map(|index| u64::try_from(index).unwrap() + 1)
        .expect("an event references the first config artifact");
    let fs_store = FsArtifactStore::open(store.as_str()).unwrap();
    let path = fs_store.object_path(&reference);
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    #[expect(
        clippy::permissions_set_readonly_false,
        reason = "the test plays a writer that ignores the store's read-only mode"
    )]
    permissions.set_readonly(false);
    fs::set_permissions(&path, permissions).unwrap();
    fs::write(&path, b"tampered").unwrap();
    let (outcome, _) = verify_cold(export_dir.path(), &["--store", store.as_str()]);
    assert_eq!(
        outcome,
        event_failure(seq, EventCheck::ArtifactMismatch),
        "the bytes are re-hashed on the way out, never trusted for being in place"
    );
}

#[test]
fn identical_inputs_give_identical_output() {
    let scratch = Scratch::new("deterministic");
    let events = chain();
    let export_dir = vector_export(&scratch, "cold", &events);
    let store = vector_store(&scratch);
    let anchor = scratch.file("anchor.json", &anchor_json());
    let with = ["--store", store.as_str(), "--anchor", anchor.as_str()];
    let (outcome, report) = verify_cold(export_dir.path(), &with);
    assert_eq!(
        (outcome.clone(), report.clone()),
        verify_cold(export_dir.path(), &with)
    );
    assert!(matches!(outcome, ColdOutcome::Verified(_)), "{outcome}");
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
    let failing = vector_export(&scratch, "tampered", &tampered);
    let with_store = ["--store", store.as_str()];
    let (failed, failed_report) = verify_cold(failing.path(), &with_store);
    assert_eq!(
        (failed.clone(), failed_report),
        verify_cold(failing.path(), &with_store),
        "a failing run repeats its report too"
    );
    assert_eq!(failed.code(), Some("rehash_mismatch"));
}
