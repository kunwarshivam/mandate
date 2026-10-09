//! E12-3 (journal spec v0.29 §9.14 "What a verifier reads", §11; DEC-783 item 8, DEC-767): the
//! trusted start of a range, judged against `cold_records.trusted_starts` (built by `cold.py`,
//! judged by `control.py`'s `trusted_start` and the oracle `resolve_start`), hand-built records for
//! each clause of §9.14, and random records whose answer this file derives from how it built them.

use std::path::Path;

use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_journal::{
    StartRequest, StoredEvent, TrustedStart, TrustedStartError, resolve_trusted_start,
};

const CTL: &str = "ctl:ws_1";
const FOREIGN_CTL: &str = "ctl:ws_2";
const ACCT: &str = "acct:ws_1:A1";
const AGENT: &str = "agent:ws_1:G1";
const FOREIGN_ACCT: &str = "acct:ws_2:A1";
const FOREIGN_AGENT: &str = "agent:ws_2:G1";
const TOKEN: &str = r#""sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa""#;
const IDS: [&str; 6] = ["E0", "E1", "E2", "E3", "E4", "E5"];
const REFUSED: Answer = Err(TrustedStartError::Refused);

type Answer = Result<TrustedStart, TrustedStartError>;

fn start(from_seq: u64, prev_hash: Digest) -> Answer {
    Ok(TrustedStart {
        from_seq,
        prev_hash,
    })
}

fn text<'a>(value: &'a Value, name: &str) -> &'a str {
    value.get(name).and_then(Value::as_str).unwrap_or_default()
}

fn digest(seed: &str) -> Digest {
    Digest::of(seed.as_bytes())
}

fn by(manifest_hash: Digest) -> StartRequest<'static> {
    StartRequest::Manifest { manifest_hash }
}

fn at(anchor_event_id: &str) -> StartRequest<'_> {
    StartRequest::Anchor { anchor_event_id }
}

/// A stored row of `stream` whose body holds `payload` (JSON text).
fn row(stream: &str, event_id: &str, event_type: &str, payload: &str) -> StoredEvent {
    let body = format!(
        r#"{{"event_id":"{event_id}","event_type":"{event_type}","payload":{payload},
        "stream_id":"{stream}"}}"#
    );
    stored(&parse(body.as_bytes()).unwrap())
}

/// The stored row of a record `{event_id, event_type, payload, stream_id}`: the resolver reads its
/// columns and its body's `payload`.
fn stored(record: &Value) -> StoredEvent {
    let bytes = to_canonical(record);
    StoredEvent {
        stream_id: text(record, "stream_id").to_owned(),
        seq: 1,
        event_id: text(record, "event_id").to_owned(),
        event_type: text(record, "event_type").to_owned(),
        schema_version: 1,
        environment: "paper".to_owned(),
        recorded_at: "2026-10-09T12:00:00.000000000Z".to_owned(),
        prev_hash: Digest::ZERO,
        hash: Digest::of(&bytes),
        body: bytes,
    }
}

/// A `SegmentExported` payload of `stream` over `first..=last` and its rule 117 manifest hash. Its
/// `last_hash` differs from `prev`, so a start read from the wrong member shows.
fn segment(stream: &str, first: u64, last: u64, prev: Digest) -> (String, Digest) {
    let (tail, file) = (digest("last").to_hex(), digest("file").to_hex());
    let prev = prev.to_hex();
    let six = format!(
        r#"{{"file_sha256":"{file}","first_prev_hash":"{prev}","first_seq":{first},
        "last_hash":"{tail}","last_seq":{last},"stream":"{stream}"}}"#
    );
    let manifest = Digest::of(&to_canonical(&parse(six.as_bytes()).unwrap()));
    let payload = six.replace(
        r#""stream":"#,
        &format!(r#""manifest_hash":"{}","stream_id":"#, manifest.to_hex()),
    );
    (payload, manifest)
}

/// An `AnchorComputed` payload over `leaves`, with a token when `stamped`.
fn anchor(leaves: &[(&str, u64, Digest)], stamped: bool) -> String {
    let leaves: Vec<String> = leaves
        .iter()
        .map(|(s, n, h)| format!(r#"{{"hash":"{}","seq":{n},"stream_id":"{s}"}}"#, h.to_hex()))
        .collect();
    let token = if stamped { TOKEN } else { "null" };
    let (leaves, root) = (leaves.join(","), "0".repeat(64));
    format!(r#"{{"leaves":[{leaves}],"root":"{root}","token":{token}}}"#)
}

/// Every vector case: its records, request and answer exactly; nothing typed here.
#[test]
#[ignore = "pending E12-3"]
fn every_trusted_start_vector_resolves_as_its_vector_says() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    let cold = fixture.get("cold_records").unwrap();
    let section = cold.get("trusted_starts").unwrap();
    let records = section.get("records").and_then(Value::as_array).unwrap();
    let records: Vec<StoredEvent> = records.iter().map(stored).collect();
    let cases = section.get("cases").and_then(Value::as_array).unwrap();
    let refused = Some(&Value::Null);
    let starts = cases.iter().filter(|c| c.get("expect") != refused);
    assert!(cases.len() >= 11 && starts.count() >= 3, "every clause");
    let mut wrong = Vec::new();
    for case in cases {
        let req = case.get("request").unwrap();
        let request = match text(req, "kind") {
            "genesis" => StartRequest::Genesis,
            "manifest" => by(Digest::from_hex(text(req, "manifest_hash")).unwrap()),
            "anchor" => at(text(req, "anchor_event_id")),
            other => panic!("a request kind the vectors do not define: {other}"),
        };
        let want = match case.get("expect").filter(|e| **e != Value::Null) {
            None => REFUSED,
            Some(e) => start(
                e.get("from_seq").and_then(Value::as_int).unwrap(),
                Digest::from_hex(text(e, "prev_hash")).unwrap(),
            ),
        };
        let from_seq = case.get("from_seq").and_then(Value::as_int).unwrap();
        let got = resolve_trusted_start(&records, text(case, "stream_id"), from_seq, request);
        if got != want {
            let name = text(case, "name");
            wrong.push(format!("{name}: expected {want:?}, got {got:?}"));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// §11 and workspace API §4.8.1: genesis is seq 1 with 64 zeros, and no other seq, records or not.
#[test]
#[ignore = "pending E12-3"]
fn genesis_is_seq_one_with_zeros_only() {
    let (payload, _) = segment(ACCT, 1, 3, Digest::ZERO);
    for records in [vec![], vec![row(CTL, "E1", "SegmentExported", &payload)]] {
        let resolve = |n| resolve_trusted_start(&records, ACCT, n, StartRequest::Genesis);
        assert_eq!(resolve(1), start(1, Digest::ZERO));
        assert_eq!((resolve(0), resolve(2)), (REFUSED, REFUSED));
    }
}

/// §9.14: a `SegmentExported` of stream `s` whose `first_seq` is `n` starts `s` at `n` with its
/// `first_prev_hash`, never at another seq, for another stream, or as an anchor; a manifest hash
/// no record holds is refused.
#[test]
#[ignore = "pending E12-3"]
fn a_segment_starts_its_own_stream_at_its_first_seq() {
    let (acct, acct_hash) = segment(ACCT, 4, 9, digest("p4"));
    let (agent, agent_hash) = segment(AGENT, 1, 3, Digest::ZERO);
    let records = [
        row(CTL, "E1", "SegmentExported", &acct),
        row(CTL, "E2", "SegmentExported", &agent),
    ];
    let resolve = |s, n, request| resolve_trusted_start(&records, s, n, request);
    assert_eq!(resolve(ACCT, 4, by(acct_hash)), start(4, digest("p4")));
    assert_eq!(resolve(AGENT, 1, by(agent_hash)), start(1, Digest::ZERO));
    for (name, s, n, request) in [
        ("the seq before", ACCT, 3, by(acct_hash)),
        ("inside the segment", ACCT, 5, by(acct_hash)),
        ("after its last_seq", ACCT, 10, by(acct_hash)),
        ("another stream", AGENT, 4, by(acct_hash)),
        ("another segment's stream", ACCT, 1, by(agent_hash)),
        ("no such manifest", ACCT, 4, by(digest("absent"))),
        ("a segment named as an anchor", ACCT, 4, at("E1")),
    ] {
        assert_eq!(resolve(s, n, request), REFUSED, "{name}");
    }
}

/// §9.14, DEC-783 item 8: an `AnchorComputed` with a `token` starts `s` at `n` from its leaf for `s`
/// whose `seq` is `n − 1`; an anchor whose `token` is `null` is never a start, though its leaves fit.
#[test]
#[ignore = "pending E12-3"]
fn only_a_stamped_anchor_starts_the_seq_after_its_leaf() {
    let (a9, g4, c3) = (digest("a9"), digest("g4"), digest("c3"));
    let leaves = [(ACCT, 9, a9), (AGENT, 4, g4), (CTL, 3, c3)];
    let records = [
        row(CTL, "S1", "AnchorComputed", &anchor(&leaves, true)),
        row(CTL, "U1", "AnchorComputed", &anchor(&leaves, false)),
    ];
    let resolve = |id, s, n| resolve_trusted_start(&records, s, n, at(id));
    assert_eq!(resolve("S1", ACCT, 10), start(10, a9));
    assert_eq!(resolve("S1", AGENT, 5), start(5, g4));
    assert_eq!(resolve("S1", CTL, 4), start(4, c3));
    for (name, id, s, n) in [
        ("at the leaf's own seq", "S1", ACCT, 9),
        ("two after the leaf", "S1", ACCT, 11),
        ("seq 0", "S1", ACCT, 0),
        ("another stream's leaf at n - 1", "S1", AGENT, 10),
        ("a stream with no leaf", "S1", "acct:ws_1:B2", 10),
        ("unstamped", "U1", ACCT, 10),
        ("unstamped, another leaf", "U1", AGENT, 5),
        ("no such anchor", "X1", ACCT, 10),
    ] {
        assert_eq!(resolve(id, s, n), REFUSED, "{name}");
    }
}

/// §9.14, DEC-767: a start is looked up among its stream's own workspace's records only, so another
/// workspace's record is refused as an absent one, and that workspace's streams use its records.
#[test]
#[ignore = "pending E12-3"]
fn another_workspaces_record_is_refused_as_an_absent_one() {
    let (seg, seg_hash) = segment(ACCT, 4, 9, digest("p4"));
    let (own, own_hash) = segment(FOREIGN_ACCT, 4, 9, digest("q4"));
    let leaves = [(ACCT, 9, digest("a9")), (FOREIGN_ACCT, 2, digest("f2"))];
    let records = [
        row(FOREIGN_CTL, "F1", "SegmentExported", &seg),
        row(FOREIGN_CTL, "F2", "AnchorComputed", &anchor(&leaves, true)),
        row(FOREIGN_CTL, "F3", "SegmentExported", &own),
    ];
    let resolve = |s, n, request| resolve_trusted_start(&records, s, n, request);
    let absent = resolve(ACCT, 4, by(digest("absent")));
    assert_eq!(resolve(ACCT, 4, by(seg_hash)), absent, "a foreign segment");
    assert_eq!(resolve(ACCT, 10, at("F2")), REFUSED, "a foreign anchor");
    let own_start = start(4, digest("q4"));
    assert_eq!(resolve(FOREIGN_ACCT, 4, by(own_hash)), own_start);
    assert_eq!(resolve(FOREIGN_ACCT, 3, at("F2")), start(3, digest("f2")));
}

/// No start from an unreadable record (AGENTS.md rule 3) or for a stream with no workspace segment,
/// not even genesis; a record answers only as its own `event_type`.
#[test]
#[ignore = "pending E12-3"]
fn an_unreadable_record_a_malformed_stream_or_another_type_is_refused() {
    let (seg, seg_hash) = segment(ACCT, 4, 9, digest("p4"));
    let (bad, bad_hash) = segment("acct::A1", 4, 9, digest("p4"));
    let (as_anchor, other) = segment(ACCT, 5, 9, digest("p5"));
    let leaves = [("acct::A1", 9, digest("a9")), (ACCT, 9, digest("a9"))];
    let stamped = anchor(&leaves, true);
    let bad_prev = seg.replace(&digest("p4").to_hex(), "x");
    let bad_leaf = stamped.replace(&digest("a9").to_hex(), "x");
    let mut unparsed = row(CTL, "B3", "AnchorComputed", &stamped);
    let mut unparsed_segment = row(CTL, "B4", "SegmentExported", &as_anchor);
    (unparsed.body, unparsed_segment.body) = (b"{".to_vec(), b"{".to_vec());
    let records = [
        row(CTL, "B1", "SegmentExported", &bad_prev),
        row(CTL, "B2", "AnchorComputed", &bad_leaf),
        unparsed,
        unparsed_segment,
        row("ctl:", "N1", "SegmentExported", &bad),
        row("ctl:", "N2", "AnchorComputed", &stamped),
        row(CTL, "T1", "VerificationRun", &stamped),
        row(CTL, "T2", "AnchorComputed", &as_anchor),
    ];
    let resolve = |s, n, request| resolve_trusted_start(&records, s, n, request);
    for (name, s, n, request) in [
        ("an unreadable first_prev_hash", ACCT, 4, by(seg_hash)),
        ("an unreadable leaf hash", ACCT, 10, at("B2")),
        ("a body that does not parse", ACCT, 10, at("B3")),
        ("no workspace segment", "acct", 1, StartRequest::Genesis),
        ("an empty workspace", "acct::A1", 1, StartRequest::Genesis),
        ("an empty workspace's segment", "acct::A1", 4, by(bad_hash)),
        ("an empty workspace's anchor", "acct::A1", 10, at("N2")),
        ("an anchor's payload under another type", ACCT, 10, at("T1")),
        ("a segment unparsed, or as an anchor", ACCT, 5, by(other)),
    ] {
        assert_eq!(resolve(s, n, request), REFUSED, "{name}");
    }
}

/// A small deterministic generator (xorshift64), so the random records need no dependency.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
}

/// Random segments and anchors on two workspaces, some stamped, some naming a foreign stream; each
/// request's answer comes only from the starts recorded while building usable records.
#[test]
#[ignore = "pending E12-3"]
fn random_records_resolve_as_built() {
    let workspaces = [
        (CTL, [ACCT, AGENT, CTL]),
        (FOREIGN_CTL, [FOREIGN_ACCT, FOREIGN_AGENT, FOREIGN_CTL]),
    ];
    let all = [ACCT, AGENT, CTL, FOREIGN_ACCT, FOREIGN_AGENT, FOREIGN_CTL];
    let (mut rng, mut wrong) = (Rng(0x2545_F491_4F6C_DD1D), Vec::new());
    for round in 0..200 {
        let (mut records, mut keys, mut serves) = (Vec::new(), Vec::new(), Vec::new());
        for i in 0..1 + rng.below(6) {
            let (ctl, own) = workspaces[rng.below(2) as usize];
            let id = IDS[i as usize];
            if rng.below(2) == 0 {
                let pool: &[&str] = if rng.below(4) == 0 { &all } else { &own[..2] };
                let s = pool[rng.below(pool.len() as u64) as usize];
                let first = 1 + rng.below(12);
                let prev = if first == 1 { Digest::ZERO } else { digest(id) };
                let (payload, hash) = segment(s, first, first + rng.below(5), prev);
                records.push(row(ctl, id, "SegmentExported", &payload));
                keys.push(by(hash));
                if s.split(':').nth(1) == ctl.split(':').nth(1) {
                    serves.push((by(hash), s, first, prev));
                }
                continue;
            }
            let stamped = rng.below(2) == 0;
            let mut leaves = Vec::new();
            for s in [own[0], own[1], own[2]] {
                if s == ctl || rng.below(2) == 0 {
                    leaves.push((s, 1 + rng.below(12), digest(&format!("{round}{id}{s}"))));
                }
            }
            records.push(row(ctl, id, "AnchorComputed", &anchor(&leaves, stamped)));
            keys.push(at(id));
            for &(s, n, h) in leaves.iter().filter(|_| stamped) {
                serves.push((at(id), s, n + 1, h));
            }
        }
        keys.push(at("absent"));
        for _ in 0..20 {
            let key = keys[rng.below(keys.len() as u64) as usize];
            let fitting: Vec<_> = serves.iter().filter(|f| f.0 == key).collect();
            let (s, n) = match fitting.get(rng.below(2 * fitting.len() as u64 + 1) as usize) {
                Some(f) => (f.1, f.2),
                None => (all[rng.below(6) as usize], rng.below(14)),
            };
            let genesis = rng.below(6) == 0;
            let hit = serves.iter().find(|f| f.0 == key && f.1 == s && f.2 == n);
            let want = match (genesis, hit) {
                (true, _) if n == 1 => start(1, Digest::ZERO),
                (true, _) | (false, None) => REFUSED,
                (false, Some(f)) => start(n, f.3),
            };
            let request = if genesis { StartRequest::Genesis } else { key };
            let got = resolve_trusted_start(&records, s, n, request);
            if got != want {
                wrong.push(format!("{round}: {s}@{n}: want {want:?}, got {got:?}"));
            }
        }
    }
    let first = wrong.first();
    assert!(wrong.is_empty(), "{} wrong, first: {first:#?}", wrong.len());
}
