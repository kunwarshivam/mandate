//! The HTTP adapter of `mandate_connections::exchange::TokenEndpoint` (DEC-2629 item 2): one
//! `POST` of [`crate::form::body`] per exchange, read by [`crate::response::token_response`].

use std::time::Duration;

use mandate_connections::ConnectError;
use mandate_connections::exchange::{ExchangeForm, TokenEndpoint, TokenResponse};
use mandate_connections::hosts::LiveTokenRequest;

#[cfg(debug_assertions)]
use crate::cell::CellTokenUrl;

/// How long the connection to the token endpoint may take.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// How long the whole request may take, its answer read.
pub const TOTAL_TIMEOUT: Duration = Duration::from_secs(15);

/// Sends the exchange's one request. [`HttpTokenEndpoint::live`] sends it only to the URL the
/// `LiveTokenRequest` it is handed carries, over HTTPS; a debug build may instead send it to a
/// cell's [`CellTokenUrl`]. Either follows no redirect, never retries, and reads at most
/// [`crate::response::MAX_BODY`] bytes of the answer.
pub struct HttpTokenEndpoint {
    _target: (),
}

impl HttpTokenEndpoint {
    /// The live adapter: HTTPS only, to the pinned token URL.
    ///
    /// # Errors
    /// [`ConnectError::EndpointFailed`] when the client cannot be built.
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub fn live() -> Result<Self, ConnectError> {
        todo!()
    }

    /// A dev or CI cell's adapter, sending to the fixture provider's `url` (DEC-2629 item 3).
    ///
    /// # Errors
    /// [`ConnectError::EndpointFailed`] when the client cannot be built.
    #[cfg(debug_assertions)]
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub fn for_cell(_url: CellTokenUrl) -> Result<Self, ConnectError> {
        todo!()
    }
}

impl TokenEndpoint for HttpTokenEndpoint {
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    fn post(
        &mut self,
        _request: LiveTokenRequest,
        _form: &ExchangeForm<'_>,
    ) -> Result<TokenResponse, ConnectError> {
        todo!()
    }
}
