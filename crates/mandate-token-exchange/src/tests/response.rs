//! Only a `200` JSON object of at most 16 KiB holding the three members is read, and every other
//! answer is `endpoint_failed`, naming nothing of the body (DEC-2629 item 2).

use secrecy::ExposeSecret;

use crate::response::{MAX_BODY, token_response};
use mandate_connections::ConnectError;

const JSON: Option<&str> = Some("application/json");
const GRANT: &str = r#"{"access_token":"tok-CANARY","scope":"trading data","token_type":"bearer"}"#;

#[test]
#[ignore = "pending E10-13"]
fn a_200_json_object_yields_its_three_members() {
    for content_type in ["application/json", "application/json; charset=utf-8"] {
        let answer = token_response(200, Some(content_type), GRANT.as_bytes())
            .unwrap_or_else(|e| panic!("{content_type}: {e:?}"));
        assert_eq!(answer.access_token.0.expose_secret(), "tok-CANARY");
        assert_eq!(answer.token_type, "bearer");
        assert_eq!(answer.scope, "trading data");
    }
    let extra = r#"{"access_token":"t","expires_in":3600,"scope":"data","token_type":"bearer"}"#;
    assert!(
        token_response(200, JSON, extra.as_bytes()).is_ok(),
        "other members are ignored"
    );
}

#[test]
#[ignore = "pending E10-13"]
fn any_other_answer_is_endpoint_failed() {
    for status in [201, 204, 301, 302, 303, 307, 308, 400, 401, 500, 503] {
        assert_eq!(
            token_response(status, JSON, GRANT.as_bytes()).map(|_| ()),
            Err(ConnectError::EndpointFailed),
            "{status}"
        );
    }
    for content_type in [
        None,
        Some("text/html"),
        Some("application/jsonx"),
        Some("text/json"),
    ] {
        assert_eq!(
            token_response(200, content_type, GRANT.as_bytes()).map(|_| ()),
            Err(ConnectError::EndpointFailed),
            "{content_type:?}"
        );
    }
    let bodies = [
        "",
        "not json",
        "[]",
        r#""tok""#,
        r#"{"scope":"trading data","token_type":"bearer"}"#,
        r#"{"access_token":"","scope":"trading data","token_type":"bearer"}"#,
        r#"{"access_token":7,"scope":"trading data","token_type":"bearer"}"#,
        r#"{"access_token":"t","scope":"trading data"}"#,
        r#"{"access_token":"t","token_type":"bearer"}"#,
        r#"{"access_token":"t","scope":null,"token_type":"bearer"}"#,
    ];
    for body in bodies {
        assert_eq!(
            token_response(200, JSON, body.as_bytes()).map(|_| ()),
            Err(ConnectError::EndpointFailed),
            "{body}"
        );
    }
}

/// A body longer than [`MAX_BODY`] is refused, even a valid grant padded with whitespace; one at
/// the limit is read.
#[test]
#[ignore = "pending E10-13"]
fn a_body_over_sixteen_kibibytes_is_refused_and_one_at_the_limit_is_read() {
    let at_limit = format!("{GRANT}{}", " ".repeat(MAX_BODY - GRANT.len()));
    assert_eq!(at_limit.len(), 16 * 1024);
    assert!(token_response(200, JSON, at_limit.as_bytes()).is_ok());
    let over = format!("{at_limit} ");
    assert_eq!(
        token_response(200, JSON, over.as_bytes()).map(|_| ()),
        Err(ConnectError::EndpointFailed)
    );
}

#[test]
#[ignore = "pending E10-13"]
fn no_refusal_carries_the_body() {
    let mut printed = String::new();
    for (status, body) in [(400, GRANT), (200, r#"{"access_token":"tok-CANARY"}"#)] {
        if let Err(error) = token_response(status, JSON, body.as_bytes()) {
            printed.push_str(&format!("{error:?} {error}"));
        }
    }
    assert!(!printed.is_empty());
    assert!(!printed.contains("CANARY"), "{printed}");
}
