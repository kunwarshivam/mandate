//! E12-3 (journal spec §11, DEC-782): the control stream's range checks `anchor_self_mismatch` and
//! `break_glass_cause_mismatch` through both verifiers, `mandate journal verify` and `verify-cold`.
//! DEC-782 runs them after checks 1 to 6 and before the anchor checks, on a control stream only,
//! reports the lowest `seq` among them, and prints `result: failed, seq <n>, <code>`.
//!
//! Oracles: the vectors' `cold_records.range_checks` and `records_access.range_checks` in
//! `fixtures/refcases/journal.json`, whose expected failures are read from the file, never written
//! here. Each chain is re-canonicalized and re-chained here, and an untouched chain must reproduce
//! the vectors' hashes, so the construction is checked against the vectors. An `AnchorComputed`
//! names the token `sha256:aa…`, which no bytes hash to, so the chains given to the verifiers store
//! a token of their own and name it instead: `anchor_self_mismatch` reads only the leaves, so the
//! vector's verdict stands. A composite chain (two failures, or another stream type) derives its
//! answer from how it was built.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use clap::Parser;
use mandate_artifacts_fs::FsArtifactStore;
use mandate_canon::{Digest, Int, Key, Object, Value, parse, to_canonical};
use mandate_cli::journal::{self, JournalCommand, cold};
use mandate_cli::{Cli, Command};
use mandate_journal::{Anchor, AnchorLeaf, ArtifactRef, ArtifactStore};
use serde_json::Value as Json;

const CTL: &str = "ctl:ws_01J8Z2";
const OTHER_TYPES: [&str; 2] = [
    "acct:ws_01J8Z2:01J8Z2ACCT00000000000000A1",
    "agent:ws_01J8Z2:agent_a",
];
const SECTIONS: [&str; 2] = ["cold_records", "records_access"];
const ZERO_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const VECTOR_TOKEN: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const TOKEN_BYTES: &[u8] = b"a timestamp token, stored so check 6 passes";

/// The two commands: `journal verify` over one segment file, `verify-cold` over a directory.
const BOTH: [&str; 2] = ["verify", "verify-cold"];

/// A scratch directory, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("mandate-ranges-{}-{n}", std::process::id()));
        fs::create_dir_all(dir.join("export")).unwrap();
        Self(dir)
    }

    fn put(&self, name: &str, bytes: &[u8]) -> String {
        let path = self.0.join(name);
        fs::write(&path, bytes).unwrap();
        path.to_str().unwrap().to_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The `range_checks` cases of `section`.
fn cases(section: &str) -> Vec<Json> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let vectors: Json = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    vectors[section]["range_checks"].as_array().unwrap().clone()
}

/// The range of the `section` case named `name`.
fn range(section: &str, name: &str) -> Range {
    Range::of(
        &cases(section)
            .into_iter()
            .find(|c| c["name"] == name)
            .unwrap(),
    )
}

/// The failing cases of `section`, each with the `(seq, check)` it expects; each section holds
/// one for every clause of its check.
fn failing(section: &str) -> Vec<(Json, u64, String)> {
    let found: Vec<_> = cases(section)
        .into_iter()
        .filter_map(|c| {
            let expect = (c["expect"]["seq"].as_u64()?, c["expect"]["check"].as_str()?);
            let (seq, check) = (expect.0, expect.1.to_owned());
            Some((c, seq, check))
        })
        .collect();
    assert_eq!(found.len(), 3, "{section}'s failing cases");
    found
}

/// A range: its trusted start and its bodies in `seq` order.
#[derive(Debug, Clone)]
struct Range {
    from_seq: u64,
    prev_hash: String,
    bodies: Vec<Json>,
}

impl Range {
    fn of(case: &Json) -> Self {
        Self {
            from_seq: case["from_seq"].as_u64().unwrap_or(1),
            prev_hash: case["prev_hash"].as_str().unwrap_or(ZERO_HASH).to_owned(),
            bodies: case["chain"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| e["body"].clone())
                .collect(),
        }
    }

    /// The events renumbered from `from_seq` and chained from `prev_hash`, each `(body, hash)`,
    /// with the vectors' token replaced by `token`.
    fn chained(&self, token: &str) -> Vec<(String, Digest)> {
        let mut prev = self.prev_hash.clone();
        let mut out = Vec::new();
        for (seq, body) in (self.from_seq..).zip(&self.bodies) {
            let mut body = body.clone();
            body["seq"] = seq.into();
            body["prev_hash"] = prev.clone().into();
            if body["payload"]["token"] == VECTOR_TOKEN {
                body["payload"]["token"] = token.into();
            }
            for reference in body["artifact_refs"].as_array_mut().unwrap() {
                if *reference == VECTOR_TOKEN {
                    *reference = token.into();
                }
            }
            let bytes = to_canonical(&parse(&serde_json::to_vec(&body).unwrap()).unwrap());
            let hash = Digest::of(&bytes);
            prev = hash.to_hex();
            out.push((String::from_utf8(bytes).unwrap(), hash));
        }
        out
    }

    fn on_stream(mut self, stream: &str) -> Self {
        for body in &mut self.bodies {
            body["stream_id"] = stream.into();
        }
        self
    }

    fn then(mut self, body: &Json) -> Self {
        self.bodies.push(body.clone());
        self
    }

    fn last_seq(&self) -> u64 {
        self.from_seq + self.bodies.len() as u64 - 1
    }

    fn stream(&self) -> &str {
        self.bodies[0]["stream_id"].as_str().unwrap()
    }
}

/// What a run answered: its report's `result:` text, its code, and whether it exits non-zero.
type Ran = (String, Option<&'static str>, bool);

/// Runs `command` over `range` written as one segment, with `--anchor` bytes when given. The
/// scratch paths hold no whitespace, so the command line is split on it.
fn run(command: &str, range: &Range, anchor: Option<&[u8]>) -> Ran {
    let scratch = Scratch::new();
    let store = scratch.0.join("store");
    let token = FsArtifactStore::open(&store)
        .unwrap()
        .put_artifact(TOKEN_BYTES)
        .unwrap();
    let events = range.chained(&token.to_string());
    let file: String = events
        .iter()
        .map(|(b, h)| format!("{{\"body\":{b},\"hash\":\"{h}\"}}\n"))
        .collect();
    let input = match command {
        "verify" => scratch.put("segment.jsonl", file.as_bytes()),
        _ => {
            scratch.put(
                "export/seg.manifest.json",
                &manifest(range, &events, file.as_bytes()),
            );
            scratch.put("export/seg.jsonl", file.as_bytes());
            scratch.0.join("export").to_str().unwrap().to_owned()
        }
    };
    let (from, prev, store) = (range.from_seq, &range.prev_hash, store.display());
    let anchor = anchor.map(|bytes| format!("--anchor {}", scratch.put("anchor.json", bytes)));
    let line = format!(
        "mandate journal {command} {input} --store {store} --from-seq {from} \
         --trusted-prev-hash {prev} {}",
        anchor.unwrap_or_default()
    );
    let mut report = Vec::new();
    let (code, failed) = match Cli::try_parse_from(line.split_whitespace())
        .unwrap()
        .command
    {
        Command::Journal(JournalCommand::Verify(args)) => {
            let outcome = journal::verify(&args, &mut report).unwrap();
            (outcome.code(), outcome.failed())
        }
        Command::Journal(JournalCommand::VerifyCold(args)) => {
            let outcome = cold::verify(&args, &mut report).unwrap();
            (outcome.code(), outcome.failed())
        }
        other => unreachable!("{other:?}"),
    };
    let report = String::from_utf8(report).unwrap();
    let result = report
        .lines()
        .find_map(|l| l.strip_prefix("result: "))
        .unwrap();
    (result.to_owned(), code, failed)
}

/// DEC-263's six-field manifest for the one segment `file` holds, from the test's own range.
fn manifest(range: &Range, events: &[(String, Digest)], file: &[u8]) -> Vec<u8> {
    let text = |s: &str| Value::Str(s.to_owned());
    let int = |n: u64| Value::Int(Int::new(n).unwrap());
    let object: Object = [
        ("stream", text(range.stream())),
        ("first_seq", int(range.from_seq)),
        ("last_seq", int(range.last_seq())),
        ("first_prev_hash", text(&range.prev_hash)),
        ("last_hash", text(&events.last().unwrap().1.to_hex())),
        ("file_sha256", text(&Digest::of(file).to_hex())),
    ]
    .into_iter()
    .map(|(k, v)| (Key::new(k).unwrap(), v))
    .collect();
    to_canonical(&Value::Object(object))
}

/// An anchor file with one leaf, for the control stream at `seq`, naming `hash`.
fn anchor_file(seq: u64, hash: Digest) -> Vec<u8> {
    let leaf = AnchorLeaf {
        stream_id: CTL.to_owned(),
        seq,
        hash,
    };
    let root = Anchor::compute(vec![leaf]).unwrap().root;
    format!("{{\"leaves\":[{{\"hash\":\"{hash}\",\"seq\":{seq},\"stream_id\":\"{CTL}\"}}],\"root\":\"{root}\"}}")
        .into_bytes()
}

#[track_caller]
fn assert_fails(ran: Ran, seq: u64, check: &str, what: &str) {
    let expected = (format!("failed, seq {seq}, {check}"), Some(check), true);
    assert_eq!(ran, expected, "{what}: DEC-782 item 3's line, code, exit");
}

#[track_caller]
fn assert_verified(ran: Ran, range: &Range, what: &str) {
    let last = range
        .chained(&ArtifactRef::of(TOKEN_BYTES).to_string())
        .pop()
        .unwrap()
        .1;
    let (stream, first, end) = (range.stream(), range.from_seq, range.last_seq());
    let expected = format!("verified, stream {stream}, seq {first} to {end}, last hash {last}");
    assert_eq!(ran, (expected, None, false), "{what}");
}

#[test]
fn the_vector_chains_rebuild_to_the_vectors_hashes() {
    for case in SECTIONS.into_iter().flat_map(cases) {
        let rebuilt = Range::of(&case).chained(VECTOR_TOKEN);
        for ((_, hash), event) in rebuilt.iter().zip(case["chain"].as_array().unwrap()) {
            assert_eq!(event["hash"], hash.to_hex(), "{}", case["name"]);
        }
        assert_eq!(rebuilt.len(), case["chain"].as_array().unwrap().len());
    }
}

#[test]
fn the_vector_ranges_that_pass_verify_on_both_commands() {
    let passing: Vec<Json> = SECTIONS
        .into_iter()
        .flat_map(cases)
        .filter(|c| c["expect"].is_null())
        .collect();
    assert_eq!(passing.len(), 3, "one anchor and two operator-read passes");
    for case in &passing {
        let range = Range::of(case);
        for command in BOTH {
            let what = format!("{command} {}", case["name"]);
            assert_verified(run(command, &range, None), &range, &what);
        }
    }
}

#[test]
fn the_control_checks_run_on_no_other_stream_type() {
    for (case, _, _) in SECTIONS.into_iter().flat_map(failing) {
        for stream in OTHER_TYPES {
            let range = Range::of(&case).on_stream(stream);
            for command in BOTH {
                let what = format!("{command} {} on {stream}", case["name"]);
                assert_verified(run(command, &range, None), &range, &what);
            }
        }
    }
}

#[test]
#[ignore = "pending E12-3"]
fn anchor_self_mismatch_vectors_fail_at_the_anchor() {
    for (case, seq, check) in failing("cold_records") {
        for command in BOTH {
            let what = format!("{command} {}", case["name"]);
            assert_fails(run(command, &Range::of(&case), None), seq, &check, &what);
        }
    }
}

#[test]
#[ignore = "pending E12-3"]
fn break_glass_cause_mismatch_vectors_fail_at_the_read() {
    for (case, seq, check) in failing("records_access") {
        for command in BOTH {
            let what = format!("{command} {}", case["name"]);
            assert_fails(run(command, &Range::of(&case), None), seq, &check, &what);
        }
    }
}

#[test]
#[ignore = "pending E12-3"]
fn the_lowest_seq_wins_between_the_two_control_checks() {
    const BAD_READ: &str = "break_glass_cause_mismatch";
    const BAD_ANCHOR: &str = "anchor_self_mismatch";
    let anchor = range("cold_records", "anchor_names_an_earlier_seq");
    let leaves = anchor.bodies[3]["payload"]["leaves"].as_array().unwrap();
    let own = leaves.iter().find(|leaf| leaf["stream_id"] == CTL).unwrap();
    assert_ne!(own["seq"], 3, "appended at seq 4, it does not name seq 3");
    let read =
        range("records_access", "operator_read_fails_until_dec_261_item_9").bodies[2].clone();
    assert_eq!(read["actor"]["kind"], "platform_operator");
    let read_first =
        range("records_access", "operator_read_citing_a_later_action").then(&anchor.bodies[3]);
    let anchor_first = anchor.then(&read);
    for command in BOTH {
        let what = format!("{command}: a bad read at 2, then a bad anchor at 4");
        assert_fails(run(command, &read_first, None), 2, BAD_READ, &what);
        let what = format!("{command}: a bad anchor at 4, then an uncaused read at 5");
        assert_fails(run(command, &anchor_first, None), 4, BAD_ANCHOR, &what);
    }
}

#[test]
#[ignore = "pending E12-3"]
fn a_control_check_is_reported_before_a_later_anchor_failure() {
    let honest = range("cold_records", "anchor_names_the_head_before_it");
    let not_the_head = Digest::of(b"not the head");
    for command in BOTH {
        let ran = run(
            command,
            &honest,
            Some(&anchor_file(honest.last_seq(), not_the_head)),
        );
        assert_eq!(
            ran.0, "failed, anchor_head_mismatch",
            "{command}: the anchor file fails"
        );
        for (case, seq, check) in SECTIONS.into_iter().flat_map(failing) {
            let range = Range::of(&case);
            let anchor = anchor_file(range.last_seq(), not_the_head);
            let what = format!("{command} {} under a failing anchor", case["name"]);
            assert_fails(run(command, &range, Some(&anchor)), seq, &check, &what);
        }
    }
}
