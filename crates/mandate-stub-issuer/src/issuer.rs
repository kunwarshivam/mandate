//! The key made at start and the tokens it signs (DEC-2100 items 4, 6 and 7).

use std::net::SocketAddr;

/// How long a token is valid, in seconds.
pub const TOKEN_TTL_S: i64 = 300;

/// The longest subject accepted, in bytes (DEC-992 item 2).
pub const MAX_SUBJECT_BYTES: usize = 1024;

/// Why a key could not be made or a token signed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueError {
    /// The subject is empty, holds a control character, or is over [`MAX_SUBJECT_BYTES`].
    Subject,
    /// The operating system's random source or the signer failed.
    Key,
    /// Not written yet.
    Unimplemented { story: &'static str },
}

/// One issuer: its exact issuer string, its audience, its key's id, and its key set. The key
/// itself is never readable.
#[derive(Debug)]
pub struct Issuer {
    /// `http://<bind>`, exactly.
    pub issuer: String,
    /// The `aud` of every token.
    pub audience: String,
    /// The key's id, 16 random bytes in lowercase hex.
    pub kid: String,
    /// The key set, one EC P-256 key, as `GET /jwks.json` answers it.
    pub jwks: String,
}

impl Issuer {
    /// A new key for the stub bound at `bind`, signing for `audience`.
    ///
    /// # Errors
    /// [`IssueError::Key`].
    pub fn new(bind: SocketAddr, audience: &str) -> Result<Self, IssueError> {
        let _ = (bind, audience);
        Err(IssueError::Unimplemented { story: "E21-52" })
    }

    /// An ES256 access token for `subject`, issued at `now_s`, Unix seconds.
    ///
    /// # Errors
    /// [`IssueError::Subject`] or [`IssueError::Key`].
    pub fn token(&self, subject: &str, now_s: i64) -> Result<String, IssueError> {
        let _ = (subject, now_s);
        Err(IssueError::Unimplemented { story: "E21-52" })
    }
}
