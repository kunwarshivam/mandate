//! The token endpoint's answer (DEC-2629 item 2).

use mandate_connections::ConnectError;
use mandate_connections::exchange::TokenResponse;

/// The most of a token answer that is read; a longer body is refused unread.
pub const MAX_BODY: usize = 16 * 1024;

/// The answer's `access_token`, `token_type` and `scope`, from a `200` whose media type is
/// `application/json` and whose body is at most [`MAX_BODY`] bytes of a JSON object holding all
/// three as non-empty strings. Any other member is ignored.
///
/// # Errors
/// [`ConnectError::EndpointFailed`] for any other answer. The error names nothing of the body.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn token_response(
    _status: u16,
    _content_type: Option<&str>,
    _body: &[u8],
) -> Result<TokenResponse, ConnectError> {
    todo!()
}
