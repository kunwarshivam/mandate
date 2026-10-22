//! The token-exchange process's start settings (connections spec §5.2 step 4; DEC-2629 item 3;
//! DEC-2633 item 2): the connection it exchanges for, and where its one request goes.
//!
//! The token URL is read first and refused before anything else: a build without debug assertions
//! has no code that could follow one, so it refuses to start whenever `MANDATE_TOKEN_URL` is set at
//! all. A debug build admits one only through [`crate::cell::parse`], in the `dev` and `ci` tiers.

use mandate_connections::record::ConnectionId;

#[cfg(debug_assertions)]
use crate::cell::{self, CellTokenUrl};

/// The connection the deployment manager starts the process for (DEC-1175 item 1).
pub const SUBJECT: &str = "MANDATE_SUBJECT";

/// The deployment's tier, read as `mandate_release::Tier::from_setting` reads it.
pub const TIER: &str = "MANDATE_DEPLOYMENT_TIER";

/// A dev or CI cell's token URL (DEC-2629 item 3, DEC-2633 item 1).
pub const TOKEN_URL: &str = "MANDATE_TOKEN_URL";

/// Where the one token request goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    /// The URL `mandate_connections::hosts::LiveTokenRequest` pins.
    Live,
    /// A dev or CI cell's fixture provider.
    #[cfg(debug_assertions)]
    Cell(CellTokenUrl),
}

/// What the process starts with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub connection: ConnectionId,
    pub endpoint: Endpoint,
}

/// Why the process refuses to start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartRefused {
    /// A build without debug assertions was given a token URL, which it cannot follow.
    TokenUrlInRelease,
    /// The token URL [`crate::cell::parse`] refused.
    #[cfg(debug_assertions)]
    Cell(cell::Refused),
    /// `MANDATE_SUBJECT` is unset or names no connection.
    Subject,
}

impl StartRefused {
    /// The code the process writes, alone, before it exits.
    pub fn code(self) -> &'static str {
        match self {
            Self::TokenUrlInRelease => "token_exchange_token_url_in_release",
            #[cfg(debug_assertions)]
            Self::Cell(refused) => refused.code(),
            Self::Subject => "token_exchange_subject",
        }
    }
}

/// The settings `var` gives, the token URL judged before the subject.
///
/// # Errors
/// [`endpoint`]'s refusal first, then [`StartRefused::Subject`].
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn settings(_var: impl Fn(&str) -> Option<String>) -> Result<Settings, StartRefused> {
    todo!()
}

/// The live endpoint when no token URL is set. A debug build admits a cell's when `tier` admits
/// `token_url`; a build without debug assertions refuses any token URL, whatever the tier.
///
/// # Errors
/// [`StartRefused::Cell`] in a debug build for a token URL [`crate::cell::parse`] refuses;
/// [`StartRefused::TokenUrlInRelease`] in any other build for any token URL, even an empty one.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn endpoint(_tier: Option<&str>, _token_url: Option<&str>) -> Result<Endpoint, StartRefused> {
    todo!()
}
