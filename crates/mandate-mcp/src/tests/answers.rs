//! The transport's answers from the loopback server: one JSON body or an event stream, and
//! every status, media type, size, or frame outside the protocol as a typed error.

use std::time::Duration;

use serde_json::json;

use super::server::{Answer, json, serve, sse, with_type};
use crate::{CallClass, Monotonic, SystemMonotonic, TransportConfig};

const READ: CallClass = CallClass::Ordinary;

#[tokio::test]
async fn a_json_answer_is_the_result_of_the_request_sent() {
    let server = serve(vec![json(
        r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}"#,
    )])
    .await;
    let transport = server.transport(TransportConfig::CONSERVATIVE);
    let result = transport
        .request(READ, "tools/list", &json!({}))
        .await
        .unwrap();
    assert_eq!(result.as_json(), r#"{"tools":[]}"#);
    let sent = &server.seen()[0];
    assert!(sent.starts_with("post /mcp http/1.1\r\n"), "{sent}");
    assert!(
        sent.contains("accept: application/json, text/event-stream\r\n"),
        "{sent}"
    );
    assert!(
        sent.contains("mcp-protocol-version: 2025-06-18\r\n"),
        "{sent}"
    );
    assert!(!sent.contains("mcp-session-id"), "{sent}");
    let body: serde_json::Value =
        serde_json::from_str(sent.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(
        body,
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}})
    );
}

#[tokio::test]
async fn an_event_stream_answer_skips_server_messages_before_the_response() {
    let stream = "event: message\r\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\r\n\r\n\
                  : a comment\nid: 7\ndata: {\"jsonrpc\":\"2.0\",\ndata:  \"id\":1,\"result\":{\"ok\":true}}\n\n";
    let server = serve(vec![sse(stream)]).await;
    let transport = server.transport(TransportConfig::CONSERVATIVE);
    let result = transport
        .request(READ, "tools/call", &json!({}))
        .await
        .unwrap();
    assert_eq!(result.as_json(), r#"{"ok":true}"#);
}

#[tokio::test]
async fn a_status_media_type_or_size_out_of_bounds_is_typed() {
    let fits = r#"{"jsonrpc":"2.0","id":1,"result":{"a":1}}"#;
    let small = TransportConfig {
        max_answer_bytes: fits.len(),
        ..TransportConfig::CONSERVATIVE
    };
    let cases: [(Answer, &str); 5] = [
        (with_type(500, "application/json", ""), "http_status"),
        (with_type(202, "application/json", ""), "http_status"),
        (with_type(200, "text/html", fits), "content_type"),
        (json(&format!("{fits} ")), "too_large"),
        (
            with_type(200, "Application/JSON; charset=utf-8", fits),
            "ok",
        ),
    ];
    for (answer, code) in cases {
        let server = serve(vec![answer]).await;
        let got = server.transport(small).request(READ, "x", &json!({})).await;
        assert_eq!(got.map_or_else(|e| e.code(), |_| "ok"), code);
    }
}

#[tokio::test]
async fn a_malformed_frame_is_typed() {
    let frames = [
        "{not json",
        r#"{"jsonrpc":"1.0","id":1,"result":{}}"#,
        r#"{"jsonrpc":"2.0","id":2,"result":{}}"#,
        r#"{"jsonrpc":"2.0","id":"1","result":{}}"#,
        r#"{"jsonrpc":"2.0","id":null,"result":{}}"#,
        r#"{"jsonrpc":"2.0","id":1,"method":"x","result":{}}"#,
        r#"{"jsonrpc":"2.0","id":1,"params":{},"result":{}}"#,
        r#"{"jsonrpc":"2.0","id":1,"result":{},"error":{"code":1}}"#,
        r#"{"jsonrpc":"2.0","id":1,"result":null}"#,
        r#"{"jsonrpc":"2.0","id":1,"error":{"message":"x"}}"#,
        r#"{"jsonrpc":"2.0","id":1,"result":{},"extra":1}"#,
    ];
    let mut answers: Vec<Answer> = frames.iter().map(|frame| json(frame)).collect();
    answers.push(sse("data: {not json\n\n"));
    answers.push(sse(
        "data: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{}}\n\n",
    ));
    answers.push(sse("data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}"));
    for (n, answer) in answers.into_iter().enumerate() {
        let server = serve(vec![answer]).await;
        let transport = server.transport(TransportConfig::CONSERVATIVE);
        let got = transport.request(READ, "x", &json!({})).await;
        let expected = if n == frames.len() + 2 {
            "no_response"
        } else {
            "malformed"
        };
        assert_eq!(
            got.map_or_else(|e| e.code(), |_| "ok"),
            expected,
            "frame {n}"
        );
    }
}

#[tokio::test]
async fn the_system_clock_moves_forward() {
    let clock = SystemMonotonic::start();
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(clock.elapsed() >= Duration::from_millis(20));
}
