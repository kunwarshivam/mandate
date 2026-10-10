//! E12-3 (DEC-893, DEC-895, DEC-767; journal spec §9.14, §10, §11): three clauses of
//! `resolve_start_from_rows` that `cold_records.trusted_starts.row_cases` do not hold, pinned on
//! rows built from that section's `rows`. An anchor starts every stream it has a leaf for, not only
//! its first; a stream id with no workspace has no start of any kind, genesis included; and a leaf
//! hash that is not 64 lowercase hex digits refuses its anchor whichever leaf the start is read
//! from. Every rebuilt row is first shown to start, so a refusal comes from the edit alone.

use std::path::Path;

use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_journal::{
    ColdRead, ResolvedStart, StartRequest, StoredEvent, TrustedStart, TrustedStartError,
    resolve_start_from_rows,
};

const REFUSED: Result<ResolvedStart, TrustedStartError> = Err(TrustedStartError::Refused);

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
/// `StreamOpened` row opens, its account stream the one that control stream's first
/// `SegmentExported` exports, and its agent stream `agent_a` of the same workspace.
struct OwnStreams {
    control: String,
    account: String,
    agent: String,
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
    let agent = format!("agent:{workspace}:agent_a");
    OwnStreams {
        control,
        account,
        agent,
    }
}

/// The index of the first row of `event_type` on the workspace's own control stream.
fn own(rows: &[StoredEvent], event_type: &str) -> usize {
    let control = own_streams(rows).control;
    let found = rows
        .iter()
        .position(|r| r.stream_id == control && r.event_type == event_type);
    found.unwrap()
}

fn payload_of(row: &StoredEvent) -> Value {
    parse(&row.body).unwrap().get("payload").unwrap().clone()
}

/// `row` with its body's payload replaced by `payload` (canonical text) and re-hashed: its columns
/// are untouched, so §11 checks 1, 2 and 4 still hold.
fn with_payload(row: &StoredEvent, payload: &str) -> StoredEvent {
    let body = String::from_utf8(row.body.clone()).unwrap();
    let start = body.find("\"payload\":").unwrap() + "\"payload\":".len();
    let end = body.find(",\"pii_refs\":").unwrap();
    let body = format!("{}{payload}{}", &body[..start], &body[end..]).into_bytes();
    assert_eq!(parse(&body).map(|b| to_canonical(&b)), Ok(body.clone()));
    StoredEvent {
        hash: Digest::of(&body),
        body,
        ..row.clone()
    }
}

/// One anchor leaf as its text records it, so a hash that is not a digest can still be written.
#[derive(Clone)]
struct Leaf {
    stream: String,
    seq: u64,
    hash: String,
}

fn leaves_of(payload: &Value) -> Vec<Leaf> {
    let leaves = payload.get("leaves").and_then(Value::as_array).unwrap();
    let leaf = |l: &Value| Leaf {
        stream: text(l, "stream_id").to_owned(),
        seq: int(l, "seq"),
        hash: text(l, "hash").to_owned(),
    };
    leaves.iter().map(leaf).collect()
}

fn leaf_text(leaf: &Leaf) -> String {
    let Leaf { stream, seq, hash } = leaf;
    format!(r#"{{"hash":"{hash}","seq":{seq},"stream_id":"{stream}"}}"#)
}

/// §10's root, computed here over each leaf's text rather than by the crate: a leaf is
/// SHA-256(0x00 ‖ its canonical text), a node SHA-256(0x01 ‖ left ‖ right), split at the largest
/// power of two below the count.
fn root(leaves: &[Leaf]) -> Digest {
    let hashes: Vec<Digest> = leaves
        .iter()
        .map(|l| Digest::of_parts(&[&[0x00], leaf_text(l).as_bytes()]))
        .collect();
    node(&hashes)
}

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

/// The vector anchor row rebuilt over `leaves` with `root`, its token kept.
fn anchor_over(rows: &[StoredEvent], leaves: &[Leaf], root: Digest) -> StoredEvent {
    let anchor = &rows[own(rows, "AnchorComputed")];
    let token = text(&payload_of(anchor), "token").to_owned();
    let leaves: Vec<String> = leaves.iter().map(leaf_text).collect();
    let (leaves, root) = (leaves.join(","), root.to_hex());
    let payload = format!(r#"{{"leaves":[{leaves}],"root":"{root}","token":"{token}"}}"#);
    with_payload(anchor, &payload)
}

/// `rows` with the anchor row rebuilt over `leaves` and their own root.
fn rows_with_anchor(rows: &[StoredEvent], leaves: &[Leaf]) -> Vec<StoredEvent> {
    let mut rows = rows.to_vec();
    let at = own(&rows, "AnchorComputed");
    rows[at] = anchor_over(&rows, leaves, root(leaves));
    rows
}

/// A `SegmentExported` of `stream` from seq `first` whose `first_prev_hash` is `prev`, built on the
/// vector's own segment row, with its rule 117 manifest hash and the manifest's cold bytes.
fn segment(rows: &[StoredEvent], stream: &str, first: u64, prev: &str) -> (StoredEvent, Digest) {
    let (file, tail, last) = ("7".repeat(64), "6".repeat(64), first + 2);
    let six = format!(
        r#"{{"file_sha256":"{file}","first_prev_hash":"{prev}","first_seq":{first},"last_hash":"{tail}","last_seq":{last},"stream":"{stream}"}}"#
    );
    let manifest = Digest::of(six.as_bytes());
    let named = format!(r#""manifest_hash":"{}","stream_id":"#, manifest.to_hex());
    let payload = six.replace(r#""stream":"#, &named);
    let row = with_payload(&rows[own(rows, "SegmentExported")], &payload);
    (row, manifest)
}

fn ready(from_seq: u64, prev_hash: &str) -> Result<ResolvedStart, TrustedStartError> {
    let prev_hash = Digest::from_hex(prev_hash).unwrap();
    Ok(ResolvedStart::Ready(TrustedStart {
        from_seq,
        prev_hash,
    }))
}

/// §9.14: a stamped anchor gives the `hash` of its leaf for `s` at `n − 1`, for every leaf it
/// holds, not only the account leaf `row_cases` start from, nor only its first; never at the leaf's
/// own seq or two after it. Checked on the vector anchor, and on it rebuilt with an agent leaf
/// between its two, so the first, a middle and the last leaf each start their stream.
#[test]
fn an_anchor_starts_every_stream_it_has_a_leaf_for() {
    let rows = vector_rows();
    let streams = own_streams(&rows);
    let anchor = &rows[own(&rows, "AnchorComputed")];
    let (id, payload) = (anchor.event_id.clone(), payload_of(anchor));
    let leaves = leaves_of(&payload);
    let leaf_streams: Vec<&str> = leaves.iter().map(|l| l.stream.as_str()).collect();
    assert_eq!(
        leaf_streams,
        [streams.account.as_str(), streams.control.as_str()],
        "the vector anchor's leaves"
    );
    assert_eq!(root(&leaves), hex(&payload, "root"), "the root oracle");
    let agent = Leaf {
        stream: streams.agent.clone(),
        seq: 6,
        hash: Digest::of(b"agent head").to_hex(),
    };
    let wider = [leaves[0].clone(), agent, leaves[1].clone()];
    let at = StartRequest::Anchor {
        anchor_event_id: &id,
    };
    for (name, rows, leaves) in [
        ("the vector anchor", rows.clone(), &leaves[..]),
        ("three leaves", rows_with_anchor(&rows, &wider), &wider[..]),
    ] {
        for leaf in leaves {
            let resolve = |n| resolve_start_from_rows(&rows, &leaf.stream, n, at);
            let next = leaf.seq + 1;
            assert_eq!(
                resolve(next),
                ready(next, &leaf.hash),
                "{name}: {}",
                leaf.stream
            );
            assert_eq!(
                resolve(leaf.seq),
                REFUSED,
                "{name}: {} at its leaf",
                leaf.stream
            );
            assert_eq!(
                resolve(next + 1),
                REFUSED,
                "{name}: {} two after",
                leaf.stream
            );
        }
    }
}

/// §9.14, DEC-767: a stream id whose workspace segment is absent or empty names no control stream
/// as its own, so it has no start of any kind, genesis at seq 1 included, though the anchor holds a
/// leaf for it and a valid segment exports it. The same rows start the account stream both ways.
#[test]
fn a_stream_id_with_no_workspace_refuses_every_kind() {
    let base = vector_rows();
    let account_stream = own_streams(&base).account;
    let account_stream = account_stream.as_str();
    let malformed = [
        "", ":", "acct", "acct:", "acct::", "acct::A1", "ctl:", "ctl::",
    ];
    let hash = Digest::of(b"no workspace head").to_hex();
    let mut leaves = leaves_of(&payload_of(&base[own(&base, "AnchorComputed")]));
    leaves.extend(malformed.iter().map(|s| Leaf {
        stream: (*s).to_owned(),
        seq: 9,
        hash: hash.clone(),
    }));
    leaves.sort_by(|a, b| a.stream.cmp(&b.stream));
    let mut rows = rows_with_anchor(&base, &leaves);
    let id = rows[own(&rows, "AnchorComputed")].event_id.clone();
    let at = StartRequest::Anchor {
        anchor_event_id: &id,
    };
    let zeros = Digest::ZERO.to_hex();
    let mut manifests = Vec::new();
    for (stream, first) in malformed
        .iter()
        .map(|s| (*s, 1))
        .chain([(account_stream, 30)])
    {
        let (row, manifest) = segment(&base, stream, first, &zeros);
        let mut copy = row.clone();
        copy.stream_id = "ctl:".to_owned();
        rows.extend([row, copy]);
        manifests.push(manifest);
    }
    let by = |manifest_hash| StartRequest::Manifest { manifest_hash };
    let account = manifests.pop().unwrap();
    let resolve = |s, n, request| resolve_start_from_rows(&rows, s, n, request);
    let leaf = leaves.iter().find(|l| l.stream == account_stream).unwrap();
    assert_eq!(
        resolve(account_stream, 10, at),
        ready(10, &leaf.hash),
        "the anchor"
    );
    match resolve(account_stream, 30, by(account)) {
        Ok(ResolvedStart::Manifest(m)) => assert!(m.confirm(ColdRead::Absent).is_err()),
        other => panic!("the account segment is a manifest start: {other:?}"),
    }
    for (stream, manifest) in malformed.iter().zip(manifests) {
        assert_eq!(
            resolve(stream, 1, StartRequest::Genesis),
            REFUSED,
            "{stream:?}: genesis"
        );
        assert_eq!(
            resolve(stream, 1, by(manifest)),
            REFUSED,
            "{stream:?}: segment"
        );
        assert_eq!(resolve(stream, 10, at), REFUSED, "{stream:?}: anchor");
    }
}

/// §9.14, §2, DEC-895 item 2: an anchor is read whole, so a leaf hash that is not 64 lowercase hex
/// digits refuses it even when the start comes from another, good leaf. Each bad form, the good
/// hash in upper case among them where it has letters, is written into the other leaf of the
/// vector anchor, with the root over the leaves' text (as the reference reads it), the recorded
/// root (as a reader that repairs the hash would), or the root with that hash zero-filled (as a
/// reader that zero-fills it would), the row re-hashed so check 4 holds. A
/// `first_prev_hash` in upper case is refused the same way; `row_cases` on the coverage branch
/// already hold a short one.
#[test]
fn a_non_hex_leaf_anywhere_refuses_the_anchor() {
    let rows = vector_rows();
    let account_stream = own_streams(&rows).account;
    let at = own(&rows, "AnchorComputed");
    let (id, payload) = (rows[at].event_id.clone(), payload_of(&rows[at]));
    let request = StartRequest::Anchor {
        anchor_event_id: &id,
    };
    let leaves = leaves_of(&payload);
    let recorded = hex(&payload, "root");
    for (start, other) in [(0, 1), (1, 0)] {
        let (s, n) = (leaves[start].stream.as_str(), leaves[start].seq + 1);
        let good = &leaves[other].hash;
        let rebuilt = rows_with_anchor(&rows, &leaves);
        let want = ready(n, &leaves[start].hash);
        assert_eq!(
            resolve_start_from_rows(&rebuilt, s, n, request),
            want,
            "{s}"
        );
        let forms = [
            ("g", "g".repeat(64)),
            ("upper case", good.to_uppercase()),
            ("63 digits", good[1..].to_owned()),
            ("65 digits", format!("{good}0")),
            ("empty", String::new()),
            ("prefixed", format!("sha256:{good}")),
        ];
        for (name, bad) in forms.into_iter().filter(|(_, bad)| bad != good) {
            let mut edited = leaves.clone();
            edited[other].hash = bad;
            let mut zeroed = edited.clone();
            zeroed[other].hash = Digest::ZERO.to_hex();
            for (root_name, root) in [
                ("text root", root(&edited)),
                ("recorded root", recorded),
                ("zero-filled root", root(&zeroed)),
            ] {
                let mut rows = rows.clone();
                rows[at] = anchor_over(&rows, &edited, root);
                let got = resolve_start_from_rows(&rows, s, n, request);
                assert_eq!(
                    got, REFUSED,
                    "{s} from leaf {start}, leaf {other} {name}, {root_name}"
                );
            }
        }
    }
    let prev = Digest::of(b"upper prev").to_hex();
    for (name, prev, starts) in [
        ("lower", prev.clone(), true),
        ("upper", prev.to_uppercase(), false),
    ] {
        let mut rows = rows.clone();
        let (row, manifest_hash) = segment(&rows, &account_stream, 30, &prev);
        rows.push(row);
        let got = resolve_start_from_rows(
            &rows,
            &account_stream,
            30,
            StartRequest::Manifest { manifest_hash },
        );
        assert_eq!(
            matches!(got, Ok(ResolvedStart::Manifest(_))),
            starts,
            "{name}: {got:?}"
        );
    }
}
