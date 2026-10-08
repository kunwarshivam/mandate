//! The guard every outbound Alpaca request passes (DEC-821 items 2 and 4).
//!
//! The only request to the live host that can exist is a [`LiveTokenRequest`]: it has no fields
//! to vary and one crate-private constructor, which the code exchange (D2c) uses (and later the
//! refresh), in the token-exchange process only, never the executor. Every other request is a
//! [`PaperRequest`], which can address only the paper host. An HTTP adapter sends an
//! [`Outbound`], never a raw URL, and an adapter that receives a raw request first passes it
//! through [`admit`].
//!
//! ```compile_fail
//! let forged = mandate_connections::hosts::LiveTokenRequest::token_endpoint();
//! ```

use crate::ConnectError;

/// Alpaca's paper trading host.
pub const PAPER_HOST: &str = "paper-api.alpaca.markets";
/// The one live-host URL, `POST` only. Crate-private, so no adapter can build a live URL of its
/// own; an adapter gets it from [`LiveTokenRequest::url`].
pub(crate) const LIVE_TOKEN_URL: &str = "https://api.alpaca.markets/oauth/token";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Patch,
    Delete,
}

/// `POST https://api.alpaca.markets/oauth/token`, and nothing else.
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

    /// Always [`Method::Post`].
    pub fn method(self) -> Method {
        Method::Post
    }

    /// Always `https://api.alpaca.markets/oauth/token`.
    pub fn url(self) -> &'static str {
        LIVE_TOKEN_URL
    }
}

/// A trading or account request, always to [`PAPER_HOST`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaperRequest {
    method: Method,
    path: String,
}

impl PaperRequest {
    /// A request for `path` on the paper host. `path` must start with `/v2/` and contain only
    /// path characters: no scheme, authority, `//`, `.` or `..` segment, percent escape, query,
    /// fragment, backslash, or whitespace. So it cannot name another host or another endpoint
    /// than the one it spells.
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

/// Admits a raw request or refuses it before it leaves the process. The match is exact, never
/// normalized: no trimming, case folding, percent decoding, or dot-segment removal.
///
/// - To the live host, only `POST https://api.alpaca.markets/oauth/token`, byte for byte.
/// - To the paper host, `https://paper-api.alpaca.markets` followed by a path
///   [`PaperRequest::new`] accepts, with the method kept.
/// - To any other host, or over anything but lowercase `https`, nothing.
pub fn admit(method: Method, url: &str) -> Result<Outbound, ConnectError> {
    let _ = (method, url);
    Err(ConnectError::Unimplemented { story: "E10-13" })
}
