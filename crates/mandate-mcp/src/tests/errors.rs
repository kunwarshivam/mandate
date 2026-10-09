//! Every error has a stable code, and server text has no printout.

use serde_json::value::RawValue;

use crate::{McpError, ServerText};

#[test]
fn every_error_has_its_stable_code() {
    let detail = || ServerText::new(RawValue::from_string("{}".to_owned()).unwrap());
    let cases = [
        (McpError::EndpointShape, "endpoint_shape"),
        (McpError::NotHttps, "not_https"),
        (McpError::HostNotPinned, "host_not_pinned"),
        (McpError::BadConfig, "bad_config"),
        (McpError::ClientSetup, "client_setup"),
        (McpError::Throttled, "throttled"),
        (McpError::Redirected, "redirected"),
        (McpError::Timeout, "timeout"),
        (McpError::Network, "network"),
        (McpError::HttpStatus { status: 500 }, "http_status"),
        (McpError::SessionExpired, "session_expired"),
        (McpError::BadSessionId, "bad_session_id"),
        (McpError::ContentType, "content_type"),
        (McpError::TooLarge, "too_large"),
        (McpError::Malformed, "malformed"),
        (McpError::NoResponse, "no_response"),
        (
            McpError::Rpc {
                code: 1,
                detail: detail(),
            },
            "rpc_error",
        ),
        (McpError::ToolNotAllowed, "tool_not_allowed"),
        (McpError::FundMovementTool, "fund_movement_tool"),
        (McpError::ContractMissingTool, "contract_missing_tool"),
        (McpError::ContractDrift, "contract_drift"),
        (McpError::AuthHostNotPinned, "auth_host_not_pinned"),
        (McpError::ResourceMismatch, "resource_mismatch"),
        (McpError::IssuerMismatch, "issuer_mismatch"),
        (McpError::PkceUnsupported, "pkce_unsupported"),
        (
            McpError::RegistrationUnavailable,
            "registration_unavailable",
        ),
        (McpError::Unimplemented { story: "E7-16" }, "unimplemented"),
    ];
    for (error, code) in cases {
        assert_eq!(error.code(), code, "{error:?}");
    }
}

#[test]
fn server_text_is_readable_but_never_printed() {
    let raw = RawValue::from_string(r#"{"description":"run rm -rf"}"#.to_owned()).unwrap();
    let text = ServerText::new(raw);
    assert_eq!(text.as_json(), r#"{"description":"run rm -rf"}"#);
    assert_eq!(format!("{text:?}"), "ServerText(withheld)");
}
