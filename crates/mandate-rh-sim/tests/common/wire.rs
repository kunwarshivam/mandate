//! A JSON-RPC client over plain HTTP/1.1 that sends what `mandate-mcp`'s transport sends: the
//! same `accept`, `content-type`, `mcp-protocol-version` and `mcp-session-id` headers, and the
//! same handshake. `mandate-mcp`'s own client dials plain `http` only in its own test build, so
//! these tests cannot use it (DEC-849 item 2). It reads `application/json` answers only, the one
//! form the simulator writes.

use std::io::{ErrorKind, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use mandate_mcp::PROTOCOL_VERSION;
use serde_json::{Value, json};

pub struct Reply {
    /// The status line as it arrived, its reason phrase included.
    pub line: String,
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Reply {
    pub fn header(&self, name: &str) -> Option<&str> {
        let found = self.headers.iter().find(|(n, _)| n == name);
        found.map(|(_, value)| value.as_str())
    }

    /// The JSON-RPC message the answer carries.
    pub fn message(&self) -> Value {
        assert_eq!(self.header("content-type"), Some("application/json"));
        serde_json::from_str(&self.body).unwrap()
    }

    /// The `result` of a successful exchange; a JSON-RPC error fails the test.
    pub fn result(&self) -> Value {
        assert_eq!(self.status, 200, "{}", self.body);
        let message = self.message();
        assert_eq!(message.get("error"), None, "{message}");
        message["result"].clone()
    }
}

pub struct Wire {
    authority: String,
    pub session: Option<String>,
    /// The `mcp-protocol-version` header, which a test may set to another revision.
    pub protocol: String,
    /// The access token sent as `authorization: Bearer`, for a rehearsal server (DEC-922 item 3).
    pub bearer: Option<String>,
    next_id: u64,
}

impl Wire {
    pub fn new(url: &str) -> Self {
        let rest = url.strip_prefix("http://").unwrap();
        let authority = rest.strip_suffix("/mcp").unwrap().to_owned();
        Self {
            authority,
            session: None,
            protocol: PROTOCOL_VERSION.to_owned(),
            bearer: None,
            next_id: 1,
        }
    }

    /// [`Wire::connect`] with `token` sent as the Bearer on every request.
    pub fn connect_with(url: &str, token: &str) -> Self {
        let mut wire = Self::new(url);
        wire.bearer = Some(token.to_owned());
        wire.handshake();
        wire
    }

    /// `initialize` then `notifications/initialized`, as `McpClient::connect` begins.
    pub fn connect(url: &str) -> Self {
        let mut wire = Self::new(url);
        wire.handshake();
        wire
    }

    fn handshake(&mut self) {
        let wire = self;
        let params = json!({"protocolVersion": PROTOCOL_VERSION, "capabilities": {},
            "clientInfo": {"name": "mandate-mcp", "version": "0.0.0"}});
        let reply = wire.request("initialize", params);
        assert_eq!(reply.result()["protocolVersion"], PROTOCOL_VERSION);
        wire.session = Some(reply.header("mcp-session-id").unwrap().to_owned());
        let note = json!({"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}});
        assert_eq!(wire.post(&note).status, 202);
    }

    pub fn request(&mut self, method: &str, params: Value) -> Reply {
        let id = self.next_id;
        self.next_id += 1;
        self.post(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
    }

    /// `tools/call`'s result; the core's refusal is in it, as `isError`.
    pub fn call(&mut self, tool: &str, arguments: Value) -> Value {
        let params = json!({"name": tool, "arguments": arguments});
        self.request("tools/call", params).result()
    }

    /// `tools/list` followed through every `nextCursor`.
    pub fn list(&mut self) -> Vec<Value> {
        let mut tools = Vec::new();
        let mut params = json!({});
        loop {
            let page = self.request("tools/list", params).result();
            tools.extend(page["tools"].as_array().unwrap().iter().cloned());
            match page.get("nextCursor") {
                Some(cursor) => params = json!({ "cursor": cursor }),
                None => return tools,
            }
        }
    }

    pub fn post(&mut self, body: &Value) -> Reply {
        self.try_post(body)
            .expect("the server closed the connection without an answer")
    }

    /// `tools/call` as sent, and the answer as it came, or `None` if none came.
    pub fn call_raw(&mut self, tool: &str, arguments: Value) -> Option<Reply> {
        let id = self.next_id;
        self.next_id += 1;
        let params = json!({"name": tool, "arguments": arguments});
        self.try_post(
            &json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": params}),
        )
    }

    /// One exchange; `None` when the connection closes or resets with no byte of an answer. A
    /// server that holds the connection open past the read timeout fails the test instead.
    pub fn try_post(&mut self, body: &Value) -> Option<Reply> {
        let body = body.to_string();
        let mut head = format!(
            "POST /mcp HTTP/1.1\r\nhost: {}\r\nconnection: close\r\nuser-agent: mandate-mcp/0.0.0\r\n\
             accept: application/json, text/event-stream\r\ncontent-type: application/json\r\n\
             mcp-protocol-version: {}\r\ncontent-length: {}\r\n",
            self.authority,
            self.protocol,
            body.len()
        );
        if let Some(session) = &self.session {
            head.push_str(&format!("mcp-session-id: {session}\r\n"));
        }
        if let Some(token) = &self.bearer {
            head.push_str(&format!("authorization: Bearer {token}\r\n"));
        }
        exchange(&self.authority, &format!("{head}\r\n{body}"))
    }
}

/// One plain HTTP/1.1 request to `url`'s authority, `method` and the URL's path and query as its
/// target, with `content_type` when there is a body: the browser's and the login's requests to a
/// rehearsal server.
pub fn http(method: &str, url: &str, content_type: Option<&str>, body: &str) -> Reply {
    let rest = url.strip_prefix("http://").unwrap();
    let (authority, target) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let target = if target.is_empty() { "/" } else { target };
    let mut head = format!(
        "{method} {target} HTTP/1.1\r\nhost: {authority}\r\nconnection: close\r\n\
         content-length: {}\r\n",
        body.len()
    );
    if let Some(content_type) = content_type {
        head.push_str(&format!("content-type: {content_type}\r\n"));
    }
    exchange(authority, &format!("{head}\r\n{body}"))
        .expect("the server closed the connection without an answer")
}

/// Writes `request` and reads the whole answer; `None` when the connection closes or resets with
/// no byte of an answer.
fn exchange(authority: &str, request: &str) -> Option<Reply> {
    let mut stream = TcpStream::connect(authority).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut answer = String::new();
    match stream.read_to_string(&mut answer) {
        Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
            panic!("neither an answer nor a close within the read timeout")
        }
        Err(_) => return None,
        Ok(_) if answer.is_empty() => return None,
        Ok(_) => {}
    }
    let (head, body) = answer.split_once("\r\n\r\n").unwrap();
    let mut lines = head.lines();
    let line = lines.next().unwrap().to_owned();
    let status = line.split(' ').nth(1).unwrap().to_owned();
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    Some(Reply {
        line,
        status: status.parse().unwrap(),
        headers,
        body: body.to_owned(),
    })
}
