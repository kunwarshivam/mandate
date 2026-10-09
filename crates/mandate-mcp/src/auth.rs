//! The founder's OAuth login (E7-24), its first step (O1a part 1, DEC-847): discovery of the
//! authorization server from the MCP server's published metadata, as the MCP authorization flow
//! describes it. No token, code, verifier, or secret passes through it.
//!
//! - **Pinned hosts only.** The protected-resource metadata is read only from the pinned MCP host;
//!   the authorization server and every endpoint it names must be on a host the connector
//!   compiles in. Nothing off those hosts is ever dialed (LT-1, connections spec §6.2 rule 1).
//! - **Identifiers exact.** The metadata's `resource` is the endpoint and its `issuer` is the URL it
//!   was read for, character for character (RFC 9728 §3.3, RFC 8414 §3.3).
//! - **A public client can log in.** `S256` must be offered and a registration endpoint must exist.

use reqwest::Url;

use crate::endpoint::PinnedEndpoint;
use crate::error::McpError;
use crate::transport::TransportConfig;

/// The authorization server discovery found, with the endpoints the login dials.
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "registration and the token exchange (O1a part 2, O1b) read them"
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthServer {
    pub(crate) resource: Url,
    pub(crate) issuer: Url,
    pub(crate) authorization_endpoint: Url,
    pub(crate) token_endpoint: Url,
    pub(crate) registration_endpoint: Url,
}

impl AuthServer {
    /// An unauthenticated `POST` of `initialize` to the endpoint, which must answer `401`; the
    /// Bearer challenge's `resource_metadata` URL, or without one the endpoint's RFC 9728 well-known
    /// URL, read from the pinned MCP host only; then the first of its `authorization_servers`,
    /// which must be on `auth_hosts`, read at its RFC 8414 well-known URL. Every endpoint named must
    /// be `https` (loopback `http` in this crate's test build) on `auth_hosts`, with no credentials
    /// or fragment; `S256` and response type `code` must be offered, and a registration endpoint
    /// must exist. No redirect is followed, and no server text reaches an error.
    pub async fn discover(
        endpoint: &PinnedEndpoint,
        auth_hosts: &[&str],
        config: &TransportConfig,
    ) -> Result<Self, McpError> {
        let _ = (endpoint, auth_hosts, config);
        Err(McpError::Unimplemented { story: "E7-24" })
    }
}
