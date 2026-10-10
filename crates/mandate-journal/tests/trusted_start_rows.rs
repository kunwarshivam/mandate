//! E12-3 (DEC-893, DEC-894, DEC-787 items 3 and 5; journal spec §9.14, §11): the trusted start
//! resolved over stored rows. Its start record is checked before it is used, more than one match
//! refuses, and a manifest start becomes a start only once the cold manifest's bytes confirm it.
//! Judged against `cold_records.trusted_starts.row_cases` (answered by `control.py`'s
//! `start_from_rows`), and against random variants of the vector rows whose answer this file
//! derives from how it built them.

use std::path::Path;

use mandate_canon::{Digest, ParseErrorKind, Value, parse};
use mandate_journal::{
    ColdRead, ManifestStart, ResolvedStart, StartRequest, StoredEvent, TrustedStart,
    TrustedStartError, resolve_start_from_rows,
};

const REFUSED: TrustedStartError = TrustedStartError::Refused;
const COLD_UNREADABLE: TrustedStartError = TrustedStartError::ColdUnreadable;

fn text<'a>(value: &'a Value, name: &str) -> &'a str {
    value.get(name).and_then(Value::as_str).unwrap_or_default()
}

fn int(value: &Value, name: &str) -> u64 {
    value.get(name).and_then(Value::as_int).unwrap()
}

fn hex(value: &Value, name: &str) -> Digest {
    Digest::from_hex(text(value, name)).unwrap()
}

fn section() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    let cold = fixture.get("cold_records").unwrap();
    cold.get("trusted_starts").unwrap().clone()
}

fn list<'a>(value: &'a Value, name: &str) -> &'a [Value] {
    value.get(name).and_then(Value::as_array).unwrap()
}

/// A vector row as stored: its columns, its hash, and its body's exact bytes.
fn stored(row: &Value) -> StoredEvent {
    StoredEvent {
        stream_id: text(row, "stream_id").to_owned(),
        seq: int(row, "seq"),
        event_id: text(row, "event_id").to_owned(),
        event_type: text(row, "event_type").to_owned(),
        schema_version: int(row, "schema_version"),
        environment: text(row, "environment").to_owned(),
        recorded_at: text(row, "recorded_at").to_owned(),
        prev_hash: hex(row, "prev_hash"),
        hash: hex(row, "hash"),
        body: text(row, "body").as_bytes().to_vec(),
    }
}

/// The vectors' own workspace's control and account streams, read from their rows rather than named
/// here, so the test follows whichever workspace the vectors are keyed to: the control stream the
/// `StreamOpened` row opens, and the account stream that control stream's first `SegmentExported`
/// exports.
fn own_streams(rows: &[StoredEvent]) -> (String, String) {
    let opened = rows.iter().find(|r| r.event_type == "StreamOpened");
    let control = opened.unwrap().stream_id.clone();
    let exported = rows
        .iter()
        .find(|r| r.stream_id == control && r.event_type == "SegmentExported");
    let body = parse(&exported.unwrap().body).unwrap();
    let account = text(body.get("payload").unwrap(), "stream_id").to_owned();
    let workspace = control.strip_prefix("ctl:").unwrap_or_default();
    assert!(
        !workspace.is_empty() && account.starts_with(&format!("acct:{workspace}:")),
        "the vectors' own control stream {control} exports its own account stream, not {account}"
    );
    (control, account)
}

fn request(req: &Value) -> StartRequest<'_> {
    match text(req, "kind") {
        "genesis" => StartRequest::Genesis,
        "manifest" => StartRequest::Manifest {
            manifest_hash: hex(req, "manifest_hash"),
        },
        "anchor" => StartRequest::Anchor {
            anchor_event_id: text(req, "anchor_event_id"),
        },
        other => panic!("a request kind the vectors do not define: {other}"),
    }
}

fn cold(state: &Value) -> ColdRead {
    match text(state, "state") {
        "read" => ColdRead::Read(text(state, "bytes").as_bytes().to_vec()),
        "absent" => ColdRead::Absent,
        "unreadable" => ColdRead::Unreadable,
        other => panic!("a cold state the vectors do not define: {other}"),
    }
}

fn start_at(expect: &Value) -> TrustedStart {
    let (from_seq, prev_hash) = (int(expect, "from_seq"), hex(expect, "prev_hash"));
    TrustedStart {
        from_seq,
        prev_hash,
    }
}

/// The manifest start `resolved` must be, so a refusal it then meets comes from `confirm`.
fn manifest(resolved: Result<ResolvedStart, TrustedStartError>, name: &str) -> ManifestStart {
    match resolved {
        Ok(ResolvedStart::Manifest(start)) => start,
        other => panic!("{name}: a manifest start to confirm, got {other:?}"),
    }
}

/// Every `row_cases` case, its rows built from the vector rows with its `replace` applied: a start
/// is `Ready` for genesis or an anchor, whatever the cold store says, and a manifest start's answer
/// is `confirm`'s with the case's cold state; a refusal for the cold manifest, and a cold store
/// that cannot answer, come from `confirm`, every other refusal from the resolver itself.
#[test]
fn every_row_case_resolves_and_confirms_as_its_vector_says() {
    let section = section();
    let base: Vec<StoredEvent> = list(&section, "rows").iter().map(stored).collect();
    let cases = list(&section, "row_cases");
    assert!(cases.len() >= 20, "every clause of DEC-893 and DEC-894");
    for case in cases {
        let name = text(case, "name");
        let mut rows = base.clone();
        if let Some(replace) = case.get("replace").filter(|r| **r != Value::Null) {
            let index = usize::try_from(int(replace, "index")).unwrap();
            rows[index] = stored(replace.get("row").unwrap());
        }
        let req = case.get("request").unwrap();
        let (stream, from_seq) = (text(case, "stream_id"), int(case, "from_seq"));
        let resolved = resolve_start_from_rows(&rows, stream, from_seq, request(req));
        let (expect, state) = (case.get("expect").unwrap(), cold(case.get("cold").unwrap()));
        let outcome = text(expect, "outcome");
        let from_cold = outcome == "cold_unreadable" || text(expect, "cause").starts_with("cold_");
        let confirms = text(req, "kind") == "manifest" && (from_cold || outcome == "start");
        let got = match resolved {
            Ok(ResolvedStart::Manifest(m)) if confirms => m.confirm(state),
            Ok(ResolvedStart::Ready(start)) if !confirms => Ok(start),
            Err(e) if !from_cold => Err(e),
            other => panic!("{name}: the wrong stage, a manifest start {confirms}: {other:?}"),
        };
        let want = match outcome {
            "start" => Ok(start_at(expect)),
            "refused" => Err(REFUSED),
            "cold_unreadable" => Err(COLD_UNREADABLE),
            other => panic!("{name}: an outcome the vectors do not define: {other}"),
        };
        assert_eq!(got, want, "{name}");
    }
}

/// `confirm` is the only way from a manifest start to a start: the cold bytes with one byte more
/// are refused, an absent or unreadable object is `ColdUnreadable`, and only the exact bytes give
/// the start, each from a fresh resolution of the same rows.
#[test]
fn only_the_exact_cold_bytes_confirm_a_manifest_start() {
    let section = section();
    let rows: Vec<StoredEvent> = list(&section, "rows").iter().map(stored).collect();
    let cases = list(&section, "row_cases");
    let case = cases
        .iter()
        .find(|c| text(c, "name") == "manifest_start_cold_confirmed");
    let case = case.unwrap();
    let (stream, from_seq) = (text(case, "stream_id"), int(case, "from_seq"));
    let req = case.get("request").unwrap();
    let resolve = || {
        manifest(
            resolve_start_from_rows(&rows, stream, from_seq, request(req)),
            "",
        )
    };
    let bytes = text(case.get("cold").unwrap(), "bytes").as_bytes().to_vec();
    let longer = [bytes.clone(), b" ".to_vec()].concat();
    let shorter = bytes[..bytes.len() - 1].to_vec();
    for (name, state, want) in [
        ("one byte more", ColdRead::Read(longer), REFUSED),
        ("one byte less", ColdRead::Read(shorter), REFUSED),
        ("no bytes", ColdRead::Read(Vec::new()), REFUSED),
        ("absent", ColdRead::Absent, COLD_UNREADABLE),
        ("unreadable", ColdRead::Unreadable, COLD_UNREADABLE),
    ] {
        assert_eq!(resolve().confirm(state), Err(want), "{name}");
    }
    let want = Ok(start_at(case.get("expect").unwrap()));
    assert_eq!(resolve().confirm(ColdRead::Read(bytes)), want);
}

/// DEC-895 item 1, the row built here rather than read from `row_cases`: a copy of the start
/// segment's `SegmentExported` whose payload writes `first_prev_hash` twice, the forged value first,
/// stored re-hashed beside the good row. `parse` reads no payload from it, so which segment it
/// records cannot be known, and the manifest start is refused rather than taken from the good row.
#[test]
fn a_duplicate_key_copy_beside_the_start_segment_refuses_it() {
    let section = section();
    let mut rows: Vec<StoredEvent> = list(&section, "rows").iter().map(stored).collect();
    let cases = list(&section, "row_cases");
    let case = cases
        .iter()
        .find(|c| text(c, "name") == "manifest_start_cold_confirmed")
        .unwrap();
    let (stream, from_seq) = (text(case, "stream_id"), int(case, "from_seq"));
    let req = case.get("request").unwrap();
    let named = text(req, "manifest_hash");
    let good = rows
        .iter()
        .find(|r| {
            r.event_type == "SegmentExported" && String::from_utf8_lossy(&r.body).contains(named)
        })
        .unwrap()
        .clone();
    let head = b"\"payload\":{";
    let at = good
        .body
        .windows(head.len())
        .position(|w| w == head)
        .unwrap()
        + head.len();
    let forged = format!("\"first_prev_hash\":\"{}\",", "e".repeat(64));
    let body = [&good.body[..at], forged.as_bytes(), &good.body[at..]].concat();
    let kind = parse(&body).map_err(|e| e.kind);
    assert_eq!(
        kind,
        Err(ParseErrorKind::DuplicateKey),
        "the copy is built as intended"
    );
    let resolve =
        |rows: &[StoredEvent]| resolve_start_from_rows(rows, stream, from_seq, request(req));
    match resolve(&rows) {
        Ok(ResolvedStart::Manifest(_)) => {}
        other => panic!("the good row alone starts, got {other:?}"),
    }
    let hash = Digest::of(&body);
    rows.push(StoredEvent { hash, body, ..good });
    assert_eq!(
        resolve(&rows),
        Err(REFUSED),
        "a candidate that does not parse is never skipped"
    );
}

/// A small deterministic generator (xorshift64), so the random variants need no dependency.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        usize::try_from(self.0 % u64::try_from(n).unwrap()).unwrap()
    }
}

/// What the oracle knows of a row: the vector start case it serves, if any, whether it is still on
/// its control stream (any, and the workspace's own), its type, whether an edit has broken one of
/// its checks, and whether its body still parses.
#[derive(Clone)]
struct Known {
    serves: Option<usize>,
    on_ctl: bool,
    own_ctl: bool,
    event_type: String,
    broken: bool,
    unreadable: bool,
}

/// A position among the 64 hex digits after `key` in `body`: the actor's `build` digest (after its
/// `sha256:`), neither a column nor the payload, so an edit there leaves the row found and touches
/// only check 4, or a segment's `first_prev_hash`, which rule 117 covers.
fn digits(body: &[u8], key: &str, rng: &mut Rng) -> usize {
    let key = format!("\"{key}\":\"");
    let at = body.windows(key.len()).position(|w| w == key.as_bytes());
    at.unwrap() + key.len() + rng.below(64)
}

/// Random variants of the vector rows: a body byte flipped under the old hash, a byte edited and
/// re-hashed (outside the payload, in a segment's `first_prev_hash`, which breaks rule 117, or in an
/// anchor's first leaf `hash`, which breaks its root), a row duplicated, a row's `stream_id` column
/// moved to an account stream, and a body given a repeated `seq` key and re-hashed. The oracle reads
/// which row serves each vector start from the vector cases and tracks each edit's effect itself: a
/// request starts only when no row of its type on the workspace's control stream has stopped
/// parsing (DEC-895 item 1) and exactly one row on the control stream serves it and that row is
/// intact, one seq later nothing serves it, and genesis is seq 1 alone, with no cold read.
#[test]
fn random_row_variants_resolve_as_built() {
    let section = section();
    let base: Vec<StoredEvent> = list(&section, "rows").iter().map(stored).collect();
    let (own_control, own_account) = own_streams(&base);
    let starts: Vec<&Value> = list(&section, "row_cases")
        .iter()
        .filter(|c| text(c.get("expect").unwrap(), "outcome") == "start")
        .filter(|c| text(c.get("request").unwrap(), "kind") != "genesis")
        .collect();
    assert_eq!(starts.len(), 3, "two manifest starts and one anchor start");
    let serving = |row: &StoredEvent| {
        starts.iter().position(|c| {
            let req = c.get("request").unwrap();
            let named = text(req, "manifest_hash");
            let ctl = row.stream_id == own_control;
            ctl && match text(req, "kind") {
                "anchor" => row.event_id == text(req, "anchor_event_id"),
                _ => String::from_utf8_lossy(&row.body).contains(named),
            }
        })
    };
    let known: Vec<Known> = base
        .iter()
        .map(|r| Known {
            serves: serving(r),
            on_ctl: r.stream_id.starts_with("ctl:"),
            own_ctl: r.stream_id == own_control,
            event_type: r.event_type.clone(),
            broken: false,
            unreadable: false,
        })
        .collect();
    assert_eq!(known.iter().filter(|k| k.serves.is_some()).count(), 3);
    let (mut rng, mut wrong) = (Rng(0x9E37_79B9_7F4A_7C15), Vec::new());
    for round in 0..300 {
        let (mut rows, mut known) = (base.clone(), known.clone());
        for _ in 0..1 + rng.below(3) {
            let i = rng.below(rows.len());
            let segment = rows[i].event_type == "SegmentExported";
            let anchor = rows[i].event_type == "AnchorComputed";
            match rng.below(6) {
                0 if !known[i].broken => {
                    let at = digits(&rows[i].body, "build", &mut rng) + 7;
                    rows[i].body[at] = b'x';
                    known[i].broken = true;
                }
                1 if !known[i].broken => {
                    let at = digits(&rows[i].body, "build", &mut rng) + 7;
                    rows[i].body[at] = if rows[i].body[at] == b'd' { b'e' } else { b'd' };
                    rows[i].hash = Digest::of(&rows[i].body);
                }
                2 if !known[i].broken && (segment || anchor) => {
                    let key = if segment { "first_prev_hash" } else { "hash" };
                    let at = digits(&rows[i].body, key, &mut rng);
                    rows[i].body[at] = if rows[i].body[at] == b'a' { b'b' } else { b'a' };
                    rows[i].hash = Digest::of(&rows[i].body);
                    known[i].broken = true;
                }
                3 => {
                    rows.push(rows[i].clone());
                    known.push(known[i].clone());
                }
                4 => {
                    rows[i].stream_id.clone_from(&own_account);
                    known[i].on_ctl = false;
                    known[i].own_ctl = false;
                }
                _ => {
                    rows[i].body.splice(1..1, b"\"seq\":0,".iter().copied());
                    rows[i].hash = Digest::of(&rows[i].body);
                    known[i].unreadable = true;
                }
            }
        }
        for (s, case) in starts.iter().enumerate() {
            let serving: Vec<&Known> = known
                .iter()
                .filter(|k| k.on_ctl && k.serves == Some(s))
                .collect();
            let needed = match text(case.get("request").unwrap(), "kind") {
                "anchor" => "AnchorComputed",
                _ => "SegmentExported",
            };
            let blocked = known
                .iter()
                .any(|k| k.unreadable && k.own_ctl && k.event_type == needed);
            let starts = !blocked && matches!(serving.as_slice(), [only] if !only.broken);
            let (stream, from_seq) = (text(case, "stream_id"), int(case, "from_seq"));
            let req = case.get("request").unwrap();
            let want = start_at(case.get("expect").unwrap());
            let got = resolve_start_from_rows(&rows, stream, from_seq, request(req));
            let got = match (got, text(req, "kind")) {
                (Ok(ResolvedStart::Manifest(m)), "manifest") => {
                    m.confirm(cold(case.get("cold").unwrap()))
                }
                (Ok(ResolvedStart::Ready(start)), "anchor") => Ok(start),
                (Ok(other), kind) => {
                    wrong.push(format!("{round}: a {kind} request gave {other:?}"));
                    continue;
                }
                (Err(e), _) => Err(e),
            };
            let want = if starts { Ok(want) } else { Err(REFUSED) };
            if got != want {
                wrong.push(format!(
                    "{round} {}: want {want:?}, got {got:?}",
                    text(case, "name")
                ));
            }
            let off = resolve_start_from_rows(&rows, stream, from_seq + 1, request(req));
            if off != Err(REFUSED) {
                wrong.push(format!("{round}: a seq nothing serves gave {off:?}"));
            }
        }
        let stream = text(starts[0], "stream_id");
        let genesis = |n| resolve_start_from_rows(&rows, stream, n, StartRequest::Genesis);
        let got = (genesis(1), genesis(0), genesis(2));
        let ready = Ok(ResolvedStart::Ready(TrustedStart::GENESIS));
        if got != (ready, Err(REFUSED), Err(REFUSED)) {
            wrong.push(format!(
                "{round}: genesis is seq 1 alone, whatever the rows: {got:?}"
            ));
        }
    }
    let first = wrong.first();
    assert!(wrong.is_empty(), "{} wrong, first: {first:#?}", wrong.len());
}
