//! E12-3 (DEC-893, DEC-767; journal spec §9.14, §11): two scoping clauses of
//! `resolve_start_from_rows` that neither `cold_records.trusted_starts.row_cases` nor
//! `trusted_start_row_pins.rs` hold, pinned on rows built from that section's `rows` and re-hashed
//! so §11 checks 1, 2 and 4 hold. A second workspace's streams start from that workspace's own
//! control rows, not only the vectors' own workspace's; and a manifest start is read only from a
//! `SegmentExported`, so a segment's payload stored under another `event_type` is no start.

use std::path::Path;

use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_journal::{
    ColdRead, ResolvedStart, StartRequest, StoredEvent, TrustedStart, TrustedStartError,
    resolve_start_from_rows,
};

const REFUSED: Result<ResolvedStart, TrustedStartError> = Err(TrustedStartError::Refused);
const FOREIGN_CONTROL: &str = "ctl:ws_01J8Z9";
const FOREIGN_ACCOUNT: &str = "acct:ws_01J8Z9:01J8Z9ACCT00000000000000B1";
const TOKEN: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn text<'a>(value: &'a Value, name: &str) -> &'a str {
    value.get(name).and_then(Value::as_str).unwrap_or_default()
}

fn int(value: &Value, name: &str) -> u64 {
    value.get(name).and_then(Value::as_int).unwrap()
}

fn hex(value: &Value, name: &str) -> Digest {
    Digest::from_hex(text(value, name)).unwrap()
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

/// `cold_records.trusted_starts.rows`, as stored.
fn vector_rows() -> Vec<StoredEvent> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    let section = fixture.get("cold_records").unwrap().get("trusted_starts");
    let rows = section.unwrap().get("rows").and_then(Value::as_array);
    rows.unwrap().iter().map(stored).collect()
}

/// The vectors' own workspace's streams, read from their rows rather than named here, so the tests
/// follow whichever workspace the vectors are keyed to: its control stream is the one the
/// `StreamOpened` row opens, and its account stream the one that control stream's first
/// `SegmentExported` exports. Neither is [`FOREIGN_CONTROL`]'s workspace's.
struct OwnStreams {
    control: String,
    account: String,
}

fn own_streams(rows: &[StoredEvent]) -> OwnStreams {
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
    assert!(
        control != FOREIGN_CONTROL && !FOREIGN_ACCOUNT.starts_with(&format!("acct:{workspace}:")),
        "the vectors' own workspace is not the foreign one"
    );
    OwnStreams { control, account }
}

/// The vector row of `event_type` on `stream`.
fn vector_row<'a>(rows: &'a [StoredEvent], stream: &str, event_type: &str) -> &'a StoredEvent {
    let found = rows
        .iter()
        .find(|r| r.stream_id == stream && r.event_type == event_type);
    found.unwrap()
}

/// What a rebuilt row records in its columns, beside its payload.
struct Columns<'a> {
    stream: &'a str,
    seq: u64,
    event_id: &'a str,
    event_type: &'a str,
    prev_hash: Digest,
}

/// `template` rebuilt with `columns` and `payload` (canonical text), written into its body as well
/// as its columns and re-hashed, so §11 checks 1, 2 and 4 hold for it.
fn rebuilt(template: &StoredEvent, columns: &Columns<'_>, payload: &str) -> StoredEvent {
    let body = String::from_utf8(template.body.clone()).unwrap();
    let start = body.find("\"payload\":").unwrap();
    let head = body[..start]
        .replace(
            &format!(r#""event_id":"{}""#, template.event_id),
            &format!(r#""event_id":"{}""#, columns.event_id),
        )
        .replace(
            &format!(r#""event_type":"{}""#, template.event_type),
            &format!(r#""event_type":"{}""#, columns.event_type),
        );
    let Columns {
        stream,
        seq,
        prev_hash,
        ..
    } = columns;
    let (prev, recorded) = (prev_hash.to_hex(), &template.recorded_at);
    let tail = format!(
        r#","pii_refs":[],"prev_hash":"{prev}","recorded_at":"{recorded}","schema_version":1,"seq":{seq},"stream_id":"{stream}"}}"#
    );
    let body = format!("{head}\"payload\":{payload}{tail}").into_bytes();
    assert_eq!(parse(&body).map(|b| to_canonical(&b)), Ok(body.clone()));
    StoredEvent {
        stream_id: (*stream).to_owned(),
        seq: *seq,
        event_id: columns.event_id.to_owned(),
        event_type: columns.event_type.to_owned(),
        prev_hash: *prev_hash,
        hash: Digest::of(&body),
        body,
        ..template.clone()
    }
}

/// A `SegmentExported` payload of `stream` over `first..=last`, its rule 117 manifest hash, and
/// the manifest's cold bytes.
fn segment(
    stream: &str,
    first: u64,
    last: u64,
    prev: Digest,
    tail: Digest,
) -> (String, Digest, Vec<u8>) {
    let (file, prev, tail) = ("7".repeat(64), prev.to_hex(), tail.to_hex());
    let six = format!(
        r#"{{"file_sha256":"{file}","first_prev_hash":"{prev}","first_seq":{first},"last_hash":"{tail}","last_seq":{last},"stream":"{stream}"}}"#
    );
    let manifest = Digest::of(six.as_bytes());
    let named = format!(r#""manifest_hash":"{}","stream_id":"#, manifest.to_hex());
    (
        six.replace(r#""stream":"#, &named),
        manifest,
        six.into_bytes(),
    )
}

fn leaf_text((stream, seq, hash): &(&str, u64, Digest)) -> String {
    let hash = hash.to_hex();
    format!(r#"{{"hash":"{hash}","seq":{seq},"stream_id":"{stream}"}}"#)
}

/// §10's root, computed here over each leaf's text rather than by the crate: a leaf is
/// SHA-256(0x00 ‖ its canonical text), a node SHA-256(0x01 ‖ left ‖ right), split at the largest
/// power of two below the count.
fn node(hashes: &[Digest]) -> Digest {
    if let [single] = hashes {
        return *single;
    }
    let mut split = 1;
    while split * 2 < hashes.len() {
        split *= 2;
    }
    let (left, right) = hashes.split_at(split);
    Digest::of_parts(&[&[0x01], node(left).as_bytes(), node(right).as_bytes()])
}

/// A stamped `AnchorComputed` payload over `leaves`, given in strictly rising `stream_id` order,
/// with their own root.
fn stamped_anchor(leaves: &[(&str, u64, Digest)]) -> String {
    let texts: Vec<String> = leaves.iter().map(leaf_text).collect();
    let hashes: Vec<Digest> = texts
        .iter()
        .map(|t| Digest::of_parts(&[&[0x00], t.as_bytes()]))
        .collect();
    let (texts, root) = (texts.join(","), node(&hashes).to_hex());
    format!(r#"{{"leaves":[{texts}],"root":"{root}","token":"{TOKEN}"}}"#)
}

fn start(from_seq: u64, prev_hash: Digest) -> TrustedStart {
    TrustedStart {
        from_seq,
        prev_hash,
    }
}

/// §9.14, DEC-767: the lookup is scoped to the requested stream's own workspace, whichever it is.
/// `ws_01J8Z9`'s control stream, after the vector's foreign segment, records a segment of its own
/// account stream over seqs 4 to 6 and then a stamped anchor whose leaf for that stream is the
/// segment's last seq and hash, and which also holds a leaf for the vectors' own account stream. The
/// foreign account stream starts at 4 from the segment, once the cold bytes confirm it, and at 7
/// from the anchor; the same rows start nothing of the own workspace's, by either record, while its own
/// anchor still starts its account stream.
#[test]
fn another_workspaces_streams_start_from_its_own_control_rows() {
    let mut rows = vector_rows();
    let OwnStreams { control, account } = own_streams(&rows);
    let foreign_head = vector_row(&rows, FOREIGN_CONTROL, "SegmentExported").clone();
    let own_anchor = vector_row(&rows, &control, "AnchorComputed").clone();
    let (prev, last) = (Digest::of(b"foreign head 3"), Digest::of(b"foreign head 6"));
    let (payload, manifest_hash, cold) = segment(FOREIGN_ACCOUNT, 4, 6, prev, last);
    let segment_row = rebuilt(
        &foreign_head,
        &Columns {
            stream: FOREIGN_CONTROL,
            seq: 2,
            event_id: "01J8Z3C6A000000000000000F2",
            event_type: "SegmentExported",
            prev_hash: foreign_head.hash,
        },
        &payload,
    );
    let leaves = [
        (account.as_str(), 9, Digest::of(b"own account head 9")),
        (FOREIGN_ACCOUNT, 6, last),
        (FOREIGN_CONTROL, 2, segment_row.hash),
    ];
    let anchor_id = "01J8Z3C6A000000000000000F3";
    let anchor_row = rebuilt(
        &own_anchor,
        &Columns {
            stream: FOREIGN_CONTROL,
            seq: 3,
            event_id: anchor_id,
            event_type: "AnchorComputed",
            prev_hash: segment_row.hash,
        },
        &stamped_anchor(&leaves),
    );
    rows.extend([segment_row, anchor_row]);
    let by = StartRequest::Manifest { manifest_hash };
    let at = StartRequest::Anchor {
        anchor_event_id: anchor_id,
    };
    let resolve = |s, n, request| resolve_start_from_rows(&rows, s, n, request);
    match resolve(FOREIGN_ACCOUNT, 4, by) {
        Ok(ResolvedStart::Manifest(m)) => {
            assert_eq!(m.confirm(ColdRead::Read(cold)), Ok(start(4, prev)));
        }
        other => panic!("the foreign segment is a manifest start: {other:?}"),
    }
    let ready = Ok(ResolvedStart::Ready(start(7, last)));
    assert_eq!(resolve(FOREIGN_ACCOUNT, 7, at), ready, "the foreign anchor");
    for (name, n, request) in [
        ("the foreign segment", 4, by),
        ("the foreign anchor's leaf", 10, at),
    ] {
        assert_eq!(resolve(&account, n, request), REFUSED, "{name}");
    }
    let own = StartRequest::Anchor {
        anchor_event_id: &own_anchor.event_id,
    };
    assert!(
        matches!(resolve(&account, 10, own), Ok(ResolvedStart::Ready(_))),
        "the own anchor still starts"
    );
    assert_eq!(resolve(FOREIGN_ACCOUNT, 10, own), REFUSED, "the own anchor");
}

/// §9.14: a manifest start is read from a `SegmentExported` only. A rule-117-valid segment of the
/// account stream from seq 30, appended to the vectors' own control stream, starts it as a
/// `SegmentExported`; the same payload recorded as an `AnchorComputed` is no start, the mirror of
/// `row_cases`' `anchor_payload_under_another_event_type`.
#[test]
fn a_segment_payload_under_another_event_type_is_no_manifest_start() {
    let base = vector_rows();
    let OwnStreams { control, account } = own_streams(&base);
    let own_anchor = vector_row(&base, &control, "AnchorComputed").clone();
    let template = vector_row(&base, &control, "SegmentExported").clone();
    let (prev, last) = (
        Digest::of(b"account head 29"),
        Digest::of(b"account head 31"),
    );
    let (payload, manifest_hash, cold) = segment(&account, 30, 31, prev, last);
    let by = StartRequest::Manifest { manifest_hash };
    for (event_type, starts) in [("SegmentExported", true), ("AnchorComputed", false)] {
        let row = rebuilt(
            &template,
            &Columns {
                stream: &control,
                seq: own_anchor.seq + 1,
                event_id: "01J8Z3C6A000000000000000S5",
                event_type,
                prev_hash: own_anchor.hash,
            },
            &payload,
        );
        let rows = [base.clone(), vec![row]].concat();
        let got = resolve_start_from_rows(&rows, &account, 30, by);
        match (got, starts) {
            (Ok(ResolvedStart::Manifest(m)), true) => {
                let confirmed = m.confirm(ColdRead::Read(cold.clone()));
                assert_eq!(confirmed, Ok(start(30, prev)), "{event_type}");
            }
            (got, false) => assert_eq!(got, REFUSED, "{event_type}"),
            (got, true) => panic!("{event_type}: a manifest start, got {got:?}"),
        }
    }
}
