//! The one error type of the transport. No variant carries text the server sent except inside a
//! [`ServerText`], whose `Debug` withholds it and which has no `Display` (CN-9).

use std::fmt;

use serde_json::value::RawValue;

/// JSON the server sent, held whole and never printed: a result, or an error's message and data.
/// The connector reads it with [`ServerText::as_json`] and decides what, if anything, leaves.
pub struct ServerText(Box<RawValue>);

impl ServerText {
    pub(crate) fn new(raw: Box<RawValue>) -> Self {
        Self(raw)
    }

    /// The JSON exactly as the server sent it.
    pub fn as_json(&self) -> &str {
        self.0.get()
    }
}

impl fmt::Debug for ServerText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ServerText(withheld)")
    }
}

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("the endpoint is not a plain URL without credentials, query, or fragment")]
    EndpointShape,
    #[error("the endpoint is not https")]
    NotHttps,
    #[error("the endpoint's host is not the pinned host")]
    HostNotPinned,
    #[error("a timeout, size cap, or budget in the configuration is zero")]
    BadConfig,
    #[error("the HTTP client could not be built")]
    ClientSetup,
    #[error("the call budget for this class is spent; nothing was sent")]
    Throttled,
    #[error("the server answered with a redirect, which is never followed")]
    Redirected,
    #[error("the request timed out")]
    Timeout,
    #[error("the connection failed")]
    Network,
    #[error("the server answered HTTP {status}")]
    HttpStatus { status: u16 },
    #[error("the server no longer knows the session")]
    SessionExpired,
    #[error("the session id is not visible ASCII")]
    BadSessionId,
    #[error("the answer is neither JSON nor an event stream")]
    ContentType,
    #[error("the answer exceeds the size cap")]
    TooLarge,
    #[error("the answer is not a well-formed JSON-RPC 2.0 response to this request")]
    Malformed,
    #[error("the event stream ended without a response to this request")]
    NoResponse,
    #[error("the server answered JSON-RPC error {code}")]
    Rpc { code: i64, detail: ServerText },
    #[error("the tool is not on the allowlist; nothing was sent")]
    ToolNotAllowed,
    #[error("the server lists a tool that can move funds")]
    FundMovementTool,
    #[error("the server's tool list lacks an allowlisted tool")]
    ContractMissingTool,
    #[error("the server's tool contract differs from the pinned one")]
    ContractDrift,
    #[error("an authorization host or endpoint is not on the pinned authorization hosts")]
    AuthHostNotPinned,
    #[error("the protected-resource metadata names another resource")]
    ResourceMismatch,
    #[error("the authorization server metadata names another issuer")]
    IssuerMismatch,
    #[error("the authorization server does not offer PKCE with S256")]
    PkceUnsupported,
    #[error("the authorization server offers no client registration")]
    RegistrationUnavailable,
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
}

impl McpError {
    /// The stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::EndpointShape => "endpoint_shape",
            Self::NotHttps => "not_https",
            Self::HostNotPinned => "host_not_pinned",
            Self::BadConfig => "bad_config",
            Self::ClientSetup => "client_setup",
            Self::Throttled => "throttled",
            Self::Redirected => "redirected",
            Self::Timeout => "timeout",
            Self::Network => "network",
            Self::HttpStatus { .. } => "http_status",
            Self::SessionExpired => "session_expired",
            Self::BadSessionId => "bad_session_id",
            Self::ContentType => "content_type",
            Self::TooLarge => "too_large",
            Self::Malformed => "malformed",
            Self::NoResponse => "no_response",
            Self::Rpc { .. } => "rpc_error",
            Self::ToolNotAllowed => "tool_not_allowed",
            Self::FundMovementTool => "fund_movement_tool",
            Self::ContractMissingTool => "contract_missing_tool",
            Self::ContractDrift => "contract_drift",
            Self::AuthHostNotPinned => "auth_host_not_pinned",
            Self::ResourceMismatch => "resource_mismatch",
            Self::IssuerMismatch => "issuer_mismatch",
            Self::PkceUnsupported => "pkce_unsupported",
            Self::RegistrationUnavailable => "registration_unavailable",
            Self::Unimplemented { .. } => "unimplemented",
        }
    }
}
