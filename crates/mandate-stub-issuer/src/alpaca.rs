//! The fixture Alpaca OAuth provider for dev and CI cells
//! ([DEC-2625](../../../docs/project/decisions/DEC-2625.md)): `GET /alpaca/oauth/authorize` and
//! `POST /alpaca/oauth/token`, answering as connections spec §5.2 and §5.5 say Alpaca's do, for one
//! configured client and redirect URI. Codes live in memory only, at most [`MAX_CODES`]; tokens are
//! not kept. Nothing here reads a clock or logs: the router passes the time in.

use std::path::Path;

use ring::rand::{SecureRandom, SystemRandom};

use crate::start::Refused;

/// The authorization page's path.
pub const AUTHORIZE_PATH: &str = "/alpaca/oauth/authorize";

/// The token endpoint's path.
pub const TOKEN_PATH: &str = "/alpaca/oauth/token";

/// The one scope request accepted and granted, as `start::REQUESTED_SCOPES` sends it.
pub const SCOPE: &str = "trading data";

/// How long a code may be redeemed, in seconds (connections spec §5.5).
pub const CODE_TTL_S: i64 = 600;

/// The most codes held at once (DEC-2625 item 5).
pub const MAX_CODES: usize = 4096;

/// The bytes of a code or a token before base64url.
pub const RANDOM_BYTES: usize = 32;

/// Where codes and tokens come from: the operating system's random source in the stub, a seeded one
/// in tests.
pub trait Random: Send + Sync {
    /// Fills `out`; `false` when the source failed.
    fn fill(&self, out: &mut [u8]) -> bool;
}

impl Random for SystemRandom {
    fn fill(&self, out: &mut [u8]) -> bool {
        SecureRandom::fill(self, out).is_ok()
    }
}

/// The client the provider serves (DEC-2625 item 2). It has no `Debug` form that could print the
/// secret.
pub struct Settings {
    pub client_id: String,
    pub client_secret: Vec<u8>,
    pub redirect_uri: String,
}

/// Why a request is refused: RFC 6749 §5.2's codes, and a full store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    InvalidRequest,
    InvalidClient,
    InvalidGrant,
    Unavailable,
}

impl Refusal {
    /// The `error` member of the answer.
    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::InvalidClient => "invalid_client",
            Self::InvalidGrant => "invalid_grant",
            Self::Unavailable => "temporarily_unavailable",
        }
    }

    /// The status of the answer: 503 for a full store, 400 otherwise.
    pub fn status(self) -> u16 {
        match self {
            Self::Unavailable => 503,
            _ => 400,
        }
    }
}

/// Why the client secret file is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretFileError {
    /// It cannot be read, or is not a regular file.
    Unreadable,
    /// A group or other permission bit is set.
    Mode,
    /// It is empty.
    Empty,
}

/// The provider: its client and the codes it issued.
#[expect(
    dead_code,
    reason = "the tests land before the implementation (DEC-77); the fields are read once it lands"
)]
pub struct Alpaca {
    settings: Settings,
    random: Box<dyn Random>,
}

impl Alpaca {
    pub fn new(settings: Settings, random: Box<dyn Random>) -> Self {
        Self { settings, random }
    }

    /// The `Location` to answer `GET /alpaca/oauth/authorize?<query>` with at `now` (Unix
    /// seconds): the redirect URI with a fresh `code` and the request's `state`.
    ///
    /// # Errors
    /// [`Refusal::InvalidRequest`] for anything but exactly DEC-2625 item 3's parameters, and
    /// [`Refusal::Unavailable`] for a full store or a failed random source.
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub fn authorize(&self, _query: &str, _now: i64) -> Result<String, Refusal> {
        todo!()
    }

    /// The JSON body to answer `POST /alpaca/oauth/token` with, for the form `body` at `now`.
    ///
    /// # Errors
    /// DEC-2625 item 4's refusals.
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub fn token(&self, _body: &str, _now: i64) -> Result<String, Refusal> {
        todo!()
    }

    /// How many codes are held.
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub fn held(&self) -> usize {
        todo!()
    }
}

/// The client secret from `path`: a regular file with no group or other permission bit.
///
/// # Errors
/// [`SecretFileError`].
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn read_secret(_path: &Path) -> Result<Vec<u8>, SecretFileError> {
    todo!()
}

/// The provider's settings from its three flags' values (DEC-2625 items 1 and 2): `None` with none
/// of them, the settings with all three, the client secret read from its file.
///
/// # Errors
/// [`Refused::Alpaca`] for some but not all three, an empty client id or redirect URI, or a secret
/// file [`read_secret`] refuses, or a redirect URI outside DEC-2623 item 2's shape.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn settings(
    _client_id: Option<&str>,
    _secret_file: Option<&str>,
    _redirect_uri: Option<&str>,
) -> Result<Option<Settings>, Refused> {
    todo!()
}
