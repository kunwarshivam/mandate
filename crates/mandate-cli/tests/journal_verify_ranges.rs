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
use std::path::{Path, PathBuf};
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

/// A new scratch directory holding an empty `export/`; [`run`] removes it.
fn scratch() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("mandate-ranges-{}-{n}", std::process::id()));
    fs::create_dir_all(dir.join("export")).unwrap();
    dir
}

fn put(dir: &Path, name: &str, bytes: &[u8]) -> String {
    fs::write(dir.join(name), bytes).unwrap();
    dir.join(name).to_str().unwrap().to_owned()
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
    let case = cases(section).into_iter().find(|c| c["name"] == name);
    Range::of(&case.unwrap())
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

/// A range: its trusted start, its bodies in `seq` order, and a `seq` whose stored hash is stale.
#[derive(Debug, Clone)]
struct Range {
    from_seq: u64,
    prev_hash: String,
    bodies: Vec<Json>,
    stale_hash_at: Option<u64>,
}

impl Range {
    fn of(case: &Json) -> Self {
        let chain = case["chain"].as_array().unwrap();
        Self {
            from_seq: case["from_seq"].as_u64().unwrap_or(1),
            prev_hash: case["prev_hash"].as_str().unwrap_or(ZERO_HASH).to_owned(),
            bodies: chain.iter().map(|e| e["body"].clone()).collect(),
            stale_hash_at: None,
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
            let stored = if self.stale_hash_at == Some(seq) {
                Digest::of(b"stale")
            } else {
                hash
            };
            out.push((String::from_utf8(bytes).unwrap(), stored));
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

/// Runs `command` over `range` written as one segment, with each `(flag, bytes)` of `files` given
/// as a file. The scratch paths hold no whitespace, so the command line is split on it.
fn run(command: &str, range: &Range, files: &[(&str, &[u8])]) -> Ran {
    let scratch = scratch();
    let store = scratch.join("store");
    FsArtifactStore::open(&store)
        .unwrap()
        .put_artifact(TOKEN_BYTES)
        .unwrap();
    let events = range.chained(&token());
    let file: String = events
        .iter()
        .map(|(b, h)| format!("{{\"body\":{b},\"hash\":\"{h}\"}}\n"))
        .collect();
    let input = match command {
        "verify" => put(&scratch, "segment.jsonl", file.as_bytes()),
        _ => {
            put(
                &scratch,
                "export/seg.manifest.json",
                &manifest(range, &events, file.as_bytes()),
            );
            put(&scratch, "export/seg.jsonl", file.as_bytes());
            scratch.join("export").to_str().unwrap().to_owned()
        }
    };
    let (from, prev, store) = (range.from_seq, &range.prev_hash, store.display());
    let files: String = files
        .iter()
        .map(|(flag, bytes)| format!("{flag} {} ", put(&scratch, flag, bytes)))
        .collect();
    let line = format!(
        "mandate journal {command} {input} --store {store} --from-seq {from} \
         --trusted-prev-hash {prev} {files}"
    );
    let mut report = Vec::new();
    let command = Cli::try_parse_from(line.split_whitespace())
        .unwrap()
        .command;
    let (code, failed) = match command {
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
    let _ = fs::remove_dir_all(&scratch);
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

fn token() -> String {
    ArtifactRef::of(TOKEN_BYTES).to_string()
}

/// An anchor file with one leaf, for the control stream at `seq`, naming `hash`, and its root.
fn anchor_file(seq: u64, hash: Digest) -> (Vec<u8>, Digest) {
    let leaf = AnchorLeaf {
        stream_id: CTL.to_owned(),
        seq,
        hash,
    };
    let root = Anchor::compute(vec![leaf]).unwrap().root;
    let leaves = format!("[{{\"hash\":\"{hash}\",\"seq\":{seq},\"stream_id\":\"{CTL}\"}}]");
    let bytes = format!("{{\"leaves\":{leaves},\"root\":\"{root}\"}}").into_bytes();
    (bytes, root)
}

#[track_caller]
fn assert_fails(ran: Ran, seq: u64, check: &str, what: &str) {
    let expected = (format!("failed, seq {seq}, {check}"), Some(check), true);
    assert_eq!(ran, expected, "{what}: DEC-782 item 3's line, code, exit");
}

#[track_caller]
fn assert_verified(ran: Ran, range: &Range, what: &str) {
    let last = range.chained(&token()).pop().unwrap().1;
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
            assert_verified(run(command, &range, &[]), &range, &what);
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
                assert_verified(run(command, &range, &[]), &range, &what);
            }
        }
    }
}

#[test]
#[ignore = "pending E12-3"]
fn the_failing_vectors_fail_at_their_seq_with_their_check() {
    for (case, seq, check) in SECTIONS.into_iter().flat_map(failing) {
        for command in BOTH {
            let what = format!("{command} {}", case["name"]);
            assert_fails(run(command, &Range::of(&case), &[]), seq, &check, &what);
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
        assert_fails(run(command, &read_first, &[]), 2, BAD_READ, &what);
        let what = format!("{command}: a bad anchor at 4, then an uncaused read at 5");
        assert_fails(run(command, &anchor_first, &[]), 4, BAD_ANCHOR, &what);
    }
}

#[test]
#[ignore = "pending E12-3"]
fn a_control_check_is_reported_before_a_later_anchor_failure() {
    let honest = range("cold_records", "anchor_names_the_head_before_it");
    let not_the_head = Digest::of(b"not the head");
    for command in BOTH {
        let (anchor, _) = anchor_file(honest.last_seq(), not_the_head);
        let ran = run(command, &honest, &[("--anchor", &anchor)]).0;
        assert_eq!(
            ran, "failed, anchor_head_mismatch",
            "{command}: the anchor fails"
        );
        for (case, seq, check) in SECTIONS.into_iter().flat_map(failing) {
            let range = Range::of(&case);
            let (anchor, _) = anchor_file(range.last_seq(), not_the_head);
            let ran = run(command, &range, &[("--anchor", &anchor)]);
            let what = format!("{command} {} under a failing anchor", case["name"]);
            assert_fails(ran, seq, &check, &what);
        }
    }
}

#[test]
#[ignore = "pending E12-3"]
fn a_control_check_is_reported_before_the_token() {
    let honest = range("cold_records", "anchor_names_the_head_before_it");
    let failing = SECTIONS.into_iter().flat_map(failing);
    let ranges = failing.map(|(case, seq, check)| (Range::of(&case), Some((seq, check))));
    for (range, expect) in std::iter::once((honest, None)).chain(ranges) {
        let head = range.chained(&token()).pop().unwrap().1;
        let (anchor, root) = anchor_file(range.last_seq(), head);
        let imprint = Digest::of(root.as_bytes());
        let claims = [b"TSTInfo ", imprint.as_bytes().as_slice()].concat();
        let tokens = [
            (b"no imprint".to_vec(), "tsa_token_invalid"),
            (claims, "tsa_verification_incomplete"),
        ];
        for (token, alone) in tokens {
            let files = [("--anchor", anchor.as_slice()), ("--token", &token)];
            let ran = run("verify-cold", &range, &files);
            match &expect {
                None => assert_eq!(ran.0, format!("failed, {alone}"), "the honest range"),
                Some((seq, check)) => assert_fails(ran, *seq, check, &format!("token: {alone}")),
            }
        }
    }
}

#[test]
fn a_check_one_to_six_is_reported_before_a_later_control_check() {
    let mut range = range("cold_records", "anchor_names_an_earlier_seq");
    range.stale_hash_at = Some(2);
    for command in BOTH {
        let what = format!("{command}: a stale hash at 2, then a bad anchor at 4");
        assert_fails(run(command, &range, &[]), 2, "rehash_mismatch", &what);
    }
}
