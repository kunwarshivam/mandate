//! A scripted HTTP/1.1 server on a loopback port: one canned answer per connection, in order,
//! and a record of every request it read. No test reaches the network.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::{McpTransport, Monotonic, PinnedEndpoint, SystemMonotonic, TransportConfig};

/// A monotonic clock a test moves by hand: nanoseconds since its origin, shared by its clones.
#[derive(Debug, Clone, Default)]
pub(crate) struct StepClock(Arc<AtomicU64>);

impl StepClock {
    pub fn set(&self, at: Duration) {
        self.0
            .store(u64::try_from(at.as_nanos()).unwrap(), Ordering::SeqCst);
    }
}

impl Monotonic for StepClock {
    fn elapsed(&self) -> Duration {
        Duration::from_nanos(self.0.load(Ordering::SeqCst))
    }
}

pub(crate) struct Answer {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub body: String,
    pub delay: Duration,
    /// The body is the members after `"jsonrpc"` and an `"id"` copied from the request read.
    pub reply: bool,
}

pub(crate) fn json(body: &str) -> Answer {
    with_type(200, "application/json", body)
}

/// A JSON answer carrying the `id` of the request it answers, so no test assumes how the
/// transport numbers its requests: `members` follow `"jsonrpc"` and that `"id"`.
pub(crate) fn reply(members: &str) -> Answer {
    Answer {
        reply: true,
        ..json(members)
    }
}

pub(crate) fn sse(body: &str) -> Answer {
    with_type(200, "text/event-stream", body)
}

pub(crate) fn with_type(status: u16, media_type: &str, body: &str) -> Answer {
    Answer {
        status,
        headers: vec![("content-type", media_type.to_owned())],
        body: body.to_owned(),
        delay: Duration::ZERO,
        reply: false,
    }
}

pub(crate) struct Loopback {
    pub url: String,
    seen: Arc<Mutex<Vec<String>>>,
}

impl Loopback {
    /// Every request read so far, head and body, with header names lower case.
    pub fn seen(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn transport(&self, config: TransportConfig) -> McpTransport {
        let endpoint = PinnedEndpoint::new("127.0.0.1", &self.url).unwrap();
        McpTransport::new(endpoint, config, Box::new(SystemMonotonic::start())).unwrap()
    }

    pub fn transport_with_clock(&self, config: TransportConfig, clock: &StepClock) -> McpTransport {
        let endpoint = PinnedEndpoint::new("127.0.0.1", &self.url).unwrap();
        McpTransport::new(endpoint, config, Box::new(clock.clone())).unwrap()
    }
}

pub(crate) async fn serve(answers: Vec<Answer>) -> Loopback {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = Arc::clone(&seen);
    tokio::spawn(async move {
        for answer in answers {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_request(&mut stream).await;
            let body = if answer.reply {
                format!(
                    r#"{{"jsonrpc":"2.0","id":{},{}}}"#,
                    request_id(&request),
                    answer.body
                )
            } else {
                answer.body
            };
            record.lock().unwrap().push(request);
            tokio::time::sleep(answer.delay).await;
            let mut head = format!("HTTP/1.1 {} X\r\nconnection: close\r\n", answer.status);
            for (name, value) in &answer.headers {
                head.push_str(&format!("{name}: {value}\r\n"));
            }
            head.push_str(&format!("content-length: {}\r\n\r\n", body.len()));
            let _ = stream.write_all((head + &body).as_bytes()).await;
            let _ = stream.shutdown().await;
        }
    });
    Loopback { url, seen }
}

/// The `id` member of the JSON-RPC request in `request`, as JSON text.
pub(crate) fn request_id(request: &str) -> String {
    let (_, body) = request.split_once("\r\n\r\n").unwrap();
    serde_json::from_str::<serde_json::Value>(body).unwrap()["id"].to_string()
}

pub(crate) async fn read_request(stream: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = stream.read(&mut buf).await.unwrap();
        bytes.extend_from_slice(&buf[..n]);
        let text = String::from_utf8_lossy(&bytes).to_ascii_lowercase();
        if let Some(end) = text.find("\r\n\r\n") {
            let length = text
                .lines()
                .find_map(|l| l.strip_prefix("content-length: "))
                .map_or(0, |n| n.trim().parse::<usize>().unwrap());
            if bytes.len() >= end + 4 + length || n == 0 {
                let (head, body) = bytes.split_at(end + 4);
                let head = String::from_utf8_lossy(head).to_ascii_lowercase();
                return head + &String::from_utf8_lossy(body);
            }
        }
    }
}
