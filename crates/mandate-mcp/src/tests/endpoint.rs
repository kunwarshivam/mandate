//! The pinned endpoint: `https` on exactly the pinned host, and plain `http` only to loopback in
//! a test build.

use crate::McpError;
use crate::PinnedEndpoint;
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
