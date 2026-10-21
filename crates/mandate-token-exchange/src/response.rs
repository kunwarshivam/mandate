//! The token endpoint's answer (DEC-2629 item 2).

use mandate_canon::{Value, parse};
use mandate_connections::ConnectError;
use mandate_connections::exchange::TokenResponse;
use mandate_connections::vault::AccessToken;
use secrecy::SecretString;

/// The most of a token answer that is read; a longer body is refused unread.
pub const MAX_BODY: usize = 16 * 1024;

/// The media type a token answer must carry, before any parameter.
const JSON: &str = "application/json";

/// The answer's `access_token`, `token_type` and `scope`, from a `200` whose media type is
/// `application/json` and whose body is at most [`MAX_BODY`] bytes of a JSON object holding all
/// three as non-empty strings. The body is read by `mandate_canon::parse`, so it fails closed on
/// anything that reader refuses: a float, an exponent, an integer out of range, a duplicate
/// member, a byte-order mark, or text that is not UTF-8. Another member that reader accepts is
/// ignored.
///
/// # Errors
/// [`ConnectError::EndpointFailed`] for any other answer. The error names nothing of the body.
pub fn token_response(
    status: u16,
    content_type: Option<&str>,
    body: &[u8],
) -> Result<TokenResponse, ConnectError> {
    if status != 200 || !is_json(content_type) || body.len() > MAX_BODY {
        return Err(ConnectError::EndpointFailed);
    }
    let value = parse(body).map_err(|_| ConnectError::EndpointFailed)?;
    let member = |name: &str| {
        value
            .get(name)
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
    };
    match (
        member("access_token"),
        member("token_type"),
        member("scope"),
    ) {
        (Some(token), Some(token_type), Some(scope)) => Ok(TokenResponse {
            access_token: AccessToken(SecretString::from(token)),
            token_type: token_type.to_owned(),
            scope: scope.to_owned(),
        }),
        _ => Err(ConnectError::EndpointFailed),
    }
}

/// Whether `content_type`'s media type, before any `;` parameter, is [`JSON`].
fn is_json(content_type: Option<&str>) -> bool {
    content_type
        .and_then(|text| text.split(';').next())
        .is_some_and(|media| media.trim().eq_ignore_ascii_case(JSON))
}
