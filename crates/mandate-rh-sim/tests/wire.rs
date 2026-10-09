//! A live guard on the mirrored client (DEC-849 item 2): against a listener written here, it must
//! send exactly what `McpTransport::post` and `McpClient::connect` send, so a drift between the
//! two fails on every run, not only once the server lands.

mod common;

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread::{self, JoinHandle};

use common::wire::Wire;
use mandate_mcp::PROTOCOL_VERSION;
use serde_json::{Value, json};

/// Answers `initialize` with session `s-1`, then the notification with `202`, one connection each,
/// and hands back every request it read.
fn record() -> (String, JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let result = json!({"jsonrpc": "2.0", "id": 1, "result": {"protocolVersion": PROTOCOL_VERSION,
        "capabilities": {}, "serverInfo": {"name": "t", "version": "1"}}})
    .to_string();
    let answers = [
        format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nmcp-session-id: s-1\r\n\
             content-length: {}\r\n\r\n{result}",
            result.len()
        ),
        "HTTP/1.1 202 Accepted\r\ncontent-length: 0\r\n\r\n".to_owned(),
    ];
    let seen = thread::spawn(move || {
        let mut seen = Vec::new();
        for answer in answers {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = String::new();
            let mut buf = [0u8; 4096];
            while !complete(&request) {
                let n = stream.read(&mut buf).unwrap();
                assert_ne!(n, 0, "the request ended early: {request}");
                request.push_str(std::str::from_utf8(&buf[..n]).unwrap());
            }
            stream.write_all(answer.as_bytes()).unwrap();
            seen.push(request);
        }
        seen
    });
    (url, seen)
}

/// Whether `request` holds its head and as many body bytes as its `content-length` says.
fn complete(request: &str) -> bool {
    request.split_once("\r\n\r\n").is_some_and(|(head, body)| {
        let length = head
            .lines()
            .find_map(|l| l.strip_prefix("content-length: "));
        body.len() >= length.unwrap().parse().unwrap()
    })
}

#[test]
fn the_wire_client_sends_the_transports_headers_and_handshake() {
    let (url, seen) = record();
    let wire = Wire::connect(&url);
    assert_eq!(wire.session.as_deref(), Some("s-1"));
    let seen = seen.join().unwrap();
    assert_eq!(seen.len(), 2);
    let authority = url
        .strip_prefix("http://")
        .unwrap()
        .strip_suffix("/mcp")
        .unwrap();
    let mut bodies = Vec::new();
    for (request, session) in seen.iter().zip([None, Some("s-1")]) {
        let (head, body) = request.split_once("\r\n\r\n").unwrap();
        let mut lines = head.lines();
        assert_eq!(lines.next(), Some("POST /mcp HTTP/1.1"));
        let headers: BTreeMap<String, &str> = lines
            .map(|l| l.split_once(": ").unwrap())
            .map(|(name, value)| (name.to_ascii_lowercase(), value))
            .collect();
        let expected = [
            ("accept", "application/json, text/event-stream"),
            ("content-type", "application/json"),
            ("mcp-protocol-version", PROTOCOL_VERSION),
            ("host", authority),
            ("user-agent", "mandate-mcp/0.0.0"),
            ("connection", "close"),
        ];
        for (name, value) in expected {
            assert_eq!(headers.get(name), Some(&value), "{name}");
        }
        let extra = usize::from(session.is_some());
        assert_eq!(
            headers.len(),
            expected.len() + 1 + extra,
            "no other header: {headers:?}"
        );
        assert_eq!(
            headers.get("content-length"),
            Some(&body.len().to_string().as_str())
        );
        assert_eq!(headers.get("mcp-session-id").copied(), session);
        bodies.push(serde_json::from_str::<Value>(body).unwrap());
    }
    let initialize = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": PROTOCOL_VERSION, "capabilities": {},
        "clientInfo": {"name": "mandate-mcp", "version": "0.0.0"}}});
    let initialized =
        json!({"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}});
    assert_eq!(bodies, [initialize, initialized]);
}
