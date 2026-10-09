//! The OAuth login's discovery against the loopback server (E7-24, O1a part 1, DEC-847, LT-1,
//! CN-9). Every refusal is checked for the server's `canary` text.

use reqwest::Url;
use serde_json::{Value, json};

use super::server::{Answer, Loopback, serve, with_type};
use crate::{AuthServer, McpError, PinnedEndpoint, TransportConfig};

const PINS: &[&str] = &["127.0.0.1"];
const CHALLENGE: Option<&str> = Some("@BASE@/meta/rs");

fn doc(value: &Value) -> Answer {
    with_type(200, "application/json", &value.to_string())
}

fn challenge(metadata_url: Option<&str>) -> Answer {
    let mut answer = with_type(401, "application/json", r#"{"error":"canary"}"#);
    if let Some(url) = metadata_url {
        let value = format!(r#"Bearer resource_metadata="{url}""#);
        answer.headers.push(("www-authenticate", value));
    }
    answer
}

fn redirect() -> Answer {
    let mut answer = with_type(302, "text/plain", "canary");
    answer.headers.push(("location", "@BASE@/x".to_owned()));
    answer
}

fn resource(named: &str, servers: &[&str]) -> Answer {
    doc(&json!({"resource": named, "authorization_servers": servers}))
}

fn metadata() -> Value {
    json!({
        "issuer": "@BASE@/as",
        "authorization_endpoint": "@BASE@/authorize",
        "token_endpoint": "@BASE@/token",
        "registration_endpoint": "@BASE@/register",
        "response_types_supported": ["code"],
        "code_challenge_methods_supported": ["S256"],
    })
}

/// The published flow, with the server's metadata `meta`.
fn flow(meta: &Value) -> Vec<Answer> {
    let prm = resource("@BASE@/mcp", &["@BASE@/as"]);
    vec![challenge(CHALLENGE), prm, doc(meta)]
}

fn edited(key: &str, value: Option<Value>) -> Value {
    let mut meta = metadata();
    let members = meta.as_object_mut().unwrap();
    match value {
        Some(value) => members.insert(key.to_owned(), value),
        None => members.remove(key),
    };
    meta
}

async fn discover(answers: Vec<Answer>, pins: &[&str]) -> (Result<AuthServer, McpError>, Loopback) {
    let server = serve(answers).await;
    let endpoint = PinnedEndpoint::new("127.0.0.1", &server.url).unwrap();
    let found = AuthServer::discover(&endpoint, pins, &TransportConfig::CONSERVATIVE).await;
    (found, server)
}

/// The refusal's code, after checking that nothing the server sent reached it.
fn refused(result: Result<AuthServer, McpError>) -> &'static str {
    let error = result.unwrap_err();
    let printed = format!("{error:?} {error}").to_lowercase();
    assert!(!printed.contains("canary"), "server text in {printed}");
    error.code()
}

fn first_lines(server: &Loopback) -> Vec<String> {
    let line = |r: &String| r.lines().next().unwrap().to_owned();
    server.seen().iter().map(line).collect()
}

fn at(server: &Loopback, path: &str) -> Url {
    Url::parse(&format!("{}{path}", server.url.trim_end_matches("/mcp"))).unwrap()
}

#[tokio::test]
async fn discovery_follows_the_challenge_to_the_pinned_servers_metadata() {
    let (found, server) = discover(flow(&metadata()), PINS).await;
    let expected = AuthServer {
        resource: at(&server, "/mcp"),
        issuer: at(&server, "/as"),
        authorization_endpoint: at(&server, "/authorize"),
        token_endpoint: at(&server, "/token"),
        registration_endpoint: at(&server, "/register"),
    };
    assert_eq!(found.unwrap(), expected);
    let metadata_line = "get /.well-known/oauth-authorization-server/as http/1.1";
    let wanted = ["post /mcp http/1.1", "get /meta/rs http/1.1", metadata_line];
    assert_eq!(first_lines(&server), wanted);
    let probe = &server.seen()[0];
    assert!(!probe.contains("\r\nauthorization:"), "{probe}");
    let (_, body) = probe.split_once("\r\n\r\n").unwrap();
    let method = &serde_json::from_str::<Value>(body).unwrap()["method"];
    assert_eq!(method, "initialize");
}

#[tokio::test]
async fn without_a_challenge_url_discovery_reads_the_endpoints_well_known_path() {
    let prm = resource("@BASE@/mcp", &["@BASE@/as"]);
    let (found, server) = discover(vec![challenge(None), prm, doc(&metadata())], PINS).await;
    assert_eq!(found.unwrap().issuer, at(&server, "/as"));
    let wanted = "get /.well-known/oauth-protected-resource/mcp http/1.1";
    assert_eq!(first_lines(&server)[1], wanted);
}

#[tokio::test]
async fn a_probe_that_is_not_a_challenge_on_the_pinned_host_goes_no_further() {
    let cases = [
        (
            challenge(Some("https://rs.example/canary")),
            "host_not_pinned",
        ),
        (
            challenge(Some("http://127.0.0.2:1/canary")),
            "host_not_pinned",
        ),
        (with_type(200, "application/json", "{}"), "http_status"),
        (redirect(), "redirected"),
    ];
    for (answer, code) in cases {
        let (found, server) = discover(vec![answer], PINS).await;
        assert_eq!(refused(found), code);
        assert_eq!(server.seen().len(), 1, "{code}");
    }
}

#[tokio::test]
async fn the_resource_must_be_the_endpoint_exactly() {
    for named in [
        "@BASE@/mcp/",
        "@BASE@/MCP",
        "@BASE@/mcp?canary",
        "https://127.0.0.1/mcp",
    ] {
        let answers = vec![challenge(CHALLENGE), resource(named, &["@BASE@/as"])];
        let (found, server) = discover(answers, PINS).await;
        assert_eq!(refused(found), "resource_mismatch", "{named}");
        assert_eq!(server.seen().len(), 2, "{named}");
    }
}

#[tokio::test]
async fn only_the_first_authorization_server_on_the_pins_is_contacted() {
    let cases: [(&[&str], &[&str], &str); 5] = [
        (&["@BASE@/as"], &["as.example"], "auth_host_not_pinned"),
        (
            &["https://as.example/canary", "@BASE@/as"],
            PINS,
            "auth_host_not_pinned",
        ),
        (&[], PINS, "malformed"),
        (&["@BASE@/as#canary"], PINS, "endpoint_shape"),
        (&["http://canary@127.0.0.1:1/as"], PINS, "endpoint_shape"),
    ];
    for (servers, pins, code) in cases {
        let answers = vec![challenge(CHALLENGE), resource("@BASE@/mcp", servers)];
        let (found, server) = discover(answers, pins).await;
        assert_eq!(refused(found), code, "{servers:?}");
        assert_eq!(server.seen().len(), 2, "{servers:?}");
    }
}

#[tokio::test]
async fn the_issuer_must_be_the_one_its_metadata_was_read_for() {
    for issuer in ["@BASE@/as/", "@BASE@/other", "https://127.0.0.1/as"] {
        let meta = edited("issuer", Some(json!(issuer)));
        let (found, _) = discover(flow(&meta), PINS).await;
        assert_eq!(refused(found), "issuer_mismatch", "{issuer}");
    }
}

#[tokio::test]
async fn every_endpoint_must_be_https_on_a_pinned_authorization_host() {
    let both: &[&str] = &["127.0.0.1", "as.example"];
    let off = "https://as.example/canary";
    let cases = [
        ("authorization_endpoint", off, PINS, "auth_host_not_pinned"),
        ("token_endpoint", off, PINS, "auth_host_not_pinned"),
        ("registration_endpoint", off, PINS, "auth_host_not_pinned"),
        (
            "token_endpoint",
            "http://as.example/canary",
            both,
            "not_https",
        ),
        (
            "authorization_endpoint",
            "@BASE@/authorize#canary",
            PINS,
            "endpoint_shape",
        ),
        (
            "token_endpoint",
            "http://canary@127.0.0.1:1/token",
            PINS,
            "endpoint_shape",
        ),
    ];
    for (key, value, pins, code) in cases {
        let (found, _) = discover(flow(&edited(key, Some(json!(value)))), pins).await;
        assert_eq!(refused(found), code, "{key} {value}");
    }
}

#[tokio::test]
async fn pkce_with_s256_a_code_flow_and_a_registration_endpoint_are_required() {
    let methods = "code_challenge_methods_supported";
    let cases = [
        (edited(methods, None), "pkce_unsupported"),
        (edited(methods, Some(json!(["plain"]))), "pkce_unsupported"),
        (edited(methods, Some(json!(["s256"]))), "pkce_unsupported"),
        (
            edited("registration_endpoint", None),
            "registration_unavailable",
        ),
        (
            edited("response_types_supported", Some(json!(["token"]))),
            "malformed",
        ),
        (edited("authorization_endpoint", None), "malformed"),
    ];
    for (meta, code) in cases {
        let (found, _) = discover(flow(&meta), PINS).await;
        assert_eq!(refused(found), code, "{meta}");
    }
}

#[tokio::test]
async fn an_unreadable_or_redirected_document_is_refused() {
    let prm = || resource("@BASE@/mcp", &["@BASE@/as"]);
    let unreadable = || with_type(200, "application/json", "canary");
    let cases = [
        (vec![unreadable()], "malformed"),
        (vec![redirect()], "redirected"),
        (vec![prm(), redirect()], "redirected"),
        (
            vec![prm(), with_type(404, "text/plain", "canary")],
            "http_status",
        ),
        (vec![prm(), unreadable()], "malformed"),
    ];
    for (rest, code) in cases {
        let answers = std::iter::once(challenge(CHALLENGE)).chain(rest).collect();
        let (found, _) = discover(answers, PINS).await;
        assert_eq!(refused(found), code);
    }
}

#[tokio::test]
async fn a_code_flow_among_other_response_types_is_accepted() {
    let meta = edited("response_types_supported", Some(json!(["token", "code"])));
    let (found, server) = discover(flow(&meta), PINS).await;
    assert_eq!(found.unwrap().token_endpoint, at(&server, "/token"));
}

/// RFC 9728 §3.1 and RFC 8414 §3.1: a terminating `/` is removed before the well-known suffix
/// is inserted between the host and the path.
#[tokio::test]
async fn a_terminating_slash_is_removed_before_the_well_known_suffix_is_inserted() {
    let meta = edited("issuer", Some(json!("@BASE@/as/")));
    let prm = resource("@BASE@/mcp/", &["@BASE@/as/"]);
    let server = serve(vec![challenge(None), prm, doc(&meta)]).await;
    let endpoint = PinnedEndpoint::new("127.0.0.1", &format!("{}/", server.url)).unwrap();
    let found = AuthServer::discover(&endpoint, PINS, &TransportConfig::CONSERVATIVE).await;
    assert_eq!(found.unwrap().issuer, at(&server, "/as/"));
    let wanted = [
        "post /mcp/ http/1.1",
        "get /.well-known/oauth-protected-resource/mcp http/1.1",
        "get /.well-known/oauth-authorization-server/as http/1.1",
    ];
    assert_eq!(first_lines(&server), wanted);
}

#[tokio::test]
async fn a_challenge_url_with_a_query_is_refused_before_it_is_dialed() {
    let (found, server) = discover(vec![challenge(Some("@BASE@/meta/rs?canary"))], PINS).await;
    assert_eq!(refused(found), "endpoint_shape");
    assert_eq!(server.seen().len(), 1);
}

#[tokio::test]
async fn an_unquoted_challenge_url_falls_back_to_the_well_known_path() {
    let mut probe = challenge(None);
    let value = "Bearer resource_metadata=@BASE@/meta/rs".to_owned();
    probe.headers.push(("www-authenticate", value));
    let prm = resource("@BASE@/mcp", &["@BASE@/as"]);
    let (found, server) = discover(vec![probe, prm, doc(&metadata())], PINS).await;
    assert_eq!(found.unwrap().issuer, at(&server, "/as"));
    let wanted = "get /.well-known/oauth-protected-resource/mcp http/1.1";
    assert_eq!(first_lines(&server)[1], wanted);
}
