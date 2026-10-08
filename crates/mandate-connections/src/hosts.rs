//! The guard every outbound Alpaca request passes (DEC-821 items 2 and 3).
//!
//! The only request to the live host that can exist is a [`LiveTokenRequest`]: it has no fields
//! to vary and one crate-private constructor, which the code exchange (D2c) uses
//! (and later the refresh). Every other request is a [`PaperRequest`], which can address only
//! the paper host. An HTTP adapter sends an [`Outbound`], never a raw URL, and an adapter that
//! receives a raw request first passes it through [`admit`].
//!
//! ```compile_fail
//! let forged = mandate_connections::hosts::LiveTokenRequest::token_endpoint();
//! ```

use crate::ConnectError;

/// Alpaca's live trading host. Reachable only through [`LiveTokenRequest`].
pub const LIVE_HOST: &str = "api.alpaca.markets";
/// Alpaca's paper trading host.
pub const PAPER_HOST: &str = "paper-api.alpaca.markets";
/// The one live-host URL, `POST` only.
pub const LIVE_TOKEN_URL: &str = "https://api.alpaca.markets/oauth/token";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Patch,
    Delete,
}

/// `POST https://api.alpaca.markets/oauth/token`, and nothing else. Its method and URL are
/// [`Method::Post`] and [`LIVE_TOKEN_URL`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveTokenRequest {
    only_the_token_endpoint: (),
}

impl LiveTokenRequest {
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the exchange builds it once E10-13 is implemented"
        )
    )]
    pub(crate) const fn token_endpoint() -> Self {
        Self {
            only_the_token_endpoint: (),
        }
    }
}

/// A trading or account request, always to [`PAPER_HOST`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaperRequest {
    method: Method,
    path: String,
}

impl PaperRequest {
    /// A request for `path` on the paper host. `path` is an absolute path (`/v2/...`) with no
    /// scheme, authority, or `..` segment, so it cannot name another host.
    pub fn new(method: Method, path: &str) -> Result<Self, ConnectError> {
        let _ = (method, path);
        Err(ConnectError::Unimplemented { story: "E10-13" })
    }

    /// `https://paper-api.alpaca.markets` followed by the path.
    pub fn url(&self) -> Result<String, ConnectError> {
        let _ = (&self.method, &self.path);
        Err(ConnectError::Unimplemented { story: "E10-13" })
    }
}

/// Every request this crate allows to leave the process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outbound {
    LiveToken(LiveTokenRequest),
    Paper(PaperRequest),
}

/// Admits a raw request or refuses it before it leaves the process. To the live host only
/// `POST /oauth/token` is admitted (exact path, no query, default port); to the paper host any
/// `/v2/` path; to any other host, or over anything but `https`, nothing.
pub fn admit(method: Method, url: &str) -> Result<Outbound, ConnectError> {
    let _ = (method, url);
    Err(ConnectError::Unimplemented { story: "E10-13" })
}
