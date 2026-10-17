//! Liveness and readiness over loopback HTTP (infrastructure design §3.4). Every answer is one of
//! a few fixed words: no id, symbol, price, quantity, or mandate content (`AGENTS.md` rule 6).

use std::io::{self, BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread;

use crate::supervisor::{Phase, Status};

/// The status line and body for a request path in a phase.
///
/// - `/livez`: `200 live` until the process has stopped, then `503 stopped`.
/// - `/readyz`: `200 ready` only while it runs cycles, else `503 not_ready`; a draining process
///   is not ready.
/// - anything else: `404 not_found`.
pub fn answer(path: &str, phase: Phase) -> (u16, &'static str) {
    match (path, phase) {
        ("/livez", Phase::Stopped(_)) => (503, "stopped"),
        ("/livez", _) => (200, "live"),
        ("/readyz", Phase::Ready) => (200, "ready"),
        ("/readyz", _) => (503, "not_ready"),
        _ => (404, "not_found"),
    }
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
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line)?;
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
