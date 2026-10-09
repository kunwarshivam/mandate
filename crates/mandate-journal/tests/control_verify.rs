//! E12-3 (journal spec v0.29 §11, §9.14 rule 113; DEC-783 item 6): the control stream's
//! `anchor_self_mismatch`, judged against the vectors' `cold_records.range_checks` (built by
//! `reference/journal/cold.py` and judged there by `anchor_self_failure` and an independent walk),
//! against hand-built chains for each clause of §11's text, and against random chains whose answer
//! this file derives from how it built them. The same for `break_glass_cause_mismatch` (§9.13 rule
//! 108, DEC-774 item 2) and the vectors' `records_access.range_checks` (`reference/journal/audit.py`,
//! judged there by `control.py`'s `break_glass_failure` and the oracle `walk_causes`).

use std::collections::BTreeSet;
use std::path::Path;

use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_journal::{
    ControlStreamCheck, ControlStreamFailure, ControlVerifyError, StoredEvent, TrustedStart,
    verify_anchor_self, verify_break_glass_causes,
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

const ACTION: &str = "PlatformOperatorAction";
const READ: &str = "RecordsAccessed";
const OPERATOR: &str = "platform_operator";

fn cause_mismatch(seq: u64) -> Answer {
    let check = ControlStreamCheck::BreakGlassCauseMismatch;
    Err(ControlVerifyError::Mismatch(ControlStreamFailure {
        seq,
        check,
    }))
}

/// One control-stream range for the break-glass check, hash-chained from its trusted start; a row
/// on `CTL` has ID `E{seq}`, one on any other stream `F{seq}`.
struct Range {
    start: TrustedStart,
    rows: Vec<StoredEvent>,
}

impl Range {
    fn from(from_seq: u64) -> Self {
        let prev_hash = match from_seq {
            1 => Digest::ZERO,
            _ => Digest::of(b"before"),
        };
        let start = TrustedStart {
            from_seq,
            prev_hash,
        };
        let rows = Vec::new();
        Self { start, rows }
    }

    /// Appends a `ty` on `stream` by an actor of `kind` citing `cause`; returns its `seq`.
    fn on(&mut self, stream: &str, ty: &str, kind: &str, cause: Option<&str>) -> u64 {
        let seq = self.start.from_seq + self.rows.len() as u64;
        let prev = self.rows.last().map_or(self.start.prev_hash, |r| r.hash);
        let id = if stream == CTL { "E" } else { "F" };
        let cause = cause.map_or("null".to_owned(), |c| format!("\"{c}\""));
        let body = format!(
            r#"{{"actor":{{"id":"x","kind":"{kind}"}},"causation_id":{cause},"event_id":"{id}{seq}",
            "event_type":"{ty}","environment":"paper","payload":{{}},"prev_hash":"{}",
            "recorded_at":"2026-10-09T12:00:00.000000000Z","seq":{seq},"stream_id":"{stream}"}}"#,
            prev.to_hex()
        );
        self.rows
            .push(stored(&parse(body.as_bytes()).unwrap(), None));
        seq
    }

    fn push(&mut self, ty: &str, kind: &str, cause: Option<&str>) -> u64 {
        self.on(CTL, ty, kind, cause)
    }

    fn causes(&self) -> Answer {
        verify_break_glass_causes(&self.rows, self.start)
    }
}

/// Every vector case: its range from the case's trusted start, and the vector's answer exactly.
#[test]
fn every_break_glass_vector_is_judged_as_its_vector_says() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    let section = fixture
        .get("records_access")
        .and_then(|s| s.get("range_checks"));
    let cases = section.and_then(Value::as_array).unwrap();
    let tails = cases
        .iter()
        .filter(|c| c.get("from_seq").and_then(Value::as_int) != Some(1));
    assert!(
        cases.len() >= 5 && tails.count() >= 1,
        "a case for each clause, a pass, and a tail"
    );
    let mut wrong = Vec::new();
    for case in cases {
        let chain = case.get("chain").and_then(Value::as_array).unwrap();
        let rows: Vec<StoredEvent> = chain
            .iter()
            .map(|e| stored(e.get("body").unwrap(), Some(text(e, "hash"))))
            .collect();
        let start = TrustedStart {
            from_seq: case.get("from_seq").and_then(Value::as_int).unwrap(),
            prev_hash: Digest::from_hex(text(case, "prev_hash")).unwrap(),
        };
        assert_eq!(
            rows[0].prev_hash, start.prev_hash,
            "the range links to its start"
        );
        let want = match case.get("expect").filter(|e| **e != Value::Null) {
            None => Ok(()),
            Some(e) => {
                assert_eq!(text(e, "check"), "break_glass_cause_mismatch");
                cause_mismatch(e.get("seq").and_then(Value::as_int).unwrap())
            }
        };
        let got = verify_break_glass_causes(&rows, start);
        if got != want {
            wrong.push(format!(
                "{}: expected {want:?}, got {got:?}",
                text(case, "name")
            ));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// §11: only a `platform_operator`'s `RecordsAccessed` is judged, whatever any other actor's read
/// or an operator's other record cites.
#[test]
fn only_an_operators_read_is_judged() {
    let mut range = Range::from(1);
    range.push("StreamOpened", "system", None);
    range.push(ACTION, OPERATOR, Some("E9"));
    for kind in ["system", "agent", "user", "broker", "client"] {
        range.push(READ, kind, Some("E1"));
        range.push(READ, kind, Some("E99"));
        range.push(READ, kind, None);
    }
    range.push("ExportCreated", OPERATOR, Some("E99"));
    range.push("StreamOpened", OPERATOR, Some("E1"));
    let action = range.push(ACTION, OPERATOR, None);
    range.push(READ, OPERATOR, Some(&format!("E{action}")));
    assert_eq!(range.causes(), Ok(()));
}

/// §11: an operator read's cause is an earlier `PlatformOperatorAction` on its own control stream;
/// a later one, the read itself, an earlier record of another type, an action stored as another
/// workspace's, and a cause naming no event each fail at the read.
#[test]
fn an_operator_read_cites_an_earlier_action_on_its_own_stream() {
    let mut good = Range::from(1);
    good.push(ACTION, OPERATOR, None);
    good.push(READ, "user", None);
    good.push(READ, OPERATOR, Some("E1"));
    good.push(READ, OPERATOR, Some("E1"));
    assert_eq!(good.causes(), Ok(()), "two reads in one window");
    let mut cases: Vec<(&str, Range, u64)> = Vec::new();
    let mut later = Range::from(1);
    let read = later.push(READ, OPERATOR, Some("E2"));
    later.push(ACTION, OPERATOR, None);
    cases.push(("a later action", later, read));
    let mut itself = Range::from(1);
    let read = itself.push(READ, OPERATOR, Some("E1"));
    cases.push(("the read itself", itself, read));
    let mut typed = Range::from(1);
    typed.push("StreamOpened", OPERATOR, None);
    let read = typed.push(READ, OPERATOR, Some("E1"));
    cases.push(("an earlier record of another type", typed, read));
    let mut operator_read = Range::from(1);
    operator_read.push(ACTION, OPERATOR, None);
    operator_read.push(READ, OPERATOR, Some("E1"));
    let read = operator_read.push(READ, OPERATOR, Some("E2"));
    cases.push(("an earlier operator read", operator_read, read));
    let mut foreign = Range::from(1);
    foreign.on(OTHER_CTL, ACTION, OPERATOR, None);
    let read = foreign.push(READ, OPERATOR, Some("F1"));
    cases.push(("an action stored as another workspace's", foreign, read));
    let mut dangling = Range::from(1);
    dangling.push(ACTION, OPERATOR, None);
    let read = dangling.push(READ, OPERATOR, Some("E99"));
    cases.push(("a cause naming no event", dangling, read));
    for (name, range, read) in cases {
        assert_eq!(range.causes(), cause_mismatch(read), "{name}");
    }
}

/// §11 "so of the same workspace": another workspace's action, journaled on its own control
/// stream, opens a window there and none here, so the full chain fails a read here that cites it.
#[test]
fn another_workspaces_action_opens_no_window_here() {
    let mut there = Range::from(1);
    there.on(OTHER_CTL, ACTION, OPERATOR, None);
    there.on(OTHER_CTL, READ, OPERATOR, Some("F1"));
    assert_eq!(there.causes(), Ok(()), "the action's own stream");
    let mut here = Range::from(1);
    here.push(ACTION, OPERATOR, None);
    let read = here.push(READ, OPERATOR, Some("F1"));
    assert_eq!(here.causes(), cause_mismatch(read));
}

/// §9.13 rule 111: the first failing read is reported, at its own `seq`, not its index.
#[test]
fn the_first_failing_read_is_reported_at_its_seq() {
    let mut range = Range::from(1);
    range.push(ACTION, OPERATOR, None);
    range.push(READ, OPERATOR, Some("E1"));
    let first = range.push(READ, OPERATOR, Some("E5"));
    range.push(ACTION, OPERATOR, None);
    range.push(READ, OPERATOR, Some("E2"));
    range.push(READ, OPERATOR, Some("E77"));
    assert_eq!(range.causes(), cause_mismatch(first));
    let mut tail = Range::from(40);
    tail.push(ACTION, OPERATOR, None);
    let read = tail.push(READ, OPERATOR, Some("E42"));
    tail.push(ACTION, OPERATOR, None);
    assert_eq!(tail.causes(), cause_mismatch(read), "seq 41, index 1");
}

/// §11: a cause before a range's trusted start is not judged by that range, but a cause the range
/// holds still is; the full chain judges a cause that names no event.
#[test]
fn a_tail_range_judges_only_the_causes_it_holds() {
    let mut tail = Range::from(5);
    tail.push(READ, OPERATOR, Some("E1"));
    tail.push(READ, OPERATOR, Some("E99"));
    tail.push(READ, OPERATOR, Some("B1"));
    assert_eq!(
        tail.causes(),
        Ok(()),
        "causes outside a tail are not judged"
    );
    let mut held = Range::from(5);
    held.push(READ, OPERATOR, Some("E4"));
    let read = held.push(READ, OPERATOR, Some("E7"));
    held.push(ACTION, OPERATOR, None);
    assert_eq!(
        held.causes(),
        cause_mismatch(read),
        "a later action the tail holds"
    );
    let mut typed = Range::from(5);
    typed.push("StreamOpened", OPERATOR, None);
    let read = typed.push(READ, OPERATOR, Some("E5"));
    assert_eq!(
        typed.causes(),
        cause_mismatch(read),
        "a held record of another type"
    );
    let mut full = Range::from(1);
    let read = full.push(READ, OPERATOR, Some("E4"));
    assert_eq!(full.causes(), cause_mismatch(read), "the full chain");
}

/// Random ranges, full and tail, of actions, some stored as another workspace's, reads by every
/// actor, and other records; the answer is this file's own walk over what it built, like `audit.py`'s
/// `walk_causes`: the IDs of this stream's actions seen so far and every ID the range holds, then,
/// for each operator read, whether its cause is among the first and, failing that, whether the
/// range could have held it.
#[test]
fn random_ranges_are_judged_as_an_independent_walk_says() {
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    let mut wrong = Vec::new();
    let mut failing = 0;
    for round in 0..600 {
        let from_seq = if rng.below(2) == 0 {
            1
        } else {
            2 + rng.below(6)
        };
        let len = 1 + rng.below(12);
        let mut range = Range::from(from_seq);
        let mut built = Vec::new();
        for _ in 0..len {
            let pick = format!("E{}", from_seq.saturating_sub(3) + rng.below(len + 6));
            let foreign = format!("F{}", from_seq + rng.below(len));
            let cause = [pick.as_str(), foreign.as_str()][usize::from(rng.below(6) == 0)];
            let kind =
                ["system", "agent", "user", "broker", "client", OPERATOR][rng.below(6) as usize];
            let (ty, kind, cause) = match rng.below(4) {
                0 => (ACTION, OPERATOR, None),
                1 => (READ, kind, Some(cause)),
                2 => (READ, OPERATOR, Some(cause)),
                _ => ("SegmentExported", kind, Some(cause)),
            };
            let stream = if ty == ACTION && rng.below(4) == 0 {
                OTHER_CTL
            } else {
                CTL
            };
            let seq = range.on(stream, ty, kind, cause);
            let id = format!("{}{seq}", if stream == CTL { "E" } else { "F" });
            built.push((seq, id, stream, ty, kind, cause.map(str::to_owned)));
        }
        let held: BTreeSet<&str> = built.iter().map(|b| b.1.as_str()).collect();
        let mut actions = BTreeSet::new();
        let mut want = Ok(());
        for (seq, id, stream, ty, kind, cause) in &built {
            let cause = cause.as_deref().unwrap_or_default();
            let judged = *ty == READ && *kind == OPERATOR;
            if judged && !actions.contains(cause) && (from_seq == 1 || held.contains(cause)) {
                want = cause_mismatch(*seq);
                break;
            }
            if *ty == ACTION && *stream == CTL {
                actions.insert(id.as_str());
            }
        }
        failing += usize::from(want.is_err());
        let got = range.causes();
        if got != want {
            wrong.push(format!("round {round}: expected {want:?}, got {got:?}"));
        }
    }
    assert!(
        (100..500).contains(&failing),
        "both answers are drawn: {failing} fail"
    );
    assert!(
        wrong.is_empty(),
        "{} wrong, first: {:#?}",
        wrong.len(),
        wrong.first()
    );
}

/// The code §11 and §9.13 rule 111 record (live; covers the only non-stub item).
#[test]
fn the_break_glass_check_carries_the_specs_code() {
    let code = ControlStreamCheck::BreakGlassCauseMismatch.code();
    assert_eq!(code, "break_glass_cause_mismatch");
}
