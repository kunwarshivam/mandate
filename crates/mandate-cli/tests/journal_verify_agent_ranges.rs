//! E12-3 slice W2 (journal spec §11, DEC-782): the agent stream's range checks
//! `intent_action_mismatch`, `mode_event_mismatch` and `held_mismatch` through `mandate journal
//! verify` and `verify-cold`, after checks 1 to 6 and before the anchor and token, on an agent
//! stream only, at the lowest `seq`, as `result: failed, seq <n>, <code>`, with no hold anchor: a
//! range from `seq` 1 is anchored on nothing, and a tail range fails closed (DEC-782 item 4).
//!
//! Oracles: `fixtures/refcases/journal.json`'s `agent_stream` chain with its `range_verification`
//! changes (re-chained here, an untouched chain reproducing the vectors' hashes) and the `hold`
//! section's `range_verification` events, each wrapped in the chain's own `AgentModeChanged`
//! envelope, since checks 1 to 6 read no payload. Expected failures are read, never written. Each
//! agent check reads one event type, so item 2's tie-break has no agent case.
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use clap::Parser;
use mandate_artifacts_fs::FsArtifactStore;
use mandate_canon::{Digest, parse, to_canonical};
use mandate_cli::journal::{self, JournalCommand, cold};
use mandate_cli::{Cli, Command};
use mandate_journal::{Anchor, AnchorLeaf, ArtifactRef, ArtifactStore};
use serde_json::{Value as Json, json};

const OTHER_TYPES: [&str; 2] = ["acct:ws_01J8Z2:01J8Z2ACCT00000000000000A1", "ctl:ws_01J8Z2"];
const BOTH: [&str; 2] = ["verify", "verify-cold"];
/// The `seq` of the chain's one `AgentModeChanged`, whose envelope wraps each `hold` event.
const MODE_SEQ: usize = 11;
const TAIL: u64 = 40;
const NO_TOKEN: &[u8] = b"no imprint";
const HELD_COPY: &str = "a_reconciliation_sets_a_hold_nobody_asked_for";

/// The vectors' list `name` of `section`, or of the top level when `section` is empty.
fn list(section: &str, name: &str) -> Vec<Json> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let vectors: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let section = (!section.is_empty()).then(|| &vectors[section]);
    section.unwrap_or(&vectors)[name]
        .as_array()
        .unwrap()
        .clone()
}

fn canonical(json: &Json) -> Vec<u8> {
    to_canonical(&parse(&serde_json::to_vec(json).unwrap()).unwrap())
}

/// A range: its trusted start and its bodies in `seq` order.
#[derive(Debug, Clone)]
struct Range {
    from_seq: u64,
    prev_hash: Digest,
    bodies: Vec<Json>,
}

impl Range {
    /// The agent chain from `seq` 1, with `changes` (`{seq, path, value}`) applied.
    fn chain(changes: &[Json]) -> Self {
        let chain = list("agent_stream", "chain");
        let mut bodies: Vec<Json> = chain.iter().map(|e| e["body"].clone()).collect();
        for change in changes {
            let mut at = &mut bodies[change["seq"].as_u64().unwrap() as usize - 1];
            for key in change["path"].as_str().unwrap().split('.') {
                at = &mut at[key];
            }
            *at = change["value"].clone();
        }
        Self::from(1, bodies)
    }

    fn from(from_seq: u64, bodies: Vec<Json>) -> Self {
        let tail = (from_seq > 1).then(|| Digest::of(b"the stored chain before the range"));
        let prev_hash = tail.unwrap_or(Digest::ZERO);
        Self {
            from_seq,
            prev_hash,
            bodies,
        }
    }

    /// A `hold` case's events from `from_seq`, each in the chain's `AgentModeChanged` envelope.
    fn hold(case: &Json, from_seq: u64) -> Self {
        let template = &Self::chain(&[]).bodies[MODE_SEQ - 1];
        let events = case["events"].as_array().unwrap().iter().enumerate();
        let bodies = events.map(|(i, event)| {
            let mut body = template.clone();
            body["event_id"] = format!("01J8ZNB0H{i:017}").into();
            for member in ["event_type", "schema_version", "payload"] {
                body[member] = event[member].clone();
            }
            body
        });
        Self::from(from_seq, bodies.collect())
    }

    /// The events renumbered from `from_seq` and chained from `prev_hash`, each `(body, hash)`.
    fn chained(&self) -> Vec<(String, Digest)> {
        let mut prev = self.prev_hash.to_hex();
        let mut out = Vec::new();
        for (seq, body) in (self.from_seq..).zip(&self.bodies) {
            let mut body = body.clone();
            body["seq"] = seq.into();
            body["prev_hash"] = prev.clone().into();
            let bytes = canonical(&body);
            let hash = Digest::of(&bytes);
            prev = hash.to_hex();
            out.push((String::from_utf8(bytes).unwrap(), hash));
        }
        out
    }

    fn last_seq(&self) -> u64 {
        self.from_seq + self.bodies.len() as u64 - 1
    }

    fn stream(&self) -> &str {
        self.bodies[0]["stream_id"].as_str().unwrap()
    }

    fn head(&self) -> Digest {
        self.chained().pop().unwrap().1
    }

    /// An anchor file with one leaf for the range's head naming `hash`.
    fn anchor(&self, hash: Digest) -> Vec<u8> {
        let (stream_id, seq) = (self.stream().to_owned(), self.last_seq());
        let leaves = json!([{"hash": hash.to_hex(), "seq": seq, "stream_id": stream_id}]);
        let leaf = AnchorLeaf {
            stream_id,
            seq,
            hash,
        };
        let root = Anchor::compute(vec![leaf]).unwrap().root.to_hex();
        serde_json::to_vec(&json!({"leaves": leaves, "root": root})).unwrap()
    }
}

/// The agent vectors' failing ranges, each with its name and the `(seq, code)` it expects.
fn agent_failing() -> Vec<(String, Range, u64, String)> {
    let cases = list("agent_stream", "range_verification");
    assert_eq!(cases.len(), 4, "one intent and three kill-switch cases");
    let case = |c: &Json| {
        assert_eq!(c["from_seq"], 1, "{}", c["name"]);
        let range = Range::chain(c["changes"].as_array().unwrap());
        let (seq, code) = (c["expect"]["seq"].as_u64(), c["expect"]["code"].as_str());
        (
            c["name"].to_string(),
            range,
            seq.unwrap(),
            code.unwrap().into(),
        )
    };
    cases.iter().map(case).collect()
}

/// The `hold` cases a caller with no anchor runs as the vector does: a full chain, or a tail
/// range the vector also gives no anchor, each with the `seq` of its first failure.
fn hold_unanchored() -> Vec<(String, Range, Option<u64>)> {
    let cases = list("hold", "range_verification");
    let found: Vec<_> = (cases.iter().filter(|c| c["anchor"].is_null()))
        .map(|c| {
            let from = c["from_seq"].as_u64().unwrap();
            let first = c["expect"].as_array().unwrap().first();
            let first = first.map(|i| from + i.as_u64().unwrap());
            (c["name"].to_string(), Range::hold(c, from), first)
        })
        .collect();
    assert!(found.iter().any(|f| f.1.from_seq > 1 && f.2.is_some()));
    assert!(found.iter().any(|f| f.1.from_seq > 1 && f.2.is_none()));
    found
}

/// The `hold` cases anchored on nothing (a full chain, or no version 2 before the range) that pass
/// and whose first version-2 `AgentModeChanged` is not a hold or a lift, with that record's index.
fn opening_with_a_copy() -> Vec<(Json, u64)> {
    let lifts = ["owner_hold", "owner_lift_hold"];
    let nothing = |c: &Json| c["anchor"].is_null() && c["from_seq"] == 1;
    let found: Vec<_> = list("hold", "range_verification")
        .into_iter()
        .filter(|c| c["expect"].as_array().unwrap().is_empty())
        .filter(|c| nothing(c) || c["anchor"]["v2_before"] == false)
        .filter_map(|c| {
            let events = c["events"].as_array().unwrap();
            let v2 = events.iter().position(|e| e["schema_version"] == 2)?;
            let reason = events[v2]["payload"]["reason"].as_str().unwrap();
            (!lifts.contains(&reason)).then_some((c, v2 as u64))
        })
        .collect();
    assert!(found.len() >= 2, "a full chain and a range after no v2");
    found
}

/// What a run answered: its report's `result:` text, its code, and whether it exits non-zero.
type Ran = (String, Option<&'static str>, bool);

fn put(dir: &Path, name: &str, bytes: &[u8]) -> String {
    fs::write(dir.join(name), bytes).unwrap();
    dir.join(name).to_str().unwrap().to_owned()
}

/// Runs `command` over `range` written as one segment, with the vectors' configuration and agent
/// artifacts stored and each `(flag, bytes)` of `files` given as a file.
fn run(command: &str, range: &Range, files: &[(&str, &[u8])]) -> Ran {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let scratch = std::env::temp_dir().join(format!("mandate-agent-{}-{n}", std::process::id()));
    fs::create_dir_all(scratch.join("export")).unwrap();
    let store = scratch.join("store");
    let mut artifacts = FsArtifactStore::open(&store).unwrap();
    for artifact in list("", "artifacts")
        .iter()
        .chain(&list("agent_stream", "artifacts"))
    {
        let bytes = artifact["canonical"].as_str().unwrap().as_bytes();
        artifacts.put_artifact(bytes).unwrap();
    }
    let events = range.chained();
    let file: String = (events.iter())
        .map(|(b, h)| format!("{{\"body\":{b},\"hash\":\"{h}\"}}\n"))
        .collect();
    let input = match command {
        "verify" => put(&scratch, "segment.jsonl", file.as_bytes()),
        _ => {
            let manifest = json!({
                "stream": range.stream(), "first_seq": range.from_seq,
                "last_seq": range.last_seq(), "first_prev_hash": range.prev_hash.to_hex(),
                "last_hash": range.head().to_hex(), "file_sha256": Digest::of(file.as_bytes()).to_hex(),
            });
            put(&scratch, "export/seg.manifest.json", &canonical(&manifest));
            put(&scratch, "export/seg.jsonl", file.as_bytes());
            scratch.join("export").to_str().unwrap().to_owned()
        }
    };
    let (from, prev, store) = (range.from_seq, &range.prev_hash, store.display());
    let files: String = (files.iter())
        .map(|(flag, bytes)| format!("{flag} {} ", put(&scratch, flag, bytes)))
        .collect();
    let line = format!(
        "mandate journal {command} {input} --store {store} --from-seq {from} \
         --trusted-prev-hash {prev} {files}"
    );
    let mut report = Vec::new();
    let parsed = Cli::try_parse_from(line.split_whitespace()).unwrap();
    let (code, failed) = match parsed.command {
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
    let result = report.lines().find_map(|l| l.strip_prefix("result: "));
    (result.unwrap().to_owned(), code, failed)
}

/// Runs both commands over `range`, each answering `want`: a failure at `(seq, code)` printed as
/// DEC-782 item 3's line with a non-zero exit, or, with `None`, the range verified.
#[track_caller]
fn expect(range: &Range, want: Option<(u64, &str)>, what: &str) {
    let (stream, first, end) = (range.stream(), range.from_seq, range.last_seq());
    let head = range.head();
    let text = match want {
        Some((seq, code)) => format!("failed, seq {seq}, {code}"),
        None => format!("verified, stream {stream}, seq {first} to {end}, last hash {head}"),
    };
    for command in BOTH {
        let ran = run(command, range, &[]);
        let expected = (text.as_str(), want.map(|w| w.1), want.is_some());
        assert_eq!((ran.0.as_str(), ran.1, ran.2), expected, "{command} {what}");
    }
}

#[test]
fn the_agent_chain_rebuilds_to_the_vectors_hashes_and_verifies() {
    let chain = Range::chain(&[]);
    for (event, (_, hash)) in list("agent_stream", "chain").iter().zip(chain.chained()) {
        assert_eq!(event["hash"], hash.to_hex(), "seq {}", event["seq"]);
    }
    expect(&chain, None, "the untouched chain");
}

/// `intent_action_mismatch`'s second clause, an `IntentProposed` caused by an
/// `ApprovalRevalidated`, has no vector until §9.1 closes the approval events' schemas, so this
/// runs the first clause and `mode_event_mismatch` only.
#[test]
#[ignore = "pending E12-3"]
fn the_failing_agent_vectors_fail_at_their_seq_with_their_check() {
    for (name, range, seq, code) in agent_failing() {
        expect(&range, Some((seq, &code)), &name);
    }
}

#[test]
#[ignore = "pending E12-3"]
fn the_failing_unanchored_hold_vectors_fail_at_their_first_failure() {
    for (name, range, first) in hold_unanchored() {
        if let Some(seq) = first {
            expect(&range, Some((seq, "held_mismatch")), &name);
        }
    }
}

/// The passing unanchored `hold` vectors verify, and so does each range anchored on nothing that
/// [`a_tail_range_fails_closed_at_its_first_version_2_copy`] moves to a tail, read from `seq` 1.
#[test]
fn the_passing_unanchored_hold_vectors_and_their_full_chains_verify() {
    for (name, range, _) in hold_unanchored().into_iter().filter(|h| h.2.is_none()) {
        expect(&range, None, &name);
    }
    for (case, _) in opening_with_a_copy() {
        let what = format!("{} from seq 1", case["name"]);
        expect(&Range::hold(&case, 1), None, &what);
    }
}

#[test]
#[ignore = "pending E12-3"]
fn a_tail_range_fails_closed_at_its_first_version_2_copy() {
    for (case, index) in opening_with_a_copy() {
        let what = format!("{} from {TAIL}", case["name"]);
        let range = Range::hold(&case, TAIL);
        expect(&range, Some((TAIL + index, "held_mismatch")), &what);
    }
}

#[test]
#[ignore = "pending E12-3"]
fn the_lowest_seq_wins_across_the_agent_checks() {
    let cases = list("hold", "range_verification");
    let hold = cases.iter().find(|c| c["name"] == HELD_COPY).unwrap();
    assert_eq!(hold["expect"], json!([0]), "a held copy fails alone");
    let held = &Range::hold(hold, 1).bodies[0];
    for (name, mut range, seq, code) in agent_failing() {
        for member in ["event_type", "schema_version", "payload"] {
            range.bodies[MODE_SEQ - 1][member] = held[member].clone();
        }
        let mode = MODE_SEQ as u64;
        let want = [(seq, code.as_str()), (mode, "held_mismatch")];
        let want = want.into_iter().min_by_key(|w| w.0);
        expect(&range, want, &format!("{name}, a held copy at {mode}"));
    }
}

#[test]
fn the_agent_checks_run_on_no_other_stream_type() {
    let agent = agent_failing().into_iter().map(|(n, r, _, _)| (n, r));
    let hold = hold_unanchored().into_iter().filter(|h| h.2.is_some());
    for (name, mut range) in agent.chain(hold.map(|(n, r, _)| (n, r))) {
        for stream in OTHER_TYPES {
            (range.bodies.iter_mut()).for_each(|b| b["stream_id"] = stream.into());
            expect(&range, None, &format!("{name} on {stream}"));
        }
    }
}

#[test]
#[ignore = "pending E12-3"]
fn an_agent_check_is_reported_before_the_anchor_and_the_token() {
    let honest = Range::chain(&[]);
    let not_the_head = Digest::of(b"not the head");
    let failing = agent_failing()
        .into_iter()
        .map(|(n, r, s, c)| (n, r, Some((s, c))));
    for (name, range, want) in std::iter::once(("honest".into(), honest, None)).chain(failing) {
        let (bad, good) = (range.anchor(not_the_head), range.anchor(range.head()));
        let bad = [("--anchor", bad.as_slice())];
        let token = [("--anchor", good.as_slice()), ("--token", NO_TOKEN)];
        let runs = [
            ("verify", &bad[..]),
            ("verify-cold", &bad),
            ("verify-cold", &token),
        ];
        for (command, files) in runs {
            let alone = ["anchor_head_mismatch", "tsa_token_invalid"][files.len() - 1];
            let expected = match &want {
                None => format!("failed, {alone}"),
                Some((seq, code)) => format!("failed, seq {seq}, {code}"),
            };
            let ran = run(command, &range, files).0;
            assert_eq!(ran, expected, "{command} {name}, {alone}");
        }
    }
}

#[test]
fn a_check_one_to_six_is_reported_before_a_later_agent_check() {
    for (name, mut range, seq, _) in agent_failing() {
        let last = range.last_seq();
        let unstored = ArtifactRef::of(b"never stored").to_string();
        range.bodies.last_mut().unwrap()["artifact_refs"] = json!([unstored]);
        let what = format!("{name}: failing at {seq}, an artifact missing at {last}");
        expect(&range, Some((last, "artifact_missing")), &what);
    }
}
