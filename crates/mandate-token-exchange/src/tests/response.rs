//! Only a `200` JSON object of at most 16 KiB holding the three members is read, and every other
//! answer is `endpoint_failed`, naming nothing of the body (DEC-2629 item 2).

use secrecy::ExposeSecret;

use crate::response::{MAX_BODY, token_response};
use mandate_connections::ConnectError;

const JSON: Option<&str> = Some("application/json");
const GRANT: &str = r#"{"access_token":"tok-CANARY","scope":"trading data","token_type":"bearer"}"#;

#[test]
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

/// The three members must be non-empty strings: an empty, a `null`, a number, or a list is refused,
/// for each of them.
#[test]
fn each_member_must_be_a_non_empty_string() {
    for member in ["access_token", "scope", "token_type"] {
        for bad in [r#""""#, "null", "7", r#"["trading"]"#, "{}"] {
            let mut fields = vec![
                ("access_token", r#""t""#.to_owned()),
                ("scope", r#""trading data""#.to_owned()),
                ("token_type", r#""bearer""#.to_owned()),
            ];
            for field in &mut fields {
                if field.0 == member {
                    field.1 = bad.to_owned();
                }
            }
            let body = format!(
                "{{{}}}",
                fields
                    .iter()
                    .map(|(name, value)| format!(r#""{name}":{value}"#))
                    .collect::<Vec<_>>()
                    .join(",")
            );
            assert_eq!(
                token_response(200, JSON, body.as_bytes()).map(|_| ()),
                Err(ConnectError::EndpointFailed),
                "{body}"
            );
        }
    }
}

/// The media type is matched without regard to case, before any parameter; a parameter alone,
/// two media types in one value, or another type with a JSON parameter is refused.
#[test]
fn the_media_type_is_json_in_any_case_and_nothing_else() {
    for accepted in [
        "Application/JSON",
        "APPLICATION/JSON;charset=UTF-8",
        " application/json ",
    ] {
        assert!(
            token_response(200, Some(accepted), GRANT.as_bytes()).is_ok(),
            "{accepted}"
        );
    }
    for refused in [
        "; charset=utf-8",
        "",
        "application/json, application/json",
        "application/json,text/html",
        "text/html; application/json",
    ] {
        assert_eq!(
            token_response(200, Some(refused), GRANT.as_bytes()).map(|_| ()),
            Err(ConnectError::EndpointFailed),
            "{refused:?}"
        );
    }
}

/// The body is read by `mandate_canon`, so what that reader refuses fails the exchange closed: a
/// duplicate member, a float or an exponent anywhere, a byte-order mark, or a body not UTF-8.
#[test]
fn a_body_the_canonical_reader_refuses_fails_closed() {
    let bodies: [&[u8]; 6] = [
        br#"{"access_token":"t","access_token":"u","scope":"trading data","token_type":"bearer"}"#,
        br#"{"access_token":"t","expires_in":3600.5,"scope":"trading data","token_type":"bearer"}"#,
        br#"{"access_token":"t","expires_in":1e3,"scope":"trading data","token_type":"bearer"}"#,
        b"\xEF\xBB\xBF{\"access_token\":\"t\",\"scope\":\"trading data\",\"token_type\":\"bearer\"}",
        b"{\"access_token\":\"t\xFF\",\"scope\":\"trading data\",\"token_type\":\"bearer\"}",
        b"\xFF\xFE",
    ];
    for body in bodies {
        assert_eq!(
            token_response(200, JSON, body).map(|_| ()),
            Err(ConnectError::EndpointFailed),
            "{}",
            String::from_utf8_lossy(body)
        );
    }
}
