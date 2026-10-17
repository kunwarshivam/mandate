//! A server on a background thread, and a blocking HTTP/1.1 client that sends raw requests, so
//! the tests see the wire bytes and need no HTTP client crate.

#![allow(dead_code, reason = "each test file uses part of the shared helpers")]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use axum::Router;

/// One answer, as the wire carried it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Reply {
    /// Every value of header `name`, matched case-insensitively.
    pub fn all(&self, name: &str) -> Vec<&str> {
        self.headers
            .iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
            .collect()
    }

    /// The one value of header `name`, or `None`.
    pub fn header(&self, name: &str) -> Option<&str> {
        let all = self.all(name);
        assert!(all.len() <= 1, "header {name} is repeated: {all:?}");
        all.first().copied()
    }

    /// Whether any header's name starts with `Access-Control-`.
    pub fn has_cors(&self) -> bool {
        self.headers
            .iter()
            .any(|(n, _)| n.to_ascii_lowercase().starts_with("access-control-"))
    }
}

/// Serves `router` on a loopback port from a thread of its own, for the rest of the test run.
pub fn serve(router: Router) -> SocketAddr {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime");
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("a loopback port");
            tx.send(listener.local_addr().expect("its address"))
                .expect("the test is waiting");
            axum::serve(listener, router).await.expect("serving");
        });
    });
    rx.recv_timeout(Duration::from_secs(10))
        .expect("the server started")
}

/// Sends `method path` with `headers` and `body` and reads the whole answer.
pub fn send(
    addr: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Reply {
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("a timeout");
    let mut request =
        format!("{method} {path} HTTP/1.1\r\nHost: api.test\r\nConnection: close\r\n");
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
    stream.write_all(request.as_bytes()).expect("write");
    stream.write_all(body).expect("write the body");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).expect("read");
    parse(&raw)
}

/// A GET with no headers.
pub fn get(addr: SocketAddr, path: &str) -> Reply {
    send(addr, "GET", path, &[], b"")
}

fn parse(raw: &[u8]) -> Reply {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("a header block");
    let head = std::str::from_utf8(&raw[..split]).expect("ASCII headers");
    let mut lines = head.split("\r\n");
    let status_line = lines.next().expect("a status line");
    let status = status_line
        .split(' ')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .expect("a status code");
    let headers: Vec<(String, String)> = lines
        .map(|line| {
            let (name, value) = line.split_once(':').expect("name: value");
            (name.trim().to_owned(), value.trim().to_owned())
        })
        .collect();
    let rest = raw[split + 4..].to_vec();
    let chunked = headers
        .iter()
        .any(|(n, v)| n.eq_ignore_ascii_case("transfer-encoding") && v.contains("chunked"));
    let body = if chunked { dechunk(&rest) } else { rest };
    Reply {
        status,
        headers,
        body,
    }
}

fn dechunk(mut raw: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    loop {
        let end = raw
            .windows(2)
            .position(|w| w == b"\r\n")
            .expect("a chunk size line");
        let size = usize::from_str_radix(std::str::from_utf8(&raw[..end]).expect("hex"), 16)
            .expect("a chunk size");
        if size == 0 {
            return body;
        }
        body.extend_from_slice(&raw[end + 2..end + 2 + size]);
        raw = &raw[end + 4 + size..];
    }
}
