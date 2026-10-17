//! The pull exporter (E21-3): `GET /metrics` serves the scrape, anything else is `404`, a failed
//! bind is an error the caller carries on past, and a stalled scraper never reaches a record
//! (OPS-11).

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use mandate_telemetry::catalog::CYCLES;
use mandate_telemetry::{
    Labels, Metrics, ProcessType, Release, TelemetryError, answer, reason, serve,
};

fn metrics() -> Metrics {
    Metrics::new(ProcessType::AgentProcess, &Release::parse("0.0.0").unwrap()).unwrap()
}

fn loopback() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 0))
}

fn get(addr: SocketAddr, request: &str) -> String {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

fn completed() -> Labels {
    Labels::new()
        .process(ProcessType::AgentProcess)
        .reason(reason::COMPLETED)
}

#[test]
fn a_scrape_is_the_encoded_text() {
    let m = metrics();
    m.count(&CYCLES, &completed());
    let exporter = serve(loopback(), m.clone()).unwrap();
    let response = get(exporter.addr(), "GET /metrics HTTP/1.1\r\nHost: x\r\n\r\n");
    let body = m.encode().unwrap();
    assert_eq!(
        response,
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain; version=0.0.4; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    );
}

#[test]
fn any_other_request_is_not_found() {
    let exporter = serve(loopback(), metrics()).unwrap();
    for request in [
        "GET / HTTP/1.1\r\n\r\n",
        "POST /metrics HTTP/1.1\r\n\r\n",
        "GET /metrics/x HTTP/1.1\r\n\r\n",
        "\r\n",
    ] {
        assert_eq!(
            get(exporter.addr(), request),
            "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\nContent-Length: 9\r\nConnection: close\r\n\r\nnot_found",
            "{request:?}"
        );
    }
}

#[test]
fn metrics_that_are_off_answer_unavailable_with_fixed_text() {
    assert_eq!(
        answer("GET", "/metrics", &Metrics::off()),
        (503, "text/plain", "telemetry_unavailable".to_owned())
    );
    let exporter = serve(loopback(), Metrics::off()).unwrap();
    assert!(
        get(exporter.addr(), "GET /metrics HTTP/1.1\r\n\r\n")
            .starts_with("HTTP/1.1 503 Service Unavailable\r\n")
    );
}

#[test]
fn a_taken_address_is_a_start_error() {
    let taken = TcpListener::bind(loopback()).unwrap();
    let addr = taken.local_addr().unwrap();
    assert_eq!(
        serve(addr, metrics()),
        Err(TelemetryError::Start(std::io::ErrorKind::AddrInUse))
    );
}

#[test]
fn a_stalled_scraper_never_blocks_a_record_and_later_scrapes_are_served() {
    let m = metrics();
    let exporter = serve(loopback(), m.clone()).unwrap();
    let _silent = TcpStream::connect(exporter.addr()).unwrap();
    let mut half = TcpStream::connect(exporter.addr()).unwrap();
    half.write_all(b"GET /met").unwrap();
    let (done, recorded) = mpsc::channel();
    let recorder = m.clone();
    thread::spawn(move || {
        for _ in 0..10_000 {
            recorder.count(&CYCLES, &completed());
        }
        done.send(()).unwrap();
    });
    recorded
        .recv_timeout(Duration::from_millis(500))
        .expect("records finish while the exporter is held by stalled scrapers");
    let response = get(exporter.addr(), "GET /metrics HTTP/1.1\r\n\r\n");
    assert!(
        response.contains(
            r#"mandate_process_cycles_total{process_type="agent_process",reason_code="completed"} 10000"#
        ),
        "{response}"
    );
}
