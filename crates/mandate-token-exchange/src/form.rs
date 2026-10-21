//! The token request's body (DEC-2629 item 2), `application/x-www-form-urlencoded`.

use mandate_connections::exchange::ExchangeForm;

/// `grant_type=authorization_code`, then `code`, `redirect_uri`, `client_id`, `client_secret` and
/// `code_verifier`, each once and in that order, every value percent-encoded except RFC 3986's
/// unreserved characters. The text holds the client secret and the code: it is sent and dropped,
/// never logged.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn body(_form: &ExchangeForm<'_>) -> String {
    todo!()
}
