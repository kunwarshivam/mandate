//! E12-3 run logic, the plan stage's trusted start and the order of its refusals (workspace API
//! §4.8.1 "Verification"; DEC-787 items 1 and 5, DEC-788 item 4, DEC-893, DEC-894). The oracles are
//! §4.8.1's rules written out here, control rows this file builds and whose starts it knows, and a
//! cold store that counts what it is asked; none calls the code under test to decide what it
//! should return.

mod common;

use std::cell::Cell;

use common::{WS_A, WS_B, permitted, tenant, text};
use mandate_audit::{
    ColdSource, ControlSnapshot, MAX_RUN_EVENTS as MAX, RecordedStart, VerificationRefusal,
    VerificationRequest, plan,
};
use mandate_canon::Digest;
use mandate_journal::{
    AnchorLeaf, ColdRead, StartRequest, StoredEvent, TrustedStart, TrustedStartError, merkle_root,
};

const NOT_FOUND: VerificationRefusal = VerificationRefusal::NotFound;
const RANGE: VerificationRefusal = VerificationRefusal::Range;
const REFUSED: VerificationRefusal = VerificationRefusal::TrustedStart(TrustedStartError::Refused);
const UNREADABLE: VerificationRefusal =
    VerificationRefusal::TrustedStart(TrustedStartError::ColdUnreadable);
const T: &str = "2026-09-21T16:00:00.000000000Z";

type Seen = Result<(u64, TrustedStart, RecordedStart), VerificationRefusal>;

/// A cold store whose one manifest object, under `.0`, is `.1` (any other hash is absent), and
/// which counts every question in `.2`.
struct Cold(Digest, ColdRead, Cell<u32>);

impl ColdSource for Cold {
    fn manifest(&self, manifest_hash: &Digest) -> ColdRead {
        self.2.set(self.2.get() + 1);
        let hit = *manifest_hash == self.0;
        if hit {
            self.1.clone()
        } else {
            ColdRead::Absent
        }
    }
}

/// `WS_A`'s plan of `stream` over `from..=to` from `by`, at stream head `head`, over `control`.
fn run(
    stream: &str,
    span: (u64, Option<u64>),
    by: StartRequest<'_>,
    head: Option<u64>,
    control: &[StoredEvent],
    cold: &Cold,
) -> Seen {
    let ctx = tenant(WS_A);
    let (from_seq, to_seq) = span;
    let request = VerificationRequest {
        stream_id: stream,
        from_seq,
        to_seq,
        trusted_start: by,
    };
    let snapshot = ControlSnapshot {
        stream_head: head,
        control,
    };
    let planned = plan(&permitted(&ctx), &request, &snapshot, cold);
    planned.map(|p| (p.to_seq(), p.start(), p.recorded_start().clone()))
}

fn own() -> String {
    format!("acct:{}:ACCT1", text(WS_A))
}

fn repeat(digit: &str) -> Digest {
    Digest::from_hex(&digit.repeat(64)).unwrap()
}

/// A `ctl:{WS_A}` row at `seq` recording `payload`: canonical, its columns its body's, and its hash
/// its body's (§11 checks 1, 2 and 4).
fn ctl_row(seq: u64, event_type: &str, payload: &str, refs: &str) -> StoredEvent {
    let (stream_id, event_id) = (
        format!("ctl:{}", text(WS_A)),
        format!("01J8Z3C6A00000000000000R0{seq}"),
    );
    let (build, zero) = ("d".repeat(64), "0".repeat(64));
    let body = format!(r#"{{"actor":{{"build":"sha256:{build}","id":"control_services","kind":"system","version":"0.1.0"}},"artifact_refs":[{refs}],"causation_id":null,"clock_source":"local","config_refs":{{}},"correlation_id":null,"envelope_version":1,"environment":"paper","event_id":"{event_id}","event_time":"{T}","event_type":"{event_type}","payload":{payload},"pii_refs":[],"prev_hash":"{zero}","recorded_at":"{T}","schema_version":1,"seq":{seq},"stream_id":"{stream_id}"}}"#).into_bytes();
    let (event_type, hash) = (event_type.to_owned(), Digest::of(&body));
    let (environment, recorded_at) = ("paper".to_owned(), T.to_owned());
    StoredEvent {
        stream_id,
        seq,
        event_id,
        event_type,
        schema_version: 1,
        environment,
        recorded_at,
        prev_hash: Digest::ZERO,
        hash,
        body,
    }
}

/// `WS_A`'s control rows: a `SegmentExported` of [`own`] over seqs 4 to 9 whose `first_prev_hash`
/// is `5` × 64, and an `AnchorComputed` whose one leaf is [`own`]'s seq 9 at `1` × 64, stamped or
/// with a `null` token. Also the segment's manifest hash (rule 117) and its cold manifest object
/// (§6.2), and the anchor's id.
fn control(stamped: bool) -> (Vec<StoredEvent>, Digest, Vec<u8>, String) {
    let (seven, five, six, own) = ("7".repeat(64), "5".repeat(64), "6".repeat(64), own());
    let manifest = format!(
        r#"{{"file_sha256":"{seven}","first_prev_hash":"{five}","first_seq":4,"last_hash":"{six}","last_seq":9,"stream":"{own}"}}"#
    );
    let hash = Digest::of(manifest.as_bytes());
    let named = format!(r#""manifest_hash":"{}","stream_id":"#, hash.to_hex());
    let segment = ctl_row(
        1,
        "SegmentExported",
        &manifest.replace(r#""stream":"#, &named),
        "",
    );
    let leaf = AnchorLeaf {
        stream_id: own.clone(),
        seq: 9,
        hash: repeat("1"),
    };
    let (root, token) = (
        merkle_root(&[leaf]).unwrap().to_hex(),
        format!(r#""sha256:{}""#, "a".repeat(64)),
    );
    let (token, refs) = if stamped {
        (&token[..], &token[..])
    } else {
        ("null", "")
    };
    let ones = "1".repeat(64);
    let payload = format!(
        r#"{{"leaves":[{{"hash":"{ones}","seq":9,"stream_id":"{own}"}}],"root":"{root}","token":{token}}}"#
    );
    let anchor = ctl_row(2, "AnchorComputed", &payload, refs);
    let id = anchor.event_id.clone();
    (vec![segment, anchor], hash, manifest.into_bytes(), id)
}

/// DEC-788 item 4: steps 4, 5 and 6 refuse a manifest request the control rows vouch for before
/// the cold store is asked anything; [`step_7_asks_the_cold_store_for_a_manifest_start_only`] is
/// the control, the same request in range asking it once.
#[test]
#[ignore = "pending E12-3"]
fn oversized_request_reads_no_cold_object() {
    let (rows, manifest_hash, bytes, _) = control(true);
    let by = StartRequest::Manifest { manifest_hash };
    let (head, foreign) = (2 * MAX, format!("acct:{}:ACCT1", text(WS_B)));
    let cases = [
        (own(), None, 9, NOT_FOUND),
        (foreign, Some(head), 9, NOT_FOUND),
        (own(), Some(head), head + 1, RANGE),
        (own(), Some(head), 4 + MAX, RANGE),
    ];
    for (stream, at, to, want) in cases {
        let cold = Cold(manifest_hash, ColdRead::Read(bytes.clone()), Cell::new(0));
        let got = run(&stream, (4, Some(to)), by, at, &rows, &cold);
        assert_eq!(got, Err(want), "{stream} at {at:?}, 4..={to}");
        assert_eq!(cold.2.get(), 0, "{stream} at {at:?}, 4..={to}: cold reads");
    }
}

/// DEC-893 item 4, DEC-894 item 1, DEC-787 item 5, through the plan: a manifest start asks the cold
/// store once, for its own manifest, and starts only on its exact bytes; other bytes are `Refused`
/// and an absent or unreadable object `ColdUnreadable`. An anchor start asks nothing. Each plan
/// records the start it was asked for.
#[test]
#[ignore = "pending E12-3"]
fn step_7_asks_the_cold_store_for_a_manifest_start_only() {
    let (rows, manifest_hash, bytes, anchor_event_id) = control(true);
    let manifest = StartRequest::Manifest { manifest_hash };
    let anchor = StartRequest::Anchor {
        anchor_event_id: &anchor_event_id,
    };
    let from_segment = Ok((
        12,
        TrustedStart {
            from_seq: 4,
            prev_hash: repeat("5"),
        },
        RecordedStart::Manifest { manifest_hash },
    ));
    let recorded = RecordedStart::Anchor {
        anchor_event_id: anchor_event_id.clone(),
    };
    let cases = [
        (manifest, 4, ColdRead::Read(bytes.clone()), from_segment, 1),
        (
            manifest,
            4,
            ColdRead::Read([&bytes[..], b" "].concat()),
            Err(REFUSED),
            1,
        ),
        (manifest, 4, ColdRead::Absent, Err(UNREADABLE), 1),
        (manifest, 4, ColdRead::Unreadable, Err(UNREADABLE), 1),
        (
            anchor,
            10,
            ColdRead::Unreadable,
            Ok((
                12,
                TrustedStart {
                    from_seq: 10,
                    prev_hash: repeat("1"),
                },
                recorded,
            )),
            0,
        ),
    ];
    for (i, (by, from, answer, want, calls)) in cases.into_iter().enumerate() {
        let cold = Cold(manifest_hash, answer, Cell::new(0));
        let got = run(&own(), (from, None), by, Some(12), &rows, &cold);
        assert_eq!(got, want, "case {i}");
        assert_eq!(cold.2.get(), calls, "case {i}: cold reads");
    }
}

/// DEC-788 item 4's order, every set of faults: another workspace's stream (step 4), `from_seq` 0
/// (step 5), 1,000,003 events (step 6) and an unrecorded manifest (step 7). The earliest fault set
/// is the refusal; with none, the request is planned.
#[test]
#[ignore = "pending E12-3"]
fn refusals_follow_dec_788s_order() {
    let (rows, hash, bytes, _) = control(true);
    let foreign = format!("acct:{}:ACCT1", text(WS_B));
    for faults in 0..16_u8 {
        let [stream, range, size, unrecorded] = [0, 1, 2, 3].map(|bit| faults >> bit & 1 == 1);
        let manifest_hash = if unrecorded {
            Digest::of(b"no such manifest")
        } else {
            hash
        };
        let stream_id = if stream { foreign.clone() } else { own() };
        let span = (
            if range { 0 } else { 4 },
            Some(if size { 6 + MAX } else { 10 }),
        );
        let want = match (stream, range || size, unrecorded) {
            (true, _, _) => Err(NOT_FOUND),
            (_, true, _) => Err(RANGE),
            (_, _, true) => Err(REFUSED),
            _ => Ok((
                10,
                TrustedStart {
                    from_seq: 4,
                    prev_hash: repeat("5"),
                },
                RecordedStart::Manifest { manifest_hash },
            )),
        };
        let cold = Cold(hash, ColdRead::Read(bytes.clone()), Cell::new(0));
        let by = StartRequest::Manifest { manifest_hash };
        let got = run(&stream_id, span, by, Some(2 * MAX), &rows, &cold);
        assert_eq!(
            got, want,
            "faults {faults:04b}: stream, range, size, start from the low bit"
        );
    }
}

/// §9.14 and DEC-893 through the plan: an anchor whose `event_id` no row has, an unstamped anchor
/// (its `token` `null`, re-hashed, DEC-896 item 4), an anchor with no leaf at `from_seq − 1`, a
/// segment from another `first_seq` and a segment of another stream each refuse the start
/// `TrustedStart(Refused)`, and none asks the cold store.
#[test]
#[ignore = "pending E12-3"]
fn step_7_refuses_a_start_that_does_not_fit() {
    let (stamped, manifest_hash, bytes, id) = control(true);
    let (unstamped, ..) = control(false);
    let other = format!("acct:{}:ACCT2", text(WS_A));
    let manifest = StartRequest::Manifest { manifest_hash };
    let anchor = StartRequest::Anchor {
        anchor_event_id: &id,
    };
    let unknown = StartRequest::Anchor {
        anchor_event_id: "01J8Z3C6A00000000000000R09",
    };
    let cases = [
        ("an unknown anchor", &stamped, own(), unknown, 10),
        ("an unstamped anchor", &unstamped, own(), anchor, 10),
        ("no leaf at seq 8", &stamped, own(), anchor, 9),
        ("no leaf at seq 10", &stamped, own(), anchor, 11),
        ("a segment from seq 4", &stamped, own(), manifest, 5),
        ("another stream's segment", &stamped, other, manifest, 4),
    ];
    for (name, rows, stream, by, from) in cases {
        let cold = Cold(manifest_hash, ColdRead::Read(bytes.clone()), Cell::new(0));
        let got = run(&stream, (from, None), by, Some(12), rows, &cold);
        assert_eq!(got, Err(REFUSED), "{name}");
        assert_eq!(cold.2.get(), 0, "{name}: cold reads");
    }
}
