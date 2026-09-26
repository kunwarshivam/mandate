//! The HTTPS transport to Alpaca's market-data host, the only Alpaca host this crate knows
//! (ADR-0001 ES-23: only the paper and data hosts are compiled in). Credentials live in
//! [`SecretString`]s, travel only as request headers marked sensitive, and never appear in
//! `Debug` output or errors (AGENTS.md rule 7).

use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::{Url, redirect};
use secrecy::{ExposeSecret, SecretString};

use crate::client::{RateHeaders, Response, Transport, TransportError};

/// The market-data host.
pub const DATA_HOST: &str = "https://data.alpaca.markets";
/// Environment variable holding the paper API key ID (ADR-0001 ES-19).
pub const KEY_ID_VAR: &str = "MANDATE_ALPACA_PAPER_KEY_ID";
/// Environment variable holding the paper API secret (ADR-0001 ES-19).
pub const SECRET_VAR: &str = "MANDATE_ALPACA_PAPER_SECRET";

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CredentialsError {
    #[error("{variable} is not set or empty")]
    Missing { variable: &'static str },
}

impl CredentialsError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(self) -> &'static str {
        match self {
            Self::Missing { .. } => "missing",
        }
    }
}

/// The paper account's market-data credentials.
#[derive(Debug)]
pub struct Credentials {
    key_id: SecretString,
    secret: SecretString,
}

impl Credentials {
    pub fn new(key_id: String, secret: String) -> Self {
        Self {
            key_id: SecretString::from(key_id),
            secret: SecretString::from(secret),
        }
    }

    /// Reads [`KEY_ID_VAR`] and [`SECRET_VAR`] through `lookup`; an empty value counts as unset.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, CredentialsError> {
        let read = |variable: &'static str| {
            lookup(variable)
                .filter(|value| !value.is_empty())
                .ok_or(CredentialsError::Missing { variable })
        };
        let key_id = read(KEY_ID_VAR)?;
        let secret = read(SECRET_VAR)?;
        Ok(Self::new(key_id, secret))
    }

    /// Reads the credentials from the process environment.
    pub fn from_env() -> Result<Self, CredentialsError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HttpSetupError {
    #[error("building the HTTPS client failed")]
    Client,
}

impl HttpSetupError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(self) -> &'static str {
        match self {
            Self::Client => "client",
        }
    }
}

const ENDPOINTS: [&str; 7] = [
    "/v2/stocks/bars?",
    "/v2/stocks/trades?",
    "/v2/stocks/quotes?",
    "/v1beta3/crypto/us/bars?",
    "/v1beta3/crypto/us/trades?",
    "/v1beta3/crypto/us/quotes?",
    "/v1/corporate-actions?",
];
const USER_AGENT: &str = concat!("mandate-marketdata/", env!("CARGO_PKG_VERSION"));
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Whether `path_and_query` names one of the six historical bars, trades, and quotes endpoints or the
/// corporate-actions endpoint, with a query of percent-encoded parameters and nothing that could
/// change the host or the request.
pub fn is_market_data_path(path_and_query: &str) -> bool {
    let Some(query) = ENDPOINTS
        .iter()
        .find_map(|endpoint| path_and_query.strip_prefix(endpoint))
    else {
        return false;
    };
    !query.contains("..")
        && query
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._~%&=:,".contains(&b))
}

/// HTTPS to [`DATA_HOST`] with the paper credentials.
#[derive(Debug)]
pub struct AlpacaDataHttp {
    client: reqwest::Client,
    base: Url,
    credentials: Credentials,
}

impl AlpacaDataHttp {
    /// HTTPS only, no redirects, bounded timeouts, and TLS on the `ring` provider.
    pub fn new(credentials: Credentials) -> Result<Self, HttpSetupError> {
        let _already_installed = rustls::crypto::ring::default_provider().install_default();
        let base = Url::parse(DATA_HOST).map_err(|_| HttpSetupError::Client)?;
        let client = reqwest::Client::builder()
            .https_only(true)
            .redirect(redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .user_agent(USER_AGENT)
            .build()
            .map_err(|_| HttpSetupError::Client)?;
        Ok(Self {
            client,
            base,
            credentials,
        })
    }

    fn url(&self, path_and_query: &str) -> Result<Url, TransportError> {
        if !is_market_data_path(path_and_query) {
            return Err(TransportError::RefusedPath);
        }
        let url = Url::parse(&format!("{DATA_HOST}{path_and_query}"))
            .map_err(|_| TransportError::RefusedPath)?;
        if url.origin() != self.base.origin() || url.fragment().is_some() {
            return Err(TransportError::RefusedPath);
        }
        Ok(url)
    }
}

fn sensitive(value: &SecretString) -> Result<HeaderValue, TransportError> {
    let mut header =
        HeaderValue::from_str(value.expose_secret()).map_err(|_| TransportError::Request)?;
    header.set_sensitive(true);
    Ok(header)
}

fn transport_error(e: &reqwest::Error) -> TransportError {
    if e.is_timeout() {
        TransportError::Timeout
    } else if e.is_connect() {
        TransportError::Connect
    } else {
        TransportError::Request
    }
}

impl Transport for AlpacaDataHttp {
    async fn get(&self, path_and_query: &str) -> Result<Response, TransportError> {
        let url = self.url(path_and_query)?;
        let response = self
            .client
            .get(url)
            .header("APCA-API-KEY-ID", sensitive(&self.credentials.key_id)?)
            .header("APCA-API-SECRET-KEY", sensitive(&self.credentials.secret)?)
            .send()
            .await
            .map_err(|e| transport_error(&e))?;
        let status = response.status().as_u16();
        let rate = rate_headers(response.headers());
        let body = response
            .bytes()
            .await
            .map_err(|e| transport_error(&e))?
            .to_vec();
        Ok(Response { status, rate, body })
    }
}

/// The `X-Ratelimit-*` headers of a response; one that is absent or not visible ASCII is `None`.
pub fn rate_headers(headers: &HeaderMap) -> RateHeaders {
    let read = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    };
    RateHeaders {
        limit: read("x-ratelimit-limit"),
        remaining: read("x-ratelimit-remaining"),
        reset: read("x-ratelimit-reset"),
    }
}
