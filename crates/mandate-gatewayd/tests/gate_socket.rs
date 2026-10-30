//! The gate socket and the serve loop's side channel (DEC-2294): a gate line reaches the thread that
//! holds the gateway and its answer comes back whole; one gate runs at a time; a malformed line is
//! refused before it reaches that thread; and a stopped server answers no side line and still
//! returns. Real Unix and loopback sockets, no provider.

use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use mandate_gateway::wire::{PutRefusal, PutReply, PutRequest, Reply};
use mandate_gateway::{CallRequest, RequestError};
use mandate_gatewayd::gate_socket::{self, SUFFIX};
use mandate_gatewayd::serve::{Handler, Limits, Log, SIDE_UNSERVED, Server};

struct Quiet;

impl Log for Quiet {
    fn line(&self, _: &str) {}
}

/// Answers each side line after `delay`, naming the line, and records what it was asked.
struct Slow {
    delay: Duration,
    asked: Arc<Mutex<Vec<String>>>,
}

impl Handler for Slow {
    fn call(&mut self, _: CallRequest) -> Reply {
        Err(RequestError::UnknownModel)
    }

    fn put(&mut self, _: &PutRequest) -> PutReply {
        Err(PutRefusal::StoreUnavailable)
    }

    fn side(&mut self, line: &str) -> String {
        self.asked.lock().unwrap().push(line.to_owned());
        thread::sleep(self.delay);
        format!("answered {}", line.replace(' ', "_"))
    }
}

fn server() -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    Server::new(
        listener,
        Limits {
            io_timeout: Duration::from_secs(5),
            max_connections: 4,
        },
        Arc::new(Quiet),
    )
}

fn socket_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gatewayd-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A side line is answered on the serving thread, by the handler, and comes back whole; once the
/// server stops, a side line is answered by nothing and the loop still returns.
#[test]
fn a_side_line_is_answered_by_the_serving_thread_until_the_server_stops() {
    let server = server();
    let side = server.side();
    let stopper = server.stopper().unwrap();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut handler = Slow {
        delay: Duration::ZERO,
        asked: Arc::clone(&asked),
    };
    let (done, ended) = mpsc::channel();
    let serving = thread::spawn(move || {
        server.run_handling(&mut handler);
        done.send(()).unwrap();
    });
    assert_eq!(
        side.ask("gate llm.chat_sonnet").as_deref(),
        Some("answered gate_llm.chat_sonnet")
    );
    assert_eq!(*asked.lock().unwrap(), ["gate llm.chat_sonnet"]);
    stopper.stop();
    assert_eq!(ended.recv_timeout(Duration::from_secs(5)), Ok(()));
    serving.join().unwrap();
    assert_eq!(side.ask("gate llm.chat_sonnet"), None);
}

/// A server run with closures alone serves no side line.
#[test]
fn a_server_without_a_side_handler_refuses_side_lines() {
    let server = server();
    let side = server.side();
    let stopper = server.stopper().unwrap();
    let serving = thread::spawn(move || {
        server.run_storing(
            |_| Err(RequestError::UnknownModel),
            |_| Err(PutRefusal::StoreUnavailable),
        );
    });
    assert_eq!(side.ask("gate x").as_deref(), Some(SIDE_UNSERVED));
    stopper.stop();
    serving.join().unwrap();
}

/// The gate socket forwards a well-formed line and prints its answer; a second request while a
/// gate runs is refused `gate_running` at once, never queued; a malformed line is refused
/// `malformed` and never reaches the serving thread; the socket is its owner's alone.
#[test]
fn the_gate_socket_runs_one_gate_at_a_time_and_refuses_malformed_lines() {
    use std::os::unix::fs::PermissionsExt;
    let server = server();
    let side = server.side();
    let stopper = server.stopper().unwrap();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut handler = Slow {
        delay: Duration::from_millis(600),
        asked: Arc::clone(&asked),
    };
    let serving = thread::spawn(move || server.run_handling(&mut handler));
    let probe = socket_dir("gate").join("probe.sock");
    let path = gate_socket::path(&probe);
    assert!(path.to_string_lossy().ends_with(SUFFIX));
    gate_socket::serve(&path, side.clone()).unwrap();
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let first = {
        let path = path.clone();
        thread::spawn(move || gate_socket::ask(&path, "llm.chat_sonnet", Duration::from_secs(5)))
    };
    thread::sleep(Duration::from_millis(150));
    assert_eq!(
        gate_socket::ask(&path, "llm.chat_haiku", Duration::from_secs(5)).unwrap(),
        "refused gate_running"
    );
    assert_eq!(
        first.join().unwrap().unwrap(),
        "answered gate_llm.chat_sonnet"
    );
    assert_eq!(
        gate_socket::ask(&path, "LLM bad", Duration::from_secs(5)).unwrap(),
        "refused malformed"
    );
    assert_eq!(
        gate_socket::ask(&path, "llm.chat_haiku", Duration::from_secs(5)).unwrap(),
        "answered gate_llm.chat_haiku",
        "the next gate runs once the first ended"
    );
    assert_eq!(
        *asked.lock().unwrap(),
        ["gate llm.chat_sonnet", "gate llm.chat_haiku"]
    );
    gate_socket::serve(&path, side).unwrap();
    stopper.stop();
    serving.join().unwrap();
}

/// A file at the gate socket's path that is not a socket is never replaced.
#[test]
fn a_file_at_the_gate_path_is_refused() {
    let path = socket_dir("gate-file").join("probe.sock.gate");
    std::fs::write(&path, b"not a socket").unwrap();
    let server = server();
    assert_eq!(
        gate_socket::serve(&path, server.side()).unwrap_err().kind(),
        std::io::ErrorKind::AlreadyExists
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"not a socket");
}
