//! A loopback server for a stub issuer, and a raw HTTP/1.1 client that sets its own `Host`.

#![allow(
    dead_code,
    reason = "each test binary uses a different part of the helpers"
)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use mandate_stub_issuer::issuer::Issuer;

/// An answer: its status, its header lines in lowercase, and its body.
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn has_cors(&self) -> bool {
        self.headers
            .iter()
            .any(|(n, _)| n.starts_with("access-control-"))
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).expect("a JSON body")
    }
}

/// Unix seconds now.
pub fn now_s() -> i64 {
    #[allow(
        clippy::disallowed_methods,
        reason = "a test reads the wall clock the stub signs at"
    )]
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("after 1970");
    i64::try_from(since.as_secs()).expect("in range")
}

/// A stub issuer for a fresh loopback port, served on that port, and the port's address.
pub fn serve(audience: &str) -> (Arc<Issuer>, SocketAddr) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let addr = listener.local_addr().expect("its address");
    let issuer = Arc::new(Issuer::new(addr, audience).expect("a key"));
    let served = Arc::clone(&issuer);
    listener.set_nonblocking(true).expect("non-blocking");
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime");
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).expect("a listener");
            mandate_stub_issuer::serve::serve(listener, served, std::future::pending())
                .await
                .expect("serving");
        });
    });
    (issuer, addr)
}

/// Sends `method path` to `addr` with `Host: host`, `headers`, and `body`, and reads the answer.
pub fn send(
    addr: SocketAddr,
    host: &str,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Reply {
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n");
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
    let mut bytes = request.into_bytes();
    bytes.extend_from_slice(body);
    raw(addr, &bytes)
}

/// Writes `request` to `addr` as it is and reads the answer.
pub fn raw(addr: SocketAddr, request: &[u8]) -> Reply {
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("a timeout");
    stream.write_all(request).expect("write");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).expect("read");
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("a header block");
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .and_then(|code| code.parse().ok())
        .expect("a status line");
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    Reply {
        status,
        headers,
        body: raw[split + 4..].to_vec(),
    }
}

/// `POST /token` from the stub's own host, as JSON.
pub fn mint(addr: SocketAddr, body: &str) -> Reply {
    send(
        addr,
        &addr.to_string(),
        "POST",
        "/token",
        &[("Content-Type", "application/json")],
        body.as_bytes(),
    )
}
