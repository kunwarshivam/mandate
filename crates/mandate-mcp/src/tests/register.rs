//! The OAuth login's client registration against the loopback server (E7-24, O1a part 2, DEC-847
//! item 5, CN-1, CN-9). Every refusal is checked for the server's `canary` text.

use reqwest::Url;
use serde_json::{Value, json};

use super::server::{Answer, Loopback, serve, with_type};
use crate::{AuthServer, ClientRegistration, LoopbackRedirect, McpError, TransportConfig};

const CALLBACK: &str = "http://127.0.0.1:49152/callback";

fn at(server: &Loopback, path: &str) -> Url {
    Url::parse(&format!("{}{path}", server.url.trim_end_matches("/mcp"))).unwrap()
}

async fn register(answer: Answer) -> (Result<ClientRegistration, McpError>, Loopback) {
    let server = serve(vec![answer]).await;
    let found = AuthServer {
        resource: at(&server, "/mcp"),
        issuer: at(&server, "/as"),
        authorization_endpoint: at(&server, "/authorize"),
        token_endpoint: at(&server, "/token"),
        registration_endpoint: at(&server, "/register"),
    };
    let redirect = LoopbackRedirect::new(49152).unwrap();
    let registered = found
        .register(redirect, &TransportConfig::CONSERVATIVE)
        .await;
    (registered, server)
}

fn created(members: &Value) -> Answer {
    with_type(201, "application/json", &members.to_string())
}

/// The refusal's code, after checking that nothing the server sent reached it.
fn refused(result: Result<ClientRegistration, McpError>) -> &'static str {
    let error = result.unwrap_err();
    let printed = format!("{error:?} {error}").to_lowercase();
    assert!(!printed.contains("canary"), "server text in {printed}");
    error.code()
}

#[test]
fn the_loopback_redirect_is_an_ip_literal_with_the_callback_path() {
    assert_eq!(LoopbackRedirect::new(0).unwrap_err().code(), "bad_config");
    for port in [1_u16, 49152, 65535] {
        let uri = LoopbackRedirect::new(port).unwrap().uri();
        assert_eq!(uri, format!("http://127.0.0.1:{port}/callback"));
    }
}

#[tokio::test]
async fn registration_asks_for_a_public_client_with_the_one_loopback_redirect() {
    let echoed = json!({"client_id": "c-1", "redirect_uris": [CALLBACK],
        "token_endpoint_auth_method": "none"});
    let minimal = json!({"client_id": "c-1", "token_endpoint_auth_method": "none"});
    for answer in [echoed, minimal] {
        let (client, server) = register(created(&answer)).await;
        let redirect = LoopbackRedirect::new(49152).unwrap();
        let expected = ClientRegistration {
            client_id: "c-1".to_owned(),
            redirect,
        };
        assert_eq!(client.unwrap(), expected, "{answer}");
        let seen = server.seen();
        assert_eq!(seen.len(), 1);
        let request = &seen[0];
        assert!(
            request.starts_with("post /register http/1.1\r\n"),
            "{request}"
        );
        assert!(
            request.contains("\r\ncontent-type: application/json"),
            "{request}"
        );
        assert!(!request.contains("\r\nauthorization:"), "{request}");
        let (_, body) = request.split_once("\r\n\r\n").unwrap();
        let wanted = json!({
            "client_name": "Mandate",
            "redirect_uris": [CALLBACK],
            "grant_types": ["authorization_code"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none",
        });
        assert_eq!(serde_json::from_str::<Value>(body).unwrap(), wanted);
    }
}

#[tokio::test]
async fn a_registration_that_issues_a_secret_is_refused_and_the_secret_never_surfaces() {
    let cases = [
        json!({"client_id": "c-1", "client_secret": "canary-secret"}),
        json!({"client_id": "c-1", "client_secret": ""}),
        json!({"client_id": "c-1", "token_endpoint_auth_method": "client_secret_basic"}),
        json!({"client_id": "c-1", "token_endpoint_auth_method": "canary"}),
    ];
    for answer in cases {
        let (client, _) = register(created(&answer)).await;
        assert_eq!(refused(client), "client_secret_issued", "{answer}");
    }
}

#[tokio::test]
async fn a_registration_that_changes_the_redirect_is_refused() {
    let other = "https://canary.example/cb";
    for listed in [json!([other]), json!([CALLBACK, other]), json!([])] {
        let answer = json!({"client_id": "c-1", "redirect_uris": listed});
        let (client, _) = register(created(&answer)).await;
        assert_eq!(refused(client), "redirect_changed", "{answer}");
    }
}

#[tokio::test]
async fn a_refused_or_unreadable_registration_returns_no_client() {
    let mut moved = with_type(302, "text/plain", "canary");
    moved.headers.push(("location", "@BASE@/x".to_owned()));
    let cases = [
        (
            with_type(400, "application/json", r#"{"error_description":"canary"}"#),
            "http_status",
        ),
        (moved, "redirected"),
        (with_type(201, "application/json", "canary"), "malformed"),
        (created(&json!({})), "malformed"),
        (created(&json!({"client_id": ""})), "malformed"),
        (created(&json!({"client_id": "has space"})), "malformed"),
        (created(&json!({"client_id": "canary\u{e9}"})), "malformed"),
        (created(&json!({"client_id": 7})), "malformed"),
    ];
    for (answer, code) in cases {
        let (client, server) = register(answer).await;
        assert_eq!(refused(client), code);
        assert_eq!(server.seen().len(), 1, "{code}");
    }
}

#[tokio::test]
async fn a_null_client_secret_is_still_a_secret() {
    let answer = json!({"client_id": "c-1", "client_secret": null});
    let (client, _) = register(created(&answer)).await;
    assert_eq!(refused(client), "client_secret_issued");
}

#[tokio::test]
async fn a_redirect_list_other_than_the_one_element_sent_is_refused() {
    for listed in [json!([CALLBACK, CALLBACK]), json!(CALLBACK)] {
        let answer = json!({"client_id": "c-1", "redirect_uris": listed});
        let (client, _) = register(created(&answer)).await;
        assert_eq!(refused(client), "redirect_changed", "{answer}");
    }
}

#[tokio::test]
async fn only_201_created_is_a_registration() {
    let body = json!({"client_id": "c-1"}).to_string();
    let (client, server) = register(with_type(200, "application/json", &body)).await;
    assert_eq!(refused(client), "http_status");
    assert_eq!(server.seen().len(), 1);
}

#[tokio::test]
async fn a_client_id_with_del_or_a_control_character_is_refused() {
    for id in ["c\u{7f}1", "c\u{1}1"] {
        let (client, _) = register(created(&json!({"client_id": id}))).await;
        assert_eq!(refused(client), "malformed", "{id:?}");
    }
}

#[tokio::test]
async fn an_answer_that_echoes_every_member_sent_registers_the_client() {
    let answer = json!({"client_id": "c-1", "client_name": "Mandate",
        "redirect_uris": [CALLBACK], "grant_types": ["authorization_code"],
        "response_types": ["code"], "token_endpoint_auth_method": "none"});
    let (client, _) = register(created(&answer)).await;
    assert_eq!(client.unwrap().client_id, "c-1");
}

#[tokio::test]
async fn an_answer_without_the_auth_method_or_with_other_grants_is_refused() {
    let none = || json!({"client_id": "c-1", "token_endpoint_auth_method": "none"});
    let with = |key: &str, value: Value| {
        let mut answer = none();
        answer
            .as_object_mut()
            .unwrap()
            .insert(key.to_owned(), value);
        answer
    };
    let cases = [
        json!({"client_id": "c-1"}),
        json!({"client_id": "c-1", "redirect_uris": [CALLBACK]}),
        with(
            "grant_types",
            json!(["authorization_code", "refresh_token"]),
        ),
        with("grant_types", json!("authorization_code")),
        with("grant_types", json!([])),
        with("response_types", json!(["code", "token"])),
        with("response_types", json!("code")),
    ];
    for answer in cases {
        let (client, server) = register(created(&answer)).await;
        assert_eq!(refused(client), "malformed", "{answer}");
        assert_eq!(server.seen().len(), 1, "{answer}");
    }
}
