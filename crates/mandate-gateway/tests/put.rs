//! A research artifact's put on the model-gateway exchange (DEC-2279 item 1): every put and every
//! reply reads back as itself, the format is pinned by frames written out by hand, a frame is a
//! put exactly when it has a `put` member, a frame the encoder would not write reads as nothing,
//! and a put crosses a connection whole, or not at all past the request frame's bound.

mod common;

use std::io::{Cursor, Read, Write};

use common::*;
use mandate_canon::{Digest, parse, to_canonical};
use mandate_gateway::frame::{self, FrameError, MAX_REQUEST_FRAME, read_frame, write_frame};
use mandate_gateway::wire::{
    PutRefusal, PutReply, PutRequest, Request, WireError, any_request_from, put_bytes, put_from,
    put_reply_bytes, put_reply_from, request_bytes,
};
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};

fn runner() -> TestRunner {
    TestRunner::new(Config {
        cases: 256,
        failure_persistence: None,
        ..Config::default()
    })
}

fn put() -> impl Strategy<Value = PutRequest> {
    (
        "\\PC{0,12}|[\\x00-\\x1f\"\\\\]{0,4}",
        proptest::collection::vec(any::<u8>(), 0..256),
    )
        .prop_map(|(workspace_id, bytes)| PutRequest {
            workspace_id,
            bytes,
        })
}

fn put_reply() -> impl Strategy<Value = PutReply> {
    prop_oneof![
        any::<[u8; 8]>().prop_map(|seed| Ok(Digest::of(&seed))),
        proptest::sample::select(PutRefusal::ALL.to_vec()).prop_map(Err),
    ]
}

#[test]
fn any_put_reads_back_as_itself_from_canonical_bytes() {
    runner()
        .run(&put(), |put| {
            let bytes = put_bytes(&put);
            prop_assert_eq!(to_canonical(&parse(&bytes).unwrap()), bytes.clone());
            prop_assert_eq!(put_from(&bytes), Ok(put.clone()));
            prop_assert_eq!(any_request_from(&bytes), Ok(Request::Put(put)));
            Ok(())
        })
        .unwrap();
}

#[test]
fn any_put_reply_reads_back_as_itself_from_canonical_bytes() {
    runner()
        .run(&put_reply(), |reply| {
            let bytes = put_reply_bytes(&reply);
            prop_assert_eq!(to_canonical(&parse(&bytes).unwrap()), bytes.clone());
            prop_assert_eq!(put_reply_from(&bytes), Ok(reply));
            Ok(())
        })
        .unwrap();
}

/// DEC-2279 item 1, pinned: a put, a stored reply and a refusal written out by hand are the
/// encoder's bytes exactly. The put carries no reference: the service computes one (item 2).
#[test]
fn the_put_wire_form_is_the_one_written_out_here() {
    let put = PutRequest {
        workspace_id: "ws1".to_owned(),
        bytes: b"abc".to_vec(),
    };
    let frame = r#"{"put":"616263","wire_version":1,"workspace_id":"ws1"}"#;
    assert_eq!(String::from_utf8(put_bytes(&put)).unwrap(), frame);
    assert_eq!(put_from(frame.as_bytes()), Ok(put));
    let stored = concat!(
        r#"{"stored":"sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad","#,
        r#""wire_version":1}"#
    );
    assert_eq!(
        put_reply_from(stored.as_bytes()),
        Ok(Ok(Digest::of(b"abc")))
    );
    assert_eq!(put_reply_bytes(&Ok(Digest::of(b"abc"))), stored.as_bytes());
    for (refusal, code) in [
        (PutRefusal::ForeignWorkspace, "foreign_workspace"),
        (PutRefusal::StoreFull, "store_full"),
        (PutRefusal::StoreUnavailable, "store_unavailable"),
    ] {
        let refused = format!(r#"{{"refused":"{code}","wire_version":1}}"#);
        assert_eq!(refusal.code(), code);
        assert_eq!(put_reply_bytes(&Err(refusal)), refused.as_bytes());
        assert_eq!(put_reply_from(refused.as_bytes()), Ok(Err(refusal)));
    }
}

#[test]
fn a_frame_is_a_put_exactly_when_it_has_a_put_member() {
    let call = request(&registry().1, 1);
    assert_eq!(
        any_request_from(&request_bytes(&call)),
        Ok(Request::Call(Box::new(call.clone())))
    );
    assert!(
        put_from(&request_bytes(&call)).is_err(),
        "a call never reads as a put"
    );
    let framed = String::from_utf8(request_bytes(&call)).unwrap();
    let with_put = framed.replacen(r#","run_kind":"#, r#","put":"00","run_kind":"#, 1);
    assert_ne!(with_put, framed);
    assert_eq!(
        to_canonical(&parse(with_put.as_bytes()).unwrap()),
        with_put.as_bytes()
    );
    assert_eq!(
        any_request_from(with_put.as_bytes()),
        Err(WireError::NotCanonical),
        "a call frame with a put member is neither a call nor a put"
    );
}

/// A put frame or reply that is not exactly what the encoder writes reads as nothing: not
/// canonical, another version, a member missing, unlisted or of the wrong kind, odd or upper-case
/// hex, a reference without its prefix, or a refusal code not listed.
#[test]
fn a_put_frame_the_encoder_would_not_write_reads_as_nothing() {
    for frame in [
        r#"{"put":"616263","workspace_id":"ws1","wire_version":1}"#,
        r#"{"put":"616263","wire_version":2,"workspace_id":"ws1"}"#,
        r#"{"put":"616263","wire_version":1}"#,
        r#"{"extra":1,"put":"616263","wire_version":1,"workspace_id":"ws1"}"#,
        r#"{"put":"61626","wire_version":1,"workspace_id":"ws1"}"#,
        r#"{"put":"61626G","wire_version":1,"workspace_id":"ws1"}"#,
        r#"{"put":"6162AB","wire_version":1,"workspace_id":"ws1"}"#,
        r#"{"put":[97],"wire_version":1,"workspace_id":"ws1"}"#,
        r#"{"put":"616263","wire_version":1,"workspace_id":null}"#,
    ] {
        assert!(put_from(frame.as_bytes()).is_err(), "{frame}");
        assert!(any_request_from(frame.as_bytes()).is_err(), "{frame}");
    }
    let hex = Digest::of(b"abc").to_hex();
    for frame in [
        format!(r#"{{"stored":"{hex}","wire_version":1}}"#),
        format!(r#"{{"stored":"sha256:{}","wire_version":1}}"#, &hex[1..]),
        format!(r#"{{"stored":"sha256:{hex}","wire_version":2}}"#),
        format!(r#"{{"refused":"store_full","stored":"sha256:{hex}","wire_version":1}}"#),
        r#"{"refused":"unknown_model","wire_version":1}"#.to_owned(),
        r#"{"refused":"store_full"}"#.to_owned(),
        r#"{"wire_version":1}"#.to_owned(),
    ] {
        assert!(put_reply_from(frame.as_bytes()).is_err(), "{frame}");
    }
}

/// One side of a connection held in memory: what the peer sent, and what this side wrote.
struct Pipe {
    input: Cursor<Vec<u8>>,
    output: Vec<u8>,
}

impl Read for Pipe {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.input.read(buf)
    }
}

impl Write for Pipe {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.output.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn answered(reply: &PutReply) -> Pipe {
    let mut input = Vec::new();
    write_frame(&mut input, &put_reply_bytes(reply), u32::MAX).unwrap();
    Pipe {
        input: Cursor::new(input),
        output: Vec::new(),
    }
}

#[test]
fn a_put_crosses_the_exchange_as_one_frame_each_way() {
    let put = PutRequest {
        workspace_id: "ws1".to_owned(),
        bytes: b"thesis".to_vec(),
    };
    let mut pipe = answered(&Ok(Digest::of(b"thesis")));
    assert_eq!(frame::put(&mut pipe, &put), Ok(Ok(Digest::of(b"thesis"))));
    let mut sent = Cursor::new(pipe.output);
    assert_eq!(
        read_frame(&mut sent, MAX_REQUEST_FRAME).map(|body| put_from(&body)),
        Ok(Ok(put))
    );
    assert_eq!(
        sent.position(),
        u64::try_from(sent.get_ref().len()).unwrap()
    );
}

#[test]
fn a_put_past_the_request_frame_bound_is_never_sent() {
    let past = usize::try_from(MAX_REQUEST_FRAME / 2).unwrap();
    let put = PutRequest {
        workspace_id: "ws1".to_owned(),
        bytes: vec![0; past],
    };
    let mut pipe = answered(&Ok(Digest::of(&put.bytes)));
    assert_eq!(frame::put(&mut pipe, &put), Err(FrameError::TooLong));
    assert!(pipe.output.is_empty(), "nothing of it is written");
}

#[test]
fn a_put_reply_that_does_not_decode_is_a_frame_error() {
    let mut input = Vec::new();
    write_frame(&mut input, br#"{"stored":"x","wire_version":1}"#, u32::MAX).unwrap();
    let mut pipe = Pipe {
        input: Cursor::new(input),
        output: Vec::new(),
    };
    let put = PutRequest {
        workspace_id: "ws1".to_owned(),
        bytes: b"x".to_vec(),
    };
    assert!(matches!(
        frame::put(&mut pipe, &put),
        Err(FrameError::Wire(_))
    ));
}
