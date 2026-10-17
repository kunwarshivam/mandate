//! Liveness and readiness on loopback, with fixed generic words only (E21-2, DEC-968 item 7;
//! `AGENTS.md` rule 6).

use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use mandate_agentd::health::{answer, serve};
use mandate_agentd::{Phase, Stop};

const PHASES: [Phase; 8] = [
    Phase::Starting,
    Phase::Ready,
    Phase::Draining,
    Phase::Stopped(Stop::Drained),
    Phase::Stopped(Stop::Fenced),
    Phase::Stopped(Stop::Refused),
    Phase::Stopped(Stop::CycleFailed),
    Phase::Stopped(Stop::DrainIncomplete),
];

#[test]
fn only_a_ready_process_answers_ready_and_only_a_stopped_one_is_not_live() {
    for phase in PHASES {
        let stopped = matches!(phase, Phase::Stopped(_));
        let live = if stopped {
            (503, "stopped")
        } else {
            (200, "live")
        };
        let ready = if phase == Phase::Ready {
            (200, "ready")
        } else {
            (503, "not_ready")
        };
        assert_eq!(answer("/livez", phase), live, "{phase:?}");
        assert_eq!(answer("/readyz", phase), ready, "{phase:?}");
        assert_eq!(answer("/", phase), (404, "not_found"));
        assert_eq!(answer("/readyz/x", phase), (404, "not_found"));
    }
}

#[test]
fn health_refuses_to_listen_off_loopback() {
    for addr in ["0.0.0.0:0", "192.0.2.1:0"] {
        let addr: SocketAddr = addr.parse().unwrap_or_else(|_| unreachable!());
        let refused = serve(addr, mandate_agentd::Status::default());
        assert_eq!(
            refused.err().map(|e| e.kind()),
            Some(ErrorKind::InvalidInput)
        );
    }
}

fn get(addr: SocketAddr, request: &str) -> String {
    let mut stream = TcpStream::connect(addr).unwrap_or_else(|e| panic!("connect: {e}"));
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap_or_else(|e| panic!("timeout: {e}"));
    stream
        .write_all(request.as_bytes())
        .unwrap_or_else(|e| panic!("write: {e}"));
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .unwrap_or_else(|e| panic!("read: {e}"));
    response
}

#[test]
fn the_listener_answers_each_path_from_the_shared_status() {
    let status = mandate_agentd::Status::default();
    let addr = serve(SocketAddr::from(([127, 0, 0, 1], 0)), status.clone())
        .unwrap_or_else(|e| panic!("bind: {e}"));
    assert!(addr.ip().is_loopback());
    assert_ne!(addr.port(), 0);
    assert_eq!(
        get(addr, "GET /readyz HTTP/1.1\r\n\r\n"),
        "HTTP/1.1 503 Service Unavailable\r\nContent-Type: text/plain\r\nContent-Length: 9\r\nConnection: close\r\n\r\nnot_ready"
    );
    assert_eq!(
        get(addr, "GET /livez HTTP/1.1\r\n\r\n"),
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 4\r\nConnection: close\r\n\r\nlive"
    );
    assert_eq!(
        get(addr, "GET /metrics HTTP/1.1\r\n\r\n"),
        "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\nContent-Length: 9\r\nConnection: close\r\n\r\nnot_found"
    );
    assert_eq!(
        get(addr, "\r\n"),
        "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\nContent-Length: 9\r\nConnection: close\r\n\r\nnot_found"
    );
}

const NOT_FOUND: &str = "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\nContent-Length: 9\r\nConnection: close\r\n\r\nnot_found";

#[test]
fn a_silent_client_does_not_hold_the_listener_from_the_next_probe() {
    let addr = serve(
        SocketAddr::from(([127, 0, 0, 1], 0)),
        mandate_agentd::Status::default(),
    )
    .unwrap_or_else(|e| panic!("bind: {e}"));
    let _silent = TcpStream::connect(addr).unwrap_or_else(|e| panic!("connect: {e}"));
    assert_eq!(
        get(addr, "GET /livez HTTP/1.1\r\n\r\n"),
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 4\r\nConnection: close\r\n\r\nlive"
    );
}

#[test]
fn a_request_line_that_reaches_the_bound_without_ending_is_answered_at_the_bound() {
    let addr = serve(
        SocketAddr::from(([127, 0, 0, 1], 0)),
        mandate_agentd::Status::default(),
    )
    .unwrap_or_else(|e| panic!("bind: {e}"));
    let long = format!("GET /{}", "a".repeat(1019));
    assert_eq!(long.len(), 1024);
    assert_eq!(get(addr, &long), NOT_FOUND);
}
