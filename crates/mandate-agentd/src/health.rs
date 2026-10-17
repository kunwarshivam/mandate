//! Liveness and readiness over loopback HTTP (infrastructure design §3.4). Every answer is one of
//! a few fixed words: no id, symbol, price, quantity, or mandate content (`AGENTS.md` rule 6).

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use mandate_process::Snapshot;
use mandate_process::probe::{self, Probe};
use mandate_time::UtcNanos;

use crate::supervisor::{Phase, Status};

/// The longest request line read, and how long a client may stay silent, so that one client
/// cannot hold the listener from the orchestrator's probes.
const LINE_BOUND: u64 = 1024;
const READ_BOUND: Duration = Duration::from_secs(1);

/// The status line and body for a request path in a phase, in the shared probe words (DEC-994
/// item 7). The phase carries no heartbeat bound yet (DEC-968 item 7).
///
/// - `/livez`: `200 live` until the process has stopped, then `503 stopped`.
/// - `/readyz`: `200 ready` only while it runs cycles, else `503 not_ready`; a draining process
///   is not ready.
/// - anything else: `404 not_found`.
pub fn answer(path: &str, phase: Phase) -> (u16, &'static str) {
    let probe = match path {
        "/livez" => Probe::Live,
        "/readyz" => Probe::Ready,
        _ => return (404, "not_found"),
    };
    let answer = probe::answer(probe, Snapshot::unbounded(phase.state()), UtcNanos::EPOCH);
    (if answer.ok { 200 } else { 503 }, answer.word)
}

/// Binds `addr`, which must be a loopback address, and answers from `status` on a thread of its
/// own until the process exits. Returns the bound address, so port 0 can be asked for.
///
/// # Errors
/// A non-loopback address, refused as `InvalidInput` before anything is bound, or the bind's own.
pub fn serve(addr: SocketAddr, status: Status) -> io::Result<SocketAddr> {
    if !addr.ip().is_loopback() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "health listens on loopback only",
        ));
    }
    let listener = TcpListener::bind(addr)?;
    let bound = listener.local_addr()?;
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let _answered = respond(stream, &status);
        }
    });
    Ok(bound)
}

fn respond(stream: TcpStream, status: &Status) -> io::Result<()> {
    stream.set_read_timeout(Some(READ_BOUND))?;
    let mut line = String::new();
    BufReader::new((&stream).take(LINE_BOUND)).read_line(&mut line)?;
    let path = line.split_whitespace().nth(1).unwrap_or("");
    let (code, body) = answer(path, status.phase());
    let reason = match code {
        200 => "OK",
        404 => "Not Found",
        _ => "Service Unavailable",
    };
    write!(
        &stream,
        "HTTP/1.1 {code} {reason}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}
