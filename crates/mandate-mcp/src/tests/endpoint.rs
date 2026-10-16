//! The pinned endpoint: `https` on exactly the pinned host, and plain `http` only to loopback in
//! a test build or the rehearsal build, which dials loopback only (DEC-922).

use crate::McpError;
use crate::PinnedEndpoint;
use crate::auth::on_auth_host_for;
use crate::endpoint::Build;

const HOST: &str = "mcp.broker.example";

fn code(build: Build, endpoint: &str) -> &'static str {
    PinnedEndpoint::for_build(HOST, endpoint, build).map_or_else(|e| e.code(), |_| "ok")
}

#[test]
fn only_the_pinned_host_is_accepted() {
    let cases = [
        ("https://mcp.broker.example/mcp", "ok"),
        ("https://MCP.Broker.Example:8443/mcp", "ok"),
        ("https://evil.example/mcp", "host_not_pinned"),
        (
            "https://mcp.broker.example.evil.example/mcp",
            "host_not_pinned",
        ),
        ("https://x.mcp.broker.example/mcp", "host_not_pinned"),
        ("https://evilmcp.broker.example/mcp", "host_not_pinned"),
        ("https://127.0.0.1/mcp", "host_not_pinned"),
        (
            "https://mcp.broker.example@evil.example/mcp",
            "endpoint_shape",
        ),
        ("https://user@mcp.broker.example/mcp", "endpoint_shape"),
        ("https://:pw@mcp.broker.example/mcp", "endpoint_shape"),
        ("https://mcp.broker.example/mcp?next=evil", "endpoint_shape"),
        ("https://mcp.broker.example/mcp#x", "endpoint_shape"),
        ("mcp.broker.example/mcp", "endpoint_shape"),
        ("wss://mcp.broker.example/mcp", "not_https"),
    ];
    for (endpoint, expected) in cases {
        assert_eq!(code(Build::Production, endpoint), expected, "{endpoint}");
        assert_eq!(code(Build::Test, endpoint), expected, "{endpoint}");
    }
}

#[test]
fn plain_http_is_refused_outside_test_builds_and_off_loopback() {
    for loopback in ["http://127.0.0.1:8080/mcp", "http://[::1]:8080/mcp"] {
        let host = if loopback.contains('[') {
            "[::1]"
        } else {
            "127.0.0.1"
        };
        let production = PinnedEndpoint::for_build(host, loopback, Build::Production);
        assert!(
            matches!(production, Err(McpError::NotHttps)),
            "{production:?}"
        );
        assert!(PinnedEndpoint::for_build(host, loopback, Build::Test).is_ok());
    }
    assert_eq!(
        code(Build::Test, "http://mcp.broker.example/mcp"),
        "not_https"
    );
    let named = PinnedEndpoint::for_build("localhost", "http://localhost:8080/mcp", Build::Test);
    assert!(matches!(named, Err(McpError::NotHttps)), "{named:?}");
    let current = PinnedEndpoint::new("127.0.0.1", "http://127.0.0.1:1/mcp");
    assert!(
        current.is_ok(),
        "this crate's own test build accepts loopback: {current:?}"
    );
}

/// DEC-922 items 1 and 2: the rehearsal build takes plain `http` to a loopback IP literal, as the
/// test build does, and refuses every host that is not one, `https` or not, the pinned
/// broker's included, so a binary built with `rh-sim` can never reach a broker.
#[test]
fn the_rehearsal_build_dials_loopback_only() {
    for (host, endpoint) in [
        ("127.0.0.1", "http://127.0.0.1:8080/mcp"),
        ("127.0.0.1", "https://127.0.0.1:8443/mcp"),
        ("[::1]", "http://[::1]:8080/mcp"),
        ("127.0.0.2", "http://127.0.0.2/mcp"),
    ] {
        let rehearsal = PinnedEndpoint::for_build(host, endpoint, Build::Rehearsal);
        assert!(rehearsal.is_ok(), "{endpoint}: {rehearsal:?}");
    }
    let cases = [
        ("https://mcp.broker.example/mcp", "off_loopback"),
        ("https://10.0.0.1/mcp", "off_loopback"),
        ("http://mcp.broker.example/mcp", "not_https"),
        ("https://mcp.broker.example/mcp?next=evil", "endpoint_shape"),
        ("wss://mcp.broker.example/mcp", "not_https"),
    ];
    for (endpoint, expected) in cases {
        assert_eq!(code(Build::Rehearsal, endpoint), expected, "{endpoint}");
    }
    let off = PinnedEndpoint::for_build("10.0.0.1", "https://10.0.0.1/mcp", Build::Rehearsal);
    assert!(matches!(off, Err(McpError::OffLoopback)), "{off:?}");
    let named = PinnedEndpoint::for_build("localhost", "https://localhost/mcp", Build::Rehearsal);
    assert!(matches!(named, Err(McpError::OffLoopback)), "{named:?}");
    let off_http = PinnedEndpoint::for_build("10.0.0.1", "http://10.0.0.1/mcp", Build::Rehearsal);
    assert!(matches!(off_http, Err(McpError::NotHttps)), "{off_http:?}");
}

/// The other builds take any pinned host: only the rehearsal build is held to loopback.
#[test]
fn only_the_rehearsal_build_is_held_to_loopback() {
    for build in [Build::Test, Build::Production] {
        let broker = PinnedEndpoint::for_build(HOST, "https://mcp.broker.example/mcp", build);
        assert!(broker.is_ok(), "{build:?}: {broker:?}");
    }
    assert_eq!(
        code(Build::Production, "http://127.0.0.1:8080/mcp"),
        "not_https"
    );
}

/// The authorization server's URLs in the rehearsal build: loopback only, plain `http` allowed
/// there, and refused off loopback whatever `auth_hosts` lists.
#[test]
fn the_rehearsal_build_takes_authorization_urls_on_loopback_only() {
    let code = |text: &str, hosts: &[&str], build: Build| {
        on_auth_host_for(text, hosts, build).map_or_else(|e| e.code(), |_| "ok")
    };
    let loopback = ["127.0.0.1"];
    let broker = ["as.broker.example"];
    assert_eq!(
        code("http://127.0.0.1:9/oauth", &loopback, Build::Rehearsal),
        "ok"
    );
    assert_eq!(
        code("https://127.0.0.1:9/oauth", &loopback, Build::Rehearsal),
        "ok"
    );
    assert_eq!(
        code("https://as.broker.example/oauth", &broker, Build::Rehearsal),
        "off_loopback"
    );
    assert_eq!(
        code("http://as.broker.example/oauth", &broker, Build::Rehearsal),
        "not_https"
    );
    assert_eq!(
        code("http://127.0.0.1:9/oauth", &broker, Build::Rehearsal),
        "auth_host_not_pinned"
    );
    assert_eq!(
        code(
            "https://as.broker.example/oauth",
            &broker,
            Build::Production
        ),
        "ok"
    );
    assert_eq!(
        code("http://127.0.0.1:9/oauth", &loopback, Build::Production),
        "not_https"
    );
}
