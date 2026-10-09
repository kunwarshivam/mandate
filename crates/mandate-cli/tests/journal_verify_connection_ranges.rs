//! E12-3 W3 (journal spec §9.8, §11; DEC-782, DEC-885, DEC-890): `connection_lifecycle_mismatch`
//! through `journal verify` and `verify-cold` on a control or account stream, in DEC-782's order.
//! A one-stream export from `seq` 1 is that stream's full chain; a tail has no anchor and fails
//! closed. The cause check never runs on one stream; DEC-890's [`NOT_RUN`] line says so.
//!
//! Oracles: the single-stream `connection*` sequences of `fixtures/refcases/journal.json`: a full
//! chain answers its `expect`, an anchored range's prefix plus records its `expect` moved past the
//! prefix (DEC-885 I1), and a tail a scan of §11's "No anchor" list, checked against the vectors.
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use clap::Parser;
use mandate_canon::{Digest, parse, to_canonical};
use mandate_cli::journal::{self, JournalCommand, cold};
use mandate_cli::{Cli, Command};
use mandate_journal::{Anchor, AnchorLeaf, ArtifactRef};
use serde_json::{Value as Json, json};

const SECTIONS: [&str; 3] = ["connections", "connection_requests", "connection_ranges"];
const AGENT: &str = "agent:ws_01J8Z2:agent_a";
const ACCOUNT: &str = "acct:ws_01J8Z2:01J8Z2ACCT00000000000000A1";
const BOTH: [&str; 2] = ["verify", "verify-cold"];
const CODE: &str = "connection_lifecycle_mismatch";
const GLASS: &str = "break_glass_cause_mismatch";
const TAIL: u64 = 40;
const NO_TOKEN: &[u8] = b"no imprint";
const OPERATOR_READ: &str = "operator_read_fails_until_dec_261_item_9";
const NOT_RUN: &str = "not run: connection_cause_mismatch (needs the control stream and its account streams together)";
/// §11's "No anchor" list (journal spec lines 3382-3384): what rules 66, 67 and 131 judge on a
/// control stream. A `ConnectionRevoked` is never judged (I6).
const CONTROL_JUDGED: &str = "ConnectionRequested ConnectionEstablished \
    ConnectionCredentialRotated ConnectionRefused";
/// "Any connection record on an account stream" (same lines), as §9.8's "Who writes what" lists
/// them (lines 2005-2008): checks, state, refreshes, and the two copies; rule 68 judges each.
const ACCOUNT_JUDGED: &str = "ConnectionChecked ConnectionStateChanged \
    ConnectionCredentialRefreshed ConnectionEstablished ConnectionCredentialRotated";

fn vectors() -> Json {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

/// `records`, each a `{base_draft, changes}` of `section`'s drafts, as stored bodies.
fn built(section: &Json, records: &Json) -> Vec<Json> {
    let build = |record: &Json| {
        let mut body = section["drafts"][record["base_draft"].as_str().unwrap()].clone();
        for change in record["changes"].as_array().unwrap() {
            let mut at = &mut body;
            for key in change["path"].as_str().unwrap().split('.') {
                at = &mut at[key];
            }
            *at = change["value"].clone();
        }
        body["recorded_at"] = body["event_time"].clone();
        body
    };
    records.as_array().unwrap().iter().map(build).collect()
}

/// A full chain's answer: the `(seq, code)` it fails at, or `None` when it verifies.
type Want = Option<(u64, String)>;

/// A sequence: its name, the chain before it (`None` when none is given), records, `expect`.
struct Sequence(String, Option<Vec<Json>>, Vec<Json>, Json);

impl Sequence {
    /// Its full chain from `seq` 1 and that chain's answer, when the vector gives one.
    fn genesis(&self) -> Option<(Range, Want)> {
        let (before, expect) = (self.1.clone().unwrap_or_default(), &self.3);
        let at = before.len() as u64 + expect["index"].as_u64().unwrap_or_default() + 1;
        let want = match expect["outcome"].as_str().unwrap() {
            "Valid" => None,
            "Mismatch" => Some((at, expect["code"].as_str()?.to_owned())),
            _ => return None,
        };
        let bodies = before.into_iter().chain(self.2.clone()).collect();
        Some((Range::from(1, bodies), want))
    }
}

/// Every sequence whose records, and the chain before them, sit on one stream.
fn single_stream() -> Vec<Sequence> {
    let vectors = vectors();
    let mut found = Vec::new();
    for section in SECTIONS.map(|s| &vectors[s]) {
        for case in section["sequences"].as_array().unwrap() {
            let before = (!case["before"].is_null()).then(|| built(section, &case["before"]));
            let records = built(section, &case["records"]);
            let all = before.iter().flatten().chain(&records);
            let streams: Vec<_> = all.map(|b| &b["stream_id"]).collect();
            if streams.iter().all(|s| *s == streams[0]) {
                let (name, expect) = (case["name"].to_string(), case["expect"].clone());
                found.push(Sequence(name, before, records, expect));
            }
        }
    }
    assert!(found.len() >= 80, "{} single-stream sequences", found.len());
    found
}

/// Each single-stream full chain the vectors answer, with its name.
fn genesis() -> Vec<(String, Range, Want)> {
    let found = single_stream().into_iter();
    let found = found.filter_map(|s| s.genesis().map(|(range, want)| (s.0, range, want)));
    found.collect()
}

/// A range: its trusted start's `seq` and its bodies in `seq` order.
struct Range {
    from_seq: u64,
    bodies: Vec<Json>,
}

impl Range {
    fn from(from_seq: u64, bodies: Vec<Json>) -> Self {
        Self { from_seq, bodies }
    }

    /// The trusted start's hash: genesis from `seq` 1, any other hash for a tail.
    fn prev_hash(&self) -> Digest {
        let tail = (self.from_seq > 1).then(|| Digest::of(b"the stored chain before the range"));
        tail.unwrap_or(Digest::ZERO)
    }

    fn on(mut self, stream: &str) -> Self {
        (self.bodies.iter_mut()).for_each(|b| b["stream_id"] = stream.into());
        self
    }

    /// The `seq` of the first record §11's "No anchor" list judges, by its stream type's list.
    fn first_judged(&self) -> Option<u64> {
        let judged = |b: &Json| {
            let listed = match b["stream_id"].as_str().unwrap().split(':').next() {
                Some("ctl") => CONTROL_JUDGED,
                Some("acct") => ACCOUNT_JUDGED,
                _ => "",
            };
            listed.split(' ').any(|kind| b["event_type"] == kind)
        };
        let index = self.bodies.iter().position(judged);
        index.map(|i| self.from_seq + i as u64)
    }

    /// The events renumbered from `from_seq` and chained from `prev_hash`, each `(body, hash)`.
    fn chained(&self) -> Vec<(String, Digest)> {
        let (mut prev, mut out) = (self.prev_hash().to_hex(), Vec::new());
        for (seq, body) in (self.from_seq..).zip(&self.bodies) {
            let mut body = body.clone();
            body["seq"] = seq.into();
            body["prev_hash"] = prev.clone().into();
            let bytes = to_canonical(&parse(&serde_json::to_vec(&body).unwrap()).unwrap());
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

/// What a run answered: its report's lines, its code, and whether it exits non-zero.
type Ran = (Vec<String>, Option<&'static str>, bool);

fn result(ran: &Ran) -> &str {
    ran.0.last().unwrap().strip_prefix("result: ").unwrap()
}

/// Asserts `ran`'s report is DEC-115's six lines or DEC-490's seven, with DEC-890's line inserted
/// just before `result:` exactly when `line`.
#[track_caller]
fn assert_form(command: &str, ran: &Ran, line: bool, what: &str) {
    let (inputs, last) = [(6, "anchor: "), (7, "token: ")][usize::from(command != "verify")];
    let before = &ran.0[ran.0.len() - 2];
    let fits = [before.starts_with(last), before == NOT_RUN][usize::from(line)];
    let (form, want) = ((ran.0.len(), fits), (inputs + usize::from(line), true));
    assert_eq!(form, want, "{command} {what}: {:?}", ran.0);
}

fn put(dir: &Path, name: &str, bytes: &[u8]) -> String {
    fs::write(dir.join(name), bytes).unwrap();
    dir.join(name).to_str().unwrap().to_owned()
}

/// Runs `command` over `range` written as one segment, with an empty artifact store and each
/// `(flag, bytes)` of `files` given as a file.
fn run(command: &str, range: &Range, files: &[(&str, &[u8])]) -> Ran {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let scratch = std::env::temp_dir().join(format!("mandate-conn-{}-{n}", std::process::id()));
    fs::create_dir_all(scratch.join("export")).unwrap();
    fs::create_dir_all(scratch.join("store")).unwrap();
    let file: String = (range.chained().iter())
        .map(|(b, h)| format!("{{\"body\":{b},\"hash\":\"{h}\"}}\n"))
        .collect();
    let (from, prev) = (range.from_seq, range.prev_hash());
    let input = match command {
        "verify" => put(&scratch, "segment.jsonl", file.as_bytes()),
        _ => {
            let manifest = json!({
                "stream": range.stream(), "first_seq": from, "last_seq": range.last_seq(),
                "first_prev_hash": prev.to_hex(), "last_hash": range.head().to_hex(),
                "file_sha256": Digest::of(file.as_bytes()).to_hex(),
            });
            let manifest = to_canonical(&parse(&serde_json::to_vec(&manifest).unwrap()).unwrap());
            put(&scratch, "export/seg.manifest.json", &manifest);
            put(&scratch, "export/seg.jsonl", file.as_bytes());
            scratch.join("export").to_str().unwrap().to_owned()
        }
    };
    let files: String = (files.iter())
        .map(|(flag, bytes)| format!("{flag} {} ", put(&scratch, flag, bytes)))
        .collect();
    let line = format!(
        "mandate journal {command} {input} --store {} --from-seq {from} \
         --trusted-prev-hash {prev} {files}",
        scratch.join("store").display()
    );
    let (mut report, parsed) = (Vec::new(), Cli::try_parse_from(line.split_whitespace()));
    let (code, failed) = match parsed.unwrap().command {
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
    (report.lines().map(str::to_owned).collect(), code, failed)
}

/// Runs both commands over `range`, each answering `want` (a failure at `(seq, code)`, or `None`
/// for verified) with its exit status, in [`assert_form`]'s form with DEC-890's line when `line`.
#[track_caller]
fn expect(range: &Range, want: Option<(u64, &str)>, line: bool, what: &str) {
    let (stream, first, end) = (range.stream(), range.from_seq, range.last_seq());
    let head = range.head();
    let text = match want {
        Some((seq, code)) => format!("failed, seq {seq}, {code}"),
        None => format!("verified, stream {stream}, seq {first} to {end}, last hash {head}"),
    };
    for command in BOTH {
        let ran = run(command, range, &[]);
        let expected = (text.as_str(), want.map(|w| w.1), want.is_some());
        assert_eq!((result(&ran), ran.1, ran.2), expected, "{command} {what}");
        assert_form(command, &ran, line, what);
    }
}

/// The `records_access` operator read's chain, which holds no connection record, and the `seq`
/// it fails `break_glass_cause_mismatch` at from `seq` 1.
fn operator_read() -> (Vec<Json>, u64) {
    let cases = vectors()["records_access"]["range_checks"].clone();
    let read = (cases.as_array().unwrap().iter()).find(|c| c["name"] == OPERATOR_READ);
    let (read, chain) = (read.unwrap(), read.unwrap()["chain"].as_array().unwrap());
    assert_eq!(read["expect"]["check"], GLASS);
    let reads = chain.iter().map(|e| e["body"].clone()).collect();
    (reads, read["expect"]["seq"].as_u64().unwrap())
}

/// With DEC-890's line but on revocations only. Valid control chains name account-stream causes
/// the export lacks, so they verify only while no single-stream run judges the cause check.
#[test]
fn the_single_stream_chains_answer_their_vectors_from_seq_1() {
    let chains = genesis();
    let kinds = ["ctl:", "acct:"].map(|stream| [(stream, false), (stream, true)]);
    for (stream, fails) in kinds.into_iter().flatten() {
        let found = |c: &&(String, Range, Want)| c.1.stream().starts_with(stream);
        assert!(chains.iter().filter(found).any(|c| c.2.is_some() == fails));
    }
    for (name, range, want) in &chains {
        let want = want.as_ref().map(|(seq, code)| (*seq, code.as_str()));
        expect(range, want, range.first_judged().is_some(), name);
    }
}

/// The scan agrees with every unanchored vector; every sequence's records and full chain (valid
/// ones too), moved to `seq` 2, the first tail, and to [`TAIL`], fail closed where it says.
#[test]
fn a_tail_range_fails_closed_at_its_first_judged_record() {
    let sequences = single_stream();
    for s in (sequences.iter()).filter(|s| s.3["outcome"] == "Unanchored") {
        let (range, index) = (Range::from(TAIL, s.2.clone()), s.3["index"].as_u64());
        assert_eq!(range.first_judged(), Some(TAIL + index.unwrap()), "{}", s.0);
    }
    let tails = sequences.iter().flat_map(|s| {
        let chain = s.genesis().map(|g| g.0.bodies);
        [Some(s.2.clone()), chain].into_iter().flatten()
    });
    let mut closed = 0;
    for (n, (from, bodies)) in tails.flat_map(|b| [(2, b.clone()), (TAIL, b)]).enumerate() {
        let range = Range::from(from, bodies);
        let want = range.first_judged().map(|seq| (seq, CODE));
        closed += usize::from(want.is_some());
        let what = format!("tail {n} from {from}");
        expect(&range, want, want.is_some(), &what);
    }
    assert!(closed >= 200, "{closed} tails fail closed");
}

/// The control checks judge `AnchorComputed` and `RecordsAccessed`, the connection check only
/// connection records, so no record fails both and DEC-782 item 2's tie-break has no case.
#[test]
fn the_lowest_seq_wins_between_the_control_and_connection_checks() {
    let (reads, glass) = operator_read();
    let failing = genesis().into_iter().filter(|g| g.2.is_some());
    for (name, range, want) in failing.filter(|g| g.1.stream().starts_with("ctl:")) {
        let first = Range::from(1, [reads.clone(), range.bodies.clone()].concat());
        expect(&first, Some((glass, GLASS)), true, &name);
        let last = Range::from(1, [range.bodies, reads.clone()].concat());
        expect(&last, Some((want.unwrap().0, CODE)), true, &name);
    }
}

/// No connection check and DEC-115's and DEC-490's reports unchanged, without DEC-890's line, on
/// an agent stream and on a control or account export holding no connection record.
#[test]
fn an_agent_export_or_one_without_connection_records_runs_no_connection_check() {
    for (name, range, _) in genesis().into_iter().filter(|g| g.2.is_some()) {
        expect(&range.on(AGENT), None, false, &format!("{name} on {AGENT}"));
    }
    let (reads, glass) = operator_read();
    let control = Range::from(1, reads.clone());
    expect(&control, Some((glass, GLASS)), false, "a read");
    let reads = Range::from(1, reads).on(ACCOUNT);
    expect(&reads, None, false, "a read on an account stream");
}

/// The DEC-890 line stays on an anchor's or a token's failure: the stream checks ran.
#[test]
fn a_connection_check_is_reported_before_the_anchor_and_the_token() {
    let failing = genesis().into_iter().filter(|g| g.2.is_some());
    let honest = genesis().into_iter().find(|g| g.2.is_none()).unwrap();
    let tail = Range::from(TAIL, honest.1.bodies.clone());
    let closed = Some((tail.first_judged().unwrap(), CODE.into()));
    let tail = ("tail".into(), tail, closed);
    let cases = [honest, tail].into_iter().chain(failing);
    let not_the_head = Digest::of(b"not the head");
    for (name, range, want) in cases {
        let (bad, good) = (range.anchor(not_the_head), range.anchor(range.head()));
        let bad = [("--anchor", bad.as_slice())];
        let token = [("--anchor", good.as_slice()), ("--token", NO_TOKEN)];
        let runs = [("verify", &bad[..]), ("verify-cold", &bad)];
        for (command, files) in runs.into_iter().chain([("verify-cold", &token[..])]) {
            let alone = ["anchor_head_mismatch", "tsa_token_invalid"][files.len() - 1];
            let expected = match &want {
                None => format!("failed, {alone}"),
                Some((seq, code)) => format!("failed, seq {seq}, {code}"),
            };
            let ran = run(command, &range, files);
            assert_eq!(result(&ran), expected, "{command} {name}, {alone}");
            assert_form(command, &ran, true, &name);
        }
    }
}

/// A run that stops at checks 1 to 6 never reaches the stream checks: it prints no DEC-890 line.
#[test]
fn a_check_one_to_six_is_reported_before_a_later_connection_check() {
    for (name, mut range, want) in genesis().into_iter().filter(|g| g.2.is_some()) {
        let last = range.last_seq();
        let unstored = ArtifactRef::of(b"never stored").to_string();
        range.bodies.last_mut().unwrap()["artifact_refs"] = json!([unstored]);
        let what = format!("{name}: {want:?}, an artifact missing at {last}");
        expect(&range, Some((last, "artifact_missing")), false, &what);
    }
}
