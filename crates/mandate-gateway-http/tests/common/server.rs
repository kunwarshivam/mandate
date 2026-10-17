//! A local HTTPS server for the transport's tests: a certificate authority and a `localhost`
//! certificate generated at run time (DEC-2200 item 11, so no key is ever committed), and a server
//! that answers each connection with the next scripted reply and records what it was sent.

use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, Issuer, KeyPair};
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};

/// A test certificate authority and a server configuration whose certificate it signed.
pub struct Pki {
    pub ca_der: Vec<u8>,
    config: Arc<ServerConfig>,
}

/// A fresh authority and a certificate for `localhost` under it.
pub fn pki() -> Pki {
    let _already_installed = rustls::crypto::ring::default_provider().install_default();
    let ca_key = KeyPair::generate().unwrap();
    let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "mandate gateway test authority");
    let ca = ca_params.self_signed(&ca_key).unwrap();
    let issuer = Issuer::new(ca_params, ca_key);
    let server_key = KeyPair::generate().unwrap();
    let server = CertificateParams::new(vec!["localhost".to_owned()])
        .unwrap()
        .signed_by(&server_key, &issuer)
        .unwrap();
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(server_key.serialize_der()));
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![server.der().clone()], key)
        .unwrap();
    Pki {
        ca_der: ca.der().to_vec(),
        config: Arc::new(config),
    }
}

/// What the server does with one connection.
#[derive(Debug, Clone)]
pub enum Reply {
    /// Waits `delay_ms`, then answers in full and closes.
    Answer {
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
        delay_ms: u64,
    },
    /// Sends `body` with no declared length, its end marked by closing the connection.
    Unsized(Vec<u8>),
    /// Answers `body` without `Connection: close`, keeps the connection open `linger_ms`, reads
    /// nothing more, and closes it: a reused connection would fail after its second request.
    KeepAlive { body: Vec<u8>, linger_ms: u64 },
    /// Declares `declared` body bytes, sends `body` (fewer), and drops the connection.
    CutShort {
        status: u16,
        declared: usize,
        body: Vec<u8>,
    },
}

impl Reply {
    pub fn ok(body: &[u8]) -> Self {
        Self::status(200, body)
    }

    pub fn status(status: u16, body: &[u8]) -> Self {
        Self::Answer {
            status,
            headers: vec![],
            body: body.to_vec(),
            delay_ms: 0,
        }
    }

    pub fn with_header(status: u16, name: &str, value: &str) -> Self {
        Self::Answer {
            status,
            headers: vec![(name.to_owned(), value.to_owned())],
            body: b"{}".to_vec(),
            delay_ms: 0,
        }
    }

    pub fn slow(delay_ms: u64) -> Self {
        Self::Answer {
            status: 200,
            headers: vec![],
            body: b"{}".to_vec(),
            delay_ms,
        }
    }
}

/// One request as the server read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Seen {
    /// The value of header `name` (lowercase), if sent once.
    pub fn header(&self, name: &str) -> Option<&str> {
        let mut values = self.headers.iter().filter(|(n, _)| n == name);
        let first = values.next().map(|(_, v)| v.as_str());
        assert!(values.next().is_none(), "header {name} sent twice");
        first
    }
}

/// A running server: its port and what it has been sent so far.
pub struct Server {
    pub port: u16,
    seen: Arc<Mutex<Vec<Seen>>>,
    handshakes: Arc<Mutex<usize>>,
}

impl Server {
    /// The requests read so far, in arrival order.
    pub fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }

    /// The TLS handshakes that completed.
    pub fn handshakes(&self) -> usize {
        *self.handshakes.lock().unwrap()
    }
}

/// Serves `replies`, one connection each, on a fresh loopback port.
pub fn serve(pki: &Pki, replies: Vec<Reply>) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let handshakes = Arc::new(Mutex::new(0));
    let config = Arc::clone(&pki.config);
    let (seen_by_server, handshakes_by_server) = (Arc::clone(&seen), Arc::clone(&handshakes));
    std::thread::spawn(move || {
        for reply in replies {
            let Ok((tcp, _)) = listener.accept() else {
                return;
            };
            handle(
                tcp,
                Arc::clone(&config),
                &reply,
                &seen_by_server,
                &handshakes_by_server,
            );
        }
    });
    Server {
        port,
        seen,
        handshakes,
    }
}

/// A loopback port with nothing listening on it.
pub fn closed_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn handle(
    tcp: TcpStream,
    config: Arc<ServerConfig>,
    reply: &Reply,
    seen: &Mutex<Vec<Seen>>,
    handshakes: &Mutex<usize>,
) {
    tcp.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut tls = StreamOwned::new(ServerConnection::new(config).unwrap(), tcp);
    while tls.conn.is_handshaking() {
        if tls.conn.complete_io(&mut tls.sock).is_err() {
            return;
        }
    }
    *handshakes.lock().unwrap() += 1;
    let Some(request) = read_request(&mut tls) else {
        return;
    };
    seen.lock().unwrap().push(request);
    match reply {
        Reply::Answer {
            status,
            headers,
            body,
            delay_ms,
        } => {
            std::thread::sleep(Duration::from_millis(*delay_ms));
            let _ = tls.write_all(&head(*status, headers, Some(body.len()), true));
            let _ = tls.write_all(body);
            tls.conn.send_close_notify();
            let _ = tls.flush();
        }
        Reply::Unsized(body) => {
            let _ = tls.write_all(&head(200, &[], None, true));
            let _ = tls.write_all(body);
            tls.conn.send_close_notify();
            let _ = tls.flush();
        }
        Reply::KeepAlive { body, linger_ms } => {
            let _ = tls.write_all(&head(200, &[], Some(body.len()), false));
            let _ = tls.write_all(body);
            let _ = tls.flush();
            std::thread::sleep(Duration::from_millis(*linger_ms));
            tls.conn.send_close_notify();
            let _ = tls.flush();
        }
        Reply::CutShort {
            status,
            declared,
            body,
        } => {
            let _ = tls.write_all(&head(*status, &[], Some(*declared), true));
            let _ = tls.write_all(body);
            let _ = tls.flush();
            let _ = tls.sock.shutdown(std::net::Shutdown::Both);
        }
    }
}

fn head(status: u16, headers: &[(String, String)], length: Option<usize>, close: bool) -> Vec<u8> {
    let mut head = format!("HTTP/1.1 {status} Scripted\r\n");
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if let Some(length) = length {
        head.push_str(&format!("Content-Length: {length}\r\n"));
    }
    if close {
        head.push_str("Connection: close\r\n");
    }
    head.push_str("\r\n");
    head.into_bytes()
}

fn read_request(tls: &mut StreamOwned<ServerConnection, TcpStream>) -> Option<Seen> {
    let mut raw = Vec::new();
    let mut buf = [0u8; 4096];
    let header_end = loop {
        if let Some(at) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break at + 4;
        }
        match tls.read(&mut buf) {
            Ok(0) => return None,
            Ok(n) => raw.extend_from_slice(&buf[..n]),
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    };
    let text = String::from_utf8(raw[..header_end].to_vec()).ok()?;
    let mut lines = text.split("\r\n");
    let mut request_line = lines.next()?.split(' ');
    let method = request_line.next()?.to_owned();
    let path = request_line.next()?.to_owned();
    let headers: Vec<(String, String)> = lines
        .filter(|line| !line.is_empty())
        .filter_map(|line| line.split_once(':'))
        .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    let length: usize = headers
        .iter()
        .find(|(n, _)| n == "content-length")
        .map_or(0, |(_, v)| v.parse().unwrap());
    let mut body = raw[header_end..].to_vec();
    while body.len() < length {
        match tls.read(&mut buf) {
            Ok(0) => return None,
            Ok(n) => body.extend_from_slice(&buf[..n]),
            Err(_) => return None,
        }
    }
    Some(Seen {
        method,
        path,
        headers,
        body,
    })
}
