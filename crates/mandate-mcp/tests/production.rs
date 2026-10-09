//! The crate as a connector links it: an integration test builds the library without
//! `cfg(test)`, so this is the production build's gate on plain `http`, which no in-crate test
//! can see.

use mandate_mcp::{McpError, PinnedEndpoint};

#[test]
fn the_production_build_refuses_plain_http_even_to_loopback() {
    for (host, endpoint) in [
        ("127.0.0.1", "http://127.0.0.1:1/mcp"),
        ("[::1]", "http://[::1]:1/mcp"),
    ] {
        let refused = PinnedEndpoint::new(host, endpoint);
        assert!(
            matches!(refused, Err(McpError::NotHttps)),
            "{endpoint}: {refused:?}"
        );
    }
    let https = PinnedEndpoint::new("127.0.0.1", "https://127.0.0.1:1/mcp");
    assert!(https.is_ok(), "{https:?}");
}
