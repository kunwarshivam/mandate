//! The founder's OAuth login (E7-24), its first steps (O1a, DEC-847): discovery of the
//! authorization server from the MCP server's published metadata, and registration of a public
//! client with a loopback redirect, as the MCP authorization flow describes them. No token, code,
//! verifier, or secret passes through them.
//!
//! - **Pinned hosts only.** The protected-resource metadata is read only from the pinned MCP host;
//!   the authorization server and every endpoint it names must be on a host the connector
//!   compiles in. Nothing off those hosts is ever dialed (LT-1, connections spec §6.2 rule 1).
//! - **Identifiers exact.** The metadata's `resource` is the endpoint and its `issuer` is the URL it
//!   was read for, character for character (RFC 9728 §3.3, RFC 8414 §3.3).
//! - **A public client only.** `S256` must be offered and a registration endpoint must exist; a
//!   registration answer that issues a secret or changes the redirect is refused.

use reqwest::Url;

use crate::endpoint::PinnedEndpoint;
use crate::error::McpError;
use crate::transport::TransportConfig;

/// The authorization server discovery found, with the endpoints the login dials.
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "the authorization request and the token exchange (O1b) read them"
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

/// The redirect a login registers: `http://127.0.0.1:<port>/callback`, an IP literal on loopback
/// (RFC 8252 §7.3). Only the port is chosen, so no other redirect can be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopbackRedirect {
    port: u16,
}

impl LoopbackRedirect {
    /// Port 0 is [`McpError::BadConfig`]: the redirect names the port the listener bound.
    pub fn new(port: u16) -> Result<Self, McpError> {
        if port == 0 {
            return Err(McpError::BadConfig);
        }
        Ok(Self { port })
    }

    pub fn uri(&self) -> String {
        format!("http://127.0.0.1:{}/callback", self.port)
    }
}

/// The client the authorization server registered: a public client id, which is not a secret
/// (RFC 6749 §2.2), and the one redirect it was registered with.
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "the authorization request (O1b) reads them")
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientRegistration {
    pub(crate) client_id: String,
    pub(crate) redirect: LoopbackRedirect,
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

    /// RFC 7591 registration at the registration endpoint, as a public client: `client_name`
    /// `Mandate`, `authorization_code` only, response type `code`, `token_endpoint_auth_method`
    /// `none`, and `redirect` as the one redirect, with no other member and no `authorization`
    /// header. An answer carrying a client secret (even empty) or another auth method is
    /// [`McpError::ClientSecretIssued`]; one listing any other redirect set is
    /// [`McpError::RedirectChanged`]; a `client_id` that is missing or not visible ASCII is
    /// [`McpError::Malformed`]. No redirect is followed, and no server text reaches an error.
    pub async fn register(
        &self,
        redirect: LoopbackRedirect,
        config: &TransportConfig,
    ) -> Result<ClientRegistration, McpError> {
        let _ = (redirect, config);
        Err(McpError::Unimplemented { story: "E7-24" })
    }
}
