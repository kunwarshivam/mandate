//! The HTTPS transport behind `mandate_sources::fetch::Fetcher` (E17-7, data-plane spec §4.2 step
//! 1, DEC-996 item 3). It runs only in a cell whose egress policy allows the allowlisted hosts; CI
//! drives the fetchers on recorded responses and never builds a request here.
//!
//! It sends only to a [`PermittedUrl`], which only the allowlist can build (DP-9); it follows no
//! redirect, so a 3xx answer is a refused status rather than a second host; it speaks HTTPS only;
//! it declares the user agent the SEC's fair-access rules ask for; and it stops reading a body at
//! the URL's response cap. Credentials travel only as headers marked sensitive and never appear in
//! `Debug` output or errors (AGENTS.md rule 7).

use std::time::Duration;

use mandate_sources::SourceError;
use mandate_sources::allowlist::PermittedUrl;
use mandate_sources::fetch::{NewsAuth, RawResponse, Transport};
use reqwest::header::HeaderValue;
use reqwest::redirect;
use secrecy::{ExposeSecret, SecretString};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Why the transport could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SetupError {
    #[error("the user agent must be visible ASCII and name a contact address")]
    UserAgent,
    #[error("building the HTTPS client failed")]
    Client,
}

impl SetupError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(self) -> &'static str {
        match self {
            Self::UserAgent => "user_agent",
            Self::Client => "client",
        }
    }
}

/// HTTPS GETs to permitted URLs.
#[derive(Debug)]
pub struct HttpsTransport {
    client: reqwest::Client,
}

/// Whether `agent` is a declared user agent: visible ASCII and spaces, at most 200 bytes, naming a
/// contact address, as the SEC's fair-access rules ask (`Company Name admin@example.com`).
pub fn is_declared_user_agent(agent: &str) -> bool {
    (1..=200).contains(&agent.len())
        && agent.contains('@')
        && agent.bytes().all(|b| b == b' ' || b.is_ascii_graphic())
}

impl HttpsTransport {
    /// HTTPS only, no redirects, bounded timeouts, and TLS on the `ring` provider.
    pub fn new(user_agent: &str) -> Result<Self, SetupError> {
        if !is_declared_user_agent(user_agent) {
            return Err(SetupError::UserAgent);
        }
        let _already_installed = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .https_only(true)
            .redirect(redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .user_agent(user_agent)
            .build()
            .map_err(|_| SetupError::Client)?;
        Ok(Self { client })
    }
}

fn sensitive(value: &SecretString) -> Result<HeaderValue, SourceError> {
    let mut header =
        HeaderValue::from_str(value.expose_secret()).map_err(|_| SourceError::Transport)?;
    header.set_sensitive(true);
    Ok(header)
}

/// Appends `chunk` to `body`, refusing once the body would pass `cap` bytes.
pub fn append_capped(body: &mut Vec<u8>, chunk: &[u8], cap: u64) -> Result<(), SourceError> {
    let total = body
        .len()
        .checked_add(chunk.len())
        .and_then(|n| u64::try_from(n).ok())
        .ok_or(SourceError::ResponseTooLarge)?;
    if total > cap {
        return Err(SourceError::ResponseTooLarge);
    }
    body.extend_from_slice(chunk);
    Ok(())
}

impl Transport for HttpsTransport {
    async fn get(
        &self,
        url: &PermittedUrl,
        auth: Option<&NewsAuth>,
    ) -> Result<RawResponse, SourceError> {
        let mut request = self.client.get(url.as_str());
        if let Some(auth) = auth {
            request = request
                .header("APCA-API-KEY-ID", sensitive(auth.key_id())?)
                .header("APCA-API-SECRET-KEY", sensitive(auth.secret())?);
        }
        let mut response = request.send().await.map_err(|_| SourceError::Transport)?;
        let status = response.status().as_u16();
        let cap = url.max_response_bytes();
        if response.content_length().is_some_and(|n| n > cap) {
            return Err(SourceError::ResponseTooLarge);
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| SourceError::Transport)? {
            append_capped(&mut body, &chunk, cap)?;
        }
        Ok(RawResponse { status, body })
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "tests: a wrong answer should stop the test"
)]
mod tests {
    use super::*;

    #[test]
    fn the_user_agent_must_name_a_contact() {
        assert!(is_declared_user_agent("Mandate research ops@example.com"));
        for bad in [
            "",
            "Mandate research",
            "a\n@b",
            &format!("{}@", "x".repeat(200)),
        ] {
            assert!(!is_declared_user_agent(bad), "{bad:?}");
        }
        assert_eq!(
            HttpsTransport::new("no contact").unwrap_err(),
            SetupError::UserAgent
        );
        assert!(HttpsTransport::new("Mandate research ops@example.com").is_ok());
    }

    #[test]
    fn a_body_stops_at_its_cap() {
        let mut body = Vec::new();
        append_capped(&mut body, b"abc", 5).unwrap();
        append_capped(&mut body, b"de", 5).unwrap();
        assert_eq!(
            append_capped(&mut body, b"f", 5),
            Err(SourceError::ResponseTooLarge)
        );
        assert_eq!(body, b"abcde");
    }

    #[test]
    fn credentials_never_print() {
        let header = sensitive(&SecretString::from("s3cret".to_owned())).unwrap();
        assert!(header.is_sensitive());
        assert!(!format!("{header:?}").contains("s3cret"));
    }
}
