//! A JSON-RPC client over plain HTTP/1.1 that sends what `mandate-mcp`'s transport sends: the
//! same `accept`, `content-type`, `mcp-protocol-version` and `mcp-session-id` headers, and the
//! same handshake. `mandate-mcp`'s own client dials plain `http` only in its own test build, so
//! these tests cannot use it (DEC-849 item 2). It reads `application/json` answers only, the one
//! form the simulator writes.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use mandate_mcp::PROTOCOL_VERSION;
use serde_json::{Value, json};

pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Reply {
    pub fn header(&self, name: &str) -> Option<&str> {
        let found = self.headers.iter().find(|(n, _)| n == name);
        found.map(|(_, value)| value.as_str())
    }

    /// The `result` of a successful exchange; a JSON-RPC error fails the test.
    pub fn result(&self) -> Value {
        assert_eq!(self.status, 200, "{}", self.body);
        assert_eq!(self.header("content-type"), Some("application/json"));
        let message: Value = serde_json::from_str(&self.body).unwrap();
        assert_eq!(message.get("error"), None, "{message}");
        message["result"].clone()
    }
}

pub struct Wire {
    authority: String,
    pub session: Option<String>,
    next_id: u64,
}

impl Wire {
    pub fn new(url: &str) -> Self {
        let rest = url.strip_prefix("http://").unwrap();
        let authority = rest.strip_suffix("/mcp").unwrap().to_owned();
        Self {
            authority,
            session: None,
            next_id: 1,
        }
    }

    /// `initialize` then `notifications/initialized`, as `McpClient::connect` begins.
    pub fn connect(url: &str) -> Self {
        let mut wire = Self::new(url);
        let params = json!({"protocolVersion": PROTOCOL_VERSION, "capabilities": {},
            "clientInfo": {"name": "mandate-mcp", "version": "0.0.0"}});
        let reply = wire.request("initialize", params);
        assert_eq!(reply.result()["protocolVersion"], PROTOCOL_VERSION);
        wire.session = Some(reply.header("mcp-session-id").unwrap().to_owned());
        let note = json!({"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}});
        assert_eq!(wire.post(&note).status, 202);
        wire
    }

    pub fn request(&mut self, method: &str, params: Value) -> Reply {
        let id = self.next_id;
        self.next_id += 1;
        self.post(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
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
        let body = body.to_string();
        let mut head = format!(
            "POST /mcp HTTP/1.1\r\nhost: {}\r\nconnection: close\r\n\
             accept: application/json, text/event-stream\r\ncontent-type: application/json\r\n\
             mcp-protocol-version: {PROTOCOL_VERSION}\r\ncontent-length: {}\r\n",
            self.authority,
            body.len()
        );
        if let Some(session) = &self.session {
            head.push_str(&format!("mcp-session-id: {session}\r\n"));
        }
        let mut stream = TcpStream::connect(&self.authority).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        stream
            .write_all(format!("{head}\r\n{body}").as_bytes())
            .unwrap();
        let mut answer = String::new();
        stream.read_to_string(&mut answer).unwrap();
        let (head, body) = answer.split_once("\r\n\r\n").unwrap();
        let mut lines = head.lines();
        let status = lines.next().unwrap().split(' ').nth(1).unwrap();
        let headers = lines
            .filter_map(|l| l.split_once(':'))
            .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_owned()))
            .collect();
        Reply {
            status: status.parse().unwrap(),
            headers,
            body: body.to_owned(),
        }
    }
}
