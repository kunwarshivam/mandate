//! The code exchange, and LT-9's canary scan of a whole login (E7-24 O1b, DEC-861).

use std::time::Duration;

use reqwest::Url;
use secrecy::ExposeSecret;
use serde_json::json;

use super::listener::{GOOD, run, secret};
use super::login::client;
use super::server::{Answer, Loopback, serve, with_type};
use crate::{AccessToken, AuthServer, AuthorizationCode};
use crate::{McpError, TransportConfig};

fn found(server: &Loopback) -> AuthServer {
    let at = |path: &str| Url::parse(&server.url.replace("/mcp", path)).unwrap();
    AuthServer {
        resource: Url::parse("https://mcp.test/mcp").unwrap(),
        issuer: at("/as"),
        authorization_endpoint: at("/authorize"),
        token_endpoint: at("/token"),
        registration_endpoint: at("/register"),
    }
}

fn code() -> AuthorizationCode {
    let (code, verifier) = (secret("canary-code"), secret("canary-verifier"));
    AuthorizationCode {
        code,
        verifier,
        client: client(49152),
    }
}

const BEARER: &str = r#"{"access_token":"canary-token","token_type":"Bearer"}"#;

fn token(members: serde_json::Value) -> Answer {
    with_type(200, "application/json", &members.to_string())
}

async fn exchange(answer: Answer) -> (Result<AccessToken, McpError>, Loopback) {
    let server = serve(vec![answer]).await;
    let result = found(&server)
        .exchange(code(), &TransportConfig::CONSERVATIVE)
        .await;
    (result, server)
}

#[tokio::test]
#[ignore = "pending E7-24"]
async fn the_exchange_posts_exactly_the_pkce_form_and_keeps_only_the_token() {
    let answers = [
        json!({"access_token": "canary-token", "token_type": "Bearer"}),
        json!({"access_token": "canary-token", "token_type": "bearer", "expires_in": 60,
            "refresh_token": "canary-refresh", "scope": "x"}),
    ];
    for answer in answers {
        let (result, server) = exchange(token(answer.clone())).await;
        let granted = result.unwrap();
        assert_eq!(granted.secret.expose_secret(), "canary-token", "{answer}");
        assert!(!format!("{granted:?}").contains("canary"));
        let seen = server.seen();
        assert_eq!(seen.len(), 1);
        let (head, body) = seen[0].split_once("\r\n\r\n").unwrap();
        assert!(head.starts_with("post /token http/1.1\r\n"), "{head}");
        assert!(
            head.contains("\r\ncontent-type: application/x-www-form-urlencoded"),
            "{head}"
        );
        assert!(!head.contains("\r\nauthorization:"), "{head}");
        let form = Url::parse(&format!("http://x/?{body}")).unwrap();
        let mut sent: Vec<_> = form
            .query_pairs()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        sent.sort();
        let wanted = "client_id=c-1 code=canary-code code_verifier=canary-verifier \
            grant_type=authorization_code redirect_uri=http://127.0.0.1:49152/callback \
            resource=https://mcp.test/mcp";
        assert_eq!(sent.join(" "), wanted);
    }
}

#[tokio::test]
#[ignore = "pending E7-24"]
async fn a_refused_or_unreadable_token_answer_gives_no_token_and_no_secret() {
    let mut moved = with_type(302, "text/plain", "canary-token");
    moved.headers.push(("location", "@BASE@/x".to_owned()));
    let denied = r#"{"error":"invalid_grant","error_description":"canary"}"#;
    let mut cases = vec![
        (with_type(400, "application/json", denied), "http_status"),
        (moved, "redirected"),
        (with_type(201, "application/json", BEARER), "http_status"),
    ];
    let unreadable = [
        "canary-token",
        r#"{"token_type":"Bearer"}"#,
        r#"{"access_token":"","token_type":"Bearer"}"#,
        r#"{"access_token":"canary token","token_type":"Bearer"}"#,
        r#"{"access_token":7,"token_type":"Bearer"}"#,
        r#"{"access_token":"canary-token","token_type":"mac"}"#,
        r#"{"access_token":"canary-token"}"#,
    ];
    for body in unreadable {
        cases.push((with_type(200, "application/json", body), "malformed"));
    }
    for (answer, wanted) in cases {
        let (result, server) = exchange(answer).await;
        let error = result.unwrap_err();
        let printed = format!("{error:?} {error}").to_lowercase();
        assert!(!printed.contains("canary"), "{printed}");
        assert_eq!(error.code(), wanted);
        assert_eq!(server.seen().len(), 1, "{wanted}");
    }
}

#[test]
fn a_token_never_prints() {
    let printed = format!(
        "{:#?}",
        AccessToken {
            secret: secret("canary-token")
        }
    );
    assert!(!printed.contains("canary"), "{printed}");
    assert!(printed.contains("AccessToken"), "{printed}");
}

#[tokio::test]
#[ignore = "pending E7-24"]
async fn a_whole_login_leaves_no_secret_in_any_output() {
    let (granted, browser, _) = run(GOOD, Duration::ZERO).await;
    let granted = granted.unwrap();
    let mut outputs = vec![browser, format!("{granted:?}")];
    let server = serve(vec![with_type(200, "application/json", BEARER)]).await;
    let token = found(&server)
        .exchange(granted, &TransportConfig::CONSERVATIVE)
        .await
        .unwrap();
    outputs.push(format!("{token:?}"));
    let refused = serve(vec![with_type(500, "text/plain", "canary-token")]).await;
    let error = found(&refused)
        .exchange(code(), &TransportConfig::CONSERVATIVE)
        .await;
    outputs.push(format!("{error:?}"));
    for output in &outputs {
        assert!(!output.to_lowercase().contains("canary"), "{output}");
    }
    assert!(
        server.seen()[0].contains("code=canary-code"),
        "the canary reaches the server"
    );
    assert_eq!(token.secret.expose_secret(), "canary-token");
}
