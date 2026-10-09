//! E12-3 (journal spec v0.29 §11, §9.14 rule 113; DEC-783 item 6): the control stream's
//! `anchor_self_mismatch`, judged against the vectors' `cold_records.range_checks` (built by
//! `reference/journal/cold.py` and judged there by `anchor_self_failure` and an independent walk),
//! against hand-built chains for each clause of §11's text, and against random chains whose answer
//! this file derives from how it built them.

use std::path::Path;

use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_journal::{
    ControlStreamCheck, ControlStreamFailure, ControlVerifyError, StoredEvent, verify_anchor_self,
};

const CTL: &str = "ctl:ws_1";
const ACCT: &str = "acct:ws_1:A1";
const OTHER_CTL: &str = "ctl:ws_2";

type Answer = Result<(), ControlVerifyError>;

/// An anchor leaf: `stream_id`, `seq`, `hash`.
type Leaf = (&'static str, u64, Digest);

/// How a test derives an anchor's own leaf from the chain before it.
type LeafOf = fn(&Chain) -> Option<Leaf>;

fn mismatch(seq: u64) -> Answer {
    let check = ControlStreamCheck::AnchorSelfMismatch;
    Err(ControlVerifyError::Mismatch(ControlStreamFailure {
        seq,
        check,
    }))
}

fn text<'a>(value: &'a Value, name: &str) -> &'a str {
    value.get(name).and_then(Value::as_str).unwrap_or_default()
}

/// A stored row of `body`, whose `hash` must be the vector's (or, with `None`, is computed).
fn stored(body: &Value, hash: Option<&str>) -> StoredEvent {
    let bytes = to_canonical(body);
    let hash = hash.map_or_else(|| Digest::of(&bytes), |h| Digest::from_hex(h).unwrap());
    assert_eq!(Digest::of(&bytes), hash, "the body re-hashes to its hash");
    StoredEvent {
        stream_id: text(body, "stream_id").to_owned(),
        seq: body.get("seq").and_then(Value::as_int).unwrap(),
        event_id: text(body, "event_id").to_owned(),
        event_type: text(body, "event_type").to_owned(),
        schema_version: 1,
        environment: text(body, "environment").to_owned(),
        recorded_at: text(body, "recorded_at").to_owned(),
        prev_hash: Digest::from_hex(text(body, "prev_hash")).unwrap(),
        hash,
        body: bytes,
    }
}

/// One control-stream range under construction, hash-chained from its trusted start.
struct Chain {
    from_seq: u64,
    rows: Vec<StoredEvent>,
}

impl Chain {
    fn from(from_seq: u64) -> Self {
        let rows = Vec::new();
        Self { from_seq, rows }
    }

    /// The leaf an honest anchor appended next would carry for its own stream.
    fn head(&self) -> Leaf {
        let seq = self.from_seq + self.rows.len() as u64 - 1;
        let start = if self.from_seq == 1 {
            Digest::ZERO
        } else {
            Digest::of(b"before")
        };
        (CTL, seq, self.rows.last().map_or(start, |r| r.hash))
    }

    /// Appends an event of type `ty` on `stream`; returns its `seq`.
    fn push(&mut self, stream: &str, ty: &str, payload: &str) -> u64 {
        let (_, prev_seq, prev) = self.head();
        let seq = prev_seq + 1;
        let body = format!(
            r#"{{"actor":{{"id":"x","kind":"system"}},"causation_id":null,"event_id":"E{seq}",
            "event_type":"{ty}","environment":"paper","payload":{payload},"prev_hash":"{}",
            "recorded_at":"2026-10-09T12:00:00.000000000Z","seq":{seq},"stream_id":"{stream}"}}"#,
            prev.to_hex()
        );
        self.rows
            .push(stored(&parse(body.as_bytes()).unwrap(), None));
        seq
    }

    fn event(&mut self, ty: &str) -> u64 {
        self.push(CTL, ty, "{}")
    }

    /// An anchor whose leaves are `own` beside an account stream's leaf.
    fn anchor(&mut self, own: Option<Leaf>) -> u64 {
        let leaf =
            |(s, n, h): Leaf| format!(r#"{{"hash":"{}","seq":{n},"stream_id":"{s}"}}"#, h.to_hex());
        let mut leaves = vec![leaf((ACCT, 7, Digest::of(b"acct")))];
        leaves.extend(own.map(leaf));
        let (leaves, root) = (leaves.join(","), "0".repeat(64));
        let payload = format!(r#"{{"leaves":[{leaves}],"root":"{root}","token":null}}"#);
        self.push(CTL, "AnchorComputed", &payload)
    }
}

/// Every vector case: its chain from seq 1, and the vector's answer exactly.
#[test]
#[ignore = "pending E12-3"]
fn every_anchor_self_vector_is_judged_as_its_vector_says() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    let section = fixture
        .get("cold_records")
        .and_then(|s| s.get("range_checks"));
    let cases = section.and_then(Value::as_array).unwrap();
    let failing = cases
        .iter()
        .filter(|c| c.get("expect") != Some(&Value::Null));
    assert!(
        cases.len() >= 4 && failing.count() >= 3,
        "a pass and a case for each clause"
    );
    let mut wrong = Vec::new();
    for case in cases {
        let chain = case.get("chain").and_then(Value::as_array).unwrap();
        let rows: Vec<StoredEvent> = chain
            .iter()
            .map(|e| stored(e.get("body").unwrap(), Some(text(e, "hash"))))
            .collect();
        let want = match case.get("expect").filter(|e| **e != Value::Null) {
            None => Ok(()),
            Some(e) => {
                assert_eq!(text(e, "check"), "anchor_self_mismatch");
                mismatch(e.get("seq").and_then(Value::as_int).unwrap())
            }
        };
        let got = verify_anchor_self(&rows);
        if got != want {
            wrong.push(format!(
                "{}: expected {want:?}, got {got:?}",
                text(case, "name")
            ));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// §11 and rule 113: every anchor's own leaf names the row before it, by `seq` (the row's, not
/// its index in the range) and by `hash`; only the leaf of the anchor's own stream counts; the
/// first failing anchor is reported at its own `seq`; and an anchor that is a range's first row
/// names an event before the trusted start, which the range does not check.
#[test]
#[ignore = "pending E12-3"]
fn an_anchor_names_the_event_just_before_it() {
    let mut opening = Chain::from(9);
    opening.anchor(Some((CTL, 1, Digest::of(b"wrong"))));
    let head = opening.head();
    opening.anchor(Some(head));
    let got = verify_anchor_self(&opening.rows);
    assert_eq!(got, Ok(()), "a tail's first anchor is not judged");
    let variants: [(&str, LeafOf); 5] = [
        ("its own seq", |c| Some((CTL, c.head().1 + 1, c.head().2))),
        ("the seq two before", |c| {
            Some((CTL, c.head().1 - 1, c.head().2))
        }),
        ("an earlier row's hash", |c| {
            Some((CTL, c.head().1, c.rows[0].hash))
        }),
        ("another stream's leaf", |c| {
            Some((OTHER_CTL, c.head().1, c.head().2))
        }),
        ("no own leaf", |_| None),
    ];
    for (name, leaf) in variants {
        let mut chain = Chain::from(9);
        chain.event("StreamOpened");
        let head = chain.head();
        chain.anchor(Some(head));
        chain.event("SegmentExported");
        let seq = chain.anchor(leaf(&chain));
        let head = chain.head();
        chain.anchor(Some(head));
        chain.anchor(None);
        assert_eq!(verify_anchor_self(&chain.rows), mismatch(seq), "{name}");
    }
}

/// A small deterministic generator (xorshift64), so the random chains need no dependency.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
}

/// Random ranges of anchors and other records, each anchor's own leaf honest or wrong in one way;
/// the expected answer is the first wrong anchor after the range's first row, as built.
#[test]
#[ignore = "pending E12-3"]
fn random_anchor_chains_fail_at_their_first_wrong_anchor() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut wrong = Vec::new();
    for round in 0..400 {
        let mut chain = Chain::from(1 + rng.below(2) * rng.below(9));
        let mut want = Ok(());
        for i in 0..1 + rng.below(10) {
            if rng.below(2) == 0 {
                chain.event("SegmentExported");
                continue;
            }
            let (s, n, h) = chain.head();
            let own = match rng.below(5) {
                0 => None,
                1 => Some((s, n + 1, h)),
                2 => Some((OTHER_CTL, n, h)),
                3 => Some((s, n, Digest::of(b"other"))),
                _ => Some((s, n, h)),
            };
            let seq = chain.anchor(own);
            if own != Some((s, n, h)) && i > 0 && want.is_ok() {
                want = mismatch(seq);
            }
        }
        let got = verify_anchor_self(&chain.rows);
        if got != want {
            wrong.push(format!("round {round}: expected {want:?}, got {got:?}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} wrong, first: {:#?}",
        wrong.len(),
        wrong.first()
    );
}

/// The code §11 and §9.13 rule 111 record (live; covers the only non-stub item).
#[test]
fn the_control_stream_check_carries_the_specs_code() {
    let code = ControlStreamCheck::AnchorSelfMismatch.code();
    assert_eq!(code, "anchor_self_mismatch");
}
