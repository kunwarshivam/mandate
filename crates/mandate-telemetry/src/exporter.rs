//! The Prometheus pull endpoint: `GET /metrics` on the address the process names, answered on a
//! thread of its own. Nothing here is on a decision cycle's path: the cycle records, and only
//! this thread reads.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use crate::{Metrics, TelemetryError};

/// The longest request line read, and how long a scraper may stay silent or slow, so that one
/// stalled scraper delays later scrapes by at most this and never reaches a record.
const LINE_BOUND: u64 = 1024;
const IO_BOUND: Duration = Duration::from_secs(1);

/// The running exporter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exporter {
    addr: SocketAddr,
}

impl Exporter {
    /// The bound address, so port 0 can be asked for.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }
}

/// The status, content type, and body for a request line's method and path: `GET /metrics` is
/// `200` with the exposition text, or `503 telemetry_unavailable` when [`Metrics::encode`]
/// refuses; anything else is `404 not_found`.
pub fn answer(method: &str, path: &str, metrics: &Metrics) -> (u16, &'static str, String) {
    if (method, path) != ("GET", "/metrics") {
        return (404, "text/plain", "not_found".to_owned());
    }
    match metrics.encode() {
        Ok(text) => (200, "text/plain; version=0.0.4; charset=utf-8", text),
        Err(_) => (503, "text/plain", "telemetry_unavailable".to_owned()),
    }
}

/// Binds `addr` and serves `metrics` until the process exits.
///
/// # Errors
/// [`TelemetryError::Start`] with the bind's or the thread's own error kind. The caller carries
/// on without an exporter: nothing else depends on it (OPS-11).
pub fn serve(addr: SocketAddr, metrics: Metrics) -> Result<Exporter, TelemetryError> {
    let start = |e: std::io::Error| TelemetryError::Start(e.kind());
    let listener = TcpListener::bind(addr).map_err(start)?;
    let bound = listener.local_addr().map_err(start)?;
    thread::Builder::new()
        .name("metrics-exporter".to_owned())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let _answered = respond(&stream, &metrics);
            }
        })
        .map_err(start)?;
    Ok(Exporter { addr: bound })
}

fn respond(stream: &TcpStream, metrics: &Metrics) -> std::io::Result<()> {
    stream.set_read_timeout(Some(IO_BOUND))?;
    stream.set_write_timeout(Some(IO_BOUND))?;
    let mut line = String::new();
    BufReader::new(stream.take(LINE_BOUND)).read_line(&mut line)?;
    let mut words = line.split_whitespace();
    let method = words.next().unwrap_or("");
    let path = words.next().unwrap_or("");
    let (code, content_type, body) = answer(method, path, metrics);
    let reason = match code {
        200 => "OK",
        404 => "Not Found",
        _ => "Service Unavailable",
    };
    let mut writer = stream;
    write!(
        writer,
        "HTTP/1.1 {code} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    writer.flush()
}
