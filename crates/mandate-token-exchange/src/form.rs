//! The token request's body (DEC-2629 item 2), `application/x-www-form-urlencoded`.

use mandate_connections::exchange::ExchangeForm;
use secrecy::ExposeSecret;

/// `grant_type=authorization_code`, then `code`, `redirect_uri`, `client_id`, `client_secret` and
/// `code_verifier`, each once and in that order, every value percent-encoded except RFC 3986's
/// unreserved characters. The text holds the client secret and the code: it is sent and dropped,
/// never logged.
pub fn body(form: &ExchangeForm<'_>) -> String {
    let members = [
        ("grant_type", "authorization_code"),
        ("code", form.code.0.expose_secret()),
        ("redirect_uri", form.redirect_uri),
        ("client_id", form.client_id.0.as_str()),
        ("client_secret", form.client_secret.0.expose_secret()),
        ("code_verifier", form.code_verifier.0.expose_secret()),
    ];
    members
        .iter()
        .map(|(name, value)| format!("{name}={}", encoded(value)))
        .collect::<Vec<_>>()
        .join("&")
}

/// `value` with every byte outside RFC 3986's unreserved set written as `%XX`, upper case.
fn encoded(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}
