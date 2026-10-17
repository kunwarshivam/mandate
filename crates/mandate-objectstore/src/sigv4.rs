//! AWS Signature Version 4 for the S3 API, header-signed, over the request exactly as it is sent.
//!
//! Every S3-compatible provider the plan names (the local test store, Cloudflare R2, Backblaze B2)
//! authenticates this way, so one signer serves all of them and a provider swap changes only the
//! endpoint. The caller passes every header it will send, `host`, `x-amz-date` and
//! `x-amz-content-sha256` among them, and all of them are signed.

use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};

/// An access key: its public identifier, its secret, and the session token of a temporary key.
#[derive(Clone)]
pub struct Credentials {
    access_key_id: String,
    secret_access_key: SecretString,
    session_token: Option<SecretString>,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Credentials { .. }")
    }
}

impl Credentials {
    /// A key from its parts.
    pub fn new(
        access_key_id: String,
        secret_access_key: SecretString,
        session_token: Option<SecretString>,
    ) -> Self {
        Self {
            access_key_id,
            secret_access_key,
            session_token,
        }
    }

    /// The session token a temporary key sends as `x-amz-security-token`.
    pub fn session_token(&self) -> Option<&str> {
        self.session_token
            .as_ref()
            .map(|token| token.expose_secret())
    }
}

/// The request to sign, as it will be sent.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    /// The HTTP method in upper case.
    pub method: &'a str,
    /// The path, already URI-encoded as it goes on the wire.
    pub path: &'a str,
    /// The query parameters, not encoded; a parameter with no value has an empty one.
    pub query: &'a [(&'a str, &'a str)],
    /// Every header sent, `host` included.
    pub headers: &'a [(&'a str, &'a str)],
    /// The hex SHA-256 of the body, the `x-amz-content-sha256` value.
    pub payload_sha256: &'a str,
}

/// The credential scope's region and service, and the signing time.
#[derive(Debug, Clone, Copy)]
pub struct Scope<'a> {
    /// The region, `us-east-1` for most S3-compatible stores and `auto` for R2.
    pub region: &'a str,
    /// `s3`, or `sts` for the security token service.
    pub service: &'a str,
    /// The time the request is signed; also the `x-amz-date` header.
    pub at: Timestamp,
}

/// The `Authorization` header value for `request`.
pub fn authorization(
    credentials: &Credentials,
    request: &Request<'_>,
    scope: &Scope<'_>,
) -> String {
    let _ = (credentials, request, scope);
    String::new()
}

/// `20130524T000000Z`: the `x-amz-date` form of a time.
pub fn amz_date(at: Timestamp) -> String {
    let _ = at;
    String::new()
}

/// The lower-case hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let _ = bytes;
    String::new()
}

/// RFC 3986 percent-encoding as SigV4 defines it: unreserved characters stay, everything else is
/// `%XX` in upper case, and `/` stays only when `keep_slash`.
pub fn uri_encode(text: &str, keep_slash: bool) -> String {
    let _ = (text, keep_slash);
    String::new()
}
