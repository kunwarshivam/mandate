//! One exchange per call between the runtime and the model-gateway service (DEC-2265 items 1 to 4):
//! a frame reads back as itself, a length past its bound is refused before any of the body is read,
//! a frame cut short or malformed is never answered, a call crosses a real connection whole, a
//! reply that does not come in time reads as timed out, and neither side writes a frame past its
//! bound.

mod common;

use std::io::{Cursor, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use common::*;
use mandate_gateway::frame::{
    self, FrameError, MAX_REPLY_FRAME, MAX_REQUEST_FRAME, read_frame, write_frame,
};
use mandate_gateway::wire::{Reply, request_bytes};
use mandate_gateway::{CallRecord, CallRequest, RequestError};
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};

fn runner() -> TestRunner {
    TestRunner::new(Config {
        cases: 256,
        failure_persistence: None,
        ..Config::default()
    })
}

/// One side of a connection held in memory: what the peer sent, and what this side wrote.
struct Pipe {
    input: Cursor<Vec<u8>>,
    output: Vec<u8>,
}

impl Pipe {
    fn carrying(input: Vec<u8>) -> Self {
        Self {
            input: Cursor::new(input),
            output: Vec::new(),
        }
    }
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

/// A reader that counts the bytes taken from it, so a test can see how far a refusal read.
struct Counting {
    inner: Cursor<Vec<u8>>,
    taken: usize,
}

impl Read for Counting {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buf)?;
        self.taken += read;
        Ok(read)
    }
}

/// A connection that gives up after a few seconds, so a side that never writes fails the test
/// rather than hanging it.
fn bounded(connection: TcpStream) -> TcpStream {
    connection
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    connection
}

fn framed(body: &[u8]) -> Vec<u8> {
    let length = u32::try_from(body.len()).unwrap();
    [length.to_be_bytes().as_slice(), body].concat()
}

fn served_request() -> CallRequest {
    let (_, model) = registry();
    request(&model, 1)
}

/// The record the gateway writes for `served_request`, called the way the service calls it.
fn served_record() -> CallRecord {
    let (mut gateway, _) = gateway();
    gateway.transport_mut().script(vec![ok_reply(800)]);
    gateway.call(&served_request()).unwrap()
}

/// DEC-2265 item 1: any body within its bound is written as its big-endian length and then itself,
/// and reads back as itself, taking nothing past its end; one past the bound is written as nothing.
#[test]
fn any_body_within_its_bound_reads_back_as_itself() {
    let bodies = (proptest::collection::vec(any::<u8>(), 0..2048), 0_u32..64);
    runner()
        .run(&bodies, |(body, slack)| {
            let length = u32::try_from(body.len()).unwrap();
            let mut written = Vec::new();
            prop_assert_eq!(write_frame(&mut written, &body, length + slack), Ok(()));
            prop_assert_eq!(&written, &framed(&body));
            written.extend_from_slice(b"next");
            let mut input = Cursor::new(written);
            prop_assert_eq!(read_frame(&mut input, length + slack), Ok(body.clone()));
            prop_assert_eq!(input.position(), u64::from(length) + 4);
            if let Some(short) = length.checked_sub(1) {
                let mut refused = Vec::new();
                prop_assert_eq!(
                    write_frame(&mut refused, &body, short),
                    Err(FrameError::TooLong)
                );
                prop_assert!(refused.is_empty(), "a refused body writes nothing");
            }
            Ok(())
        })
        .unwrap();
}

/// DEC-2265 item 2: the bounds are the ones DEC-2265 states — a 4 MiB request frame and a 24 MiB
/// reply frame. Pinned as literals because every other test compares a length against the constant
/// itself, so a constant that changed value would keep them all passing (the `*` → `/` mutant that
/// turned 4 MiB into 0 survived exactly that way).
#[test]
fn the_frame_bounds_are_the_ones_the_decision_states() {
    assert_eq!(MAX_REQUEST_FRAME, 4 * 1024 * 1024, "a 4 MiB request frame");
    assert_eq!(MAX_REPLY_FRAME, 24 * 1024 * 1024, "a 24 MiB reply frame");
}

/// DEC-2265 item 2: a length one past its bound, or any longer, is refused having read only the
/// four bytes of the length, so a peer cannot make either side read or hold an oversized body.
#[test]
fn a_length_past_the_bound_is_refused_before_a_byte_of_the_body_is_read() {
    for (bound, length) in [
        (MAX_REQUEST_FRAME, MAX_REQUEST_FRAME + 1),
        (MAX_REQUEST_FRAME, u32::MAX),
        (MAX_REPLY_FRAME, MAX_REPLY_FRAME + 1),
        (16, 17),
    ] {
        let mut input = Counting {
            inner: Cursor::new([length.to_be_bytes().as_slice(), &[b'{'; 64]].concat()),
            taken: 0,
        };
        assert_eq!(read_frame(&mut input, bound), Err(FrameError::TooLong));
        assert_eq!(input.taken, 4, "only the length is read past bound {bound}");
    }
    let at_bound = framed(&[b'x'; 16]);
    assert_eq!(
        read_frame(&mut Cursor::new(at_bound), 16),
        Ok(vec![b'x'; 16])
    );
}

/// DEC-2265 item 2: a request frame cut short anywhere, in its length or its body, reads as
/// truncated, and the service neither answers it nor writes a byte back.
#[test]
fn a_frame_cut_short_anywhere_is_never_answered() {
    let whole = framed(&request_bytes(&served_request()));
    for cut in 0..whole.len() {
        let mut pipe = Pipe::carrying(whole[..cut].to_vec());
        let mut answered = false;
        let served = frame::serve(&mut pipe, |_| {
            answered = true;
            Err(RequestError::UnknownModel)
        });
        assert_eq!(served, Err(FrameError::Truncated), "cut at {cut}");
        assert!(!answered, "a frame cut at {cut} is never answered");
        assert!(pipe.output.is_empty(), "nothing is written back at {cut}");
    }
}

/// DEC-2265 items 2 and 5: a whole frame that does not decode to a request, one not JSON, empty,
/// of another version, or a reply where a request belongs, is never answered and gets no reply.
#[test]
fn a_frame_that_does_not_decode_is_never_answered() {
    let request = String::from_utf8(request_bytes(&served_request())).unwrap();
    let bodies = [
        b"not json".to_vec(),
        Vec::new(),
        request
            .replace(r#""wire_version":1"#, r#""wire_version":2"#)
            .into_bytes(),
        br#"{"refused":"unknown_model","wire_version":1}"#.to_vec(),
    ];
    for body in bodies {
        let mut pipe = Pipe::carrying(framed(&body));
        let mut answered = false;
        let served = frame::serve(&mut pipe, |_| {
            answered = true;
            Err(RequestError::UnknownModel)
        });
        assert!(matches!(served, Err(FrameError::Wire(_))), "{served:?}");
        assert!(!answered, "an undecodable frame is never answered");
        assert!(pipe.output.is_empty(), "an undecodable frame gets no reply");
    }
}

/// DEC-2265 items 1, 5 and 6: over a real connection, a request reaches the service as itself and
/// its record comes back whole, prompt and response included; a refusal comes back as itself.
#[test]
fn a_call_crosses_one_connection_and_comes_back_whole() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (received, seen) = mpsc::channel();
    let service = thread::spawn(move || {
        let (mut gateway, _) = gateway();
        gateway.transport_mut().script(vec![ok_reply(800)]);
        for _ in 0..2 {
            let mut connection = bounded(listener.accept().unwrap().0);
            frame::serve(&mut connection, |request| {
                let reply = gateway.call(&request);
                received.send((request, reply.clone())).unwrap();
                reply
            })
            .unwrap();
        }
    });

    let mut connection = bounded(TcpStream::connect(address).unwrap());
    let reply = frame::call(&mut connection, &served_request()).unwrap();
    let (request, served) = seen.recv().unwrap();
    assert_eq!(request, served_request());
    assert_eq!(reply, served);
    let record = reply.unwrap();
    assert!(record.prompt.is_some() && record.response.is_some());
    assert_eq!(record, served_record());

    let mut foreign = served_request();
    foreign.workspace_id = "ws_other".to_owned();
    let mut connection = bounded(TcpStream::connect(address).unwrap());
    assert_eq!(
        frame::call(&mut connection, &foreign),
        Ok(Err(RequestError::ForeignWorkspace))
    );
    service.join().unwrap();
}

/// DEC-2265 item 4: when the service holds a call past the caller's socket timeout, the caller's
/// exchange ends as timed out rather than waiting on it.
#[test]
fn a_reply_that_does_not_come_in_time_reads_as_timed_out() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (release, released) = mpsc::channel::<()>();
    let service = thread::spawn(move || {
        let mut connection = bounded(listener.accept().unwrap().0);
        let _request = read_frame(&mut connection, MAX_REQUEST_FRAME);
        released.recv().unwrap();
    });

    let mut connection = TcpStream::connect(address).unwrap();
    connection
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    assert_eq!(
        frame::call(&mut connection, &served_request()),
        Err(FrameError::TimedOut)
    );
    release.send(()).unwrap();
    service.join().unwrap();
}

/// DEC-2265 items 1 and 3: the service never writes a reply past its bound; it writes nothing, so
/// the caller reads the call as lost. A request past its own bound is never sent.
#[test]
fn neither_side_writes_a_frame_past_its_bound() {
    let mut record = served_record();
    record.response = Some(vec![0x5a; 13 * 1024 * 1024]);
    let mut pipe = Pipe::carrying(framed(&request_bytes(&served_request())));
    let reply: Reply = Ok(record);
    assert_eq!(frame::serve(&mut pipe, |_| reply), Err(FrameError::TooLong));
    assert!(pipe.output.is_empty(), "an oversized reply writes nothing");
    let mut caller = Pipe::carrying(pipe.output);
    assert_eq!(
        frame::call(&mut caller, &served_request()),
        Err(FrameError::Truncated)
    );

    let mut oversized = served_request();
    oversized
        .inputs
        .insert("bars".to_owned(), "x".repeat(5 * 1024 * 1024));
    let mut pipe = Pipe::carrying(Vec::new());
    assert_eq!(frame::call(&mut pipe, &oversized), Err(FrameError::TooLong));
    assert!(pipe.output.is_empty(), "an oversized request is never sent");
}
