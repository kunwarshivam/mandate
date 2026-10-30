//! The service's serve loop over real loopback connections (DEC-2265 items 1, 2, 4 and 9, E21-62):
//! a call crosses it whole, a stalled peer is cut off at the timeout and frees its slot, a
//! connection past the cap is closed unread, a frame past its bound or malformed is never answered,
//! stopping ends the loop, and a canary marker shows in no log line.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use mandate_canon::Digest;
use mandate_gateway::frame::{self, FrameError, MAX_REQUEST_FRAME};
use mandate_gateway::kinds::{KeyOwner, Purpose};
use mandate_gateway::registry::ModelRef;
use mandate_gateway::wire::{Reply, request_bytes};
use mandate_gateway::{CallRequest, RequestError};
use mandate_gatewayd::serve::{Limits, Log, Server, Stopper};
use mandate_time::UtcNanos;

const CANARY: &str = "CANARY-7f3a9c-never-logged";

/// The log lines a server wrote, kept for the test to read.
#[derive(Default)]
struct Lines(Mutex<Vec<String>>);

impl Log for Lines {
    fn line(&self, line: &str) {
        self.0.lock().unwrap().push(line.to_owned());
    }
}

impl Lines {
    fn all(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }

    /// Waits up to about two seconds, in 200 looks, for a line containing `part`.
    fn wait_for(&self, part: &str) -> bool {
        (0..200).any(|_| {
            let found = self.all().iter().any(|line| line.contains(part));
            if !found {
                thread::sleep(Duration::from_millis(10));
            }
            found
        })
    }
}

/// A running server: its address, its log, the requests it answered, and how to stop it.
struct Running {
    address: SocketAddr,
    lines: Arc<Lines>,
    answered: mpsc::Receiver<CallRequest>,
    stopper: Stopper,
    ended: mpsc::Receiver<()>,
}

impl Running {
    /// Stops the server and checks its loop returns within five seconds.
    fn stop(self) -> Vec<String> {
        self.stopper.stop();
        assert_eq!(
            self.ended.recv_timeout(Duration::from_secs(5)),
            Ok(()),
            "the loop did not return after the stop"
        );
        self.lines.all()
    }
}

/// Serves on loopback, answering every request `reply`.
fn running(limits: Limits, reply: Reply) -> Running {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let lines = Arc::new(Lines::default());
    let server = Server::new(listener, limits, Arc::clone(&lines) as Arc<dyn Log>);
    let stopper = server.stopper().unwrap();
    let (seen, answered) = mpsc::channel();
    let (end, ended) = mpsc::channel();
    thread::spawn(move || {
        server.run(|request| {
            seen.send(request).unwrap();
            reply.clone()
        });
        end.send(()).unwrap();
    });
    Running {
        address,
        lines,
        answered,
        stopper,
        ended,
    }
}

fn limits(io_timeout_ms: u64, max_connections: usize) -> Limits {
    Limits {
        io_timeout: Duration::from_millis(io_timeout_ms),
        max_connections,
    }
}

fn request(marker: &str) -> CallRequest {
    CallRequest {
        call_id: mandate_canon::encode_ulid(1 << 70),
        workspace_id: "ws_1".to_owned(),
        agent_id: Some("agent-a".to_owned()),
        purpose: Purpose::Research,
        model: ModelRef {
            model_id: "llm.research_agent".to_owned(),
            model_version: "0.1.0".to_owned(),
            content_hash: Digest::of(b"model"),
        },
        inputs: BTreeMap::from([("question".to_owned(), marker.to_owned())]),
        as_of: UtcNanos::from_parts(1_792_155_600, 0).unwrap(),
        deadline: UtcNanos::from_parts(1_792_155_660, 0).unwrap(),
        observations: vec![marker.to_owned()],
        key_owner: KeyOwner::Platform,
        run_kind: None,
        turns: Vec::new(),
        tools: Vec::new(),
        spend_ceiling: None,
    }
}

/// A client connection that gives up after five seconds, so a server that never answers fails the
/// test rather than hanging it.
fn connect(address: SocketAddr) -> TcpStream {
    let connection = TcpStream::connect(address).unwrap();
    connection
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    connection
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    connection
}

/// DEC-2265 items 1 and 5: a request crosses the service whole to the one thread holding the
/// gateway, and its reply comes back as itself; the log names the reply's code.
#[test]
fn a_call_crosses_the_service_whole() {
    let server = running(limits(2_000, 4), Err(RequestError::ForeignWorkspace));
    let sent = request("hello");
    let reply = frame::call(&mut connect(server.address), &sent).unwrap();
    assert_eq!(reply, Err(RequestError::ForeignWorkspace));
    assert_eq!(server.answered.recv().unwrap(), sent);
    let lines = server.stop();
    assert!(
        lines.contains(&"call refused: foreign_workspace".to_owned()),
        "{lines:?}"
    );
}

/// DEC-2265 item 4: a peer that opens a connection and sends nothing is cut off at the service's
/// timeout, and the slot it held serves the next call.
#[test]
fn a_stalled_peer_is_cut_off_and_frees_its_slot() {
    let server = running(limits(200, 1), Err(RequestError::Tools));
    let mut stalled = connect(server.address);
    assert!(server.lines.wait_for("request refused: timed_out"));
    let mut rest = Vec::new();
    assert_eq!(
        stalled.read_to_end(&mut rest).unwrap(),
        0,
        "closed, nothing written"
    );
    let reply = frame::call(&mut connect(server.address), &request("next")).unwrap();
    assert_eq!(reply, Err(RequestError::Tools));
    server.stop();
}

/// DEC-2265 item 2, E21-62: a connection past the cap is closed before any of it is read, and the
/// gateway answers nothing for it; the cap counts open connections, so a call fits once one ends.
#[test]
fn a_connection_past_the_cap_is_closed_unread() {
    let server = running(limits(1_500, 1), Err(RequestError::Tools));
    let holding = connect(server.address);
    thread::sleep(Duration::from_millis(100));
    let refused = frame::call(&mut connect(server.address), &request("past the cap"));
    assert!(
        matches!(refused, Err(FrameError::Truncated | FrameError::Io(_))),
        "{refused:?}"
    );
    assert!(server.lines.wait_for("connection refused: at_capacity"));
    assert!(server.answered.try_recv().is_err(), "nothing answered");
    drop(holding);
    assert!(server.lines.wait_for("request refused: truncated"));
    assert_eq!(
        fits_once_freed(server.address, "fits"),
        Some(Err(RequestError::Tools)),
        "a call fits once the held connection's slot is given back"
    );
    server.stop();
}

/// The first call that fits, within two seconds: the server logs a connection's refusal while the
/// connection's thread still holds its slot, and gives the slot back only when that thread ends,
/// so a call right after the log line may still be over the cap and closed unread.
fn fits_once_freed(address: SocketAddr, id: &str) -> Option<Reply> {
    (0..100).find_map(|_| match frame::call(&mut connect(address), &request(id)) {
        Ok(reply) => Some(reply),
        Err(_) => {
            thread::sleep(Duration::from_millis(20));
            None
        }
    })
}

/// DEC-2265 item 2: a length past the request bound is refused unread, and a body that is not a
/// request is refused; neither reaches the gateway, and the connection closes with nothing written.
#[test]
fn a_frame_past_its_bound_or_malformed_is_never_answered() {
    let server = running(limits(2_000, 4), Err(RequestError::Tools));
    let mut long = connect(server.address);
    long.write_all(&(MAX_REQUEST_FRAME + 1).to_be_bytes())
        .unwrap();
    let mut rest = Vec::new();
    assert_eq!(long.read_to_end(&mut rest).unwrap(), 0);
    assert!(server.lines.wait_for("request refused: too_long"));

    let mut malformed = connect(server.address);
    frame::write_frame(
        &mut malformed,
        b"{\"not\":\"a request\"}",
        MAX_REQUEST_FRAME,
    )
    .unwrap();
    assert_eq!(malformed.read_to_end(&mut rest).unwrap(), 0);
    assert!(server.lines.wait_for("request refused: malformed"));

    assert!(server.answered.try_recv().is_err(), "nothing answered");
    server.stop();
}

/// DEC-2265 item 9: a marker sent in a request's inputs and observations, in a malformed frame, and
/// in a frame cut short shows in none of the service's log lines, while the log does record each
/// exchange by its code.
#[test]
fn the_canary_shows_in_no_log_line() {
    let server = running(limits(500, 4), Err(RequestError::Transcript));
    assert_eq!(
        frame::call(&mut connect(server.address), &request(CANARY)).unwrap(),
        Err(RequestError::Transcript)
    );
    server.answered.recv().unwrap();

    let mut malformed = connect(server.address);
    frame::write_frame(&mut malformed, CANARY.as_bytes(), MAX_REQUEST_FRAME).unwrap();
    let mut rest = Vec::new();
    malformed.read_to_end(&mut rest).unwrap();

    let mut cut = connect(server.address);
    let body = request_bytes(&request(CANARY));
    cut.write_all(&u32::try_from(body.len()).unwrap().to_be_bytes())
        .unwrap();
    cut.write_all(&body[..body.len() / 2]).unwrap();
    drop(cut);

    assert!(server.lines.wait_for("request refused: truncated"));
    let lines = server.stop();
    assert!(lines.contains(&"call refused: transcript".to_owned()));
    assert!(lines.contains(&"request refused: malformed".to_owned()));
    for line in &lines {
        assert!(!line.contains(CANARY), "the canary leaked into {line:?}");
        assert!(
            !line.contains("CANARY"),
            "part of the canary leaked into {line:?}"
        );
    }
}

/// E21-62: stopping the server ends its loop, and a connection after the stop is not served.
#[test]
fn stopping_ends_the_loop() {
    let server = running(limits(500, 4), Err(RequestError::Tools));
    let address = server.address;
    server.stop();
    let after = TcpStream::connect(address)
        .map_err(|_| FrameError::Truncated)
        .and_then(|mut connection| frame::call(&mut connection, &request("after")));
    assert!(after.is_err(), "{after:?}");
}

/// The coordinator's guard on #2086: an `answer` that panics stops the listener as the panic
/// unwinds, so the call in flight reads as lost and the port refuses every connection after it,
/// rather than accepting connections that nothing will answer.
#[test]
fn a_panicking_answer_stops_the_listener() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = Server::new(
        listener,
        limits(2_000, 4),
        Arc::new(Lines::default()) as Arc<dyn Log>,
    );
    let (end, ended) = mpsc::channel();
    thread::spawn(move || {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            server.run(|_request| panic!("the gateway failed"))
        }));
        end.send(outcome.is_err()).unwrap();
    });
    let lost = frame::call(&mut connect(address), &request("in flight"));
    assert!(lost.is_err(), "{lost:?}");
    assert_eq!(
        ended.recv_timeout(Duration::from_secs(5)),
        Ok(true),
        "the panic ended the loop"
    );
    let refused = (0..200).any(|_| {
        thread::sleep(Duration::from_millis(10));
        TcpStream::connect(address).is_err()
    });
    assert!(refused, "the listener still accepts after the panic");
}
