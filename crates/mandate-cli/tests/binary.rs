//! The `mandate` binary's edge, run as a process: `journal verify` and `journal verify-cold`
//! print their report on standard output and exit on the outcome, non-zero with the result on
//! standard error for any failure (E5-4, E5-8). The library tests cover the checks; these pin
//! that `main` dispatches to them, which nothing in-process can.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

use mandate_journal::{AppendOutcome, MemoryJournal, StoredEvent, StreamId, export_segment};
use mandate_journal_cold::SegmentManifest;
use mandate_time::UtcNanos;

const STREAM: &str = "acct:ws_1:ACCT1";
const T: &str = "2026-09-21T14:00:00.000000000Z";
const ZERO_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("mandate-cli-binary-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

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

fn event_id(n: u64) -> String {
    format!("01J8Z3M4{n:018}")
}

fn opened_draft() -> String {
    format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{}","stream_id":"{STREAM}",
        "event_type":"StreamOpened","schema_version":1,"event_time":"{T}","clock_source":"local",
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
        "event_type":"MarkUpdated","schema_version":1,"event_time":"{T}","clock_source":"local",
        "causation_id":null,"correlation_id":null,
        "actor":{{"kind":"system","id":"executor","version":"0.1.0","build":"sha256:{}"}},
        "config_refs":{{}},"payload":{{"instrument_id":"inst","price":"1","source":"quote",
        "feed":"iex","risk_clock":"{T}"}},"artifact_refs":[],"pii_refs":[]}}"#,
        event_id(n),
        "3".repeat(64)
    )
}

/// Four rows of one stream, `seq` 1 to 4, sealed by the journal's own append protocol; none
/// references an artifact.
fn sealed_rows() -> Vec<StoredEvent> {
    let stream = StreamId::parse(STREAM).unwrap();
    let mut journal = MemoryJournal::new();
    let epoch = journal.take_ownership(&stream);
    let now = UtcNanos::parse(T).unwrap();
    let outcome = journal.append(&stream, 0, epoch, now, &[opened_draft().as_bytes()]);
    assert!(
        matches!(outcome, AppendOutcome::Committed(_)),
        "{outcome:?}"
    );
    for n in 1..=3 {
        let outcome = journal.append(&stream, n, epoch, now, &[mark_draft(n).as_bytes()]);
        assert!(
            matches!(outcome, AppendOutcome::Committed(_)),
            "{outcome:?}"
        );
    }
    journal.rows(&stream).to_vec()
}

fn mandate(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mandate"))
        .args(args)
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[test]
fn journal_verify_prints_its_report_and_exits_on_the_result() {
    let scratch = Scratch::new("verify");
    let rows = sealed_rows();
    let export = scratch.file("segment.jsonl", &export_segment(&rows));
    let output = mandate(&["journal", "verify", export.as_str()]);
    assert!(
        output.status.success(),
        "a verified export exits 0: {}",
        text(&output.stderr)
    );
    assert_eq!(
        text(&output.stdout),
        format!(
            "export: {export}
lines: 4
trusted start: seq 1, prev_hash {ZERO_HASH}
artifact store: none given
anchor: none given
result: verified, stream {STREAM}, seq 1 to 4, last hash {}
",
            rows[3].hash.to_hex()
        )
    );
    let mut tampered = export_segment(&rows);
    let second_hash = rows[1].hash.to_hex();
    let at = text(&tampered).find(&second_hash).unwrap();
    tampered
        .get_mut(at..at + 64)
        .unwrap()
        .copy_from_slice(&[b'a'; 64]);
    let export = scratch.file("tampered.jsonl", &tampered);
    let output = mandate(&["journal", "verify", export.as_str()]);
    assert!(!output.status.success(), "a failing export exits non-zero");
    assert!(
        text(&output.stdout).contains("result: failed, seq 2, rehash_mismatch"),
        "{}",
        text(&output.stdout)
    );
    assert!(
        text(&output.stderr).contains("failed, seq 2, rehash_mismatch"),
        "the result is on standard error too: {}",
        text(&output.stderr)
    );
}

#[test]
#[ignore = "pending E5-8"]
fn journal_verify_cold_prints_its_report_and_exits_on_the_result() {
    let scratch = Scratch::new("verify-cold");
    let rows = sealed_rows();
    let export = scratch.dir("cold");
    let dir = PathBuf::from(&export);
    fs::write(dir.join("seq-1.jsonl"), export_segment(&rows)).unwrap();
    fs::write(
        dir.join("seq-1.manifest.json"),
        SegmentManifest::of(&rows)
            .unwrap()
            .to_canonical_bytes()
            .unwrap()
            .as_slice(),
    )
    .unwrap();
    let output = mandate(&["journal", "verify-cold", export.as_str()]);
    assert!(
        output.status.success(),
        "a verified cold export exits 0: {}",
        text(&output.stderr)
    );
    assert_eq!(
        text(&output.stdout),
        format!(
            "export: {export}
segments: 1
trusted start: seq 1, prev_hash {ZERO_HASH}
artifact store: none given
anchor: none given
token: none given
result: verified, stream {STREAM}, seq 1 to 4, last hash {}
",
            rows[3].hash.to_hex()
        )
    );
    let empty = scratch.dir("empty");
    let output = mandate(&["journal", "verify-cold", empty.as_str()]);
    assert!(!output.status.success(), "a refused export exits non-zero");
    assert!(output.stdout.is_empty(), "and reports nothing");
    assert!(
        text(&output.stderr).contains("cold_export_incomplete"),
        "the refusal's code is on standard error: {}",
        text(&output.stderr)
    );
}
