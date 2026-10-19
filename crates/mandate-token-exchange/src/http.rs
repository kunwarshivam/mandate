//! The HTTP adapter of `mandate_connections::exchange::TokenEndpoint` (DEC-2629 item 2): one
//! `POST` of [`crate::form::body`] per exchange, read by [`crate::response::token_response`].

use std::time::Duration;

use mandate_connections::ConnectError;
use mandate_connections::exchange::{ExchangeForm, TokenEndpoint, TokenResponse};
use mandate_connections::hosts::LiveTokenRequest;
use reqwest::header::CONTENT_TYPE;

#[cfg(debug_assertions)]
use crate::cell::CellTokenUrl;
use crate::form::body;
use crate::response::{MAX_BODY, token_response};

/// How long the connection to the token endpoint may take.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// How long the whole request may take, its answer read.
pub const TOTAL_TIMEOUT: Duration = Duration::from_secs(15);

/// The request body's media type.
const FORM: &str = "application/x-www-form-urlencoded";

/// Where the request goes.
pub(crate) enum Target {
    /// The URL the `LiveTokenRequest` carries.
    Live,
    /// A dev or CI cell's fixture provider (DEC-2629 item 3).
    #[cfg(debug_assertions)]
    Cell(CellTokenUrl),
}

/// Sends the exchange's one request. [`HttpTokenEndpoint::live`] sends it only to the URL the
/// `LiveTokenRequest` it is handed carries, over HTTPS; a debug build may instead send it to a
/// cell's [`CellTokenUrl`]. Either follows no redirect, never retries, and reads at most
/// [`crate::response::MAX_BODY`] bytes of the answer.
pub struct HttpTokenEndpoint {
    pub(crate) runtime: tokio::runtime::Runtime,
    pub(crate) client: reqwest::Client,
    target: Target,
}

impl HttpTokenEndpoint {
    /// The live adapter: HTTPS only, to the pinned token URL.
    ///
    /// # Errors
    /// [`ConnectError::EndpointFailed`] when the client cannot be built.
    pub fn live() -> Result<Self, ConnectError> {
        Self::built(true, Target::Live, TOTAL_TIMEOUT)
    }

    /// A dev or CI cell's adapter, sending to the fixture provider's `url` (DEC-2629 item 3),
    /// which [`crate::cell::parse`] admitted on a loopback host, over `http` or `https`.
    ///
    /// # Errors
    /// [`ConnectError::EndpointFailed`] when the client cannot be built.
    #[cfg(debug_assertions)]
    pub fn for_cell(url: CellTokenUrl) -> Result<Self, ConnectError> {
        Self::built(false, Target::Cell(url), TOTAL_TIMEOUT)
    }

    /// The client both adapters share: no redirect, no retry, bounded timeouts with `total` for
    /// the whole request, TLS on `ring`, and a runtime of its own for the one request.
    pub(crate) fn built(
        https_only: bool,
        target: Target,
        total: Duration,
    ) -> Result<Self, ConnectError> {
        let _already_installed = rustls::crypto::ring::default_provider().install_default();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| ConnectError::EndpointFailed)?;
        let client = reqwest::Client::builder()
            .https_only(https_only)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(total)
            .build()
            .map_err(|_| ConnectError::EndpointFailed)?;
        Ok(Self {
            runtime,
            client,
            target,
        })
    }
}

impl TokenEndpoint for HttpTokenEndpoint {
    fn post(
        &mut self,
        request: LiveTokenRequest,
        form: &ExchangeForm<'_>,
    ) -> Result<TokenResponse, ConnectError> {
        let url = match &self.target {
            Target::Live => request.url(),
            #[cfg(debug_assertions)]
            Target::Cell(url) => url.as_str(),
        };
        let sent = self
            .client
            .post(url)
            .header(CONTENT_TYPE, FORM)
            .body(body(form));
        self.runtime.block_on(async move {
            let mut response = sent
                .send()
                .await
                .map_err(|_| ConnectError::EndpointFailed)?;
            let status = response.status().as_u16();
            let content_type = response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
            let limit = u64::try_from(MAX_BODY).map_err(|_| ConnectError::EndpointFailed)?;
            if response
                .content_length()
                .is_some_and(|length| length > limit)
            {
                return Err(ConnectError::EndpointFailed);
            }
            let mut read = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| ConnectError::EndpointFailed)?
            {
                if read.len().saturating_add(chunk.len()) > MAX_BODY {
                    return Err(ConnectError::EndpointFailed);
                }
                read.extend_from_slice(&chunk);
            }
            token_response(status, content_type.as_deref(), &read)
        })
    }
}
