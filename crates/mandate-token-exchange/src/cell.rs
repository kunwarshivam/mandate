//! The token URL of a dev or CI cell's fixture provider (DEC-2629 item 3, DEC-2625). Compiled
//! only with debug assertions: a release build has no code that reads one, so it can send the
//! token request only to the URL `mandate_connections::hosts::LiveTokenRequest` pins.

/// Why a cell token URL is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// `MANDATE_DEPLOYMENT_TIER` is not `dev` or `ci`.
    Tier,
    /// The URL is not `http` or `https` on a loopback host with a path and nothing else: no user
    /// information, query, or fragment, and never an Alpaca host.
    Url,
}

impl Refused {
    /// The code the process writes.
    pub fn code(self) -> &'static str {
        match self {
            Self::Tier => "token_exchange_cell_tier",
            Self::Url => "token_exchange_cell_url",
        }
    }
}

/// A token URL the cell's tier admits, kept exactly as given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellTokenUrl(String);

impl CellTokenUrl {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Admits `text` as the cell's token URL when `tier` (`MANDATE_DEPLOYMENT_TIER`, read as
/// `mandate_release::Tier::from_setting` reads it) is `dev` or `ci`.
///
/// # Errors
/// [`Refused::Tier`] first, then [`Refused::Url`].
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn parse(_tier: Option<&str>, _text: &str) -> Result<CellTokenUrl, Refused> {
    todo!()
}
